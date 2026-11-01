use egui::{Rect, Vec2, pos2, vec2};

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScreenPlace {
    x: f32,
    y: f32,
}

impl Default for ScreenPlace {
    fn default() -> Self {
        Self { x: 0.5, y: 0.5 }
    }
}

impl ScreenPlace {
    pub fn rect(self, area: Rect, size: Vec2) -> Rect {
        let room = (area.size() - size).max(Vec2::ZERO);
        let min = area.min
            + vec2(
                room.x * self.x.clamp(0.0, 1.0),
                room.y * self.y.clamp(0.0, 1.0),
            );
        Rect::from_min_size(pos2(min.x.round(), min.y.round()), size)
    }

    pub fn dragged(self, delta: Vec2, area: Rect, size: Vec2) -> Self {
        let room = area.size() - size;
        let step = |at: f32, by: f32, room: f32| {
            if room > 0.0 {
                (at + by / room).clamp(0.0, 1.0)
            } else {
                at
            }
        };
        Self {
            x: step(self.x, delta.x, room.x),
            y: step(self.y, delta.y, room.y),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Vec2 = vec2(480.0, 432.0);

    fn window(width: f32, height: f32) -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(width, height))
    }

    #[test]
    fn a_dragged_screen_stays_inside_the_window() {
        let area = window(1280.0, 800.0);
        let place = ScreenPlace::default().dragged(vec2(5000.0, -5000.0), area, SCREEN);
        let rect = place.rect(area, SCREEN);
        assert_eq!(rect.right(), area.right());
        assert_eq!(rect.top(), area.top());
        let back = place.dragged(vec2(-100.0, 0.0), area, SCREEN);
        assert_eq!(back.rect(area, SCREEN).right(), area.right() - 100.0);
    }

    #[test]
    fn a_placed_screen_keeps_its_spot_when_the_window_resizes() {
        let place = ScreenPlace::default().dragged(vec2(200.0, 0.0), window(1280.0, 800.0), SCREEN);
        assert_eq!(place.rect(window(1280.0, 800.0), SCREEN).left(), 600.0);
        let narrow = place.rect(window(800.0, 600.0), SCREEN);
        assert!(window(800.0, 600.0).contains_rect(narrow));
        assert_eq!(narrow.left(), 240.0);
    }

    #[test]
    fn resetting_puts_the_screen_back_in_the_middle() {
        let area = window(1280.0, 800.0);
        let moved = ScreenPlace::default().dragged(vec2(-300.0, 120.0), area, SCREEN);
        assert_ne!(moved.rect(area, SCREEN).center(), area.center());
        let reset = ScreenPlace::default();
        assert_eq!(reset.rect(area, SCREEN).center(), area.center());
    }
}
