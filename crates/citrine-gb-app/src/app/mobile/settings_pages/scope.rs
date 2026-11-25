use crate::app::big_screen::glyphs::{Family, shoulder_text};
use crate::app::mobile::kit;
use crate::app::ui_state::per_game::Scope;
use crate::app::ui_state::settings_tree::Surface;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::widgets::chip::{TAG_PAD, paint_tag, tag_width};
use crate::app::widgets::info_mark;
use crate::app::widgets::settings::scope::{self, Tag};
use egui::epaint::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Color32, FontId, Id, Rect, Sense, Ui, pos2, vec2};

pub(super) struct Chips {
    pub(super) font: f32,
    pub(super) height: f32,
}

impl Chips {
    pub(super) fn new(big: bool) -> Self {
        if big {
            Self {
                font: 16.0,
                height: 26.0,
            }
        } else {
            Self {
                font: 14.0,
                height: 24.0,
            }
        }
    }
}

pub(super) fn segments(ui: &mut Ui, current: &mut Scope, game: &str, shoulders: Option<Family>) {
    let game_label = format!("{} {game}", scope::MARK);
    let options = [
        (Scope::AllGames, scope::ALL_GAMES.to_owned()),
        (Scope::ThisGame, game_label),
    ];
    segments_of(ui, Id::new("scope_segment"), current, &options, shoulders);
}

pub(in crate::app::mobile) fn segments_of<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: Id,
    current: &mut T,
    options: &[(T, String)],
    shoulders: Option<Family>,
) {
    let height = 40.0_f32.max(ui.spacing().interact_size.y);
    let (whole, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let visuals = ui.visuals().clone();
    let radius = height / 2.0;
    ui.painter()
        .rect_filled(whole, radius, kit::icon_tile_fill(&visuals));
    let mut pad = 3.0;
    if let Some(family) = shoulders {
        use crate::app::ui_state::keybinds::PadButton;
        let left = shoulder(
            ui,
            pos2(whole.left() + 20.0, whole.center().y),
            shoulder_text(family, PadButton::LeftTrigger2),
        );
        let right = shoulder(
            ui,
            pos2(whole.right() - 20.0, whole.center().y),
            shoulder_text(family, PadButton::RightTrigger2),
        );
        pad = 20.0 + left.max(right) / 2.0 + 8.0;
    }
    let inner = Rect::from_min_max(
        pos2(whole.left() + pad, whole.top() + 3.0),
        pos2(whole.right() - pad, whole.bottom() - 3.0),
    );
    let font = FontId::proportional(15.0);
    let natural: Vec<f32> = options
        .iter()
        .map(|(_, text)| {
            ui.painter()
                .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
                .size()
                .x
                + SEGMENT_INSET
        })
        .collect();
    let widths = segment_widths(&natural, inner.width());
    let accent = visuals.selection.bg_fill;
    let mut left = inner.left();
    for (index, ((option, text), width)) in options.iter().zip(widths).enumerate() {
        let rect = Rect::from_min_size(pos2(left, inner.top()), vec2(width, inner.height()));
        left += width;
        let response = ui.interact(rect, id.with(index), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::SelectableLabel,
                true,
                *current == *option,
                text,
            )
        });
        crate::app::big_screen::ring::mark(
            &response,
            crate::app::big_screen::ring::Mark::Ring {
                rect,
                radius: radius - 3.0,
            },
        );
        if response.clicked() {
            *current = *option;
        }
        let on = *current == *option;
        if on {
            ui.painter().rect_filled(rect, radius - 3.0, accent);
        }
        let ink = if on {
            Color32::from_gray(20)
        } else {
            visuals.text_color()
        };
        let mut job =
            LayoutJob::single_section(text.to_owned(), TextFormat::simple(font.clone(), ink));
        job.wrap = TextWrapping::truncate_at_width(rect.width() - SEGMENT_INSET);
        let galley = ui.painter().layout_job(job);
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, ink);
    }
}

