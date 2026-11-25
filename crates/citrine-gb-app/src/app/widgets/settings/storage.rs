use crate::app::ui_state::settings_tree::GroupId;
use crate::app::widgets::info_mark::{info_mark, info_mark_width};
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::page::{Heading, Page};
use crate::app::widgets::settings::your_data::{self, YourDataUi};
use crate::icons;
use crate::media::{format_size, shown_size};
use crate::storage::kinds::{Kind, Section, Usage};
use egui::{Align, Layout, RichText, Ui, vec2};
use std::collections::BTreeSet;
use strum::IntoEnumIterator;

pub const CLEAR: &str = "Clear";
pub const CLEAR_ALL: &str = "Clear all";
pub const COUNTING: &str = "Counting…";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Clear(Kind),
    ClearAll,
}

#[derive(Default)]
pub struct StorageUi {
    pub usage: Usage,
    pub counting: BTreeSet<Kind>,
    pub counted_at: Option<web_time::Instant>,
    pub request: Option<Request>,
    pub asking: Option<Vec<Kind>>,
    pub kept: Option<(Kind, &'static str)>,
}

impl StorageUi {
    pub fn bytes(&self, kind: Kind) -> u64 {
        self.usage.get(&kind).copied().unwrap_or(0)
    }

    pub fn total(&self, section: Section) -> u64 {
        rows(section).map(|kind| shown_size(self.bytes(kind))).sum()
    }

    pub fn rows(&self, section: Section) -> impl Iterator<Item = Kind> + '_ {
        rows(section).filter(|kind| kind.listed(self.bytes(*kind)))
    }

    pub fn size_text(&self, kind: Kind) -> String {
        if self.counting.contains(&kind) {
            COUNTING.to_owned()
        } else {
            format_size(self.bytes(kind))
        }
    }

    pub fn total_text(&self, section: Section) -> String {
        if rows(section).any(|kind| self.counting.contains(&kind)) {
            COUNTING.to_owned()
        } else {
            format_size(self.total(section))
        }
    }
}

pub fn rows(section: Section) -> impl Iterator<Item = Kind> {
    section.kinds().filter(|kind| kind.shown())
}

pub fn section_icon(section: Section) -> &'static str {
    match section {
        Section::Data => GroupId::YourData.icon(),
        Section::Cache => GroupId::Cache.icon(),
    }
}

const ROW_GAP: f32 = 8.0;
const TIGHT_GAP: f32 = 3.0;

pub fn show(ui: &mut Ui, state: &mut StorageUi, data: &mut YourDataUi) {
    let mut request = None;
    let every: Vec<Kind> = Section::iter()
        .flat_map(|section| state.rows(section))
        .collect();
    let fit = Fit::new(ui, state, &every);
    Page::show(ui, |page| {
        for section in Section::iter() {
            let total = state.total_text(section);
            let heading = Heading::new(section_icon(section), section.title())
                .aside(&total)
                .aside_inset(fit.clear_column());
            page.section(heading, |ui| {
                Form::show(ui, ("storage", section), |form| {
                    form.custom(|ui| {
                        let listed: Vec<Kind> = state.rows(section).collect();
                        for kind in listed {
                            if let Some(clear) = fitted_row(ui, state, data, kind, fit) {
                                request = Some(clear);
                            }
                        }
                    });
                    if section == Section::Data {
                        your_data::actions(form, data);
                    }
                    if section == Section::Cache {
                        form.custom(|ui| {
                            ui.horizontal(|ui| {
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    let empty = state.total(Section::Cache) == 0;
                                    let text = format!("{} {CLEAR_ALL}", icons::BROOM);
                                    let button = egui::Button::new(text)
                                        .min_size(vec2(0.0, your_data::BUTTON_HEIGHT));
                                    if ui.add_enabled(!empty, button).clicked() {
                                        request = Some(Request::ClearAll);
                                    }
                                });
                            });
                        });
                    }
                })
            });
        }
    });
    if request.is_some() {
        state.request = request;
    }
}

#[derive(Clone, Copy)]
struct Fit {
    gap: f32,
    clear_width: f32,
}

impl Fit {
    fn clear_column(self) -> f32 {
        if self.clear_width > 0.0 {
            self.gap + self.clear_width
        } else {
            0.0
        }
    }

