use crate::app::tabs::TabViewer;
use crate::app::widgets::memory::{MemoryAction, MemoryView, MemoryViewState};
use citrine_gb::debug::Debug;
use citrine_gb::debug::code::Address;
use citrine_gb::debug::memory::MemoryBlock;
use egui::Ui;

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    let running = viewer.emulator.is_running();
    let machine = viewer.emulator.debug_mut();
    let state = &mut viewer.ui.memory;
    let mut blocks = Vec::new();
    machine.memory_blocks(&mut blocks);
    let Some(block) = state.pick(&blocks).copied() else {
        return;
    };
    let mut bytes = vec![0; block.size as usize];
    machine.peek(Address::at(block.id, state.bank(), 0), &mut bytes);
    state.track(&block, Debug::cycles(&*machine), &bytes);
    let watchable = machine.features().watchpoints;
    let watches = machine.breakpoints().watches().to_vec();
    let view = MemoryView::new(&blocks, &block, &bytes, state);
    let view = if watchable {
        view.watches(&watches)
    } else {
        view
    };
    let action = view.editable(!running).show(ui);
    match action {
        Some(MemoryAction::Poke(at, byte)) => machine.poke(at, &[byte]),
        Some(MemoryAction::Watch(watch)) => machine.breakpoints().watch(watch),
        Some(MemoryAction::Unwatch(watch)) => machine.breakpoints().unwatch(watch),
        Some(MemoryAction::GoTo(text)) => match find(&*machine, &blocks, &block, state, &text) {
            Some((found, at)) => state.show_at(&found, at, 1),
            None => state.go_to_failed(),
        },
        None => {}
    }
}

fn find(
    machine: &dyn Debug,
    blocks: &[MemoryBlock],
    shown: &MemoryBlock,
    state: &MemoryViewState,
    text: &str,
) -> Option<(MemoryBlock, Address)> {
    let (bank, addr) = parse_address(text)?;
    let mapped = machine.locate(addr);
    let at = |block: &MemoryBlock, bank: u16| {
        let offset = addr.checked_sub(block.start)?;
        let found = Address::at(block.id, bank, offset);
        let absolute = found.gba_address().is_none_or(|at| at == addr);
        (offset < block.size && block.holds(bank) && absolute).then_some(found)
    };
    if let Some(bank) = bank {
        return blocks
            .iter()
            .find_map(|block| Some((*block, at(block, bank)?)));
    }
    if let Some(found) = at(shown, state.bank()) {
        return Some((*shown, found));
    }
    if let Some(mapped) = mapped
        && let Some(block) = blocks.iter().find(|block| block.contains(mapped))
    {
        return Some((*block, mapped));
    }
    blocks
        .iter()
        .find_map(|block| Some((*block, at(block, block.first_bank)?)))
}

pub fn parse_address(text: &str) -> Option<(Option<u16>, u32)> {
    let hex = |part: &str| {
        let part = part.trim().trim_start_matches('$');
        let part = part
            .strip_prefix("0x")
            .or_else(|| part.strip_prefix("0X"))
            .unwrap_or(part);
        u32::from_str_radix(part, 16).ok()
    };
    match text.split_once(':') {
        Some((bank, addr)) => Some((Some(u16::try_from(hex(bank)?).ok()?), hex(addr)?)),
        None => Some((None, hex(text)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_reads_as_hex_with_an_optional_bank() {
        assert_eq!(parse_address("C123"), Some((None, 0xC123)));
        assert_eq!(parse_address("$ff80"), Some((None, 0xFF80)));
        assert_eq!(parse_address("0x4000"), Some((None, 0x4000)));
        assert_eq!(parse_address("03:D010"), Some((Some(3), 0xD010)));
        assert_eq!(parse_address("wram"), None);
        assert_eq!(parse_address(""), None);
    }

    #[cfg(feature = "gba")]
    #[test]
    fn a_gba_address_finds_its_rom_bank_from_any_bank() {
        use citrine_gb::debug::code::BlockId;
        use citrine_gb::gba::machine::Gba;
        let gba = Gba::new(&vec![0; 0x2_0000]);
        let mut blocks = Vec::new();
        gba.memory_blocks(&mut blocks);
        let rom = *blocks
            .iter()
            .find(|block| block.id == BlockId::GamePak)
            .unwrap();
        let mut state = MemoryViewState::default();
        state.show_at(&rom, Address::at(BlockId::GamePak, 1, 0), 1);

        let found = |text: &str| find(&gba, &blocks, &rom, &state, text).map(|(_, at)| at);

        assert_eq!(
            found("08000010"),
            Some(Address::at(BlockId::GamePak, 0, 0x10))
        );
        assert_eq!(
            found("0801FFFF"),
            Some(Address::at(BlockId::GamePak, 1, 0xFFFF))
        );
        assert_eq!(found("03000020"), Some(Address::gba(BlockId::Iwram, 0x20)));
    }
}
