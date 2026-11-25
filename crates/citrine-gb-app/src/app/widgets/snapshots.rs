use crate::storage::SaveStore;
use crate::storage::cloud::OtherBattery;
use citrine_gb::error::GbError;
use egui::emath::GuiRounding;
use egui::{ColorImage, Context, TextureHandle, TextureOptions};

#[derive(Default)]
pub struct SnapshotsState {
    loaded_key: Option<String>,
    slots: Vec<SlotView>,
    renaming: Option<(usize, String)>,
    renaming_battery: Option<String>,
    deleting: Option<(usize, String)>,
    autosaves: Vec<SlotView>,
    filed: Vec<SlotView>,
    all_autosaves: bool,
    deleting_autosave: Option<(usize, String)>,
    loading_autosave: Option<PendingLoad>,
    others: Vec<OtherBattery>,
    using_battery: Option<(String, String)>,
    deleting_battery: Option<(String, String)>,
    cloud_too: bool,
    stale: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingLoad {
    pub id: usize,
    pub label: String,
    pub was_running: bool,
}

pub const FOLDED_AUTOSAVES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutosaveEntry {
    Mine(usize),
    Filed(usize),
}

pub fn newest_first(mine: &[u64], filed: &[u64]) -> Vec<AutosaveEntry> {
    let mut rows: Vec<(u64, AutosaveEntry)> = mine
        .iter()
        .enumerate()
        .map(|(index, at)| (*at, AutosaveEntry::Mine(index)))
        .chain(
            filed
                .iter()
                .enumerate()
                .map(|(index, at)| (*at, AutosaveEntry::Filed(index))),
        )
        .collect();
    rows.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    rows.into_iter().map(|(_, row)| row).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotNews {
    Saved,
    Overwrote,
    Loaded,
    Empty,
    Deleted,
    Renamed,
}

pub fn snapshot_message(news: SnapshotNews, name: &str) -> String {
    match news {
        SnapshotNews::Saved => format!("Saved {name}"),
        SnapshotNews::Overwrote => format!("Overwrote {name}"),
        SnapshotNews::Loaded => format!("Loaded {name}"),
        SnapshotNews::Empty => format!("{name} is empty"),
        SnapshotNews::Deleted => format!("Deleted {name}"),
        SnapshotNews::Renamed => format!("Renamed to {name}"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailedStep {
    Load,
    Save,
    Delete,
    Rename,
    SwitchConsole,
    Import,
    Switch,
    Close,
}

pub fn failed_message(step: FailedStep, what: &str, err: &GbError) -> String {
    log::warn!("{step:?} of {what} failed: {err:?}");
    let text = match step {
        FailedStep::Load => return format!("Couldn't load {what}. {}", load_failure(err)),
        FailedStep::Save => format!("Couldn't save {what}."),
        FailedStep::Delete => format!("Couldn't delete {what}."),
        FailedStep::Rename => format!("Couldn't rename {what}."),
        FailedStep::SwitchConsole => format!("Couldn't switch {what}."),
        FailedStep::Import => format!("Couldn't import {what}."),
        FailedStep::Switch => format!("Couldn't switch to {what}."),
        FailedStep::Close => format!("Couldn't save {what}, so it stays open."),
    };
    match write_failure(err) {
        Some(reason) => format!("{text} {reason}"),
        None => text,
    }
}

pub fn named(name: &str, fallback: &str) -> String {
    if name.is_empty() {
        fallback.to_string()
    } else {
        format!("'{name}'")
    }
}

fn write_failure(err: &GbError) -> Option<&'static str> {
    let GbError::IO(err) = err else {
        return None;
    };
    (err.kind() == std::io::ErrorKind::StorageFull).then_some("The device is out of storage.")
}

const OTHER_CONSOLE: &str = "It's for a different console.";

fn load_failure(err: &GbError) -> &'static str {
    match err {
        #[cfg(feature = "gba")]
        GbError::NewerState => "It needs a newer version of Citrine.",
        #[cfg(feature = "gba")]
        GbError::OlderState => "It was saved by an older version of Citrine.",
        GbError::RmpDecode(_) => "It was saved by a different version of Citrine.",
        GbError::OtherGame => "It belongs to another game.",
        #[cfg(feature = "gba")]
        GbError::NotGba => OTHER_CONSOLE,
        #[cfg(feature = "nds")]
        GbError::NotNds => OTHER_CONSOLE,
        GbError::GbaGame
        | GbError::NdsGame
        | GbError::MissingRomCartridgeType
        | GbError::RomTooSmall
        | GbError::RomTooBig => OTHER_CONSOLE,
        _ => "The file is damaged.",
    }
}

pub fn quick_slot_message(keys: Option<&str>, name: &str) -> String {
    match keys {
        Some(keys) => format!("{keys} now use '{name}'"),
        None => format!("Quick save now uses '{name}'"),
    }
}

pub enum NameEdit {
    Editing,
    Commit,
    Cancel,
}

pub fn name_editor(ui: &mut egui::Ui, buffer: &mut String, hint: &str) -> NameEdit {
    let side = crate::app::mobile::touch_square(ui);
    let width = if side > 0.0 {
        ui.available_width()
    } else {
        180.0
    };
    let response = ui.add(
        egui::TextEdit::singleline(buffer)
            .desired_width(width)
            .min_size(egui::vec2(0.0, side))
            .vertical_align(egui::Align::Center)
            .hint_text(hint)
            .char_limit(crate::storage::MAX_SAVE_NAME),
    );

    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        NameEdit::Cancel
    } else if response.lost_focus() {
        NameEdit::Commit
    } else {
        if !response.has_focus() {
            response.request_focus();
        }
        NameEdit::Editing
    }
}

pub struct SlotView {
    pub slot: usize,
    pub saved_at: u64,
    pub texture: TextureHandle,
    pub name: Option<String>,
    pub console: Option<citrine_gb::console::ConsoleKind>,
}

impl SlotView {
    pub fn display_name(&self) -> String {
        match &self.name {
            Some(name) => name.clone(),
            None => crate::storage::default_slot_name(self.slot),
        }
    }
}

impl SnapshotsState {
    pub fn sync(&mut self, ctx: &Context, store: &SaveStore, rom_key: &str) {
        if !self.stale && self.loaded_key.as_deref() == Some(rom_key) {
            return;
        }
        self.reload(ctx, store, rom_key);
    }

    pub fn reload(&mut self, ctx: &Context, store: &SaveStore, rom_key: &str) {
        let same_game = self.loaded_key.as_deref() == Some(rom_key);
        let (mut filed, slots): (Vec<_>, Vec<_>) = store
            .snapshot_infos(rom_key)
            .into_iter()
            .partition(|info| info.filed);
        filed.sort_by_key(|info| std::cmp::Reverse(info.saved_at));
        self.slots = views(ctx, slots, "snapshot", rom_key);
        self.filed = views(ctx, filed, "snapshot", rom_key);
        self.autosaves = views(ctx, store.autosave_infos(rom_key), "autosave", rom_key);
        self.others = store.other_batteries(rom_key);
        self.loaded_key = Some(rom_key.to_string());
        self.stale = false;
        if !same_game {
            self.renaming = None;
            self.deleting = None;
            self.deleting_autosave = None;
            self.all_autosaves = false;
            self.loading_autosave = None;
            self.using_battery = None;
            self.deleting_battery = None;
        }
    }

    pub fn other_batteries(&self) -> &[OtherBattery] {
        &self.others
    }

    pub fn cloud_too(&mut self) -> &mut bool {
        &mut self.cloud_too
    }

    pub fn ask_use_battery(&mut self, token: String, name: String) {
        self.using_battery = Some((token, name));
    }

    pub fn use_battery_question(&self) -> Option<String> {
        self.using_battery.as_ref().map(|(_, name)| {
            format!("Play with '{name}'? The battery save you use now moves to its place.")
        })
    }

    pub fn answer_use_battery(&mut self, choice: Option<bool>) -> Option<(String, String)> {
        let confirmed = choice?;
        self.using_battery.take().filter(|_| confirmed)
    }

    pub fn ask_delete_battery(&mut self, token: String, name: String) {
        self.cloud_too = true;
        self.deleting_battery = Some((token, name));
    }

    pub fn delete_battery_question(&self) -> Option<(String, String)> {
        self.deleting_battery
            .as_ref()
            .map(|(token, name)| (token.clone(), format!("Delete {name}?")))
    }

    pub fn answer_delete_battery(&mut self, choice: Option<bool>) -> Option<(String, String)> {
        let confirmed = choice?;
        self.deleting_battery.take().filter(|_| confirmed)
    }

    pub fn invalidate(&mut self) {
        self.stale = true;
    }

    pub fn slots(&self) -> &[SlotView] {
        &self.slots
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn autosaves(&self) -> &[SlotView] {
        &self.autosaves
    }

    pub fn filed_autosaves(&self) -> &[SlotView] {
        &self.filed
    }

    pub fn autosave_list(&self) -> Vec<AutosaveEntry> {
        let times = |views: &[SlotView]| views.iter().map(|view| view.saved_at).collect::<Vec<_>>();
        let mut rows = newest_first(&times(&self.autosaves), &times(&self.filed));
        rows.truncate(self.shown_autosaves());
        rows
    }

    pub fn page_times(&self) -> Vec<u64> {
        self.slots
            .iter()
            .chain(&self.autosaves)
            .chain(&self.filed)
            .map(|view| view.saved_at)
            .chain(self.others.iter().filter_map(|other| other.saved_at))
            .collect()
    }

    fn autosave_count(&self) -> usize {
        self.autosaves.len() + self.filed.len()
    }

    pub fn shown_autosaves(&self) -> usize {
        if self.all_autosaves {
            self.autosave_count()
        } else {
            self.autosave_count().min(FOLDED_AUTOSAVES)
        }
    }

    pub fn fold_label(&self) -> Option<String> {
        if self.autosave_count() <= FOLDED_AUTOSAVES {
            None
        } else if self.all_autosaves {
            Some("Show fewer".to_string())
        } else {
            Some(format!("Show all {}", self.autosave_count()))
        }
    }

    pub fn toggle_all_autosaves(&mut self) {
        self.all_autosaves = !self.all_autosaves;
    }

    pub fn ask_delete_autosave(&mut self, id: usize, label: String) {
        self.cloud_too = true;
        self.deleting_autosave = Some((id, label));
    }

    pub fn delete_autosave_question(&self) -> Option<(usize, String)> {
        self.deleting_autosave
            .as_ref()
            .map(|(id, label)| (*id, format!("Delete the autosave from {label}?")))
    }

    pub fn answer_delete_autosave(&mut self, choice: Option<bool>) -> Option<(usize, String)> {
        match choice {
            None => None,
            Some(confirmed) => self.deleting_autosave.take().filter(|_| confirmed),
        }
    }

    pub fn ask_load_autosave(&mut self, load: PendingLoad) {
        self.loading_autosave = Some(load);
    }

    pub fn load_autosave_question(&self) -> Option<String> {
        self.loading_autosave
            .as_ref()
            .map(|load| format!("Load the autosave from {}?", load.label))
    }

    pub fn answer_load_autosave(&mut self, choice: Option<bool>) -> Option<(bool, PendingLoad)> {
        let confirmed = choice?;
        self.loading_autosave.take().map(|load| (confirmed, load))
    }

    pub fn begin_rename(&mut self, slot: usize, current: String) {
        self.renaming_battery = None;
        self.renaming = Some((slot, current));
    }

    pub fn cancel_rename(&mut self) {
        self.renaming = None;
        self.renaming_battery = None;
    }

    pub fn begin_battery_rename(&mut self, current: String) {
        self.renaming = None;
        self.renaming_battery = Some(current);
    }

    pub fn battery_rename_buffer(&mut self) -> Option<&mut String> {
        self.renaming_battery.as_mut()
    }

    pub fn take_battery_rename(&mut self) -> Option<String> {
        self.renaming_battery.take()
    }

    pub fn rename_buffer(&mut self, slot: usize) -> Option<&mut String> {
        match &mut self.renaming {
            Some((editing, text)) if *editing == slot => Some(text),
            _ => None,
        }
    }

    pub fn take_rename(&mut self) -> Option<(usize, String)> {
        self.renaming.take()
    }

    pub fn ask_delete(&mut self, slot: usize, name: String) {
        self.cloud_too = true;
        self.deleting = Some((slot, name));
    }

    pub fn delete_question(&self) -> Option<(usize, String)> {
        self.deleting
            .as_ref()
            .map(|(slot, name)| (*slot, format!("Delete {name}?")))
    }

    pub fn answer_delete(&mut self, choice: Option<bool>) -> Option<usize> {
        match choice {
            None => None,
            Some(confirmed) => self
                .deleting
                .take()
                .filter(|_| confirmed)
                .map(|(slot, _)| slot),
        }
    }
}

fn views(
    ctx: &Context,
    infos: Vec<crate::storage::SnapshotInfo>,
    kind: &str,
    rom_key: &str,
) -> Vec<SlotView> {
    infos
        .into_iter()
        .map(|info| {
            let image = ColorImage::from_rgba_unmultiplied(info.thumb_size(), &info.thumbnail);
            SlotView {
                slot: info.slot,
                saved_at: info.saved_at,
                texture: ctx.load_texture(
                    format!("{kind}_{rom_key}_{}", info.slot),
                    image,
                    TextureOptions::NEAREST,
                ),
                name: info.name,
                console: info.console,
            }
        })
        .collect()
}

pub fn content_end(ui: &egui::Ui) -> f32 {
    ui.cursor().top() - ui.spacing().item_spacing.y
}

pub fn stamp_row_with(
    ui: &mut egui::Ui,
    stamp: String,
    console: Option<citrine_gb::console::ConsoleKind>,
    cloud: Option<crate::online::cloud::SyncState>,
    choose: bool,
) -> StampRow {
    let mut chosen = false;
    let mut tip = None;
    let mut taken = None;
    let height = if cloud.is_some() {
        crate::app::widgets::cloud_mark::height(ui)
    } else {
        0.0
    };
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            match console {
                Some(kind) => tip = Some(stamp_with_badge(ui, stamp, kind)),
                None => {
                    ui.small(stamp);
                }
            }
            if let Some(state) = cloud {
                let mark = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if choose {
                        chosen = ui
                            .button(format!("{} Choose a copy", crate::icons::CLOUD_WARNING))
                            .on_hover_text(state.text())
                            .clicked();
                    } else {
                        crate::app::widgets::cloud_mark::cloud_status(ui, state);
                    }
                });
                taken = Some(mark.response.rect);
            }
        },
    );
    StampRow {
        chosen,
        tip: tip.map(|tip| RowTip {
            tip,
            buttons: ui.cursor().top(),
            taken: taken.unwrap_or(egui::Rect::NOTHING),
        }),
    }
}