const SEGMENT_INSET: f32 = 20.0;

fn segment_widths(natural: &[f32], room: f32) -> Vec<f32> {
    let count = natural.len().max(1) as f32;
    let equal = room / count;
    let total: f32 = natural.iter().sum();
    if natural.iter().all(|width| *width <= equal) || total > room {
        return vec![equal; natural.len()];
    }
    let spare = (room - total) / count;
    natural.iter().map(|width| width + spare).collect()
}

fn shoulder(ui: &Ui, center: egui::Pos2, text: &str) -> f32 {
    let ink = ui.visuals().strong_text_color();
    let paper = ui.visuals().panel_fill;
    let size = 22.0;
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_owned(), FontId::proportional(size * 0.55), paper);
    let width = (galley.size().x + size * 0.6).max(size);
    let rect = Rect::from_center_size(center, vec2(width, size));
    ui.painter().rect_filled(rect, size / 2.0, ink);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, paper);
    width
}

pub(in crate::app::mobile) fn tag_chip(ui: &mut Ui, tag: &Tag, game: &str, big: bool) -> bool {
    let chips = Chips::new(big);
    let font = FontId::proportional(chips.font);
    let text = match tag {
        Tag::Differs(value) => {
            let room = ui.available_width() - 2.0 * TAG_PAD;
            with_game(ui, game, value, room, &font)
        }
        Tag::Pick { game, value } => {
            let room = ui.available_width() - 2.0 * TAG_PAD;
            let value = format!("{value} {}", crate::icons::X);
            named_game(ui, game, ": ", &value, room, &font)
        }
        _ => tag.words(),
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text, font, ui.visuals().strong_text_color());
    let sense = if tag.drops() {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(tag_width(&galley), chips.height), sense);
    let hover = tag.hover(game);
    let (kind, label) = if tag.drops() {
        (egui::WidgetType::Button, scope::USE_ALL.to_owned())
    } else {
        (egui::WidgetType::Label, hover.clone())
    };
    response.widget_info(|| egui::WidgetInfo::labeled(kind, true, &label));
    if ui.is_rect_visible(rect) {
        paint_tag(ui, rect, galley, tag.own());
    }
    response.on_hover_text(hover).clicked() && tag.drops()
}

pub(super) fn chip_column(ui: &Ui, rows: &[RowId]) -> f32 {
    let strong = ui.visuals().strong_text_color();
    rows.iter()
        .map(|row| {
            ui.painter()
                .layout_no_wrap(
                    row.label(Surface::Phone).to_owned(),
                    FontId::proportional(16.0),
                    strong,
                )
                .size()
                .x
        })
        .fold(0.0_f32, f32::max)
        + info_mark::width(ui)
}

pub(super) fn mark(
    ui: &mut Ui,
    row: RowId,
    tag: &Tag,
    top: f32,
    column: f32,
    chips: &Chips,
) -> bool {
    let drawn = ui.min_rect().bottom() - top;
    if drawn < 1.0 {
        return false;
    }
    let height = kit::ROW_HEIGHT.max(ui.spacing().interact_size.y + 6.0);
    let cy = if drawn > height + 4.0 {
        top + height / 2.0
    } else {
        top + drawn / 2.0
    };
    let left = ui.max_rect().left() + 12.0 + column + 8.0;
    let text = match tag {
        Tag::Differs(value) => format!("{} {value}", scope::MARK),
        _ => tag.words(),
    };
    let rect = paint_chip(ui, left, cy, text, tag.own(), chips);
    if !tag.drops() {
        return false;
    }
    let response = ui
        .interact(rect, Id::new(("scope_chip", row)), Sense::click())
        .on_hover_text(scope::USE_ALL);
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, scope::USE_ALL));
    response.clicked()
}

const NAME_MIN: usize = 5;

const NAME_MAX: usize = 16;

fn text_width(ui: &Ui, text: &str, font: &FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
        .size()
        .x
}

