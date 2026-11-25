use crate::icons;
use strum_macros::EnumIter;

pub const MAX_GAP: u8 = 96;

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum Arrangement {
    #[default]
    Stacked,
    SideBySide,
    BigAndSmall,
}

impl Arrangement {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stacked => "Stacked",
            Self::SideBySide => "Side by side",
            Self::BigAndSmall => "Big and small",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Stacked => icons::SQUARE_SPLIT_VERTICAL,
            Self::SideBySide => icons::SQUARE_SPLIT_HORIZONTAL,
            Self::BigAndSmall => icons::PICTURE_IN_PICTURE,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Stacked => Self::SideBySide,
            Self::SideBySide => Self::BigAndSmall,
            Self::BigAndSmall => Self::Stacked,
        }
    }
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

impl Corner {
    pub fn label(self) -> &'static str {
        match self {
            Self::TopLeft => "Top left",
            Self::TopRight => "Top right",
            Self::BottomLeft => "Bottom left",
            Self::BottomRight => "Bottom right",
        }
    }

    pub fn left(self) -> bool {
        matches!(self, Self::TopLeft | Self::BottomLeft)
    }

    pub fn top(self) -> bool {
        matches!(self, Self::TopLeft | Self::TopRight)
    }
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum SmallSize {
    Quarter,
    #[default]
    Third,
    Half,
}

impl SmallSize {
    pub fn label(self) -> &'static str {
        match self {
            Self::Quarter => "A quarter as wide",
            Self::Third => "A third as wide",
            Self::Half => "Half as wide",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Self::Quarter => "A quarter",
            Self::Third => "A third",
            Self::Half => "Half",
        }
    }

    pub fn share(self) -> f32 {
        match self {
            Self::Quarter => 0.25,
            Self::Third => 1.0 / 3.0,
            Self::Half => 0.5,
        }
    }
}

#[derive(
    Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, EnumIter,
)]
pub enum Rotation {
    #[default]
    Upright,
    Right,
    UpsideDown,
    Left,
}

impl Rotation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Upright => "Upright",
            Self::Right => "Turned right",
            Self::UpsideDown => "Upside down",
            Self::Left => "Turned left",
        }
    }

    pub fn quarter_turns(self) -> u8 {
        match self {
            Self::Upright => 0,
            Self::Right => 1,
            Self::UpsideDown => 2,
            Self::Left => 3,
        }
    }
}

macro_rules! shown_by_label {
    ($($kind:ty),*) => {
        $(impl std::fmt::Display for $kind {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.label())
            }
        })*
    };
}

shown_by_label!(Arrangement, Corner, SmallSize, Rotation);

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Placement {
    #[serde(deserialize_with = "known_arrangement")]
    pub arrangement: Arrangement,
    pub swapped: bool,
    pub one_screen: bool,
}

fn known_arrangement<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Arrangement, D::Error> {
    let value = <serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).unwrap_or_default())
}

impl Placement {
    pub fn chip(self) -> (&'static str, &'static str) {
        match (self.one_screen, self.swapped) {
            (true, false) => (icons::DS_TOP_ONLY, "Top only"),
            (true, true) => (icons::DS_BOTTOM_ONLY, "Bottom only"),
            _ => (self.arrangement.icon(), self.arrangement.label()),
        }
    }

    pub fn with_away(self, away: Option<usize>) -> Self {
        match away {
            Some(away) => Self {
                one_screen: true,
                swapped: away == 0,
                ..self
            },
            None => self,
        }
    }

    pub fn touch_hidden(self) -> bool {
        self.one_screen && !self.swapped
    }

