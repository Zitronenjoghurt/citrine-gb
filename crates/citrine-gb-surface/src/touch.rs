use crate::mode::ScreenShape;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScreenTouch {
    pub screen: u8,
    pub x: u8,
    pub y: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Area {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
}

impl Area {
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.left && y >= self.top && x < self.left + self.width && y < self.top + self.height
    }
}

pub fn touch_in(
    area: Area,
    turns: u8,
    screen: u8,
    shape: ScreenShape,
    x: f32,
    y: f32,
) -> Option<ScreenTouch> {
    if !shape.touch || !area.contains(x, y) || area.width <= 0.0 || area.height <= 0.0 {
        return None;
    }
    let along = ((x - area.left) / area.width, (y - area.top) / area.height);
    let (u, v) = match turns % 4 {
        1 => (along.1, 1.0 - along.0),
        2 => (1.0 - along.0, 1.0 - along.1),
        3 => (1.0 - along.1, along.0),
        _ => along,
    };
    Some(ScreenTouch {
        screen,
        x: pixel(u, shape.width)?,
        y: pixel(v, shape.height)?,
    })
}

fn pixel(along: f32, size: u16) -> Option<u8> {
    let last = f32::from(size.saturating_sub(1));
    let at = (along * f32::from(size)).floor().clamp(0.0, last);
    u8::try_from(at as u32).ok()
}

pub fn clamped(touch: ScreenTouch, screens: &[ScreenShape]) -> Option<ScreenTouch> {
    let shape = screens.get(usize::from(touch.screen))?;
    shape.touch.then(|| ScreenTouch {
        screen: touch.screen,
        x: touch
            .x
            .min(u8::try_from(shape.width.saturating_sub(1)).unwrap_or(u8::MAX)),
        y: touch
            .y
            .min(u8::try_from(shape.height.saturating_sub(1)).unwrap_or(u8::MAX)),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Pointer<S> {
    Local,
    Surface(S),
}

#[derive(Debug)]
pub struct Stylus<S> {
    owner: Option<Pointer<S>>,
}

impl<S> Default for Stylus<S> {
    fn default() -> Self {
        Self { owner: None }
    }
}

impl<S: Copy + PartialEq> Stylus<S> {
    pub fn owner(&self) -> Option<Pointer<S>> {
        self.owner
    }

    pub fn pick(
        &mut self,
        pressing: impl IntoIterator<Item = (Pointer<S>, Option<ScreenTouch>)>,
    ) -> Option<ScreenTouch> {
        let pressing: Vec<_> = pressing.into_iter().collect();
        let held = self.owner.and_then(|owner| {
            pressing
                .iter()
                .find(|(who, _)| *who == owner)
                .and_then(|(_, touch)| *touch)
        });
        if held.is_some() {
            return held;
        }
        let next = pressing
            .iter()
            .find_map(|(who, touch)| touch.map(|t| (*who, t)));
        self.owner = next.map(|(who, _)| who);
        next.map(|(_, touch)| touch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTTOM: ScreenShape = ScreenShape {
        width: 256,
        height: 192,
        touch: true,
    };
    const AREA: Area = Area {
        left: 10.0,
        top: 20.0,
        width: 512.0,
        height: 384.0,
    };

    #[test]
    fn corners_map_to_the_first_and_last_pixel() {
        let at = |x, y| touch_in(AREA, 0, 1, BOTTOM, x, y).map(|t| (t.x, t.y));
        assert_eq!(at(10.0, 20.0), Some((0, 0)));
        assert_eq!(at(521.9, 403.9), Some((255, 191)));
        assert_eq!(at(266.0, 212.0), Some((128, 96)));
    }

    #[test]
    fn outside_the_area_is_no_touch() {
        assert_eq!(touch_in(AREA, 0, 1, BOTTOM, 9.0, 30.0), None);
        assert_eq!(touch_in(AREA, 0, 1, BOTTOM, 522.0, 30.0), None);
        assert_eq!(touch_in(AREA, 0, 1, BOTTOM, 50.0, 404.0), None);
    }

    #[test]
    fn a_screen_without_touch_ignores_presses() {
        let top = ScreenShape {
            touch: false,
            ..BOTTOM
        };
        assert_eq!(touch_in(AREA, 0, 0, top, 50.0, 50.0), None);
    }

    #[test]
    fn turned_screens_undo_their_turn() {
        let turned = Area {
            left: 0.0,
            top: 0.0,
            width: 192.0,
            height: 256.0,
        };
        let at = |turns, x, y| touch_in(turned, turns, 1, BOTTOM, x, y).map(|t| (t.x, t.y));
        assert_eq!(at(1, 191.5, 0.0), Some((0, 0)));
        assert_eq!(at(1, 0.0, 255.5), Some((255, 191)));
        assert_eq!(at(3, 0.0, 255.5), Some((0, 0)));
        let upside = |x, y| touch_in(AREA, 2, 1, BOTTOM, x, y).map(|t| (t.x, t.y));
        assert_eq!(upside(10.0, 20.0), Some((255, 191)));
    }

    #[test]
    fn the_host_clamps_and_checks_the_screen() {
        let screens = [
            ScreenShape {
                touch: false,
                ..BOTTOM
            },
            BOTTOM,
        ];
        let far = ScreenTouch {
            screen: 1,
            x: 255,
            y: 250,
        };
        assert_eq!(
            clamped(far, &screens),
            Some(ScreenTouch {
                screen: 1,
                x: 255,
                y: 191
            })
        );
        assert_eq!(clamped(ScreenTouch { screen: 0, ..far }, &screens), None);
        assert_eq!(clamped(ScreenTouch { screen: 5, ..far }, &screens), None);
    }

    #[test]
    fn the_first_press_keeps_the_stylus_until_it_lifts() {
        let mut stylus = Stylus::<u32>::default();
        let t = |x| Some(ScreenTouch { screen: 1, x, y: 0 });
        assert_eq!(
            stylus.pick([(Pointer::Local, None), (Pointer::Surface(1), t(5))]),
            t(5)
        );
        assert_eq!(
            stylus.pick([(Pointer::Local, t(9)), (Pointer::Surface(1), t(6))]),
            t(6)
        );
        assert_eq!(stylus.owner(), Some(Pointer::Surface(1)));
        assert_eq!(
            stylus.pick([(Pointer::Local, t(9)), (Pointer::Surface(1), None)]),
            t(9)
        );
        assert_eq!(stylus.owner(), Some(Pointer::Local));
        assert_eq!(stylus.pick([(Pointer::Local, None)]), None);
        assert_eq!(stylus.owner(), None);
    }
}
