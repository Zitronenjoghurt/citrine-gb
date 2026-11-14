use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScreenShape {
    pub width: u16,
    pub height: u16,
    pub touch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum View {
    Screen(u8),
    Controller,
    GameInfo,
    Pane(String),
}

impl View {
    pub fn travels(&self) -> bool {
        !matches!(self, Self::Pane(_))
    }

    pub fn fits(&self, screens: &[ScreenShape]) -> bool {
        match self {
            Self::Screen(index) => usize::from(*index) < screens.len(),
            Self::Controller | Self::GameInfo | Self::Pane(_) => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Preset {
    BottomScreen,
    Controller,
    GameInfo,
}

impl Preset {
    pub const ALL: [Self; 3] = [Self::BottomScreen, Self::Controller, Self::GameInfo];

    pub fn label(self) -> &'static str {
        match self {
            Self::BottomScreen => "Bottom screen",
            Self::Controller => "Controller",
            Self::GameInfo => "Game info",
        }
    }

    pub fn offered(self, screens: &[ScreenShape]) -> bool {
        match self {
            Self::BottomScreen => touch_screen(screens).is_some(),
            Self::Controller | Self::GameInfo => true,
        }
    }

    fn views(self, screens: &[ScreenShape]) -> Vec<View> {
        let touch = touch_screen(screens).map(View::Screen);
        match self {
            Self::BottomScreen => touch.into_iter().collect(),
            Self::Controller => touch.into_iter().chain([View::Controller]).collect(),
            Self::GameInfo => vec![View::GameInfo],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Mode {
    Preset(Preset),
    Custom(Vec<View>),
}

impl Default for Mode {
    fn default() -> Self {
        Self::Preset(Preset::Controller)
    }
}

impl Mode {
    pub fn default_for(screens: &[ScreenShape]) -> Self {
        if Preset::BottomScreen.offered(screens) {
            Self::Preset(Preset::BottomScreen)
        } else {
            Self::Preset(Preset::Controller)
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Preset(preset) => preset.label(),
            Self::Custom(_) => "Custom",
        }
    }

    pub fn views(&self, screens: &[ScreenShape]) -> Vec<View> {
        match self {
            Self::Preset(preset) if preset.offered(screens) => preset.views(screens),
            Self::Preset(_) => Self::default_for(screens).views(screens),
            Self::Custom(views) => views.iter().filter(|v| v.fits(screens)).cloned().collect(),
        }
    }

    pub fn shows_screen(&self, screens: &[ScreenShape], index: u8) -> bool {
        self.views(screens).contains(&View::Screen(index))
    }

    pub fn toggled(&self, screens: &[ScreenShape], view: View) -> Self {
        let mut views = self.views(screens);
        if let Some(at) = views.iter().position(|v| *v == view) {
            views.remove(at);
        } else {
            views.push(view);
            views.sort_by_key(order);
        }
        Self::Custom(views)
    }
}

fn order(view: &View) -> (u8, u8) {
    match view {
        View::Screen(index) => (0, *index),
        View::GameInfo => (1, 0),
        View::Controller => (2, 0),
        View::Pane(_) => (3, 0),
    }
}

pub fn touch_screen(screens: &[ScreenShape]) -> Option<u8> {
    screens
        .iter()
        .position(|s| s.touch)
        .and_then(|at| u8::try_from(at).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DS: [ScreenShape; 2] = [
        ScreenShape {
            width: 256,
            height: 192,
            touch: false,
        },
        ScreenShape {
            width: 256,
            height: 192,
            touch: true,
        },
    ];
    const GB: [ScreenShape; 1] = [ScreenShape {
        width: 160,
        height: 144,
        touch: false,
    }];

    #[test]
    fn a_ds_game_starts_on_the_bottom_screen() {
        assert_eq!(Mode::default_for(&DS), Mode::Preset(Preset::BottomScreen));
        assert_eq!(Mode::default_for(&DS).views(&DS), vec![View::Screen(1)]);
    }

    #[test]
    fn a_game_without_touch_starts_as_a_controller() {
        assert_eq!(Mode::default_for(&GB), Mode::Preset(Preset::Controller));
        assert!(!Preset::BottomScreen.offered(&GB));
        assert_eq!(
            Mode::Preset(Preset::BottomScreen).views(&GB),
            vec![View::Controller]
        );
    }

    #[test]
    fn the_ds_controller_keeps_the_touch_screen() {
        assert_eq!(
            Mode::Preset(Preset::Controller).views(&DS),
            vec![View::Screen(1), View::Controller]
        );
        assert_eq!(
            Mode::Preset(Preset::Controller).views(&GB),
            vec![View::Controller]
        );
    }

    #[test]
    fn custom_drops_screens_the_game_lacks() {
        let mode = Mode::Custom(vec![View::Screen(0), View::Screen(1), View::GameInfo]);
        assert_eq!(mode.views(&GB), vec![View::Screen(0), View::GameInfo]);
        assert!(mode.shows_screen(&DS, 1));
        assert!(!mode.shows_screen(&GB, 1));
    }

    #[test]
    fn toggling_a_preset_makes_it_custom_in_a_fixed_order() {
        let mode = Mode::Preset(Preset::BottomScreen).toggled(&DS, View::Controller);
        assert_eq!(mode, Mode::Custom(vec![View::Screen(1), View::Controller]));
        let mode = mode.toggled(&DS, View::Screen(0));
        assert_eq!(
            mode,
            Mode::Custom(vec![View::Screen(0), View::Screen(1), View::Controller])
        );
        let mode = mode.toggled(&DS, View::Screen(1));
        assert_eq!(mode, Mode::Custom(vec![View::Screen(0), View::Controller]));
    }

    #[test]
    fn panes_never_travel() {
        assert!(!View::Pane("Saves".into()).travels());
        assert!(View::Screen(1).travels());
    }
}