pub struct StampRow {
    pub chosen: bool,
    tip: Option<RowTip>,
}

struct RowTip {
    tip: BadgeTip,
    buttons: f32,
    taken: egui::Rect,
}

impl StampRow {
    pub fn show_tip(self, ui: &egui::Ui, card: egui::Rect, name: egui::Rect, above: f32) {
        if let Some(row) = self.tip {
            show_badge_tip(ui, row, card, name, above);
        }
    }
}

const BADGE_TIP_GAP: f32 = 4.0;
const BADGE_TIP_EDGE: f32 = 8.0;
const BADGE_TIP_CLEAR_ABOVE: f32 = 4.0;
const BADGE_TIP_CLEAR_BELOW: f32 = 2.0;
const BADGE_TIP_MARGIN: egui::Margin = egui::Margin::symmetric(6, 4);

struct BadgeTip {
    response: egui::Response,
    badge: egui::Rect,
    date: egui::Rect,
    console: citrine_gb::console::ConsoleKind,
}

fn stamp_with_badge(
    ui: &mut egui::Ui,
    stamp: String,
    console: citrine_gb::console::ConsoleKind,
) -> BadgeTip {
    use crate::app::ui_state::settings::{KIND_ICON_16_SIZE, kind_icon_16, kind_name};
    let wrap = Some(egui::TextWrapMode::Extend);
    let date = egui::WidgetText::from(egui::RichText::new(stamp.as_str()).small()).into_galley(
        ui,
        wrap,
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    let badge = egui::WidgetText::from(
        egui::RichText::new(kind_icon_16(console))
            .size(KIND_ICON_16_SIZE)
            .color(ui.visuals().weak_text_color()),
    )
    .into_galley(ui, wrap, f32::INFINITY, egui::TextStyle::Body);
    let date_middle =
        ink(ui.ctx(), &date, |c| c.is_ascii_digit()).map_or(date.size().y / 2.0, |ink| ink.middle);
    let badge_middle =
        ink(ui.ctx(), &badge, |_| true).map_or(badge.size().y / 2.0, |ink| ink.middle);
    let reach = (date_middle - date.size().y / 2.0).abs() + KIND_ICON_16_SIZE / 2.0;
    let gap = ui.spacing().item_spacing.x;
    let ppp = ui.pixels_per_point();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(
            date.size().x + gap + badge.size().x,
            (date.size().y.max(2.0 * reach) * ppp).ceil() / ppp,
        ),
        egui::Sense::hover(),
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, stamp.as_str()));
    let date_top = rect.top().round_to_pixels(ppp)
        + ((rect.height() - date.size().y) / 2.0).round_to_pixels(ppp);
    let badge_rect = egui::Rect::from_min_size(
        egui::pos2(
            rect.left() + date.size().x + gap,
            date_top + ((date_middle - badge_middle) * ppp).round() / ppp,
        ),
        badge.size(),
    );
    let date_width = date.size().x;
    let painter = ui.painter();
    painter.galley(
        egui::pos2(rect.left(), date_top),
        date,
        ui.visuals().text_color(),
    );
    painter.galley(badge_rect.min, badge, ui.visuals().weak_text_color());
    let badge_response = ui.interact(badge_rect, response.id.with("badge"), egui::Sense::hover());
    let saved_on = format!("Saved on {}", kind_name(console));
    badge_response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &saved_on));
    BadgeTip {
        response: badge_response,
        badge: badge_rect,
        date: egui::Rect::from_min_size(rect.min, egui::vec2(date_width, rect.height())),
        console,
    }
}

