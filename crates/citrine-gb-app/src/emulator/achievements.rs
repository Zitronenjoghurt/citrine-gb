use super::Emulator;
use citrine_gb::console::{Console as _, ConsoleKind};
use citrine_gb::machine::Machine;
use citrine_gb_achievements::{Client, Console, Memory};

pub(crate) struct MachineMemory<'a>(&'a Machine);

impl Memory for MachineMemory<'_> {
    fn read(&mut self, address: u32, buffer: &mut [u8]) -> usize {
        match self.0.as_gb() {
            Some(gb) => gb.achievement_read(address, buffer),
            None => 0,
        }
    }
}

pub(super) fn after_frame(client: &mut Option<Client>, machine: &Machine) {
    if let Some(client) = client.as_mut().filter(|client| client.runs_frames()) {
        crate::profile::scope!("achievements");
        client.do_frame(&mut MachineMemory(machine));
    }
}

pub(crate) struct AchievementSeat<'a> {
    pub(crate) client: &'a mut Client,
    machine: &'a Machine,
}

impl AchievementSeat<'_> {
    pub(crate) fn parts(&mut self) -> (&mut Client, MachineMemory<'_>) {
        (&mut *self.client, MachineMemory(self.machine))
    }

    pub(crate) fn load_running_game(&mut self) {
        let Some(console) = console_of(self.machine) else {
            self.client.unload_game();
            return;
        };
        let rom = self.machine.rom_bytes();
        self.client
            .load_game(console, &rom, &mut MachineMemory(self.machine));
    }
}

fn console_of(machine: &Machine) -> Option<Console> {
    let gb = machine.as_gb().filter(|gb| gb.cartridge.has_rom_loaded)?;
    match machine.kind() {
        ConsoleKind::Gbc if gb.cartridge.header.cgb_mode.is_cgb() => Some(Console::GameBoyColor),
        ConsoleKind::Gb | ConsoleKind::Gbc => Some(Console::GameBoy),
        _ => None,
    }
}

impl Emulator {
    pub(crate) fn with_achievements<R>(
        &mut self,
        f: impl FnOnce(&mut AchievementSeat<'_>) -> R,
    ) -> Option<R> {
        if !self.seat.is_home() {
            return None;
        }
        let core = self.seat.core_mut();
        let mut seat = AchievementSeat {
            client: core.achievements.get_or_insert_with(Client::new),
            machine: &core.machine,
        };
        Some(f(&mut seat))
    }

    #[cfg(test)]
    pub(crate) fn core_is_away(&self) -> bool {
        !self.seat.is_home()
    }

    pub(crate) fn has_achievements(&self) -> bool {
        self.seat.is_home() && self.seat.core().achievements.is_some()
    }

    pub(super) fn achievements_follow_game(&mut self) {
        let core = self.seat.core_mut();
        if let Some(client) = core.achievements.as_mut() {
            let mut seat = AchievementSeat {
                client,
                machine: &core.machine,
            };
            if seat.client.user().is_some() {
                seat.load_running_game();
            }
        }
    }

    pub(super) fn achievements_reset(&mut self) {
        if let Some(client) = self.seat.core_mut().achievements.as_mut() {
            client.reset();
        }
    }
}
