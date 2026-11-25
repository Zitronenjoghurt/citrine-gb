use crate::app::ui_state::ds_screens::ScreenContext;
use citrine_gb::console::ConsoleKind;
use egui::{Rect, Vec2, vec2};
use std::collections::BTreeMap;
use strum_macros::EnumIter;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum LayoutConsole {
    GameBoy,
    Gba,
    Ds,
}

impl LayoutConsole {
    pub fn of(kind: ConsoleKind) -> Self {
        match kind {
            ConsoleKind::Gb | ConsoleKind::Gbc => Self::GameBoy,
            ConsoleKind::Gba => Self::Gba,
            ConsoleKind::Nds => Self::Ds,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub enum LayoutView {
    Portrait,
    Landscape,
    Desktop,
    BigScreen,
}

impl LayoutView {
    pub fn of(context: ScreenContext) -> Self {
        match context {
            ScreenContext::Desktop => Self::Desktop,
            ScreenContext::Portrait => Self::Portrait,
            ScreenContext::Landscape => Self::Landscape,
        }
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    EnumIter,
)]
pub enum Item {
    Screen,
    TouchScreen,
    Dpad,
    Face,
    L,
    R,
    Select,
    Start,
    Talk,
    MicMark,
}

impl Item {
    pub fn is_screen(self) -> bool {
        matches!(self, Self::Screen | Self::TouchScreen)
    }

    pub fn smallest(self, default: Vec2) -> f32 {
        if self.is_screen() {
            MIN_SCREEN_WIDTH.min(default.x)
        } else {
            default.x * MIN_GROUP_SCALE
        }
    }

    pub fn largest(self, default: Vec2) -> f32 {
        if self.is_screen() {
            f32::INFINITY
        } else {
            default.x * MAX_GROUP_SCALE
        }
    }
}

const MIN_SCREEN_WIDTH: f32 = 96.0;
const MIN_GROUP_SCALE: f32 = 0.6;
const MAX_GROUP_SCALE: f32 = 2.0;

#[derive(Debug, Default, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Spot {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    #[serde(default)]
    pub room: [f32; 2],
}

impl Spot {
    pub fn of(rect: Rect, room: Rect) -> Self {
        let room_size = room.size().max(Vec2::splat(1.0));
        let centre = (rect.center() - room.min) / room_size;
        Self {
            x: centre.x,
            y: centre.y,
            width: rect.width() / room_size.x,
            room: [room_size.x, room_size.y],
        }
    }

    fn saved_room(self, now: Vec2) -> Vec2 {
        let [width, height] = self.room;
        if width >= 1.0 && height >= 1.0 {
            vec2(width, height)
        } else {
            now
        }
    }

    fn in_room(self, room: Rect, default: Vec2) -> Rect {
        let width = self.width * room.width();
        let aspect = if default.x > 0.0 {
            default.y / default.x
        } else {
            1.0
        };
        let centre = room.min + vec2(self.x, self.y) * room.size();
        Rect::from_center_size(centre, vec2(width, width * aspect))
    }

    pub fn rect(self, room: Rect, default: Vec2) -> Rect {
        let saved = self.saved_room(room.size());
        let was = self.in_room(Rect::from_min_size(egui::Pos2::ZERO, saved), default);
        let turned = (room.width() > room.height()) != (saved.x > saved.y);
        let grow = if turned {
            room.size().min_elem() / saved.min_elem().max(1.0)
        } else {
            (room.size() / saved.max(Vec2::splat(1.0))).min_elem()
        };
        let size = was.size() * grow;
        let x = anchored(was.x_range(), saved.x, room.x_range(), size.x);
        let y = anchored(was.y_range(), saved.y, room.y_range(), size.y);
        fit_inside(Rect::from_min_size(egui::pos2(x, y), size), room)
    }
}

fn anchored(was: egui::Rangef, saved: f32, now: egui::Rangef, size: f32) -> f32 {
    let grow = size / was.span().max(f32::EPSILON);
    let before = was.min;
    let after = saved - was.max;
    if was.center() < saved / 3.0 {
        now.min + before * grow
    } else if was.center() > saved * 2.0 / 3.0 {
        now.max - after * grow - size
    } else {
        now.min + was.center() / saved.max(1.0) * now.span() - size / 2.0
    }
}

pub fn place_screens(spots: &[(Spot, Vec2)], room: Rect) -> Vec<Rect> {
    let Some(&(first, _)) = spots.first() else {
        return Vec::new();
    };
    let saved = first.saved_room(room.size());
    let frame = Rect::from_min_size(egui::Pos2::ZERO, saved);
    let was: Vec<Rect> = spots
        .iter()
        .map(|&(spot, default)| spot.in_room(frame, default))
        .collect();
    let group = was.iter().copied().reduce(Rect::union).unwrap_or(frame);
    let stretch = room.size() / saved.max(Vec2::splat(1.0));
    let scale = stretch.min_elem();
    let span = |rect: Rect, axis: usize| {
        if axis == 0 {
            rect.x_range()
        } else {
            rect.y_range()
        }
    };
    let stacked = [0, 1].map(|axis| {
        was.len() > 1
            && was.iter().enumerate().all(|(i, a)| {
                was[i + 1..].iter().all(|b| {
                    let (a, b) = (span(*a, axis), span(*b, axis));
                    a.max <= b.min + 0.01 || b.max <= a.min + 0.01
                })
            })
    });
    let place = |axis: usize, rect: Rect, size: f32| {
        let (was, all) = (span(rect, axis), span(group, axis));
        if stacked[axis] {
            let free = all.span() - was.span();
            let share = if free > 0.5 {
                ((was.min - all.min) / free).clamp(0.0, 1.0)
            } else {
                0.5
            };
            room.min[axis] + was.min * stretch[axis] + share * (was.span() * stretch[axis] - size)
        } else {
            room.min[axis] + was.center() * stretch[axis] - size / 2.0
        }
    };
    was.iter()
        .map(|&rect| {
            let size = rect.size() * scale;
            let at = egui::pos2(place(0, rect, size.x), place(1, rect, size.y));
            fit_inside(Rect::from_min_size(at, size), room)
        })
        .collect()
}

pub fn fit_inside(rect: Rect, room: Rect) -> Rect {
    let shrink = (room.width() / rect.width())
        .min(room.height() / rect.height())
        .min(1.0);
    let size = rect.size() * if shrink.is_finite() { shrink } else { 1.0 };
    let centre = rect.center();
    let half = size / 2.0;
    let x = centre.x.clamp(
        room.left() + half.x,
        (room.right() - half.x).max(room.left() + half.x),
    );
    let y = centre.y.clamp(
        room.top() + half.y,
        (room.bottom() - half.y).max(room.top() + half.y),
    );
    Rect::from_center_size(egui::pos2(x, y), size)
}

pub type Spots = BTreeMap<Item, Spot>;

const BIG_TOP_ROOM: Vec2 = vec2(888.0, 400.0);
const BIG_TOP: [(Item, [f32; 4]); 8] = [
    (Item::Screen, [0.0, 0.0, 524.0, 393.0]),
    (Item::TouchScreen, [532.0, 0.0, 296.0, 222.0]),
    (Item::Dpad, [20.0, 216.0, 160.0, 168.0]),
    (Item::L, [20.0, 150.0, 122.0, 45.0]),
    (Item::R, [556.0, 232.0, 122.0, 45.0]),
    (Item::Select, [556.0, 290.0, 122.0, 45.0]),
    (Item::Start, [556.0, 345.0, 122.0, 45.0]),
    (Item::Face, [730.0, 242.0, 140.0, 140.0]),
];

const STACKED_ROOM: Vec2 = vec2(400.0, 888.0);
const STACKED: [(Item, [f32; 4]); 8] = [
    (Item::Screen, [0.0, 0.0, 400.0, 300.0]),
    (Item::TouchScreen, [0.0, 300.0, 400.0, 300.0]),
    (Item::L, [12.0, 616.0, 120.0, 36.0]),
    (Item::R, [268.0, 616.0, 120.0, 36.0]),
    (Item::Dpad, [14.0, 678.0, 150.0, 150.0]),
    (Item::Face, [232.0, 670.0, 156.0, 156.0]),
    (Item::Select, [104.0, 842.0, 92.0, 30.0]),
    (Item::Start, [204.0, 842.0, 92.0, 30.0]),
];

pub fn big_top_phone(button_scale: f32) -> Spots {
    phone_spots(BIG_TOP_ROOM, &BIG_TOP, button_scale)
}

pub fn stacked_phone(button_scale: f32) -> Spots {
    phone_spots(STACKED_ROOM, &STACKED, button_scale)
}

fn phone_spots(room: Vec2, table: &[(Item, [f32; 4])], button_scale: f32) -> Spots {
    let room = Rect::from_min_size(egui::Pos2::ZERO, room);
    table
        .iter()
        .map(|&(item, [x, y, width, height])| {
            let rect = Rect::from_min_size(egui::pos2(x, y), vec2(width, height));
            let mut spot = Spot::of(rect, room);
            if !item.is_screen() {
                spot.width *= button_scale;
            }
            (item, spot)
        })
        .collect()
}

pub fn with_defaults(mut spots: Spots, defaults: Spots) -> Spots {
    for (item, spot) in defaults {
        spots.entry(item).or_insert(spot);
    }
    spots
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct Entry {
    console: LayoutConsole,
    view: LayoutView,
    spots: Spots,
}

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ScreenLayouts {
    entries: Vec<Entry>,
}

impl ScreenLayouts {
    fn entry(&self, console: LayoutConsole, view: LayoutView) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.console == console && e.view == view)
    }

    pub fn spots(&self, console: LayoutConsole, view: LayoutView) -> Spots {
        self.entry(console, view)
            .map(|e| e.spots.clone())
            .unwrap_or_default()
    }

    pub fn spot(&self, console: LayoutConsole, view: LayoutView, item: Item) -> Option<Spot> {
        self.entry(console, view)?.spots.get(&item).copied()
    }

    pub fn edited(&self, console: LayoutConsole, view: LayoutView) -> bool {
        self.entry(console, view)
            .is_some_and(|e| !e.spots.is_empty())
    }

    pub fn set(&mut self, console: LayoutConsole, view: LayoutView, item: Item, spot: Spot) {
        match self
            .entries
            .iter_mut()
            .find(|e| e.console == console && e.view == view)
        {
            Some(entry) => {
                entry.spots.insert(item, spot);
            }
            None => self.entries.push(Entry {
                console,
                view,
                spots: Spots::from([(item, spot)]),
            }),
        }
    }

    pub fn reset(&mut self, console: LayoutConsole, view: LayoutView) -> Spots {
        let Some(index) = self
            .entries
            .iter()
            .position(|e| e.console == console && e.view == view)
        else {
            return Spots::new();
        };
        self.entries.remove(index).spots
    }

    pub fn restore(&mut self, console: LayoutConsole, view: LayoutView, spots: Spots) {
        self.reset(console, view);
        if !spots.is_empty() {
            self.entries.push(Entry {
                console,
                view,
                spots,
            });
        }
    }

    pub fn edited_views(&self) -> impl Iterator<Item = (LayoutConsole, LayoutView, &Spots)> {
        self.entries
            .iter()
            .filter(|e| !e.spots.is_empty())
            .map(|e| (e.console, e.view, &e.spots))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    const ROOM: Rect = Rect {
        min: pos2(0.0, 100.0),
        max: pos2(400.0, 700.0),
    };

    #[test]
    fn a_spot_gives_back_the_rect_it_was_taken_from() {
        let rect = Rect::from_min_size(pos2(40.0, 200.0), vec2(200.0, 150.0));
        let back = Spot::of(rect, ROOM).rect(ROOM, rect.size());
        assert!((back.min - rect.min).length() < 0.01, "{back:?}");
        assert!((back.size() - rect.size()).length() < 0.01, "{back:?}");
    }

    #[test]
    fn a_spot_keeps_the_items_shape_and_stays_in_the_room() {
        let spot = Spot {
            x: 0.98,
            y: 0.02,
            width: 0.5,
            ..Default::default()
        };
        let rect = spot.rect(ROOM, vec2(256.0, 192.0));
        assert!((rect.height() / rect.width() - 0.75).abs() < 0.001);
        assert!(ROOM.contains_rect(rect), "{rect:?}");
        assert_eq!(rect.right(), ROOM.right());
        assert_eq!(rect.top(), ROOM.top());
    }

    #[test]
    fn a_spot_wider_than_the_room_shrinks_to_fit() {
        let spot = Spot {
            x: 0.5,
            y: 0.5,
            width: 2.0,
            ..Default::default()
        };
        let rect = spot.rect(ROOM, vec2(256.0, 192.0));
        assert!(ROOM.contains_rect(rect.shrink(0.01)), "{rect:?}");
        assert!((rect.width() - ROOM.width()).abs() < 0.01);
    }

    #[test]
    fn moved_screens_keep_their_place_in_the_room_in_any_room() {
        let saved = Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 760.0));
        let screen = vec2(256.0, 192.0);
        let top = Rect::from_min_size(pos2(172.0, 188.0), screen);
        let touch = Rect::from_min_size(pos2(200.0, 380.0), screen * 1.5);
        let spots = [
            (Spot::of(top, saved), screen),
            (Spot::of(touch, saved), screen),
        ];
        for size in [
            vec2(600.0, 760.0),
            vec2(300.0, 800.0),
            vec2(1900.0, 1040.0),
            vec2(1260.0, 480.0),
        ] {
            let room = Rect::from_min_size(pos2(10.0, 40.0), size);
            let placed = place_screens(&spots, room);
            let [a, b] = [placed[0], placed[1]];
            assert!(room.expand(0.01).contains_rect(a), "{size:?}: {a:?}");
            assert!(room.expand(0.01).contains_rect(b), "{size:?}: {b:?}");
            assert!(
                !a.shrink(0.01).intersects(b.shrink(0.01)),
                "{size:?}: {a:?} {b:?}"
            );
            assert!((b.width() / a.width() - 1.5).abs() < 0.001);
            assert!((a.height() / a.width() - 0.75).abs() < 0.001);
            let x = |r: Rect| (r.center().x - room.left()) / room.width();
            let y = |at: f32| (at - room.top()) / room.height();
            assert!((x(a) - 300.0 / 600.0).abs() < 0.001, "{size:?}: {a:?}");
            assert!((x(b) - 392.0 / 600.0).abs() < 0.001, "{size:?}: {b:?}");
            assert!(
                (y(a.top()) - 188.0 / 760.0).abs() < 0.001,
                "{size:?}: {a:?}"
            );
            assert!(
                (y(b.bottom()) - 668.0 / 760.0).abs() < 0.001,
                "{size:?}: {b:?}"
            );
            let scale = (size / vec2(600.0, 760.0)).min_elem();
            assert!((a.width() - 256.0 * scale).abs() < 0.01, "{size:?}: {a:?}");
        }
        let same = place_screens(&spots, saved);
        assert!((same[0].min - top.min).length() < 0.01, "{same:?}");
        assert!((same[1].max - touch.max).length() < 0.01, "{same:?}");
    }