fn show_badge_tip(ui: &egui::Ui, row: RowTip, card: egui::Rect, name: egui::Rect, above: f32) {
    let RowTip {
        tip,
        buttons,
        taken,
    } = row;
    let name_text = crate::app::ui_state::settings::kind_name(tip.console);
    let lines = [
        format!("Saved on {name_text}"),
        format!("Saved on\n{name_text}"),
    ];
    let sizes = [tip_size(ui, &lines[0]), tip_size(ui, &lines[1])];
    let room = TipRoom {
        window: ui.ctx().content_rect().shrink(BADGE_TIP_EDGE),
        badge: tip.badge,
        card,
        ceiling: above,
        stamp: tip.date.top().min(tip.badge.top()),
        buttons,
        name,
        keep: [tip.date, tip.badge, taken],
    };
    let (frame, line) = room.place(sizes);
    let style = ui.style();
    let close = egui::Shadow {
        offset: [0, 1],
        blur: 1,
        spread: 0,
        color: style.visuals.popup_shadow.color,
    };
    let mut tooltip = egui::Tooltip::for_enabled(&tip.response);
    tooltip.popup = tooltip
        .popup
        .anchor(frame)
        .align(egui::RectAlign {
            parent: egui::Align2::LEFT_TOP,
            child: egui::Align2::LEFT_TOP,
        })
        .align_alternatives(&[])
        .gap(0.0)
        .frame(
            egui::Frame::popup(style)
                .inner_margin(BADGE_TIP_MARGIN)
                .shadow(close),
        );
    tooltip.show(|ui| {
        ui.label(&lines[line]);
    });
}

