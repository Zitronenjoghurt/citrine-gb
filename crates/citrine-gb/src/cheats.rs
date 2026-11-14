pub(crate) mod gb;
#[cfg(feature = "gba")]
mod gba;
#[cfg(feature = "nds")]
mod nds;
mod ops;

use crate::console::ConsoleKind;
use ops::OpList;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    GameGenie,
    GameShark,
    ActionReplay,
    CodeBreaker,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Self::GameGenie => "Game Genie",
            Self::GameShark => "GameShark",
            Self::ActionReplay => "Action Replay",
            Self::CodeBreaker => "CodeBreaker",
        }
    }

    pub fn of(console: ConsoleKind) -> &'static [Format] {
        match console {
            ConsoleKind::Gb | ConsoleKind::Gbc => &[Self::GameGenie, Self::GameShark],
            ConsoleKind::Gba => &[Self::GameShark, Self::ActionReplay, Self::CodeBreaker],
            ConsoleKind::Nds => &[Self::ActionReplay],
        }
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CheatError {
    #[error("no code")]
    Empty,
    #[error("line {line} is not a code")]
    Unreadable { line: usize },
    #[error("this build has no cheats for this console")]
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Program {
    Ops(OpList),
    #[cfg(feature = "nds")]
    Ds(nds::ArCode),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cheat {
    format: Format,
    program: Program,
}

impl Cheat {
    pub fn parse(console: ConsoleKind, text: &str) -> Result<Self, CheatError> {
        let lines: Vec<(usize, String)> = text
            .lines()
            .enumerate()
            .map(|(index, line)| {
                let clean: String = line.chars().filter(|c| !c.is_whitespace()).collect();
                (index + 1, clean.to_ascii_uppercase())
            })
            .filter(|(_, line)| !line.is_empty())
            .collect();
        if lines.is_empty() {
            return Err(CheatError::Empty);
        }
        match console {
            ConsoleKind::Gb | ConsoleKind::Gbc => gb::parse(&lines),
            #[cfg(feature = "gba")]
            ConsoleKind::Gba => gba::parse(&lines),
            #[cfg(feature = "nds")]
            ConsoleKind::Nds => nds::parse(&lines),
            #[cfg(not(feature = "gba"))]
            ConsoleKind::Gba => Err(CheatError::Unsupported),
            #[cfg(not(feature = "nds"))]
            ConsoleKind::Nds => Err(CheatError::Unsupported),
        }
    }

    pub fn format(&self) -> Format {
        self.format
    }
}

pub trait CheatTarget {
    fn cheat_read(&mut self, address: u32, width: u8) -> u32;
    fn cheat_write(&mut self, address: u32, width: u8, value: u32);
    fn cheat_rom(&self) -> &[u8];
    fn rom_offsets(&self, address: u32, out: &mut Vec<usize>);
    fn set_rom_patches(&mut self, patches: &[(usize, u8)]);
}

pub fn apply(target: &mut (impl CheatTarget + ?Sized), cheats: &[Cheat]) {
    let mut bytes = Vec::new();
    let mut offsets = Vec::new();
    for cheat in cheats {
        #[cfg(feature = "nds")]
        let Program::Ops(list) = &cheat.program else {
            continue;
        };
        #[cfg(not(feature = "nds"))]
        let Program::Ops(list) = &cheat.program;
        for patch in &list.patches {
            offsets.clear();
            target.rom_offsets(patch.address, &mut offsets);
            let width = usize::from(patch.width);
            for &offset in &offsets {
                let rom = target.cheat_rom();
                let Some(old) = rom.get(offset..offset + width) else {
                    continue;
                };
                let old = old
                    .iter()
                    .rev()
                    .fold(0u32, |value, &byte| value << 8 | u32::from(byte));
                if patch.check.is_some_and(|check| check != old) {
                    continue;
                }
                let value = patch.value.to_le_bytes();
                bytes.extend((0..width).map(|index| (offset + index, value[index])));
            }
        }
    }
    target.set_rom_patches(&bytes);
    for cheat in cheats {
        match &cheat.program {
            Program::Ops(list) => ops::run(&list.ops, target),
            #[cfg(feature = "nds")]
            Program::Ds(code) => nds::run(code, target),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PatchedRom {
    pristine: Option<std::sync::Arc<[u8]>>,
    applied: Vec<(usize, u8)>,
}

impl PatchedRom {
    pub fn pristine<'a>(&'a self, rom: &'a std::sync::Arc<[u8]>) -> &'a std::sync::Arc<[u8]> {
        self.pristine.as_ref().unwrap_or(rom)
    }

    pub fn set(&mut self, rom: &mut std::sync::Arc<[u8]>, patches: &[(usize, u8)]) {
        if self.applied == patches {
            return;
        }
        let pristine = self.pristine.take().unwrap_or_else(|| rom.clone());
        self.applied = patches.to_vec();
        if patches.is_empty() {
            *rom = pristine;
            return;
        }
        let mut patched = pristine.to_vec();
        for &(offset, byte) in patches {
            if let Some(slot) = patched.get_mut(offset) {
                *slot = byte;
            }
        }
        *rom = patched.into();
        self.pristine = Some(pristine);
    }
}
