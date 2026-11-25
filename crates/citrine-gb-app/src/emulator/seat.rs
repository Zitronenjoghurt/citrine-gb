use super::gimmicks::Cart;
use super::runner::{Core, Plan, Ran};
use super::tape_play::{TapePlay, TapeSpot};
use citrine_gb::console::{Console, ConsoleKind, Timing, VideoSpec};
use citrine_gb::debug::Debug;
use citrine_gb::gb::GbModel;
use citrine_gb::machine::Machine;
use citrine_gb_threads::{Lendable, Lent, Split};
#[cfg(threaded)]
use {citrine_gb_threads::Threads, std::time::Duration};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Facts {
    pub(crate) has_game: bool,
    pub(crate) kind: ConsoleKind,
    pub(crate) model: Option<GbModel>,
    pub(crate) video: VideoSpec,
    pub(crate) timing: Timing,
    pub(crate) cart: Cart,
    pub(crate) step_pending: bool,
    pub(crate) linked: bool,
    pub(crate) partnered: bool,
    #[cfg(feature = "nds")]
    pub(crate) walker: bool,
    #[cfg(ds_wifi)]
    pub(crate) wifi: bool,
    pub(crate) sample_rate: u32,
    pub(crate) channels: &'static [&'static str],
    pub(crate) mic: bool,
    #[cfg(feature = "nds")]
    pub(crate) mic_line: bool,
    pub(crate) tape: Option<TapeSpot>,
    pub(crate) recording_frames: Option<u64>,
}

impl Facts {
    fn of(core: &Core) -> Self {
        let machine = &core.machine;
        Self {
            has_game: machine.as_gb().is_none_or(|gb| gb.cartridge.has_rom_loaded),
            kind: machine.kind(),
            model: machine.as_gb().map(|gb| gb.model),
            video: machine.video(),
            timing: machine.timing(),
            cart: Cart::of(machine),
            step_pending: machine.step_pending(),
            linked: core.link.is_some(),
            partnered: core.partner.is_some(),
            #[cfg(feature = "nds")]
            walker: core.walker.is_some(),
            #[cfg(ds_wifi)]
            wifi: core.wifi.is_some(),
            sample_rate: machine.sample_rate(),
            channels: machine.audio_channels(),
            mic: machine.mic_in_use(),
            #[cfg(feature = "nds")]
            mic_line: core.mic.is_some(),
            tape: core.tape_play.as_ref().map(TapePlay::spot),
            recording_frames: core.rewind.recording_frames(),
        }
    }
}

pub(crate) struct Seat {
    lent: Lent<Core>,
    facts: Facts,
    #[cfg(threaded)]
    parked: bool,
}

impl Lendable for Core {
    type Plan = Plan;

    fn work(&mut self, plan: Plan, split: &mut Split<'_>) {
        let ran = self.step(&plan, split);
        self.ran = Some(Ran {
            finished: Some(web_time::Instant::now()),
            ..ran
        });
    }
}

#[cfg(threaded)]
const _: fn() = || {
    fn send<T: Send>() {}
    send::<Core>();
};

impl Default for Seat {
    fn default() -> Self {
        Self::new(Core::new(Machine::Gb(Box::new(
            citrine_gb::gb::GameBoy::new_empty(GbModel::Dmg),
        ))))
    }
}

impl Seat {
    pub(crate) fn new(core: Core) -> Self {
        let facts = Facts::of(&core);
        Self {
            lent: Lent::new(Box::new(core)),
            facts,
            #[cfg(threaded)]
            parked: false,
        }
    }

    pub(crate) fn core(&self) -> &Core {
        self.lent.get()
    }

    pub(crate) fn core_mut(&mut self) -> &mut Core {
        self.lent.get_mut()
    }

    pub(crate) fn is_home(&self) -> bool {
        self.lent.is_home()
    }

    pub(crate) fn facts(&self) -> Facts {
        self.lent.home().map_or(self.facts, Facts::of)
    }

    pub(crate) fn replace(&mut self, core: Core) {
        *self.lent.get_mut() = core;
    }

    #[cfg(threaded)]
    pub(crate) fn run_on(&mut self, threads: &Threads) {
        self.lent.run_on(threads, "console");
    }

    #[cfg(threaded)]
    pub(crate) fn follow_visibility(&mut self, visible: bool, threads: &Threads) {
        if !visible && self.lent.has_worker() {
            self.lent.park();
            self.parked = true;
        } else if visible && self.parked {
            self.parked = false;
            self.run_on(threads);
        }
    }

    pub(crate) fn can_lend(&self) -> bool {
        self.lent.has_started()
    }

    #[cfg(any(test, all(feature = "dev-route", target_arch = "wasm32")))]
    pub(crate) fn has_worker(&self) -> bool {
        self.lent.has_worker()
    }

    pub(crate) fn lend(&mut self, plan: Plan) {
        if !self.lent.has_worker() {
            return;
        }
        self.facts = Facts::of(self.lent.get());
        self.lent.lend(plan);
    }

    #[cfg(threaded)]
    pub(crate) fn wait(&self, timeout: Duration) -> bool {
        self.lent.wait(timeout)
    }
}