struct TipRoom {
    window: egui::Rect,
    badge: egui::Rect,
    card: egui::Rect,
    ceiling: f32,
    stamp: f32,
    buttons: f32,
    name: egui::Rect,
    keep: [egui::Rect; 3],
}

impl TipRoom {
    fn place(&self, sizes: [egui::Vec2; 2]) -> (egui::Rect, usize) {
        let below = |size| self.beside_card(size, self.card.bottom() + BADGE_TIP_CLEAR_ABOVE);
        [
            (self.right(sizes[0]), 0, false),
            (self.right(sizes[1]), 1, false),
            (self.above(sizes[0]), 0, false),
            (self.above(sizes[1]), 1, false),
            (self.name_row(sizes[0]), 0, true),
            (self.name_row(sizes[1]), 1, true),
            (below(sizes[0]), 0, false),
            (below(sizes[1]), 1, false),
        ]
        .into_iter()
        .find(|(frame, _, over_name)| self.fits(frame, *over_name))
        .map_or((below(sizes[0]), 0), |(frame, line, _)| (frame, line))
    }

    fn right(&self, size: egui::Vec2) -> egui::Rect {
        egui::Rect::from_min_size(
            egui::pos2(
                self.badge.right() + BADGE_TIP_GAP,
                self.buttons - BADGE_TIP_CLEAR_BELOW - size.y,
            ),
            size,
        )
    }

    fn above(&self, size: egui::Vec2) -> egui::Rect {
        self.beside_card(size, self.card.top() - BADGE_TIP_CLEAR_BELOW - size.y)
    }

    fn name_row(&self, size: egui::Vec2) -> egui::Rect {
        self.beside_card(size, self.stamp - size.y)
    }

    fn beside_card(&self, size: egui::Vec2, top: f32) -> egui::Rect {
        let left = (self.badge.right() - size.x)
            .min(self.window.right() - size.x)
            .max(self.window.left());
        egui::Rect::from_min_size(egui::pos2(left, top), size)
    }

    fn fits(&self, frame: &egui::Rect, over_name: bool) -> bool {
        let meets = |zone: egui::Rect| frame.intersect(zone).is_positive();
        let buttons = egui::Rect::from_x_y_ranges(
            self.card.x_range(),
            (self.buttons - BADGE_TIP_CLEAR_BELOW)..=(self.card.bottom() + BADGE_TIP_CLEAR_ABOVE),
        );
        self.window.contains_rect(*frame)
            && frame.top() >= self.ceiling + BADGE_TIP_CLEAR_ABOVE
            && !meets(buttons)
            && (over_name || !meets(self.name))
            && !self.keep.iter().any(|zone| meets(*zone))
    }
}

fn tip_size(ui: &egui::Ui, text: &str) -> egui::Vec2 {
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    let style = ui.style();
    galley.size()
        + BADGE_TIP_MARGIN.sum()
        + egui::Vec2::splat(2.0 * style.visuals.window_stroke.width)
}

#[derive(Clone, Copy)]
struct GlyphRows {
    first: u16,
    end: u16,
    moment: f32,
    weight: f32,
}

type GlyphKey = (char, [u16; 2], [u16; 2]);

#[derive(Clone, Default)]
struct GlyphInk(std::collections::HashMap<GlyphKey, Option<GlyphRows>>);

#[derive(Clone, Copy, Debug)]
struct Ink {
    #[cfg_attr(not(test), allow(dead_code))]
    rows: egui::Rangef,
    middle: f32,
}

