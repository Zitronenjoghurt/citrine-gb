use egui::{Id, TextStyle, Ui};
use std::hash::{DefaultHasher, Hash, Hasher};

const MARGIN: f32 = 8.0;

#[cfg(test)]
thread_local! {
    pub(super) static ALWAYS_FULL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(super) static SKIPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn always_full() -> bool {
    ALWAYS_FULL.get()
}

#[cfg(not(test))]
fn always_full() -> bool {
    false
}

#[cfg(test)]
fn count_skip() {
    SKIPS.set(SKIPS.get() + 1);
}

#[cfg(not(test))]
fn count_skip() {}

#[derive(Clone, Debug, Default)]
struct Measured {
    key: u64,
    starts: Vec<f32>,
    end: f32,
    left: f32,
    right: f32,
}

pub(super) struct HiddenTail {
    id: Id,
    key: u64,
    top: f32,
    origin: f32,
    starts: Vec<f32>,
    known: Option<Measured>,
}

impl HiddenTail {
    pub(super) fn new(ui: &Ui, markdown: &str) -> Self {
        let id = ui.id().with("info_hidden_tail");
        let key = layout_key(ui, markdown);
        let ctx = ui.ctx();
        let may_skip = !always_full() && ctx.memory(|memory| memory.focused().is_none());
        let known = may_skip
            .then(|| ui.data(|data| data.get_temp::<Measured>(id)))
            .flatten()
            .filter(|known| known.key == key);
        Self {
            id,
            key,
            top: ui.cursor().top(),
            origin: ui.max_rect().left(),
            starts: Vec::new(),
            known,
        }
    }

    pub(super) fn skip_rest(&mut self, ui: &mut Ui, index: usize) -> bool {
        let at = ui.cursor().top() - self.top;
        self.starts.push(at);
        let Some(known) = &self.known else {
            return false;
        };
        let below = ui.cursor().top() > ui.clip_rect().bottom() + MARGIN;
        if !below || known.starts.get(index) != Some(&at) {
            return false;
        }
        count_skip();
        ui.add_space(known.end - at);
        ui.expand_to_include_x(self.origin + known.left);
        ui.expand_to_include_x(self.origin + known.right);
        true
    }

    pub(super) fn measured(self, ui: &Ui) {
        let drawn = ui.min_rect();
        let measured = Measured {
            key: self.key,
            starts: self.starts,
            end: drawn.bottom() - self.top,
            left: drawn.left() - self.origin,
            right: drawn.right() - self.origin,
        };
        ui.data_mut(|data| data.insert_temp(self.id, measured));
    }
}

fn layout_key(ui: &Ui, markdown: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    markdown.hash(&mut hasher);
    let spacing = &ui.style().spacing;
    let sizes = [
        ui.available_width(),
        ui.ctx().pixels_per_point(),
        spacing.item_spacing.x,
        spacing.item_spacing.y,
        spacing.interact_size.y,
        ui.text_style_height(&TextStyle::Body),
        ui.text_style_height(&TextStyle::Heading),
        ui.text_style_height(&TextStyle::Monospace),
        ui.text_style_height(&TextStyle::Small),
        ui.text_style_height(&TextStyle::Button),
    ];
    for size in sizes {
        size.to_bits().hash(&mut hasher);
    }
    hasher.finish()
}
