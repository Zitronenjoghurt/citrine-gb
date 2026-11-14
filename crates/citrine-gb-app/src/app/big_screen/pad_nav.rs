use crate::app::ui_state::keybinds::PadButton;
use crate::pad_input::{PadAxis, PadInput, PadKey};
use web_time::{Duration, Instant};

pub const MENU_HOLD: Duration = Duration::from_secs(1);
pub const HOLD_SHOWN: Duration = Duration::from_millis(300);
const REPEAT_DELAY: Duration = Duration::from_millis(400);
const REPEAT_EVERY: Duration = Duration::from_millis(110);
const STICK_ON: f32 = 0.6;
const STICK_OFF: f32 = 0.35;
const SCROLL_DEAD: f32 = 0.2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Move(Dir),
    Confirm,
    Back,
    Menu,
    PagePrevious,
    PageNext,
    FaceWest,
    FaceNorth,
    TriggerLeft,
    TriggerRight,
    Start,
}

#[derive(Debug, Default, PartialEq)]
pub struct Fed {
    pub commands: Vec<Command>,
    pub to_game: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Buttons,
    StickX,
    StickY,
    HatX,
    HatY,
}

#[derive(Debug, Default)]
pub struct PadNav {
    select: bool,
    start: bool,
    both_since: Option<Instant>,
    fired: bool,
    held: Option<(Dir, Source, Instant)>,
    scroll: f32,
}

impl PadNav {
    pub fn feed(
        &mut self,
        input: PadInput,
        playing: bool,
        confirm_east: bool,
        now: Instant,
    ) -> Fed {
        let commands = self.watch_combo(input, now).into_iter().collect::<Vec<_>>();
        if playing {
            self.held = None;
            self.scroll = 0.0;
            let menu = matches!(input, PadInput::Pressed(PadKey::Known(PadButton::Mode)));
            let mut commands = commands;
            if menu {
                commands.push(Command::Menu);
            }
            return Fed {
                commands,
                to_game: !menu,
            };
        }
        let mut commands = commands;
        commands.extend(self.ui_command(input, confirm_east, now));
        Fed {
            commands,
            to_game: false,
        }
    }

    pub fn tick(&mut self, now: Instant) -> Vec<Command> {
        let mut commands = Vec::new();
        if let Some(since) = self.both_since
            && !self.fired
            && now.duration_since(since) >= MENU_HOLD
        {
            self.fired = true;
            commands.push(Command::Menu);
        }
        if let Some((dir, source, next)) = self.held
            && now >= next
        {
            self.held = Some((dir, source, next + REPEAT_EVERY));
            commands.push(Command::Move(dir));
        }
        commands
    }

    pub fn hold_progress(&self, now: Instant) -> Option<f32> {
        let held = now.duration_since(self.both_since?);
        if self.fired || held < HOLD_SHOWN {
            return None;
        }
        Some(((held - HOLD_SHOWN).as_secs_f32() / (MENU_HOLD - HOLD_SHOWN).as_secs_f32()).min(1.0))
    }

    pub fn scroll_speed(&self) -> f32 {
        self.scroll
    }

    pub fn holding_combo(&self) -> bool {
        self.both_since.is_some()
    }

    fn watch_combo(&mut self, input: PadInput, now: Instant) -> Option<Command> {
        let (button, down) = match input {
            PadInput::Pressed(PadKey::Known(button)) => (button, true),
            PadInput::Released(PadKey::Known(button)) => (button, false),
            PadInput::Disconnected | PadInput::Dropped => {
                self.select = false;
                self.start = false;
                self.both_since = None;
                self.fired = false;
                return None;
            }
            _ => return None,
        };
        match button {
            PadButton::Select => self.select = down,
            PadButton::Start => self.start = down,
            _ => return None,
        }
        if self.select && self.start {
            self.both_since.get_or_insert(now);
        } else {
            self.both_since = None;
            self.fired = false;
        }
        None
    }