fn ink(ctx: &egui::Context, galley: &egui::Galley, keep: impl Fn(char) -> bool) -> Option<Ink> {
    let keep = &keep;
    let glyphs: Vec<(f32, GlyphKey)> = galley
        .rows
        .iter()
        .flat_map(move |row| {
            row.glyphs
                .iter()
                .filter(move |glyph| keep(glyph.chr) && !glyph.uv_rect.is_nothing())
                .map(move |glyph| {
                    let uv = glyph.uv_rect;
                    (
                        row.pos.y + glyph.pos.y + uv.offset.y,
                        (glyph.chr, uv.min, uv.max),
                    )
                })
        })
        .collect();
    let id = egui::Id::new("glyph_ink");
    let mut rows: Vec<Option<Option<GlyphRows>>> = ctx.data_mut(|data| {
        let known = &data.get_temp_mut_or_default::<GlyphInk>(id).0;
        glyphs
            .iter()
            .map(|(_, key)| known.get(key).copied())
            .collect()
    });
    if rows.iter().any(Option::is_none) {
        let atlas = ctx.fonts(|fonts| fonts.image());
        let seen = |(_, min, max): GlyphKey| {
            let coverage = |y: u16| {
                (min[0]..max[0])
                    .map(|x| atlas[(usize::from(x), usize::from(y))].a())
                    .max()
                    .unwrap_or(0)
            };
            let mut inked_rows = (min[1]..max[1]).filter(|&y| coverage(y) >= 128);
            let first = inked_rows.next()?;
            let last = inked_rows.next_back().unwrap_or(first);
            let (moment, weight) = (min[1]..max[1]).fold((0.0, 0.0), |(moment, weight), y| {
                let w = f32::from(coverage(y)) / 255.0;
                (moment + (f32::from(y - min[1]) + 0.5) * w, weight + w)
            });
            Some(GlyphRows {
                first: first - min[1],
                end: last + 1 - min[1],
                moment,
                weight,
            })
        };
        for (row, (_, key)) in rows.iter_mut().zip(&glyphs) {
            if row.is_none() {
                *row = Some(seen(*key));
            }
        }
        ctx.data_mut(|data| {
            let known = &mut data.get_temp_mut_or_default::<GlyphInk>(id).0;
            for (row, (_, key)) in rows.iter().zip(&glyphs) {
                known.insert(*key, row.flatten());
            }
        });
    }
    let ppp = ctx.pixels_per_point();
    let (span, moment, weight) = rows
        .into_iter()
        .zip(&glyphs)
        .filter_map(|(row, (top, _))| {
            let glyph = row.flatten()?;
            Some((
                egui::Rangef::new(
                    top + f32::from(glyph.first) / ppp,
                    top + f32::from(glyph.end) / ppp,
                ),
                top * glyph.weight + glyph.moment / ppp,
                glyph.weight,
            ))
        })
        .reduce(|a, b| {
            (
                egui::Rangef::new(a.0.min.min(b.0.min), a.0.max.max(b.0.max)),
                a.1 + b.1,
                a.2 + b.2,
            )
        })?;
    Some(Ink {
        rows: span,
        middle: moment / weight,
    })
}