    #[test]
    fn a_button_keeps_its_distance_to_the_nearest_edges() {
        let saved = Rect::from_min_size(pos2(0.0, 0.0), vec2(915.0, 412.0));
        let size = vec2(120.0, 120.0);
        let face = Rect::from_min_size(pos2(915.0 - 16.0 - 120.0, 412.0 - 24.0 - 120.0), size);
        let spot = Spot::of(face, saved);
        let shorter = Rect::from_min_size(pos2(0.0, 47.0), vec2(915.0, 365.0));
        let rect = spot.rect(shorter, size);
        assert!((shorter.right() - rect.right() - 16.0 * 365.0 / 412.0).abs() < 0.01);
        assert!((shorter.bottom() - rect.bottom() - 24.0 * 365.0 / 412.0).abs() < 0.01);
        let bigger = Rect::from_min_size(pos2(0.0, 0.0), vec2(1830.0, 824.0));
        let rect = spot.rect(bigger, size);
        assert!((rect.width() - 240.0).abs() < 0.01, "{rect:?}");
        assert!(
            (bigger.right() - rect.right() - 32.0).abs() < 0.01,
            "{rect:?}"
        );
        assert!(
            (bigger.bottom() - rect.bottom() - 48.0).abs() < 0.01,
            "{rect:?}"
        );
    }