    fn new(ui: &Ui, state: &StorageUi, kinds: &[Kind]) -> Self {
        let body = |text: &str| text_width(ui, text, egui::TextStyle::Body);
        let widest =
            |width: &dyn Fn(Kind) -> f32| kinds.iter().map(|kind| width(*kind)).fold(0.0, f32::max);
        let size_width = widest(&|kind| body(&state.size_text(kind)));
        let name_width =
            widest(&|kind| body(kind.name()) + kind.hint().map_or(0.0, |_| info_mark_width(ui)));
        let icon_width = widest(&|kind| body(kind.icon())) + ICON_GAP;
        let cache = kinds.iter().any(|kind| kind.section() == Section::Cache);
        let padding = 2.0 * ui.spacing().button_padding.x;
        let button = |text: &str| text_width(ui, text, egui::TextStyle::Button) + padding;
        let available = ui.available_width();
        let clear_width = if cache { button(icons::BROOM) } else { 0.0 };
        let tiers = [ROW_GAP, TIGHT_GAP].map(|gap| Fit { gap, clear_width });
        let needs = |fit: &Fit| {
            let clear = if cache {
                fit.gap + fit.clear_width
            } else {
                0.0
            };
            icon_width + name_width + fit.gap + size_width + clear
        };
        tiers
            .into_iter()
            .find(|fit| needs(fit) <= available)
            .unwrap_or(tiers[1])
    }
}

pub fn counted_size(state: &StorageUi, data: &YourDataUi, kind: Kind) -> Option<String> {
    if state.counting.contains(&kind) {
        return None;
    }
    let counted = kind.counted(your_data::count(data, kind)?)?;
    Some(format!("{counted} · {}", state.size_text(kind)))
}

fn text_width(ui: &Ui, text: &str, style: egui::TextStyle) -> f32 {
    egui::WidgetText::from(text)
        .into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, style)
        .size()
        .x
}

const ICON_GAP: f32 = 6.0;
const NAME_RING_PAD: f32 = 3.0;

#[cfg(feature = "nds")]
pub fn settings_row(form: &mut Form, state: &StorageUi, kind: Kind) -> Option<Request> {
    use crate::app::widgets::reset_slider::{RESET_GAP, reset_width};
    let bytes = state.bytes(kind);
    let size = state.size_text(kind);
    let add = |ui: &mut Ui| {
        let size_width = text_width(ui, &size, egui::TextStyle::Body);
        crate::app::widgets::settings::form::need(ui, size_width + RESET_GAP + reset_width(ui));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = RESET_GAP;
            let room = (ui.available_width() - reset_width(ui) - RESET_GAP).max(0.0);
            ui.allocate_ui_with_layout(
                vec2(room, ui.spacing().interact_size.y),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_width(room);
                    ui.label(RichText::new(size).weak());
                },
            );
            let broom = crate::app::mobile::compact(ui, egui::Button::new(icons::BROOM))
                .min_size(vec2(reset_width(ui), 0.0));
            let tip = match kind.hint() {
                Some(hint) => format!("{CLEAR}. {hint}"),
                None => CLEAR.to_owned(),
            };
            ui.add_enabled(bytes > 0, broom)
                .on_hover_text(tip)
                .clicked()
        })
        .inner
    };
    let clicked = form.row(kind.name(), add);
    clicked.then_some(Request::Clear(kind))
}