    fn ui_command(&mut self, input: PadInput, confirm_east: bool, now: Instant) -> Option<Command> {
        match input {
            PadInput::Pressed(PadKey::Known(button)) => {
                if let Some(dir) = dpad(button) {
                    return self.press(dir, Source::Buttons, now);
                }
                self.button(button, confirm_east)
            }
            PadInput::Pressed(PadKey::Other("DPadCenter")) => Some(Command::Confirm),
            PadInput::Released(PadKey::Known(button)) => {
                if let Some(dir) = dpad(button) {
                    self.release(Some(dir), Source::Buttons);
                }
                None
            }
            PadInput::Moved(axis, value) => self.axis(axis, value, now),
            PadInput::Disconnected | PadInput::Dropped => {
                self.held = None;
                self.scroll = 0.0;
                None
            }
            _ => None,
        }
    }

    fn button(&self, button: PadButton, confirm_east: bool) -> Option<Command> {
        let (confirm, back) = if confirm_east {
            (PadButton::East, PadButton::South)
        } else {
            (PadButton::South, PadButton::East)
        };
        match button {
            _ if button == confirm => Some(Command::Confirm),
            _ if button == back => Some(Command::Back),
            PadButton::West => Some(Command::FaceWest),
            PadButton::North => Some(Command::FaceNorth),
            PadButton::LeftTrigger => Some(Command::PagePrevious),
            PadButton::RightTrigger => Some(Command::PageNext),
            PadButton::LeftTrigger2 => Some(Command::TriggerLeft),
            PadButton::RightTrigger2 => Some(Command::TriggerRight),
            PadButton::Start if !self.select => Some(Command::Start),
            PadButton::Mode => Some(Command::Menu),
            _ => None,
        }
    }

    fn axis(&mut self, axis: PadAxis, value: f32, now: Instant) -> Option<Command> {
        let (source, negative, positive) = match axis {
            PadAxis::LeftStickX => (Source::StickX, Dir::Left, Dir::Right),
            PadAxis::LeftStickY => (Source::StickY, Dir::Down, Dir::Up),
            PadAxis::DPadX => (Source::HatX, Dir::Left, Dir::Right),
            PadAxis::DPadY => (Source::HatY, Dir::Down, Dir::Up),
            PadAxis::RightStickY => {
                self.scroll = if value.abs() < SCROLL_DEAD {
                    0.0
                } else {
                    (value.abs() - SCROLL_DEAD) / (1.0 - SCROLL_DEAD) * -value.signum()
                };
                return None;
            }
            _ => return None,
        };
        let current = self
            .held
            .filter(|(_, held, _)| *held == source)
            .map(|(dir, ..)| dir);
        let wanted = if value <= -STICK_ON {
            Some(negative)
        } else if value >= STICK_ON {
            Some(positive)
        } else if value.abs() < STICK_OFF {
            None
        } else {
            current
        };
        if wanted == current {
            return None;
        }
        match wanted {
            Some(dir) => self.press(dir, source, now),
            None => {
                self.release(current, source);
                None
            }
        }
    }

    fn press(&mut self, dir: Dir, source: Source, now: Instant) -> Option<Command> {
        self.held = Some((dir, source, now + REPEAT_DELAY));
        Some(Command::Move(dir))
    }

    fn release(&mut self, dir: Option<Dir>, source: Source) {
        if self
            .held
            .is_some_and(|(held, from, _)| from == source && Some(held) == dir)
        {
            self.held = None;
        }
    }
}