    pub fn act(self, action: ScreensAction) -> Self {
        match action {
            ScreensAction::Arrange(arrangement) => Self {
                arrangement,
                one_screen: false,
                ..self
            },
            ScreensAction::Swap => Self {
                swapped: !self.swapped,
                ..self
            },
            ScreensAction::OneScreen => Self {
                one_screen: !self.one_screen,
                ..self
            },
            ScreensAction::NextLayout => Self {
                arrangement: self.arrangement.next(),
                one_screen: false,
                ..self
            },
            ScreensAction::SeparateTabs => self,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreensAction {
    Arrange(Arrangement),
    Swap,
    OneScreen,
    NextLayout,
    SeparateTabs,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ScreenContext {
    #[default]
    Desktop,
    Portrait,
    Landscape,
}

impl ScreenContext {
    pub fn on_phone(self) -> Self {
        match self {
            Self::Desktop => Self::Portrait,
            other => other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShownRows {
    pub separate_tabs: bool,
    pub small: bool,
    pub gap: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct DsScreens {
    pub desktop: Placement,
    pub portrait: Placement,
    pub landscape: Placement,
    pub corner: Corner,
    pub small: SmallSize,
    pub gap: u8,
    pub rotation: Rotation,
    pub separate_tabs: bool,
}

impl Default for DsScreens {
    fn default() -> Self {
        Self {
            desktop: Placement::default(),
            portrait: Placement::default(),
            landscape: Placement {
                arrangement: Arrangement::SideBySide,
                ..Placement::default()
            },
            corner: Corner::default(),
            small: SmallSize::default(),
            gap: 0,
            rotation: Rotation::default(),
            separate_tabs: false,
        }
    }
}

impl DsScreens {
    pub fn placement(&self, context: ScreenContext) -> Placement {
        match context {
            ScreenContext::Desktop => self.desktop,
            ScreenContext::Portrait => self.portrait,
            ScreenContext::Landscape => self.landscape,
        }
    }

    pub fn placement_mut(&mut self, context: ScreenContext) -> &mut Placement {
        match context {
            ScreenContext::Desktop => &mut self.desktop,
            ScreenContext::Portrait => &mut self.portrait,
            ScreenContext::Landscape => &mut self.landscape,
        }
    }

    pub fn apply(&mut self, context: ScreenContext, action: ScreensAction) {
        match action {
            ScreensAction::SeparateTabs => self.separate_tabs = !self.separate_tabs,
            _ => {
                let placement = self.placement_mut(context);
                *placement = placement.act(action);
            }
        }
    }

    pub fn with_placements_of(self, shown: &DsScreens) -> Self {
        Self {
            desktop: shown.desktop,
            portrait: shown.portrait,
            landscape: shown.landscape,
            ..self
        }
    }

    pub fn tabs_shown(&self, context: ScreenContext) -> bool {
        self.separate_tabs && context == ScreenContext::Desktop
    }

    pub fn shown_rows(&self, context: ScreenContext) -> ShownRows {
        self.shown_rows_with(self.placement(context), context)
    }

    pub fn shown_rows_with(&self, placement: Placement, context: ScreenContext) -> ShownRows {
        let tabs = self.tabs_shown(context);
        let together = !tabs && !placement.one_screen;
        ShownRows {
            separate_tabs: context == ScreenContext::Desktop,
            small: together && placement.arrangement == Arrangement::BigAndSmall,
            gap: together,
        }
    }

    pub fn gap_label(&self) -> String {
        if self.gap == 0 {
            "None".to_owned()
        } else {
            self.gap.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    #[test]
    fn settings_hide_the_rows_that_do_not_apply() {
        let mut screens = DsScreens::default();
        let desktop = ScreenContext::Desktop;
        let rows = screens.shown_rows(desktop);
        assert!(rows.separate_tabs && rows.gap && !rows.small);
        screens.desktop.arrangement = Arrangement::BigAndSmall;
        assert!(screens.shown_rows(desktop).small);
        screens.desktop.one_screen = true;
        let rows = screens.shown_rows(desktop);
        assert!(!rows.small && !rows.gap);
        screens.desktop.one_screen = false;
        screens.separate_tabs = true;
        let rows = screens.shown_rows(desktop);
        assert!(!rows.small && !rows.gap && rows.separate_tabs);
        let phone = screens.shown_rows(ScreenContext::Portrait);
        assert!(phone.gap && !phone.separate_tabs);
    }

    #[test]
    fn next_layout_visits_every_layout_once() {
        let mut seen = vec![Arrangement::Stacked];
        let mut at = Arrangement::Stacked.next();
        while at != Arrangement::Stacked {
            seen.push(at);
            at = at.next();
        }
        assert_eq!(seen, Arrangement::iter().collect::<Vec<_>>());
    }

    #[test]
    fn old_settings_without_ds_screens_load_the_defaults() {
        let screens: DsScreens = serde_json::from_str("{}").unwrap();
        assert_eq!(screens, DsScreens::default());
        assert_eq!(screens.landscape.arrangement, Arrangement::SideBySide);
        assert_eq!(screens.portrait.arrangement, Arrangement::Stacked);
    }

    #[test]
    fn each_context_keeps_its_own_placement() {
        let mut screens = DsScreens::default();
        screens.placement_mut(ScreenContext::Portrait).arrangement = Arrangement::BigAndSmall;
        screens.placement_mut(ScreenContext::Portrait).swapped = true;
        assert_eq!(
            screens.placement(ScreenContext::Desktop),
            Placement::default()
        );
        assert!(screens.placement(ScreenContext::Portrait).swapped);
    }

    #[test]
    fn one_screen_chips_name_the_screen_they_keep() {
        let top = Placement {
            one_screen: true,
            ..Placement::default()
        };
        let bottom = Placement {
            swapped: true,
            ..top
        };
        assert_eq!(top.chip().1, "Top only");
        assert_eq!(bottom.chip().1, "Bottom only");
        assert!(top.touch_hidden() && !bottom.touch_hidden());
    }

    #[test]
    fn the_chip_reads_top_only_while_the_bottom_screen_is_lent() {
        let placement = Placement {
            arrangement: Arrangement::SideBySide,
            ..Placement::default()
        };
        assert_eq!(placement.with_away(Some(1)).chip().1, "Top only");
        assert_eq!(placement.with_away(Some(0)).chip().1, "Bottom only");
        assert_eq!(placement.with_away(None).chip().1, "Side by side");
        let swapped = Placement {
            swapped: true,
            ..placement
        };
        assert_eq!(swapped.with_away(Some(1)).chip().1, "Top only");
        assert_eq!(swapped.with_away(None), swapped);
    }

    #[test]
    fn picking_a_layout_shows_both_screens_again() {
        let mut screens = DsScreens::default();
        let context = ScreenContext::Desktop;
        screens.apply(context, ScreensAction::OneScreen);
        assert!(screens.desktop.one_screen);
        screens.apply(context, ScreensAction::Arrange(Arrangement::SideBySide));
        assert_eq!(screens.desktop.arrangement, Arrangement::SideBySide);
        assert!(!screens.desktop.one_screen);
        screens.apply(context, ScreensAction::OneScreen);
        screens.apply(context, ScreensAction::NextLayout);
        assert_eq!(screens.desktop.arrangement, Arrangement::BigAndSmall);
        assert!(!screens.desktop.one_screen);
    }

    #[test]
    fn every_switch_turns_back_off_where_it_turned_on() {
        let context = ScreenContext::Landscape;
        for action in [
            ScreensAction::Swap,
            ScreensAction::OneScreen,
            ScreensAction::SeparateTabs,
        ] {
            let mut screens = DsScreens::default();
            screens.apply(context, action);
            assert_ne!(screens, DsScreens::default(), "{action:?}");
            screens.apply(context, action);
            assert_eq!(screens, DsScreens::default(), "{action:?}");
        }
    }

    #[test]
    fn separate_tabs_only_apply_on_desktop() {
        let screens = DsScreens {
            separate_tabs: true,
            ..DsScreens::default()
        };
        assert!(screens.tabs_shown(ScreenContext::Desktop));
        assert!(!screens.tabs_shown(ScreenContext::Portrait));
        assert!(!screens.tabs_shown(ScreenContext::Landscape));
    }
}