pub(super) fn with_game(ui: &Ui, game: &str, value: &str, room: f32, font: &FontId) -> String {
    named_game(ui, game, " ", value, room, font)
}

fn named_game(ui: &Ui, game: &str, gap: &str, value: &str, room: f32, font: &FontId) -> String {
    let chars: Vec<char> = game.chars().collect();
    let full = format!("{game}{gap}{value}");
    if chars.len() <= NAME_MAX && text_width(ui, &full, font) <= room {
        return full;
    }
    let (mut low, mut high) = (0, chars.len().min(NAME_MAX));
    while low < high {
        let keep = (low + high).div_ceil(2);
        let short: String = chars[..keep].iter().collect();
        let shortened = format!("{}\u{2026}{gap}{value}", short.trim_end());
        if text_width(ui, &shortened, font) <= room {
            low = keep;
        } else {
            high = keep - 1;
        }
    }
    if low < NAME_MIN.min(chars.len()) {
        return format!("{} {value}", scope::MARK);
    }
    let short: String = chars[..low].iter().collect();
    format!("{}\u{2026}{gap}{value}", short.trim_end())
}

fn paint_chip(ui: &Ui, left: f32, cy: f32, text: String, own: bool, chips: &Chips) -> Rect {
    let ink = ui.visuals().strong_text_color();
    let galley = ui
        .painter()
        .layout_no_wrap(text, FontId::proportional(chips.font), ink);
    let size = vec2(tag_width(&galley), chips.height);
    let rect = Rect::from_min_size(pos2(left, cy - size.y / 2.0), size);
    paint_tag(ui, rect, galley, own);
    rect
}

pub(super) fn holds_focus(ui: &Ui, top: f32) -> bool {
    let ctx = ui.ctx();
    let Some(focused) = ctx.memory(|m| m.focused()) else {
        return false;
    };
    let Some(response) = ctx.read_response(focused) else {
        return false;
    };
    let row = Rect::from_min_max(
        pos2(ui.max_rect().left(), top),
        pos2(ui.max_rect().right(), ui.min_rect().bottom()),
    );
    row.contains(response.rect.center())
}

#[cfg(test)]
mod tests {
    use crate::app::mobile::kit;
    use crate::app::ui_state::ui_theme::{UiTheme, contrast, own_value};
    use crate::app::widgets::chip::tag_stroke;
    use strum::IntoEnumIterator;

    fn themed(theme: UiTheme) -> egui::Visuals {
        let ctx = egui::Context::default();
        theme.apply(&ctx);
        ctx.style().visuals.clone()
    }

    #[test]
    fn scope_halves_follow_their_labels_only_when_equal_halves_cut_one() {
        assert_eq!(super::segment_widths(&[60.0, 70.0], 200.0), [100.0, 100.0]);
        assert_eq!(super::segment_widths(&[80.0, 110.0], 200.0), [85.0, 115.0]);
        assert_eq!(super::segment_widths(&[90.0, 150.0], 200.0), [100.0, 100.0]);
    }

    #[test]
    fn a_games_own_tag_stands_out_on_a_card() {
        for theme in UiTheme::iter() {
            let visuals = themed(theme);
            let edge = tag_stroke(&visuals, true).color;
            let ground = kit::card_fill(&visuals);
            let ratio = contrast(edge, ground);
            assert!(
                ratio >= 3.0,
                "{theme} tag edge {edge:?} on {ground:?}: {ratio}"
            );
        }
    }

    #[test]
    fn a_games_own_values_read_in_every_theme() {
        for theme in UiTheme::iter() {
            let visuals = themed(theme);
            let value = own_value(&visuals);
            for ground in [visuals.panel_fill, kit::card_fill(&visuals)] {
                let ratio = contrast(value, ground);
                assert!(
                    ratio >= 4.5,
                    "{theme} value {value:?} on {ground:?}: {ratio}"
                );
            }
        }
    }
}