fn name_label(ui: &mut Ui, name: &str, room: f32) {
    let shown = short_name(name, room, |text| {
        text_width(ui, text, egui::TextStyle::Body)
    });
    if shown == name {
        ui.add(egui::Label::new(name).selectable(false).truncate());
        return;
    }
    let label = ui.add(egui::Label::new(shown).selectable(false).truncate());
    let lift = ((ui.spacing().interact_size.y - label.rect.height()) / 2.0).max(0.0);
    let target = label.rect.expand2(vec2(NAME_RING_PAD, lift));
    let whole = ui.interact(target, label.id.with("whole"), egui::Sense::click());
    whole.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, name));
    let tapped_id = whole.id.with("tapped");
    let was_tapped = ui.data(|data| data.get_temp::<bool>(tapped_id).unwrap_or(false));
    let tapped = if whole.clicked_by(egui::PointerButton::Primary) {
        !was_tapped
    } else {
        was_tapped && !whole.clicked_elsewhere()
    };
    if tapped != was_tapped {
        ui.data_mut(|data| data.insert_temp(tapped_id, tapped));
    }
    let focused = whole.has_focus();
    if !(focused || tapped || egui::Tooltip::should_show_tooltip(&whole, false)) {
        return;
    }
    let ring_reach = lift + ui.visuals().widgets.active.expansion;
    let margin = egui::Margin {
        top: ring_reach.round() as i8,
        bottom: ring_reach.round() as i8,
        ..ui.spacing().menu_margin
    };
    egui::Area::new(whole.id.with("popup"))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .fixed_pos(label.rect.min - vec2(f32::from(margin.left), f32::from(margin.top)))
        .show(ui.ctx(), |ui| {
            let framed = egui::Frame::popup(ui.style())
                .inner_margin(margin)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(name)
                            .selectable(false)
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                });
            if focused {
                ui.painter().rect_stroke(
                    framed.response.rect,
                    ui.visuals().menu_corner_radius,
                    crate::app::ui_state::ui_theme::focus_ring_stroke(ui.ctx()),
                    egui::StrokeKind::Inside,
                );
            }
        });
}

fn short_name(name: &str, room: f32, measure: impl Fn(&str) -> f32) -> String {
    let short = crate::app::tabs::shorten(name, room, measure);
    let Some(kept) = short.strip_suffix('…') else {
        return short;
    };
    let mid_word = !name[kept.len()..].starts_with(' ');
    match kept.rsplit_once(' ') {
        Some((before, fragment)) if mid_word && fragment.chars().count() < 3 => {
            format!("{}…", before.trim_end())
        }
        _ => short,
    }
}

fn fitted_row(
    ui: &mut Ui,
    state: &StorageUi,
    data: &YourDataUi,
    kind: Kind,
    fit: Fit,
) -> Option<Request> {
    let bytes = state.bytes(kind);
    let height = ui.spacing().interact_size.y;
    let cache = kind.section() == Section::Cache;
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.set_min_height(height);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if cache {
                let button = egui::Button::new(icons::BROOM).min_size(vec2(fit.clear_width, 0.0));
                clicked = ui
                    .add_enabled(bytes > 0, button)
                    .on_hover_text(CLEAR)
                    .clicked();
                ui.add_space(fit.gap);
            } else {
                ui.add_space(fit.clear_column());
            }
            let note = state
                .kept
                .filter(|(kept, _)| *kept == kind)
                .map(|(_, note)| note);
            let mark = kind.hint().map_or(0.0, |_| info_mark_width(ui) + ICON_GAP);
            let note_width = note.map_or(0.0, |note| {
                text_width(ui, note, egui::TextStyle::Small) + ICON_GAP
            });
            let body = |text: &str| text_width(ui, text, egui::TextStyle::Body);
            let name_need =
                body(kind.icon()) + ICON_GAP + body(kind.name()) + mark + note_width + fit.gap;
            let amount = counted_size(state, data, kind)
                .filter(|full| name_need + body(full) <= ui.available_width())
                .unwrap_or_else(|| state.size_text(kind));
            ui.label(RichText::new(amount).weak());
            ui.add_space(fit.gap);
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = ICON_GAP;
                ui.label(kind.icon());
                let room = (ui.available_width() - mark - note_width).max(0.0);
                name_label(ui, kind.name(), room);
                if let Some(hint) = kind.hint() {
                    info_mark(ui, hint);
                }
                if let Some(note) = note {
                    ui.label(RichText::new(note).weak().small());
                }
            });
        });
    });
    clicked.then_some(Request::Clear(kind))
}

pub fn ask_message(kinds: &[Kind]) -> String {
    match kinds {
        [kind] => format!("Clear {}?", kind.in_sentence()),
        _ => "Clear the cache?".to_owned(),
    }
}

pub struct ClearAsk {
    pub list: Vec<String>,
    pub notes: Vec<String>,
}

pub fn clear_ask(state: &StorageUi, kinds: &[Kind]) -> ClearAsk {
    let held: Vec<Kind> = match kinds {
        [kind] => vec![*kind],
        _ => kinds
            .iter()
            .copied()
            .filter(|kind| state.bytes(*kind) > 0)
            .collect(),
    };
    ClearAsk {
        list: held
            .iter()
            .map(|kind| format!("{}, {}", kind.name(), state.size_text(*kind)))
            .collect(),
        notes: costs(&held),
    }
}