pub fn relative_time(saved_at: u64) -> String {
    let now = web_time::SystemTime::now()
        .duration_since(web_time::SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let elapsed = now.saturating_sub(saved_at);
    match elapsed {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", elapsed / 60),
        3600..=86399 => format!("{}h ago", elapsed / 3600),
        _ => format!("{}d ago", elapsed / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [citrine_gb::console::ConsoleKind; 4] = [
        citrine_gb::console::ConsoleKind::Gb,
        citrine_gb::console::ConsoleKind::Gbc,
        citrine_gb::console::ConsoleKind::Gba,
        citrine_gb::console::ConsoleKind::Nds,
    ];
    const NAMES: [&str; 4] = ["Before the gym", "Route 32", "Champion", "Victory Road"];

    struct BadgeRows {
        ctx: egui::Context,
        width: f32,
        indent: f32,
        cards: bool,
        buttons: std::cell::RefCell<Vec<egui::Rect>>,
    }

    impl BadgeRows {
        fn new(width: f32, indent: f32) -> Self {
            let ctx = egui::Context::default();
            crate::app::Citrine::setup_fonts(&ctx);
            ctx.style_mut(|style| style.interaction.tooltip_delay = 0.0);
            Self {
                ctx,
                width,
                indent,
                cards: false,
                buttons: std::cell::RefCell::default(),
            }
        }

        fn cards(width: f32, indent: f32) -> Self {
            Self {
                cards: true,
                ..Self::new(width, indent)
            }
        }

        fn card(&self, ui: &mut egui::Ui, index: usize, above: f32) {
            let kind = KINDS[index];
            let mut name = egui::Rect::NOTHING;
            let mut row = None;
            let card = ui.horizontal(|ui| {
                ui.allocate_exact_size(egui::vec2(60.0, 40.0), egui::Sense::hover());
                ui.vertical(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| name = ui.label(egui::RichText::new(NAMES[index]).strong()).rect,
                    );
                    row = Some(stamp_row_with(
                        ui,
                        "Today 21:04".to_owned(),
                        Some(kind),
                        None,
                        false,
                    ));
                    ui.horizontal(|ui| {
                        for text in ["Load", "Overwrite", crate::icons::DOTS_THREE] {
                            self.buttons.borrow_mut().push(ui.button(text).rect);
                        }
                    });
                });
            });
            if let Some(row) = row {
                row.show_tip(ui, card.response.rect, name, above);
            }
        }

        fn run(&self, events: Vec<egui::Event>, time: f64) -> Vec<(String, egui::Rect, Ink)> {
            self.buttons.borrow_mut().clear();
            let output = self.ctx.run(
                egui::RawInput {
                    events,
                    time: Some(time),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(self.width, 400.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.add_space(40.0);
                        ui.horizontal(|ui| {
                            ui.add_space(self.indent);
                            ui.vertical(|ui| {
                                for (index, kind) in KINDS.into_iter().enumerate() {
                                    let above = content_end(ui);
                                    if !self.cards {
                                        let row = ui.scope(|ui| {
                                            stamp_row_with(
                                                ui,
                                                "Today 21:04".to_owned(),
                                                Some(kind),
                                                None,
                                                false,
                                            )
                                        });
                                        row.inner.show_tip(
                                            ui,
                                            row.response.rect,
                                            egui::Rect::NOTHING,
                                            above,
                                        );
                                        continue;
                                    }
                                    if index > 0 {
                                        ui.separator();
                                    }
                                    self.card(ui, index, above);
                                }
                            });
                        });
                    });
                },
            );
            output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => {
                        let rect = text.galley.rect.translate(text.pos.to_vec2());
                        let digits = ink(&self.ctx, &text.galley, |c| c.is_ascii_digit())
                            .or_else(|| ink(&self.ctx, &text.galley, |_| true))?;
                        Some((
                            text.galley.text().to_owned(),
                            rect,
                            Ink {
                                rows: egui::Rangef::new(
                                    digits.rows.min + text.pos.y,
                                    digits.rows.max + text.pos.y,
                                ),
                                middle: digits.middle + text.pos.y,
                            },
                        ))
                    }
                    _ => None,
                })
                .collect()
        }

        fn hover_gbc(&self) -> Option<(egui::Rect, egui::Rect, String)> {
            use crate::app::ui_state::settings::kind_icon_16;
            use citrine_gb::console::ConsoleKind;
            let gbc = self
                .run(Vec::new(), 0.0)
                .into_iter()
                .find(|(text, ..)| text == kind_icon_16(ConsoleKind::Gbc))
                .map(|(_, rect, _)| rect)
                .unwrap();
            if gbc.right() > self.width - BADGE_TIP_EDGE {
                return None;
            }
            let mut shown = Vec::new();
            for frame in 1..4 {
                shown = self.run(
                    vec![egui::Event::PointerMoved(gbc.center())],
                    f64::from(frame) * 0.1,
                );
            }
            let (text, tip) = shown
                .into_iter()
                .find(|(text, ..)| text.starts_with("Saved on"))
                .map(|(text, rect, _)| (text, rect))
                .expect("hovering the badge names its console");
            let frame = self.frame(tip);
            let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(self.width, 400.0));
            assert!(
                window.contains_rect(frame),
                "{frame:?} leaves the window {window:?}"
            );
            Some((gbc, frame, text))
        }

        fn frame(&self, tip: egui::Rect) -> egui::Rect {
            let style = self.ctx.style();
            (tip + BADGE_TIP_MARGIN).expand(style.visuals.window_stroke.width)
        }

        fn assert_covers_nothing(&self, gbc: egui::Rect, frame: egui::Rect) -> f32 {
            use crate::app::ui_state::settings::kind_icon_16;
            let overlaps = |zone: egui::Rect| frame.intersect(zone).is_positive();
            let own = |rect: &egui::Rect| rect.y_range().contains(gbc.center().y);
            let badges: Vec<String> = KINDS
                .iter()
                .map(|kind| kind_icon_16(*kind).to_owned())
                .collect();
            let mut clearance = f32::INFINITY;
            for (text, rect, ink) in &self.run(Vec::new(), 1.0) {
                let zone = if badges.contains(text) {
                    let zone = egui::Rect::from_x_y_ranges(rect.x_range(), ink.rows);
                    let apart = [
                        zone.left() - frame.right(),
                        frame.left() - zone.right(),
                        zone.top() - frame.bottom(),
                        frame.top() - zone.bottom(),
                    ];
                    clearance = clearance.min(apart.into_iter().fold(0.0, f32::max));
                    zone
                } else if NAMES.contains(&text.as_str()) {
                    if text == NAMES[1] && frame.bottom() <= gbc.top() {
                        continue;
                    }
                    *rect
                } else if text == "Today 21:04" && own(rect) {
                    *rect
                } else {
                    continue;
                };
                assert!(!overlaps(zone), "{frame:?} covers {text:?} at {zone:?}");
            }
            for button in self.buttons.borrow().iter() {
                let zone = egui::Rect::from_min_max(
                    button.min - egui::vec2(0.0, BADGE_TIP_CLEAR_BELOW),
                    button.max + egui::vec2(0.0, BADGE_TIP_CLEAR_ABOVE),
                );
                assert!(
                    !overlaps(zone),
                    "{frame:?} comes within reach of the button {button:?}"
                );
            }
            clearance
        }
    }

    #[test]
    fn saved_on_opens_beside_its_badge_above_the_buttons() {
        let rows = BadgeRows::cards(400.0, 0.0);
        let (gbc, frame, text) = rows.hover_gbc().unwrap();
        assert_eq!(text, "Saved on Game Boy Color");
        assert!(
            frame.left() >= gbc.right(),
            "{frame:?} starts left of {gbc:?}"
        );
        rows.assert_covers_nothing(gbc, frame);
    }

    #[test]
    fn saved_on_keeps_its_card_readable_at_any_distance_from_the_edge() {
        for zoom in [1.0, 1.5] {
            let mut two_lines = false;
            let mut over_name = false;
            let mut clearance = f32::INFINITY;
            for indent in (0..=300).step_by(10) {
                let rows = BadgeRows::cards(400.0, indent as f32);
                rows.ctx.set_zoom_factor(zoom);
                rows.run(Vec::new(), 0.0);
                let Some((gbc, frame, text)) = rows.hover_gbc() else {
                    break;
                };
                clearance = clearance.min(rows.assert_covers_nothing(gbc, frame));
                if frame.left() >= gbc.right() {
                    two_lines |= text.contains('\n');
                } else {
                    over_name |= frame.bottom() <= gbc.top();
                }
            }
            assert!(
                two_lines,
                "no distance from the edge wraps the hint at {zoom}x"
            );
            assert!(
                over_name,
                "no distance from the edge opens the hint over its card's name at {zoom}x"
            );
            println!("badge clearance at {zoom}x: {clearance} pt");
        }
    }

    #[test]
    fn each_badge_centres_on_its_date() {
        for zoom in [1.0, 1.5, 2.0] {
            badges_centre_on_their_dates(zoom);
        }
    }

    fn badges_centre_on_their_dates(zoom: f32) {
        use crate::app::ui_state::settings::kind_icon_16;
        use citrine_gb::console::ConsoleKind;
        let rows = BadgeRows::new(400.0, 0.0);
        rows.ctx.set_zoom_factor(zoom);
        let texts = rows.run(Vec::new(), 0.0);
        let dates: Vec<_> = texts
            .iter()
            .filter(|(text, ..)| text == "Today 21:04")
            .map(|(_, _, ink)| *ink)
            .collect();
        assert_eq!(dates.len(), 4);
        let pitches: Vec<f32> = dates
            .windows(2)
            .map(|w| w[1].middle - w[0].middle)
            .collect();
        let spacing = rows.ctx.style().spacing.item_spacing.y * zoom;
        let slack = if (spacing - spacing.round()).abs() < 0.01 {
            0.01
        } else {
            1.0 / zoom + 0.01
        };
        for pitch in &pitches {
            assert!(
                (pitch - pitches[0]).abs() <= slack,
                "rows {pitches:?} apart at {zoom}x"
            );
        }
        for (row, kind) in [
            ConsoleKind::Gb,
            ConsoleKind::Gbc,
            ConsoleKind::Gba,
            ConsoleKind::Nds,
        ]
        .into_iter()
        .enumerate()
        {
            let badge = texts
                .iter()
                .find(|(text, ..)| text == kind_icon_16(kind))
                .map(|(_, _, ink)| *ink)
                .unwrap();
            let date = dates[row];
            assert!(
                (badge.middle - date.middle).abs() <= 0.5 / zoom + 0.01,
                "{kind:?} at {zoom}x: badge ink {badge:?}, date digits {date:?}"
            );
            if let Some(next) = dates.get(row + 1) {
                assert!(
                    badge.rows.max + 2.0 <= next.rows.min,
                    "{kind:?}: badge ink {badge:?} meets the next date {next:?}"
                );
            }
        }
    }

    #[test]
    fn saved_on_shows_the_console_icon_not_a_short_name() {
        use citrine_gb::console::ConsoleKind;
        for kind in [
            ConsoleKind::Gb,
            ConsoleKind::Gbc,
            ConsoleKind::Gba,
            ConsoleKind::Nds,
        ] {
            let ctx = egui::Context::default();
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| stamp_with_badge(ui, "Today".to_owned(), kind));
            });
            let texts: Vec<String> = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                    _ => None,
                })
                .collect();
            assert_eq!(
                texts,
                vec![
                    "Today".to_owned(),
                    crate::app::ui_state::settings::kind_icon_16(kind).to_owned()
                ]
            );
        }
    }

    fn undecodable() -> GbError {
        use citrine_gb::console::ConsoleKind;
        use citrine_gb::machine::{Firmware, Machine};
        use citrine_gb::persistence::state::{SAVE_QUALITY, StateBody};
        let packed = StateBody {
            kind: ConsoleKind::Gb,
            data: vec![0xc3],
        }
        .pack(SAVE_QUALITY)
        .expect("pack");
        let rom = citrine_gb::rom::Rom::new(&[0; 0x8000]);
        match Machine::from_state(&packed, &rom, 0, &Firmware::default()) {
            Ok(_) => panic!("a lone true loaded as a state"),
            Err(err) => err,
        }
    }

    #[cfg(feature = "nds")]
    #[test]
    fn each_load_failure_has_its_sentence() {
        let damaged = std::io::Error::from(std::io::ErrorKind::UnexpectedEof);
        let cases = [
            (GbError::NewerState, "It needs a newer version of Citrine."),
            (
                GbError::OlderState,
                "It was saved by an older version of Citrine.",
            ),
            (
                undecodable(),
                "It was saved by a different version of Citrine.",
            ),
            (GbError::IO(damaged), "The file is damaged."),
            (GbError::NotGba, "It's for a different console."),
            (GbError::NotNds, "It's for a different console."),
            (GbError::RomTooSmall, "It's for a different console."),
            (GbError::OtherGame, "It belongs to another game."),
        ];
        for (err, reason) in cases {
            assert_eq!(
                failed_message(FailedStep::Load, "Boss fight", &err),
                format!("Couldn't load Boss fight. {reason}")
            );
        }
    }

    #[test]
    fn each_failed_step_names_the_item() {
        let err = GbError::IO(std::io::Error::other("lost"));
        let cases = [
            (
                FailedStep::SwitchConsole,
                "'Tetris' to Game Boy Color",
                "Couldn't switch 'Tetris' to Game Boy Color.",
            ),
            (
                FailedStep::Import,
                "'red.sav'",
                "Couldn't import 'red.sav'.",
            ),
            (FailedStep::Switch, "Old run", "Couldn't switch to Old run."),
            (FailedStep::Delete, "Old run", "Couldn't delete Old run."),
            (FailedStep::Rename, "Old run", "Couldn't rename Old run."),
            (
                FailedStep::Close,
                "'Tetris'",
                "Couldn't save 'Tetris', so it stays open.",
            ),
        ];
        for (step, what, text) in cases {
            assert_eq!(failed_message(step, what, &err), text);
        }
    }

    #[test]
    fn a_full_disk_says_so() {
        let full = GbError::IO(std::io::Error::from(std::io::ErrorKind::StorageFull));
        assert_eq!(
            failed_message(FailedStep::Rename, "Old run", &full),
            "Couldn't rename Old run. The device is out of storage."
        );
        assert_eq!(
            failed_message(FailedStep::Close, "'Tetris'", &full),
            "Couldn't save 'Tetris', so it stays open. The device is out of storage."
        );
        let locked = GbError::IO(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        assert_eq!(
            failed_message(FailedStep::Rename, "Old run", &locked),
            "Couldn't rename Old run."
        );
    }

    #[test]
    fn an_unnamed_item_uses_its_fallback() {
        assert_eq!(named("", "the game"), "the game");
        assert_eq!(named("Tetris", "the game"), "'Tetris'");
    }

    #[test]
    fn a_gb_state_that_does_not_decode_is_a_decode_error() {
        assert!(matches!(undecodable(), GbError::RmpDecode(_)));
    }

    fn debug_args(text: &str) -> Vec<String> {
        let calls = ["error", "success", "info", "hint", "notify", "notify_error"]
            .map(|name| format!("{name}("))
            .into_iter()
            .chain([format!("Err({}!(", "format")]);
        let mut found = Vec::new();
        for call in calls {
            for (start, _) in text.match_indices(&call) {
                let before = text[..start].chars().next_back();
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let mut depth = 1;
                let args = &text[start + call.len()..];
                let end = args
                    .char_indices()
                    .find(|&(_, c)| {
                        depth += match c {
                            '(' => 1,
                            ')' => -1,
                            _ => 0,
                        };
                        depth == 0
                    })
                    .map_or(args.len(), |(end, _)| end);
                let arg = &args[..end];
                if arg.contains(":?}") || arg.contains(":#?}") {
                    let line = text[..start].matches('\n').count() + 1;
                    found.push(format!("{line}: {}{}", call, arg.trim()));
                }
            }
        }
        found
    }

    #[test]
    fn the_notice_scan_reads_calls_split_over_lines() {
        let call = "error";
        let split =
            format!("toasts\n    .{call}(format!(\n        \"Couldn't: {{err:?}}\"\n    ));");
        let hits = debug_args(&split);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].starts_with("2: "), "{hits:?}");
        let fine = format!("log::{call}!(\"{{err:?}}\"); toasts.{call}(\"Saved\");");
        assert!(debug_args(&fine).is_empty());
    }

    #[test]
    fn no_error_notice_shows_debug_text() {
        let mut found = Vec::new();
        let mut dirs = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).expect("src") {
                let path = entry.expect("entry").path();
                if path.is_dir() {
                    dirs.push(path);
                    continue;
                }
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                for hit in debug_args(&text) {
                    found.push(format!("{}:{hit}", path.display()));
                }
            }
        }
        assert!(
            found.is_empty(),
            "notices show Debug text; use failed_message: {found:#?}"
        );
    }

    #[test]
    fn snapshot_messages_use_the_name() {
        let all = [
            SnapshotNews::Saved,
            SnapshotNews::Overwrote,
            SnapshotNews::Loaded,
            SnapshotNews::Empty,
            SnapshotNews::Deleted,
            SnapshotNews::Renamed,
        ];
        for news in all {
            let message = snapshot_message(news, "Boss fight");
            assert!(message.contains("Boss fight"), "got {message}");
            assert!(!message.contains('\''), "got {message}");
            assert!(!message.to_lowercase().contains("slot"), "got {message}");
        }
    }

    #[test]
    fn delete_waits_for_an_answer() {
        let mut state = SnapshotsState::default();
        state.ask_delete(1, "Slot 1".to_string());
        assert_eq!(
            state.delete_question().map(|(_, q)| q).as_deref(),
            Some("Delete Slot 1?")
        );
        assert_eq!(state.answer_delete(None), None);
        assert_eq!(state.answer_delete(Some(true)), Some(1));
        assert_eq!(state.delete_question(), None);
    }

    #[test]
    fn cancel_deletes_nothing() {
        let mut state = SnapshotsState::default();
        state.ask_delete(2, "Boss fight".to_string());
        assert_eq!(
            state.delete_question().map(|(_, q)| q).as_deref(),
            Some("Delete Boss fight?")
        );
        assert_eq!(state.answer_delete(Some(false)), None);
        assert_eq!(state.answer_delete(Some(true)), None);
    }

    #[test]
    fn long_autosave_lists_fold() {
        let ctx = egui::Context::default();
        let root =
            crate::storage::scratch_dir().join(format!("citrine-fold-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = SaveStore::at(root.clone());
        let frame = vec![0u8; 160 * 144 * 4];
        let mut state = SnapshotsState::default();

        store
            .store_autosave(
                "GAME",
                b"a",
                &frame,
                citrine_gb::console::ConsoleKind::Gb,
                10,
            )
            .expect("autosave");
        state.reload(&ctx, &store, "GAME");
        assert_eq!(state.shown_autosaves(), 1);
        assert_eq!(state.fold_label(), None);

        for _ in 0..4 {
            store
                .store_autosave(
                    "GAME",
                    b"a",
                    &frame,
                    citrine_gb::console::ConsoleKind::Gb,
                    10,
                )
                .expect("autosave");
        }
        state.reload(&ctx, &store, "GAME");
        assert_eq!(state.autosaves()[0].slot, 4, "newest first");
        assert_eq!(state.shown_autosaves(), FOLDED_AUTOSAVES);
        assert_eq!(state.fold_label().as_deref(), Some("Show all 5"));
        state.toggle_all_autosaves();
        assert_eq!(state.shown_autosaves(), 5);
        assert_eq!(state.fold_label().as_deref(), Some("Show fewer"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn autosaves_of_both_owners_list_as_one_newest_first() {
        use AutosaveEntry::{Filed, Mine};
        assert_eq!(
            newest_first(&[400, 100], &[500, 300, 50]),
            [Filed(0), Mine(0), Filed(1), Mine(1), Filed(2)]
        );
        assert_eq!(
            newest_first(&[200, 200], &[200]),
            [Mine(0), Mine(1), Filed(0)]
        );
        assert!(newest_first(&[], &[]).is_empty());
    }

    #[test]
    fn deleting_an_autosave_waits_for_an_answer() {
        let mut state = SnapshotsState::default();
        state.ask_delete_autosave(7, "05:50".to_string());
        assert_eq!(
            state.delete_autosave_question().map(|(_, q)| q).as_deref(),
            Some("Delete the autosave from 05:50?")
        );
        assert_eq!(state.answer_delete_autosave(None), None);
        assert_eq!(
            state.answer_delete_autosave(Some(true)),
            Some((7, "05:50".to_string()))
        );
        state.ask_delete_autosave(8, "1h ago".to_string());
        assert_eq!(state.answer_delete_autosave(Some(false)), None);
        assert_eq!(state.delete_autosave_question(), None);
    }

    #[test]
    fn a_cancelled_load_gives_back_the_pause_state() {
        let mut state = SnapshotsState::default();
        let load = PendingLoad {
            id: 3,
            label: "just now".to_string(),
            was_running: true,
        };
        state.ask_load_autosave(load.clone());
        assert!(state.load_autosave_question().is_some());
        assert_eq!(state.answer_load_autosave(None), None);
        assert_eq!(state.answer_load_autosave(Some(false)), Some((false, load)));
        assert_eq!(state.load_autosave_question(), None);
    }

    #[test]
    fn the_question_has_delete_and_cancel() {
        let ctx = egui::Context::default();
        let mut output = ctx.run(egui::RawInput::default(), |_| {});
        for _ in 0..3 {
            output = ctx.run(egui::RawInput::default(), |ctx| {
                crate::app::widgets::confirm::confirm(
                    ctx,
                    "test_confirm",
                    "Delete Slot 1?",
                    "Delete",
                );
            });
        }
        let texts: Vec<String> = output
            .shapes
            .into_iter()
            .filter_map(|clipped| match clipped.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        for wanted in ["Delete Slot 1?", "Delete", "Cancel"] {
            assert!(
                texts.iter().any(|text| text == wanted),
                "{wanted} in {texts:?}"
            );
        }
    }
}