    #[test]
    fn layouts_are_kept_per_console_and_per_view() {
        let mut layouts = ScreenLayouts::default();
        let spot = Spot {
            x: 0.25,
            y: 0.5,
            width: 0.3,
            ..Default::default()
        };
        layouts.set(LayoutConsole::Ds, LayoutView::Landscape, Item::Dpad, spot);
        assert_eq!(
            layouts.spot(LayoutConsole::Ds, LayoutView::Landscape, Item::Dpad),
            Some(spot)
        );
        for (console, view) in [
            (LayoutConsole::Ds, LayoutView::Portrait),
            (LayoutConsole::Ds, LayoutView::Desktop),
            (LayoutConsole::Gba, LayoutView::Landscape),
            (LayoutConsole::GameBoy, LayoutView::Landscape),
        ] {
            assert_eq!(layouts.spot(console, view, Item::Dpad), None);
        }
    }

    #[test]
    fn reset_hands_back_the_spots_so_undo_restores_them() {
        let mut layouts = ScreenLayouts::default();
        let spot = Spot {
            x: 0.5,
            y: 0.5,
            width: 0.8,
            ..Default::default()
        };
        layouts.set(LayoutConsole::Gba, LayoutView::Portrait, Item::Screen, spot);
        let taken = layouts.reset(LayoutConsole::Gba, LayoutView::Portrait);
        assert!(!layouts.edited(LayoutConsole::Gba, LayoutView::Portrait));
        layouts.restore(LayoutConsole::Gba, LayoutView::Portrait, taken);
        assert_eq!(
            layouts.spot(LayoutConsole::Gba, LayoutView::Portrait, Item::Screen),
            Some(spot)
        );
    }