pub const DATA_STAYS: &str = "Your saves and everything else in Your data stay.";
pub const COMES_BACK: &str = "The app downloads or rebuilds each one when it next needs it.";

fn costs(kinds: &[Kind]) -> Vec<String> {
    let mut notes = vec![DATA_STAYS.to_owned()];
    match kinds {
        [kind] => notes.extend(cost(*kind).map(str::to_owned)),
        _ => notes.push(COMES_BACK.to_owned()),
    }
    notes
}

fn cost(kind: Kind) -> Option<&'static str> {
    match kind {
        Kind::Covers => Some("Game covers download again."),
        Kind::GameInfo => Some("Game info downloads again."),
        Kind::AchievementBadges => Some("Achievement badges download again."),
        Kind::GameListIndex => {
            Some("The game list index builds again, so the library reads each game file again.")
        }
        Kind::CompiledCode => {
            Some("Compiled game code compiles again, so each game starts slower once.")
        }
        Kind::VideoCodec => Some("The video codec downloads again the next time you record."),
        Kind::GameArchive => Some("Game Archive games download again when you play them."),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn megabytes(text: &str) -> f64 {
        let (number, unit) = text.split_once(' ').unwrap();
        let value: f64 = number.parse().unwrap();
        match unit {
            "B" => value / 1_048_576.0,
            "KB" => value / 1024.0,
            "MB" => value,
            _ => value * 1024.0,
        }
    }

    #[test]
    fn a_section_total_is_the_sum_of_its_rows_as_shown() {
        let mut state = StorageUi::default();
        for (kind, megs) in [
            (Kind::States, 13.44),
            (Kind::Screenshots, 6.34),
            (Kind::Recordings, 69.04),
            (Kind::GameCopies, 16.04),
        ] {
            state.usage.insert(kind, (megs * 1_048_576.0_f64) as u64);
        }
        state.usage.insert(Kind::Saves, 480 * 1024 + 300);
        let rows_sum: f64 = rows(Section::Data)
            .map(|kind| megabytes(&format_size(state.bytes(kind))))
            .sum();
        assert_eq!(
            format_size(state.total(Section::Data)),
            format!("{rows_sum:.1} MB")
        );
    }

    #[test]
    fn a_clear_ask_lists_each_kind_with_its_size_and_says_what_each_costs() {
        let mut state = StorageUi::default();
        state.usage.insert(Kind::Covers, 3 << 20);
        state.usage.insert(Kind::AchievementBadges, 1 << 20);
        state.usage.insert(Kind::GameArchive, 40 << 20);
        let single = clear_ask(&state, &[Kind::Covers]);
        assert_eq!(single.list, vec!["Game covers, 3.0 MB".to_owned()]);
        assert_eq!(
            single.notes,
            vec![
                DATA_STAYS.to_owned(),
                "Game covers download again.".to_owned()
            ]
        );
        let info = clear_ask(&state, &[Kind::GameInfo]);
        assert_eq!(info.list, vec!["Game info, 0 B".to_owned()]);
        assert_eq!(
            info.notes,
            vec![
                DATA_STAYS.to_owned(),
                "Game info downloads again.".to_owned()
            ]
        );
        let all = clear_ask(
            &state,
            &[
                Kind::Covers,
                Kind::GameInfo,
                Kind::AchievementBadges,
                Kind::GameArchive,
            ],
        );
        assert_eq!(
            all.list,
            vec![
                "Game covers, 3.0 MB".to_owned(),
                "Achievement badges, 1.0 MB".to_owned(),
                "Game Archive, 40.0 MB".to_owned(),
            ]
        );
        assert_eq!(
            all.notes,
            vec![DATA_STAYS.to_owned(), COMES_BACK.to_owned()]
        );
        let one_held = clear_ask(&state, &[Kind::GameInfo, Kind::AchievementBadges]);
        assert_eq!(
            one_held.notes,
            vec![
                DATA_STAYS.to_owned(),
                "Achievement badges download again.".to_owned()
            ]
        );
    }

    #[test]
    fn a_clear_all_ask_says_what_it_costs_in_two_lines() {
        let mut state = StorageUi::default();
        let cache: Vec<Kind> = Kind::iter()
            .filter(|kind| kind.section() == Section::Cache)
            .collect();
        for kind in &cache {
            state.usage.insert(*kind, 1 << 20);
        }
        let all = clear_ask(&state, &cache);
        assert_eq!(all.list.len(), cache.len());
        assert_eq!(
            all.notes,
            vec![DATA_STAYS.to_owned(), COMES_BACK.to_owned()]
        );
        for kind in &cache {
            let notes = clear_ask(&state, &[*kind]).notes;
            assert_eq!(notes.first().map(String::as_str), Some(DATA_STAYS));
            assert!(notes.len() <= 2, "{notes:?}");
            if let Some(note) = notes.get(1) {
                assert!(note.ends_with('.'), "{note:?} is not a sentence");
                assert!(
                    note.to_lowercase().contains(&kind.name().to_lowercase()),
                    "{note:?} doesn't name {}",
                    kind.name()
                );
            }
        }
    }

    #[test]
    fn data_and_cache_names_start_in_one_column_at_every_width() {
        let mut state = StorageUi::default();
        for kind in Kind::iter() {
            state.usage.insert(kind, 128 << 20);
        }
        for width in [217.0, 269.0, 330.0, 400.0] {
            let ctx = egui::Context::default();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 4000.0),
                )),
                ..Default::default()
            };
            let mut starts = (None, None);
            for _ in 0..2 {
                let output = ctx.run(input.clone(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.allocate_ui(egui::vec2(width, 4000.0), |ui| {
                            show(ui, &mut state, &mut YourDataUi::default())
                        });
                    });
                });
                for clipped in output.shapes {
                    if let egui::Shape::Text(text) = clipped.shape {
                        let shown = text.galley.text();
                        if shown == "Saves" {
                            starts.0 = Some(text.pos.x);
                        } else if shown.starts_with("Game c") && !shown.starts_with("Game copies") {
                            starts.1 = Some(text.pos.x);
                        }
                    }
                }
            }
            let (Some(data), Some(cache)) = starts else {
                panic!("{width}: {starts:?}");
            };
            assert!(
                (data - cache).abs() < 0.5,
                "{width}: Saves at {data}, Game covers at {cache}"
            );
        }
    }

    #[test]
    fn every_size_and_both_totals_end_at_one_x() {
        let mut state = StorageUi::default();
        state.usage.insert(Kind::Saves, 480 << 10);
        state.usage.insert(Kind::States, 1 << 20);
        state.usage.insert(Kind::Covers, 10 << 20);
        state.usage.insert(Kind::GameInfo, 2 << 20);
        let sizes = ["480 KB", "1.0 MB", "1.5 MB", "10.0 MB", "2.0 MB", "12.0 MB"];
        for width in [269.0, 400.0] {
            let ctx = egui::Context::default();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 4000.0),
                )),
                ..Default::default()
            };
            let mut ends = std::collections::BTreeMap::new();
            for _ in 0..2 {
                let output = ctx.run(input.clone(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.allocate_ui(egui::vec2(width, 4000.0), |ui| {
                            show(ui, &mut state, &mut YourDataUi::default())
                        });
                    });
                });
                for clipped in output.shapes {
                    if let egui::Shape::Text(text) = clipped.shape
                        && sizes.contains(&text.galley.text())
                    {
                        ends.insert(
                            text.galley.text().to_owned(),
                            text.galley.rect.right() + text.pos.x,
                        );
                    }
                }
            }
            assert_eq!(ends.len(), sizes.len(), "{width}: {ends:?}");
            let right = ends.values().fold(f32::MIN, |a, b| a.max(*b));
            for (text, end) in &ends {
                assert!(
                    (right - end).abs() < 0.5,
                    "{width}: {text} ends at {end}, others at {right}"
                );
            }
        }
    }

    #[test]
    fn every_row_keeps_its_icon_and_clear_all_ends_in_the_clear_column_at_every_width() {
        let mut state = StorageUi::default();
        for kind in Kind::iter() {
            state.usage.insert(kind, 128 << 20);
        }
        let row_icons: Vec<&str> = Section::iter()
            .flat_map(|section| state.rows(section))
            .map(Kind::icon)
            .collect();
        let clear_all = format!("{} {CLEAR_ALL}", icons::BROOM);
        for width in [217.0, 240.0, 257.0, 400.0, 496.0] {
            let ctx = egui::Context::default();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 4000.0),
                )),
                ..Default::default()
            };
            let mut shown = BTreeSet::new();
            let mut clear_ends = Vec::new();
            let mut clear_all_end = None;
            for _ in 0..2 {
                shown.clear();
                clear_ends.clear();
                let output = ctx.run(input.clone(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.allocate_ui(egui::vec2(width, 4000.0), |ui| {
                            show(ui, &mut state, &mut YourDataUi::default())
                        });
                    });
                });
                for clipped in output.shapes {
                    if let egui::Shape::Text(text) = clipped.shape {
                        let end = text.galley.rect.right() + text.pos.x;
                        let shown_text = text.galley.text().to_owned();
                        if shown_text == icons::BROOM {
                            clear_ends.push(end);
                        } else if shown_text == clear_all {
                            clear_all_end = Some(end);
                        }
                        shown.insert(shown_text);
                    }
                }
            }
            for icon in &row_icons {
                assert!(shown.contains(*icon), "{width}: an icon is missing");
            }
            assert!(!shown.contains(CLEAR), "{width}: a Clear button shows text");
            let end = clear_all_end.unwrap_or_else(|| panic!("{width}: no Clear all"));
            assert!(!clear_ends.is_empty(), "{width}: no Clear buttons");
            for clear in &clear_ends {
                assert!(
                    (clear - end).abs() < 0.5,
                    "{width}: Clear ends at {clear}, Clear all at {end}"
                );
            }
        }
    }

    #[test]
    fn a_cut_name_ends_on_a_whole_word_or_three_letters_with_no_space() {
        let measure = |text: &str| text.chars().count() as f32 * 10.0;
        assert_eq!(short_name("Saves", 50.0, measure), "Saves");
        assert_eq!(
            short_name("BIOS and boot ROMs", 150.0, measure),
            "BIOS and boot…"
        );
        assert_eq!(
            short_name("Achievement badges", 150.0, measure),
            "Achievement…"
        );
        assert_eq!(
            short_name("Achievement badges", 160.0, measure),
            "Achievement bad…"
        );
    }

    #[test]
    fn names_run_to_their_size_and_a_cut_one_shows_whole_on_focus() {
        let mut state = StorageUi::default();
        for kind in Kind::iter() {
            let bytes = if kind.section() == Section::Cache {
                128 << 30
            } else {
                0
            };
            state.usage.insert(kind, bytes);
        }
        let ctx = egui::Context::default();
        let mut frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 4000.0),
                )),
                events,
                ..Default::default()
            };
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui(egui::vec2(217.0, 4000.0), |ui| {
                        show(ui, &mut state, &mut YourDataUi::default())
                    });
                });
            });
            output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                    _ => None,
                })
                .collect::<Vec<String>>()
        };
        frame(Vec::new());
        let texts = frame(Vec::new());
        for kind in rows(Section::Data).filter(|kind| kind.listed(0)) {
            assert!(
                texts.iter().any(|text| text == kind.name()),
                "{} is cut at 217 pt: {texts:?}",
                kind.name()
            );
        }
        let cut: Vec<(Kind, String)> = rows(Section::Cache)
            .filter_map(|kind| {
                let shown = texts.iter().find(|text| {
                    text.ends_with('…') && kind.name().starts_with(text.trim_end_matches('…'))
                })?;
                Some((kind, shown.clone()))
            })
            .collect();
        assert!(!cut.is_empty(), "no name is cut at 217 pt: {texts:?}");
        for (kind, shown) in &cut {
            let kept = shown.trim_end_matches('…');
            assert_eq!(kept, kept.trim_end(), "{shown}");
            let mid_word = !kind.name()[kept.len()..].starts_with(' ');
            let fragment = kept.rsplit(' ').next().unwrap_or(kept);
            assert!(!mid_word || fragment.chars().count() >= 3, "{shown}");
        }
        let tab = egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let mut read = BTreeSet::new();
        for _ in 0..80 {
            let mut seen = frame(vec![tab.clone()]);
            seen.extend(frame(Vec::new()));
            for (kind, _) in &cut {
                if seen.iter().any(|text| text == kind.name()) {
                    read.insert(*kind);
                }
            }
        }
        for (kind, shown) in &cut {
            assert!(read.contains(kind), "Tab never shows {shown} whole");
        }
    }

    #[test]
    fn a_cut_name_shows_whole_in_its_own_row_and_its_focus_is_a_ring() {
        let mut state = StorageUi::default();
        for kind in Kind::iter() {
            state.usage.insert(kind, 128 << 30);
        }
        let ctx = egui::Context::default();
        let mut frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 4000.0),
                )),
                events,
                ..Default::default()
            };
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui(egui::vec2(217.0, 4000.0), |ui| {
                        show(ui, &mut state, &mut YourDataUi::default())
                    });
                });
            });
            output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) => Some((
                        text.galley.text().to_owned(),
                        text.galley.rect.translate(text.pos.to_vec2()),
                        text.underline,
                    )),
                    _ => None,
                })
                .collect::<Vec<(String, egui::Rect, egui::Stroke)>>()
        };
        frame(Vec::new());
        let shown = frame(Vec::new());
        let cut: Vec<&str> = Kind::iter()
            .map(Kind::name)
            .filter(|name| {
                shown.iter().any(|(text, _, _)| {
                    text.ends_with('…') && name.starts_with(text.trim_end_matches('…'))
                })
            })
            .collect();
        assert!(!cut.is_empty(), "no name is cut at 217 pt");
        let row_names: Vec<egui::Rect> = shown
            .iter()
            .filter(|(text, _, _)| {
                text.ends_with('…') || Kind::iter().any(|kind| kind.name() == text)
            })
            .map(|(_, rect, _)| *rect)
            .collect();
        let row_height = ctx.style().spacing.interact_size.y;
        let tab = egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let mut opened = BTreeSet::new();
        for _ in 0..80 {
            frame(vec![tab.clone()]);
            let texts = frame(Vec::new());
            let Some(focused) = ctx
                .memory(|m| m.focused())
                .and_then(|id| ctx.read_response(id))
            else {
                continue;
            };
            for (text, rect, underline) in &texts {
                assert_eq!(*underline, egui::Stroke::NONE, "{text} is underlined");
                if !cut.contains(&text.as_str()) {
                    continue;
                }
                assert!(
                    focused.rect.height() >= row_height - 0.5,
                    "{text}: the ring target {:?} is shorter than a row",
                    focused.rect
                );
                assert!(
                    focused.rect.y_range().contains(rect.center().y),
                    "{text} at {rect:?} opens outside its row at {:?}",
                    focused.rect
                );
                for other in row_names
                    .iter()
                    .filter(|other| !focused.rect.contains(other.center()))
                {
                    assert!(
                        !rect.intersects(*other),
                        "{text} at {rect:?} covers the row name at {other:?}"
                    );
                }
                opened.insert(text.clone());
            }
        }
        assert_eq!(opened.len(), cut.len(), "{opened:?} of {cut:?}");
    }

    #[test]
    fn a_kind_still_counting_shows_counting_in_its_row_and_its_section_total() {
        let mut state = StorageUi::default();
        state.usage.insert(Kind::Saves, 2 << 20);
        state.usage.insert(Kind::Covers, 1 << 20);
        state.counting.insert(Kind::GameCopies);
        assert_eq!(state.size_text(Kind::GameCopies), COUNTING);
        assert_eq!(state.total_text(Section::Data), COUNTING);
        assert_eq!(state.total_text(Section::Cache), "1.0 MB");
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 4000.0),
            )),
            ..Default::default()
        };
        let mut shown = Vec::new();
        for _ in 0..2 {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui(egui::vec2(400.0, 4000.0), |ui| {
                        show(ui, &mut state, &mut YourDataUi::default())
                    });
                });
            });
            shown = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                    _ => None,
                })
                .collect();
        }
        let counting = shown.iter().filter(|text| *text == COUNTING).count();
        assert_eq!(counting, 2, "{shown:?}");
        assert!(shown.iter().any(|text| text == "1.0 MB"), "{shown:?}");
    }

    #[test]
    fn a_focused_cut_name_wears_its_ring_on_the_whole_name_on_top() {
        let mut state = StorageUi::default();
        for kind in Kind::iter() {
            state.usage.insert(kind, 128 << 30);
        }
        let ctx = egui::Context::default();
        let ring = crate::app::ui_state::ui_theme::focus_ring(&ctx.style().visuals);
        let expansion = ctx.style().visuals.widgets.active.expansion;
        let mut frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 4000.0),
                )),
                events,
                ..Default::default()
            };
            ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui(egui::vec2(217.0, 4000.0), |ui| {
                        show(ui, &mut state, &mut YourDataUi::default())
                    });
                });
            })
            .shapes
        };
        frame(Vec::new());
        frame(Vec::new());
        let names: Vec<&str> = Kind::iter().map(Kind::name).collect();
        let tab = egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let mut ringed = 0;
        for _ in 0..80 {
            frame(vec![tab.clone()]);
            let mut shapes = frame(Vec::new());
            for _ in 0..8 {
                shapes = frame(Vec::new());
            }
            let Some(focused) = ctx
                .memory(|m| m.focused())
                .and_then(|id| ctx.read_response(id))
            else {
                continue;
            };
            let Some((at, text)) =
                shapes
                    .iter()
                    .enumerate()
                    .find_map(|(at, clipped)| match &clipped.shape {
                        egui::Shape::Text(text)
                            if names.contains(&text.galley.text())
                                && focused.rect.y_range().contains(text.pos.y + 1.0) =>
                        {
                            Some((at, text.galley.rect.translate(text.pos.to_vec2())))
                        }
                        _ => None,
                    })
            else {
                continue;
            };
            if !focused.rect.x_range().contains(text.left()) {
                continue;
            }
            let rings: Vec<egui::Rect> = shapes[at..]
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(rect)
                        if rect.stroke.color == ring && rect.stroke.width > 0.0 =>
                    {
                        Some(rect.rect)
                    }
                    _ => None,
                })
                .collect();
            assert!(
                rings.iter().any(|rect| rect.contains_rect(text)
                    && rect.contains_rect(focused.rect.expand(expansion))),
                "the ring around {text:?} is not drawn over the whole name: {rings:?}"
            );
            ringed += 1;
        }
        assert!(ringed > 0, "no cut name took focus");
    }

    #[test]
    fn a_tap_shows_a_cut_name_whole_and_a_tap_elsewhere_closes_it() {
        let mut state = StorageUi::default();
        for kind in Kind::iter() {
            state.usage.insert(kind, 128 << 30);
        }
        let ctx = egui::Context::default();
        let mut frame = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 4000.0),
                )),
                events,
                ..Default::default()
            };
            ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui(egui::vec2(217.0, 4000.0), |ui| {
                        show(ui, &mut state, &mut YourDataUi::default())
                    });
                });
            })
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Text(text) => Some((
                    text.galley.text().to_owned(),
                    text.galley.rect.translate(text.pos.to_vec2()),
                )),
                _ => None,
            })
            .collect::<Vec<(String, egui::Rect)>>()
        };
        let tap = |at: egui::Pos2| {
            let press = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            [
                vec![egui::Event::PointerMoved(at), press(true)],
                vec![press(false)],
                vec![egui::Event::PointerGone],
            ]
        };
        frame(Vec::new());
        let texts = frame(Vec::new());
        let (name, cut) = rows(Section::Cache)
            .find_map(|kind| {
                let (_, rect) = texts.iter().find(|(text, _)| {
                    text.ends_with('…') && kind.name().starts_with(text.trim_end_matches('…'))
                })?;
                Some((kind.name(), *rect))
            })
            .expect("no name is cut at 217 pt");
        let whole_shown =
            |texts: &[(String, egui::Rect)]| texts.iter().any(|(text, _)| text == name);
        assert!(!whole_shown(&texts), "{name} shows whole before a tap");
        for events in tap(cut.center()) {
            frame(events);
        }
        let mut texts = Vec::new();
        for _ in 0..4 {
            texts = frame(Vec::new());
        }
        assert!(
            whole_shown(&texts),
            "a tap on {name} does not show it whole"
        );
        assert!(
            ctx.memory(|memory| memory.focused()).is_none(),
            "a tap on {name} takes the focus, so it would wear the ring"
        );
        for events in tap(egui::pos2(900.0, 3000.0)) {
            frame(events);
        }
        for _ in 0..4 {
            texts = frame(Vec::new());
        }
        assert!(!whole_shown(&texts), "a tap elsewhere leaves {name} whole");
    }
}