fn dpad(button: PadButton) -> Option<Dir> {
    match button {
        PadButton::DPadUp => Some(Dir::Up),
        PadButton::DPadDown => Some(Dir::Down),
        PadButton::DPadLeft => Some(Dir::Left),
        PadButton::DPadRight => Some(Dir::Right),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(button: PadButton) -> PadInput {
        PadInput::Pressed(PadKey::Known(button))
    }

    fn release(button: PadButton) -> PadInput {
        PadInput::Released(PadKey::Known(button))
    }

    #[test]
    fn the_d_pad_moves_focus_and_repeats_while_held() {
        let mut nav = PadNav::default();
        let start = Instant::now();
        let fed = nav.feed(press(PadButton::DPadDown), false, false, start);
        assert_eq!(fed.commands, vec![Command::Move(Dir::Down)]);
        assert!(!fed.to_game);
        assert!(nav.tick(start + Duration::from_millis(200)).is_empty());
        assert_eq!(
            nav.tick(start + REPEAT_DELAY),
            vec![Command::Move(Dir::Down)]
        );
        nav.feed(
            release(PadButton::DPadDown),
            false,
            false,
            start + REPEAT_DELAY,
        );
        assert!(nav.tick(start + Duration::from_secs(2)).is_empty());
    }

    #[test]
    fn the_stick_moves_once_past_its_edge_and_again_after_coming_back() {
        let mut nav = PadNav::default();
        let now = Instant::now();
        let moved = |nav: &mut PadNav, value| {
            nav.feed(
                PadInput::Moved(PadAxis::LeftStickX, value),
                false,
                false,
                now,
            )
            .commands
        };
        assert_eq!(moved(&mut nav, 0.9), vec![Command::Move(Dir::Right)]);
        assert!(moved(&mut nav, 0.5).is_empty());
        assert!(moved(&mut nav, 0.1).is_empty());
        assert_eq!(moved(&mut nav, -0.8), vec![Command::Move(Dir::Left)]);
    }

    #[test]
    fn confirm_follows_the_pad_family() {
        let mut nav = PadNav::default();
        let now = Instant::now();
        let south =
            |nav: &mut PadNav, east| nav.feed(press(PadButton::South), false, east, now).commands;
        let east = |nav: &mut PadNav, swapped| {
            nav.feed(press(PadButton::East), false, swapped, now)
                .commands
        };
        assert_eq!(south(&mut nav, false), vec![Command::Confirm]);
        assert_eq!(east(&mut nav, false), vec![Command::Back]);
        assert_eq!(south(&mut nav, true), vec![Command::Back]);
        assert_eq!(east(&mut nav, true), vec![Command::Confirm]);
    }

    #[test]
    fn select_and_start_held_for_a_second_open_the_menu_once() {
        let mut nav = PadNav::default();
        let start = Instant::now();
        assert!(
            nav.feed(press(PadButton::Select), true, false, start)
                .to_game
        );
        assert!(
            nav.feed(press(PadButton::Start), true, false, start)
                .to_game
        );
        assert!(nav.tick(start + Duration::from_millis(900)).is_empty());
        assert!(
            nav.hold_progress(start + Duration::from_millis(900))
                .is_some()
        );
        assert_eq!(nav.tick(start + MENU_HOLD), vec![Command::Menu]);
        assert!(nav.tick(start + Duration::from_secs(3)).is_empty());
        nav.feed(release(PadButton::Start), false, false, start);
        nav.feed(press(PadButton::Start), false, false, start + MENU_HOLD);
        assert_eq!(nav.tick(start + MENU_HOLD * 2), vec![Command::Menu]);
    }

    #[test]
    fn a_quick_select_and_start_still_reaches_the_game() {
        let mut nav = PadNav::default();
        let start = Instant::now();
        nav.feed(press(PadButton::Select), true, false, start);
        nav.feed(press(PadButton::Start), true, false, start);
        assert!(
            nav.hold_progress(start + Duration::from_millis(100))
                .is_none()
        );
        nav.feed(
            release(PadButton::Start),
            true,
            false,
            start + Duration::from_millis(200),
        );
        assert!(nav.tick(start + Duration::from_secs(2)).is_empty());
    }

    #[test]
    fn guide_opens_the_menu_during_play_and_never_reaches_the_game() {
        let mut nav = PadNav::default();
        let fed = nav.feed(press(PadButton::Mode), true, false, Instant::now());
        assert_eq!(fed.commands, vec![Command::Menu]);
        assert!(!fed.to_game);
        let fed = nav.feed(press(PadButton::South), true, false, Instant::now());
        assert!(fed.commands.is_empty());
        assert!(fed.to_game);
    }

    #[test]
    fn the_remote_centre_button_confirms() {
        let mut nav = PadNav::default();
        let fed = nav.feed(
            PadInput::Pressed(PadKey::Other("DPadCenter")),
            false,
            false,
            Instant::now(),
        );
        assert_eq!(fed.commands, vec![Command::Confirm]);
    }
}