    #[test]
    fn layouts_survive_a_save_and_load() {
        let mut layouts = ScreenLayouts::default();
        layouts.set(
            LayoutConsole::Ds,
            LayoutView::Desktop,
            Item::TouchScreen,
            Spot {
                x: 0.75,
                y: 0.5,
                width: 0.25,
                ..Default::default()
            },
        );
        let json = serde_json::to_string(&layouts).unwrap();
        let back: ScreenLayouts = serde_json::from_str(&json).unwrap();
        assert_eq!(back, layouts);
    }

    #[test]
    fn stacked_phone_buttons_stay_under_the_screens_when_the_bars_take_room() {
        let spots = stacked_phone(1.0);
        let size = |item: Item| {
            STACKED
                .iter()
                .find(|(i, _)| *i == item)
                .map(|&(_, [_, _, width, height])| vec2(width, height))
                .unwrap()
        };
        for height in [888.0, 773.0, 700.0] {
            let room = Rect::from_min_size(pos2(0.0, 51.0), vec2(400.0, height));
            let screens = place_screens(
                &[
                    (spots[&Item::Screen], size(Item::Screen)),
                    (spots[&Item::TouchScreen], size(Item::TouchScreen)),
                ],
                room,
            );
            for item in [
                Item::L,
                Item::R,
                Item::Dpad,
                Item::Face,
                Item::Select,
                Item::Start,
            ] {
                let button = spots[&item].rect(room, size(item));
                assert!(
                    room.expand(0.01).contains_rect(button),
                    "{height}: {item:?}"
                );
                for screen in &screens {
                    assert!(
                        !button.shrink(0.01).intersects(*screen),
                        "{height}: {item:?} {button:?} {screen:?}"
                    );
                }
            }
        }
    }
}
