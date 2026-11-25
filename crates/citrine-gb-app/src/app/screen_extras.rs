use crate::app::Citrine;
use crate::app::hud::INSET;
use crate::app::ui_state::UiState;
use crate::app::ui_state::screen_layouts::{Item, LayoutConsole, LayoutView};
use crate::app::widgets::layout_editor::EditItem;
use egui::{Painter, Rect, Ui, Vec2, pos2};

impl Citrine {
    pub(crate) fn wanted_buttons(&self) -> Vec<(Item, &'static str)> {
        #[cfg(feature = "nds")]
        if self.talk_button_wanted() {
            return vec![(Item::Talk, crate::icons::MICROPHONE)];
        }
        Vec::new()
    }

    fn wanted_marks(&self, editing: bool) -> Vec<(Item, Vec2)> {
        #[cfg(feature = "nds")]
        if (editing && self.mic_mark_offered()) || self.mic_live() {
            return vec![(Item::MicMark, crate::app::mic_mark::SIZE)];
        }
        #[cfg(not(feature = "nds"))]
        let _ = editing;
        Vec::new()
    }

    pub(crate) fn follow_screen_marks(&mut self) {
        self.ui.screen_marks = self.wanted_marks(self.editing_layout());
    }
}

fn paint_mark(item: Item, painter: &Painter, rect: Rect) {
    #[cfg(feature = "nds")]
    if item == Item::MicMark {
        crate::app::mic_mark::paint(painter, rect);
    }
    #[cfg(not(feature = "nds"))]
    let _ = (item, painter, rect);
}

pub(crate) fn place_marks(
    ui: &Ui,
    state: &UiState,
    console: LayoutConsole,
    view: LayoutView,
    room: Rect,
    main: Rect,
    taken: &[Rect],
) -> Vec<EditItem> {
    let spots = state.settings.layouts.spots(console, view);
    state
        .screen_marks
        .iter()
        .map(|&(item, size)| {
            let default = beside(main, size, room, taken).unwrap_or_else(|| {
                Rect::from_min_size(
                    pos2(main.right() - size.x - INSET, main.top() + INSET),
                    size,
                )
            });
            let rect = spots
                .get(&item)
                .map_or(default, |spot| spot.rect(room, size));
            paint_mark(item, ui.painter(), rect);
            EditItem::new(item, rect, size)
        })
        .collect()
}

const BESIDE_GAP: f32 = 8.0;

fn beside(main: Rect, size: Vec2, room: Rect, taken: &[Rect]) -> Option<Rect> {
    [main.right() + BESIDE_GAP, main.left() - BESIDE_GAP - size.x]
        .into_iter()
        .map(|left| Rect::from_min_size(pos2(left, main.top()), size))
        .find(|rect| room.contains_rect(*rect) && !taken.iter().any(|t| t.intersects(*rect)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::vec2;

    #[test]
    fn the_mark_sits_beside_the_screen_where_nothing_else_is() {
        let room = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 800.0));
        let main = Rect::from_min_size(pos2(60.0, 0.0), vec2(280.0, 210.0));
        let size = vec2(20.0, 20.0);
        let right = beside(main, size, room, &[main]).expect("room on the right");
        assert_eq!(right.left(), main.right() + BESIDE_GAP);
        assert_eq!(right.top(), main.top());
        let buttons = Rect::from_min_size(pos2(345.0, 0.0), vec2(55.0, 100.0));
        let left = beside(main, size, room, &[main, buttons]).expect("room on the left");
        assert_eq!(left.right(), main.left() - BESIDE_GAP);
        let wide = Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0));
        assert_eq!(beside(wide, size, room, &[wide]), None);
    }
}
