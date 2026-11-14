use super::input::Input;
use super::pair::{self, Cable, LinkPort};
use crate::console::{Console, Keys};

#[derive(Debug)]
pub struct LocalCable<C: LinkPort> {
    cable: Cable<C::Message>,
}

impl<C: LinkPort> Default for LocalCable<C> {
    fn default() -> Self {
        Self {
            cable: Cable::default(),
        }
    }
}

impl<C: Console + LinkPort + Send> LocalCable<C> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn plugged(&self) -> bool {
        self.cable.plugged
    }

    pub fn set_plugged(&mut self, plugged: bool) {
        self.cable.plugged = plugged;
    }

    pub fn run_frame(&mut self, consoles: [&mut C; 2], keys: [Keys; 2]) {
        pair::run_pair(consoles, &mut self.cable, &keys.map(Input::new), [true; 2]);
    }

    pub fn unplug(&mut self, mut consoles: [&mut C; 2]) {
        self.cable.plugged = false;
        if consoles.iter().any(|console| console.link_plugged()) {
            pair::disconnect(&mut consoles, &mut self.cable);
        }
    }
}

#[cfg(all(test, feature = "persistence"))]
mod tests {
    use crate::ReadMemory;
    use crate::WriteMemory;
    use crate::console::Keys;
    use crate::gb::{GameBoy, GbModel};
    use crate::netplay::LocalCable;
    use crate::test_support::{COUNT, cart, done, red, saved, sent, trade_program};

    const IDLE: [Keys; 2] = [Keys::empty(); 2];

    const GO: [Keys; 2] = [Keys::A; 2];

    fn silver() -> GameBoy {
        let mut gb = GameBoy::new_empty(GbModel::Cgb);
        gb.load_rom(&cart(0x80, 0x10, &trade_program(0x80, 0x40, 3)))
            .unwrap();
        gb
    }

    fn lines(gb: &mut GameBoy) -> [u8; 2] {
        [0x10, 0x20].map(|select| {
            gb.joypad.write_naive(0xFF00, select);
            gb.joypad.read_naive(0xFF00) & 0x0F
        })
    }

    fn trade(red: &mut GameBoy, silver: &mut GameBoy, cable: &mut LocalCable<GameBoy>) -> usize {
        (1..=600)
            .find(|_| {
                cable.run_frame([red, silver], GO);
                done(red) && done(silver)
            })
            .expect("the trade did not finish in 10 seconds")
    }

    #[test]
    fn red_and_silver_trade_over_a_local_cable_and_keep_it_in_their_saves() {
        let (mut red, mut silver) = (red(), silver());
        assert_eq!(red.model, GbModel::Dmg);
        assert_eq!(silver.model, GbModel::Cgb);
        for _ in 0..30 {
            red.run_frame();
        }
        let mut cable = LocalCable::new();
        cable.set_plugged(true);
        trade(&mut red, &mut silver, &mut cable);

        assert!(red.link_plugged() && silver.link_plugged());
        assert_eq!(saved(&mut red), sent(0x40, 3));
        assert_eq!(saved(&mut silver), sent(0x10, 1));

        cable.unplug([&mut red, &mut silver]);
        assert!(!cable.plugged());
        assert!(!red.link_plugged() && !silver.link_plugged());
    }

    #[test]
    fn without_the_cable_the_master_reads_ff_and_the_other_game_keeps_waiting() {
        let (mut red, mut silver) = (red(), silver());
        let mut cable = LocalCable::new();
        for _ in 0..120 {
            cable.run_frame([&mut red, &mut silver], GO);
        }
        assert!(!red.link_plugged());
        assert!(done(&red) && !done(&silver));
        assert_eq!(saved(&mut red), vec![0xFF; COUNT as usize]);
    }

    #[test]
    fn a_cable_plugged_in_after_both_games_ran_alone_still_trades() {
        let (mut red, mut silver) = (red(), silver());
        let mut cable = LocalCable::new();
        for _ in 0..120 {
            cable.run_frame([&mut red, &mut silver], IDLE);
        }
        assert!(!done(&red) && !done(&silver));
        cable.set_plugged(true);
        trade(&mut red, &mut silver, &mut cable);
        assert_eq!(saved(&mut red), sent(0x40, 3));
        assert_eq!(saved(&mut silver), sent(0x10, 1));
    }

    #[test]
    fn buttons_reach_each_console_on_its_own() {
        let (mut red, mut silver) = (red(), silver());
        let mut cable = LocalCable::new();
        cable.run_frame([&mut red, &mut silver], [Keys::A, Keys::START | Keys::LEFT]);
        assert_eq!(lines(&mut red), [0x0E, 0x0F]);
        assert_eq!(lines(&mut silver), [0x07, 0x0D]);
    }
}
