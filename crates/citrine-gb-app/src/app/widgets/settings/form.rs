use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::widgets::chip::{CHIP_HEIGHT, chip_text, paint_tag, tag_width};
use crate::app::widgets::info_mark::{info_mark, info_mark_in_line};
use crate::app::widgets::reset_slider::{reset_height, reset_width, without_reset};
use crate::app::widgets::settings::scope::Tag;
use egui::{
    Align, Id, Label, Layout, Rangef, Rect, RichText, Sense, Shape, TextStyle, TextWrapMode, Ui,
    UiBuilder, Vec2, WidgetText, pos2, vec2,
};
use std::hash::Hash;
use std::sync::Arc;

const MAX_LABEL_SHARE: f32 = 0.4;

const MAX_TEXT_LABEL_SHARE: f32 = 0.65;

#[derive(Clone, Copy, Default)]
struct LabelColumn {
    pass: u64,
    widest: f32,
    previous: f32,
}

impl LabelColumn {
    fn load(ui: &Ui, id: Id, pass: u64) -> Self {
        let column = ui
            .data(|d| d.get_temp::<LabelColumn>(id))
            .unwrap_or_default();
        if column.pass == pass {
            return column;
        }
        LabelColumn {
            pass,
            widest: 0.0,
            previous: column.widest,
        }
    }
}

const LABEL_WRAP_SLACK: f32 = 1.0;

fn leading_space_width(row: &egui::epaint::text::PlacedRow) -> Option<f32> {
    let first = row.row.glyphs.first()?;
    if !first.chr.is_whitespace() {
        return None;
    }
    row.row.glyphs.get(1).map(|word| word.pos.x)
}

fn rows_start_at_words(galley: Arc<egui::Galley>) -> Arc<egui::Galley> {
    if galley
        .rows
        .iter()
        .skip(1)
        .all(|row| leading_space_width(row).is_none())
    {
        return galley;
    }
    let mut galley = Arc::unwrap_or_clone(galley);
    let pixels = galley.pixels_per_point;
    for row in galley.rows.iter_mut().skip(1) {
        if let Some(width) = leading_space_width(row) {
            row.pos.x -= (width * pixels).round() / pixels;
        }
    }
    Arc::new(galley)
}

#[derive(Clone, Copy)]
struct NarrowColumn {
    pass: u64,
    narrowest: f32,
    previous: f32,
}

impl NarrowColumn {
    fn load(ui: &Ui, id: Id, pass: u64) -> Self {
        match ui.data(|d| d.get_temp::<NarrowColumn>(id)) {
            Some(column) if column.pass == pass => column,
            Some(column) => NarrowColumn {
                pass,
                narrowest: f32::INFINITY,
                previous: column.narrowest,
            },
            None => NarrowColumn {
                pass,
                narrowest: f32::INFINITY,
                previous: f32::INFINITY,
            },
        }
    }
}

fn shared_control_column(ui: &Ui, room: f32, floor: f32) -> f32 {
    let ctx = ui.ctx();
    let Some(page) = super::page::column(ctx) else {
        return room.max(floor);
    };
    let id = page.with("narrow_label_column");
    let mut column = NarrowColumn::load(ui, id, ctx.cumulative_pass_nr());
    let fits = room >= floor;
    if fits {
        column.narrowest = column.narrowest.min(room);
    }
    ctx.data_mut(|d| d.insert_temp(id, column));
    if fits && column.previous.is_finite() && room < column.previous - 0.5 {
        ctx.request_discard("settings control column narrowed");
    }
    room.max(floor).min(column.previous).max(floor)
}

fn settled_control_column(ui: &Ui) -> Option<f32> {
    let page = super::page::column(ui.ctx())?;
    let column = ui.data(|d| d.get_temp::<NarrowColumn>(page.with("narrow_label_column")))?;
    let pass = ui.ctx().cumulative_pass_nr();
    let settled = if column.pass == pass {
        column.previous
    } else {
        column.narrowest
    };
    settled.is_finite().then_some(settled)
}

#[derive(Clone, Copy, Default)]
struct RowNeed {
    pass: u64,
    widest: f32,
    fixed: f32,
    trailing: f32,
    whole: f32,
    select: bool,
    edge: bool,
}

fn row_key() -> Id {
    Id::new("settings_form_row")
}

pub fn need(ui: &Ui, width: f32) {
    note_need(ui, width, false, false, false, false);
}

pub const SELECT_MIN: f32 = 60.0;

pub fn select_need(ui: &Ui, width: f32) {
    note_need(ui, width, true, false, false, false);
}

pub fn shrinking_select_need(ui: &Ui, width: f32) {
    note_need(ui, width, true, true, false, false);
}

pub fn on_select_edge(ui: &Ui) {
    note_need(ui, 0.0, false, false, false, true);
}

pub fn cut_select_need(ui: &Ui, whole: f32, floor: f32) {
    note_need(ui, whole, true, true, true, false);
    note_need(ui, floor, true, false, false, false);
}

fn note_need(ui: &Ui, width: f32, select: bool, shrinks: bool, whole: bool, edge: bool) {
    let ctx = ui.ctx();
    let Some(row) = ctx.data(|d| d.get_temp::<Option<Id>>(row_key())).flatten() else {
        return;
    };
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let mut need = d.get_temp::<RowNeed>(row).unwrap_or_default();
        if need.pass != pass {
            need = RowNeed {
                pass,
                widest: 0.0,
                fixed: 0.0,
                trailing: 0.0,
                whole: 0.0,
                select: false,
                edge: false,
            };
        }
        need.widest = need.widest.max(width);
        if !shrinks {
            need.fixed = need.fixed.max(width);
        }
        if whole {
            need.whole = need.whole.max(width);
        }
        need.select |= select;
        need.edge |= edge;
        d.insert_temp(row, need);
    });
}

pub const DESKTOP_LABELS: f32 = 135.61;

fn gutter_key() -> Id {
    Id::new("settings_form_gutter")
}

fn lent_edge_key() -> Id {
    Id::new("settings_page_lent_edge")
}

pub fn gutter_taken<R>(ui: &mut Ui, edge: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let key = gutter_key();
    let outer = ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    let lent = ui.data(|d| d.get_temp::<f32>(lent_edge_key()));
    ui.data_mut(|d| {
        d.insert_temp(key, true);
        d.insert_temp(lent_edge_key(), edge);
    });
    let inner = add(ui);
    ui.data_mut(|d| {
        d.insert_temp(key, outer);
        d.insert_temp(lent_edge_key(), lent.unwrap_or(0.0));
    });
    inner
}

fn one_column_key() -> Id {
    Id::new("settings_form_one_column")
}

pub fn selects_shrink(ui: &Ui) -> bool {
    ui.data(|d| d.get_temp::<bool>(one_column_key()))
        .unwrap_or(false)
}

fn shared_labels_key() -> Id {
    Id::new("settings_shared_labels")
}

pub fn shared_labels<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let key = shared_labels_key();
    let outer = ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    ui.data_mut(|d| d.insert_temp(key, true));
    let inner = add(ui);
    ui.data_mut(|d| d.insert_temp(key, outer));
    inner
}

#[cfg(test)]
thread_local! {
    static WIDEST_LABEL: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
}

#[cfg(test)]
pub fn take_widest_label() -> f32 {
    WIDEST_LABEL.with(|widest| widest.replace(0.0))
}

#[cfg(test)]
thread_local! {
    static DOCK_ROWS: std::cell::RefCell<Vec<DockRow>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
pub struct DockRow {
    pub label: String,
    pub section: Id,
    pub value_x: f32,
    pub floor: f32,
    pub line: f32,
    pub shared: bool,
    pub page: f32,
}

#[cfg(test)]
pub fn take_dock_rows() -> Vec<DockRow> {
    DOCK_ROWS.with(|rows| rows.take())
}

fn reset_taken_key() -> Id {
    Id::new("settings_reset_taken")
}

pub fn keeps_reset_column(ui: &Ui) -> bool {
    let ctx = ui.ctx();
    super::page::column(ctx).is_some()
        && !ctx
            .data(|d| d.get_temp::<bool>(reset_taken_key()))
            .unwrap_or(false)
        && ctx
            .data(|d| d.get_temp::<Option<Id>>(row_key()))
            .flatten()
            .is_some()
}

pub fn field_width(ui: &Ui) -> f32 {
    let width = ui.available_width();
    if keeps_reset_column(ui) {
        let reset = crate::app::widgets::reset_slider::reset_width(ui)
            + crate::app::widgets::reset_slider::RESET_GAP;
        (width - reset).max(0.0)
    } else {
        width
    }
}

pub fn reset_taken<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let key = reset_taken_key();
    let outer = ui.data(|d| d.get_temp::<bool>(key)).unwrap_or(false);
    ui.data_mut(|d| d.insert_temp(key, true));
    let inner = add(ui);
    ui.data_mut(|d| d.insert_temp(key, outer));
    inner
}

pub fn value_box(ui: &Ui, width: f32) -> f32 {
    let ctx = ui.ctx();
    let Some(page) = super::page::column(ctx) else {
        return width;
    };
    let id = page.with("value_box");
    let mut boxes = LabelColumn::load(ui, id, ctx.cumulative_pass_nr());
    boxes.widest = boxes.widest.max(width);
    ctx.data_mut(|d| d.insert_temp(id, boxes));
    if width > boxes.previous + 0.5 {
        ctx.request_discard("settings value box grew");
    }
    width.max(boxes.previous)
}

fn need_more(ui: &Ui, extra: f32) {
    let ctx = ui.ctx();
    let Some(row) = ctx.data(|d| d.get_temp::<Option<Id>>(row_key())).flatten() else {
        return;
    };
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let mut need = d.get_temp::<RowNeed>(row).unwrap_or_default();
        if need.pass != pass {
            need = RowNeed {
                pass,
                ..Default::default()
            };
        }
        need.widest += extra;
        need.fixed += extra;
        need.trailing += extra;
        d.insert_temp(row, need);
    });
}

#[derive(Clone)]
pub struct RowTag {
    pub tag: Tag,
    pub hover: String,
}

impl RowTag {
    fn words(&self, short: bool) -> String {
        if short {
            self.tag.short_words()
        } else {
            self.tag.words()
        }
    }

    fn galley(&self, ui: &Ui, short: bool) -> std::sync::Arc<egui::Galley> {
        WidgetText::from(chip_text(self.words(short)).color(ui.visuals().strong_text_color()))
            .into_galley(
                ui,
                Some(TextWrapMode::Extend),
                f32::INFINITY,
                TextStyle::Button,
            )
    }

    fn size(&self, ui: &Ui, galley: &egui::Galley, short: bool) -> Vec2 {
        match self.tag {
            Tag::Differs(_) => vec2(reset_width(ui), reset_height(ui)),
            _ if self.words(short).is_empty() => Vec2::ZERO,
            _ => vec2(tag_width(galley), CHIP_HEIGHT),
        }
    }

    fn width(&self, ui: &Ui, short: bool) -> f32 {
        self.size(ui, &self.galley(ui, short), short).x
    }

    pub(crate) fn show(&self, ui: &mut Ui, short: bool) -> bool {
        if self.words(short).is_empty() {
            return false;
        }
        let galley = self.galley(ui, short);
        let sense = if self.tag.drops() {
            Sense::click()
        } else {
            Sense::hover()
        };
        let size = self.size(ui, &galley, short);
        let height = size.y.min(ui.available_height());
        let (rect, response) = ui.allocate_exact_size(vec2(size.x, height), sense);
        let chip = Rect::from_center_size(rect.center(), size);
        let hover = match self.tag.short_hover() {
            Some(hover) if short => hover,
            _ => self.hover.clone(),
        };
        let label = match self.tag {
            Tag::Differs(_) => self.hover.clone(),
            _ if short => hover.clone(),
            _ => self.tag.words(),
        };
        let kind = if self.tag.drops() {
            egui::WidgetType::Button
        } else {
            egui::WidgetType::Label
        };
        response.widget_info(|| egui::WidgetInfo::labeled(kind, ui.is_enabled(), &label));
        if ui.is_rect_visible(chip) {
            paint_tag(ui, chip, galley, self.tag.own());
        }
        let response = response.on_hover_text(&hover);
        if self.tag.drops() {
            response
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
        } else {
            false
        }
    }
}

fn short_galley(ui: &Ui) -> std::sync::Arc<egui::Galley> {
    WidgetText::from(chip_text(Tag::Own.short_words()).color(ui.visuals().strong_text_color()))
        .into_galley(
            ui,
            Some(TextWrapMode::Extend),
            f32::INFINITY,
            TextStyle::Button,
        )
}

fn control_need(ui: &Ui) -> f32 {
    let Some(row) = ui.data(|d| d.get_temp::<Option<Id>>(row_key())).flatten() else {
        return 0.0;
    };
    let need = settled_need(ui, row);
    if need.pass == ui.ctx().cumulative_pass_nr() {
        need.fixed - need.trailing
    } else {
        0.0
    }
}

fn settled_need(ui: &Ui, row: Id) -> RowNeed {
    ui.data(|d| d.get_temp::<RowNeed>(row)).unwrap_or_default()
}

impl RowNeed {
    fn column_share(&self, room: f32, wrapped_room: f32, shrinks: bool) -> f32 {
        if !shrinks {
            self.widest.min(room.max(self.fixed.max(self.whole)))
        } else if room >= self.fixed - self.trailing {
            self.widest.min(room)
        } else {
            self.widest.min(wrapped_room.max(self.fixed))
        }
    }
}

pub(super) fn settle_selects(ui: &Ui, page: Id) {
    let pass = ui.ctx().cumulative_pass_nr();
    let Some(selects) = ui.data(|d| d.get_temp::<LabelColumn>(page.with("selects"))) else {
        return;
    };
    if selects.pass == pass && (selects.widest - selects.previous).abs() > 0.5 {
        ui.ctx().request_discard("settings select column changed");
    }
}

fn check_need(ui: &Ui, row: Id, used: RowNeed) {
    let pass = ui.ctx().cumulative_pass_nr();
    let Some(need) = ui.data(|d| d.get_temp::<RowNeed>(row)) else {
        return;
    };
    if need.pass == pass && ((need.widest - used.widest).abs() > 0.5 || need.edge != used.edge) {
        ui.ctx().request_discard("settings value need changed");
    }
}

pub struct Form<'a> {
    ui: &'a mut Ui,
    label_width: f32,
    label_room: f32,
    longest: f32,
    widest: f32,
    select_need: f32,
    widest_select: f32,
    widest_dock_select: f32,
    select_label: f32,
    widest_select_label: f32,
    stacked_rows: usize,
    one_column: bool,
    widest_need: f32,
    striped: bool,
    text_only: bool,
    body_rows: usize,
    in_page: bool,
    narrow: bool,
    after_box: Option<f32>,
    nest: f32,
    section_column: f32,
    section_widest: f32,
    line_widest: f32,
    own_line_widest: f32,
    own_lines: Vec<f32>,
    section_squeeze: f32,
    widest_squeeze: f32,
    least_room: f32,
    least_line_room: f32,
    #[cfg(test)]
    section: Id,
    tag: Option<RowTag>,
    tag_clicked: bool,
    tag_column: f32,
    widest_tag: f32,
    short_tags: bool,
    tag_excess: f32,
    last_value: Option<Rangef>,
}

impl Form<'_> {
    pub fn show<R>(ui: &mut Ui, id_salt: impl Hash, add_rows: impl FnOnce(&mut Form) -> R) -> R {
        let page = super::page::column(ui.ctx());
        let own = Id::new(("settings_form", id_salt));
        let id = page.unwrap_or(own);
        let needs_id = id.with("value_needs");
        let section_id = own.with("section_column");
        let squeeze_id = own.with("section_squeeze");
        let tags_id = id.with("tags");
        let shorts_id = id.with("short_tags");
        let pass = ui.ctx().cumulative_pass_nr();
        let width = ui.available_width();

        let mut column = LabelColumn::load(ui, id, pass);
        let mut needs = LabelColumn::load(ui, needs_id, pass);
        let mut section = LabelColumn::load(ui, section_id, pass);
        let mut squeeze = LabelColumn::load(ui, squeeze_id, pass);
        let section_gap = ui.spacing().item_spacing.x / 2.0;
        let section_fits = squeeze.previous <= section_gap + 0.01;
        let lent = ui
            .data(|d| d.get_temp::<f32>(lent_edge_key()))
            .unwrap_or(0.0);
        let narrow = width + lent < super::page::STACK_BELOW;
        let taken = ui
            .data(|d| d.get_temp::<bool>(gutter_key()))
            .unwrap_or(false);
        let gutter = if page.is_some() && !narrow && !taken {
            super::page::text_edge(ui)
        } else {
            0.0
        };
        let width = width - gutter;
        let gap = ui.spacing().item_spacing.x;
        let shared = ui
            .data(|d| d.get_temp::<bool>(shared_labels_key()))
            .unwrap_or(false);
        let page_column = if shared {
            column.previous.max(DESKTOP_LABELS)
        } else {
            column.previous
        };
        let one_column =
            page.is_some() && !narrow && page_column <= width - gap - needs.previous + 0.5;
        let (label_width, longest) = if one_column {
            (page_column, page_column)
        } else {
            (
                column.previous.min(width * MAX_LABEL_SHARE),
                column.previous,
            )
        };
        let select_id = if one_column { id } else { own };
        let selects_id = select_id.with("selects");
        let select_labels_id = select_id.with("select_labels");
        let mut selects = LabelColumn::load(ui, selects_id, pass);
        let mut select_labels = LabelColumn::load(ui, select_labels_id, pass);
        let mut carried = one_column.then(|| {
            (
                LabelColumn::load(ui, own.with("selects"), pass),
                LabelColumn::load(ui, own.with("select_labels"), pass),
            )
        });
        let mut tags = LabelColumn::load(ui, tags_id, pass);
        let mut shorts = LabelColumn::load(ui, shorts_id, pass);

        ui.data_mut(|d| {
            d.insert_temp(gutter_key(), taken || gutter > 0.0);
            d.insert_temp(lent_edge_key(), 0.0f32);
        });
        let outer_one_column = selects_shrink(ui);
        ui.data_mut(|d| d.insert_temp(one_column_key(), one_column));
        let room = ui.available_rect_before_wrap();
        let inner = ui
            .scope_builder(
                UiBuilder::new()
                    .max_rect(room.with_min_x(room.left() + gutter))
                    .layout(Layout::top_down(Align::Min)),
                |ui| {
                    ui.set_width(width);
                    let mut form = Form {
                        ui,
                        label_width,
                        label_room: column.previous.min(width * MAX_TEXT_LABEL_SHARE),
                        longest,
                        widest: 0.0,
                        select_need: selects.previous,
                        widest_select: 0.0,
                        widest_dock_select: 0.0,
                        select_label: select_labels.previous,
                        widest_select_label: 0.0,
                        stacked_rows: 0,
                        one_column,
                        widest_need: 0.0,
                        striped: false,
                        text_only: false,
                        body_rows: 0,
                        in_page: page.is_some(),
                        narrow,
                        after_box: None,
                        nest: 0.0,
                        section_column: if section_fits { section.previous } else { 0.0 },
                        section_widest: 0.0,
                        line_widest: 0.0,
                        own_line_widest: 0.0,
                        own_lines: Vec::new(),
                        section_squeeze: if section_fits { squeeze.previous } else { 0.0 },
                        widest_squeeze: 0.0,
                        least_room: f32::INFINITY,
                        least_line_room: f32::INFINITY,
                        #[cfg(test)]
                        section: own,
                        tag: None,
                        tag_clicked: false,
                        tag_column: tags.previous,
                        widest_tag: 0.0,
                        short_tags: shorts.previous > 0.5,
                        tag_excess: 0.0,
                        last_value: None,
                    };
                    let inner = add_rows(&mut form);
                    column.widest = column.widest.max(form.widest);
                    selects.widest = selects.widest.max(form.widest_select);
                    select_labels.widest = select_labels.widest.max(form.widest_select_label);
                    if let Some((selects, labels)) = carried.as_mut() {
                        selects.widest = selects.widest.max(form.widest_dock_select);
                        labels.widest = labels.widest.max(form.widest_select_label);
                    }
                    needs.widest = needs.widest.max(form.widest_need);
                    let fit = |widest: f32, room: f32| widest - room <= section_gap + 0.01;
                    let (widest, room) = if fit(form.own_line_widest, form.least_line_room) {
                        (form.own_line_widest, form.least_line_room)
                    } else if fit(form.line_widest, form.least_room) {
                        (form.line_widest, form.least_room)
                    } else {
                        (form.section_widest, form.least_room)
                    };
                    let cuts_a_line = form
                        .own_lines
                        .iter()
                        .filter(|line| **line > widest + 0.5)
                        .count()
                        == 1;
                    section.widest = section.widest.max(widest);
                    squeeze.widest = squeeze
                        .widest
                        .max(form.widest_squeeze)
                        .max(section.widest - room)
                        .max(if cuts_a_line { section_gap + 1.0 } else { 0.0 });
                    tags.widest = tags.widest.max(form.widest_tag);
                    shorts.widest = shorts.widest.max(form.tag_excess);
                    inner
                },
            )
            .inner;
        ui.data_mut(|d| {
            d.insert_temp(gutter_key(), taken);
            d.insert_temp(lent_edge_key(), lent);
        });
        ui.data_mut(|d| d.insert_temp(one_column_key(), outer_one_column));

        if column.widest > column.previous {
            ui.ctx().request_discard("settings label column grew");
        }
        #[cfg(test)]
        if shared && page.is_some() {
            WIDEST_LABEL.with(|widest| widest.set(widest.get().max(column.widest)));
        }
        if tags.widest > tags.previous {
            ui.ctx().request_discard("settings tag column grew");
        }
        if shorts.widest > 0.5 && shorts.previous <= 0.5 {
            ui.ctx().request_discard("settings tags got short");
        }
        if select_labels.widest > select_labels.previous {
            ui.ctx().request_discard("settings select labels grew");
        }
        if needs.widest > needs.previous + 0.5 {
            ui.ctx().request_discard("settings value column grew");
        }
        if !one_column && (selects.widest - selects.previous).abs() > 0.5 {
            ui.ctx().request_discard("settings select column changed");
        }
        if (section.widest - section.previous).abs() > 0.5 {
            ui.ctx().request_discard("settings section column changed");
        }
        let fits_now = squeeze.widest <= section_gap + 0.01;
        if fits_now != section_fits || (fits_now && (squeeze.widest - squeeze.previous).abs() > 0.1)
        {
            ui.ctx().request_discard("settings section gap changed");
        }
        ui.data_mut(|d| {
            d.insert_temp(id, column);
            d.insert_temp(selects_id, selects);
            d.insert_temp(select_labels_id, select_labels);
            if let Some((selects, labels)) = carried {
                d.insert_temp(own.with("selects"), selects);
                d.insert_temp(own.with("select_labels"), labels);
            }
            d.insert_temp(needs_id, needs);
            d.insert_temp(section_id, section);
            d.insert_temp(squeeze_id, squeeze);
            d.insert_temp(tags_id, tags);
            d.insert_temp(shorts_id, shorts);
        });
        inner
    }

    pub fn with_tag(&mut self, tag: Option<RowTag>, add: impl FnOnce(&mut Self)) -> bool {
        let hide = tag.as_ref().is_some_and(|tag| tag.tag.game_scope());
        self.tag = tag;
        self.tag_clicked = false;
        let ctx = self.ui.ctx().clone();
        without_reset(&ctx, hide, || add(self));
        self.tag = None;
        std::mem::take(&mut self.tag_clicked)
    }

    pub fn striped(&mut self) {
        self.striped = true;
    }

    pub fn text_only(&mut self, widest_value_word: f32) {
        self.text_only = true;
        let value_room =
            self.ui.available_width() - self.label_room - self.ui.spacing().item_spacing.x;
        self.label_width = if value_room < widest_value_word {
            0.0
        } else {
            self.label_room
        };
    }

    pub fn header(&mut self, label: &str, value: &str) {
        self.labelled(RichText::new(label).strong(), None, |ui| {
            ui.add(Label::new(RichText::new(value).strong()).wrap());
        });
    }

    pub fn text_row<R>(&mut self, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
        self.labelled_at(label, None, text_height(), add)
    }

    pub fn row<R>(&mut self, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
        let stripe = self.striped && self.body_rows.is_multiple_of(2);
        self.body_rows += 1;
        if !stripe {
            return self.labelled(label, None, add);
        }
        let background = self.ui.painter().add(Shape::Noop);
        let top = self.ui.cursor().top();
        let inner = self.labelled(label, None, add);
        let gap = self.ui.spacing().item_spacing.y / 2.0;
        let rect = Rect::from_x_y_ranges(
            self.row_x(),
            (top - gap)..=(self.ui.min_rect().bottom() + gap),
        );
        let fill = self.ui.visuals().faint_bg_color;
        self.ui.painter().set(
            background,
            Shape::rect_filled(
                rect,
                self.ui.visuals().widgets.noninteractive.corner_radius,
                fill,
            ),
        );
        inner
    }

    pub fn row_with_hint<R>(
        &mut self,
        label: &str,
        hint: &str,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        self.labelled(label, Some(hint), add)
    }

    pub fn toggle(&mut self, label: &str, on: &mut bool) -> bool {
        self.toggle_when(label, None, true, on)
    }

    pub fn toggle_with_hint(&mut self, label: &str, hint: &str, on: &mut bool) -> bool {
        self.toggle_when(label, Some(hint), true, on)
    }

    pub fn toggle_when(
        &mut self,
        label: &str,
        hint: Option<&str>,
        enabled: bool,
        on: &mut bool,
    ) -> bool {
        self.toggle_row(label, hint, enabled, on)
    }

    fn toggle_row(
        &mut self,
        label: &str,
        hint: Option<&str>,
        enabled: bool,
        on: &mut bool,
    ) -> bool {
        let check = |ui: &mut Ui, on: &mut bool| {
            ui.add_enabled(enabled, crate::app::widgets::check::Check::new(on, ""))
                .changed()
        };
        if !self.stacked() {
            return self.box_first(label, hint, enabled, on);
        }
        self.measure(label, hint);
        self.stacked_gap();
        let top = self.ui.cursor().top();
        let height = self.ui.spacing().interact_size.y;
        let tag = self.tag.take();
        let mut clicked = false;
        let changed = self
            .ui
            .allocate_ui_with_layout(
                vec2(self.ui.available_width(), height),
                Layout::right_to_left(Align::Center),
                |ui| {
                    let changed = check(ui, on);
                    if let Some(tag) = &tag {
                        clicked = tag.show(ui, false);
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(Label::new(label).wrap());
                        if let Some(hint) = hint {
                            info_mark(ui, hint);
                        }
                    });
                    changed
                },
            )
            .inner;
        self.tag_clicked |= clicked;
        self.mark_row(top);
        self.spot(label, top);
        changed
    }

    fn box_first(&mut self, label: &str, hint: Option<&str>, enabled: bool, on: &mut bool) -> bool {
        let top = self.ui.cursor().top();
        let left = self.ui.max_rect().left();
        let height = self.ui.spacing().interact_size.y;
        let slot = super::page::gutter(self.ui);
        let nest = self.nest;
        let tag = self.tag.take();
        let (tag_width, _, short) = self.tag_columns(tag.as_ref());
        let column = if tag.is_some() {
            tag_width + self.ui.spacing().item_spacing.x
        } else {
            0.0
        };
        let mut clicked = false;
        let (changed, indent) = self
            .ui
            .horizontal(|ui| {
                ui.set_min_height(height);
                let right = ui.max_rect().right();
                let row_top = ui.cursor().top();
                ui.add_space(nest);
                let (rect, _) = ui.allocate_exact_size(vec2(slot, height), Sense::hover());
                ui.add_space(-ui.spacing().item_spacing.x);
                let mut changed =
                    crate::app::widgets::check::hanging_box(ui, rect, enabled, on).changed();
                let mark = if hint.is_some() {
                    crate::app::widgets::info_mark::width(ui)
                } else {
                    0.0
                };
                let text = ui
                    .scope(|ui| {
                        ui.set_max_width((ui.available_width() - mark - column).max(0.0));
                        ui.add_enabled(enabled, Label::new(label).wrap().sense(Sense::click()))
                    })
                    .inner;
                if text.clicked() {
                    *on = !*on;
                    changed = true;
                }
                if let Some(hint) = hint {
                    info_mark(ui, hint);
                }
                if let Some(tag) = &tag {
                    let rect = Rect::from_min_size(
                        egui::pos2(right - tag_width, row_top),
                        vec2(tag_width, height),
                    );
                    clicked = ui
                        .scope_builder(
                            UiBuilder::new()
                                .max_rect(rect)
                                .layout(Layout::left_to_right(Align::Center)),
                            |ui| tag.show(ui, short),
                        )
                        .inner;
                }
                (changed, text.rect.left() - left)
            })
            .inner;
        self.tag_clicked |= clicked;
        self.mark_row(top);
        self.spot(label, top);
        self.after_box = Some(indent);
        changed
    }

    pub fn tree_row<R>(&mut self, row: RowId, add: impl FnOnce(&mut Self) -> R) -> R {
        let outer = self.nest;
        if !self.stacked() {
            self.nest += row.depth() as f32 * super::page::gutter(self.ui);
        }
        let inner = add(self);
        self.nest = outer;
        inner
    }

    pub fn hang(&self) -> f32 {
        if self.in_page && !self.narrow {
            super::page::BOX_HANG
        } else {
            0.0
        }
    }

    fn row_x(&self) -> egui::Rangef {
        self.ui.max_rect().x_range()
    }

    fn measure(&mut self, label: &str, hint: Option<&str>) {
        let measured = WidgetText::from(label)
            .into_galley(
                self.ui,
                Some(TextWrapMode::Extend),
                f32::INFINITY,
                TextStyle::Body,
            )
            .size()
            .x;
        let mark = if hint.is_some() {
            crate::app::widgets::info_mark::width(self.ui)
        } else {
            0.0
        };
        self.widest = self.widest.max(measured + mark);
    }

    fn stacked_gap(&mut self) {
        if !self.stacked() {
            return;
        }
        if self.stacked_rows > 0 {
            let gap = self.ui.spacing().item_spacing.y;
            self.ui.add_space(gap);
        }
        self.stacked_rows += 1;
    }

    pub fn buttons<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> R {
        self.stacked_gap();
        let top = self.ui.cursor().top();
        let inner = self.under(|ui| ui.horizontal_wrapped(add).inner);
        self.mark_row(top);
        inner
    }

    pub fn custom<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> R {
        self.stacked_gap();
        let top = self.ui.cursor().top();
        let inner = self.ui.with_layout(Layout::top_down(Align::Min), add).inner;
        self.mark_row(top);
        inner
    }

    fn mark_row(&mut self, top: f32) {
        self.after_box = None;
        if !self.ui.is_visible() {
            return;
        }
        super::page::mark(
            super::page::Mark::Row,
            Rect::from_x_y_ranges(self.row_x(), top..=self.ui.min_rect().bottom()),
        );
    }

    fn spot(&self, label: &str, top: f32) {
        if !self.ui.is_visible() {
            return;
        }
        super::spotlight::mark(
            self.ui,
            label,
            Rect::from_x_y_ranges(self.row_x(), top..=self.ui.min_rect().bottom()),
        );
    }

    pub fn ctx(&self) -> &egui::Context {
        self.ui.ctx()
    }

    fn stacked(&self) -> bool {
        if self.in_page {
            self.narrow
        } else {
            self.label_width < self.longest
        }
    }

    pub fn hint(&mut self, text: impl Into<RichText>) {
        let pull = self.ui.spacing().item_spacing.y / 2.0;
        self.ui.add_space(-pull);
        let top = self.ui.cursor().top();
        self.under(|ui| ui.add(Label::new(text.into().weak()).wrap()));
        if !self.ui.is_visible() {
            self.ui.add_space(pull);
            return;
        }
        super::page::mark(
            super::page::Mark::Note,
            Rect::from_x_y_ranges(self.row_x(), top..=self.ui.min_rect().bottom()),
        );
        self.ui.add_space(pull);
    }

    pub fn wide_hint(&mut self, text: impl Into<RichText>) {
        let pull = self.ui.spacing().item_spacing.y / 2.0;
        self.ui.add_space(-pull);
        let top = self.ui.cursor().top();
        self.full(|ui| ui.add(Label::new(text.into().weak()).wrap()));
        super::page::mark(
            super::page::Mark::Note,
            Rect::from_x_y_ranges(
                self.ui.max_rect().x_range(),
                top..=self.ui.min_rect().bottom(),
            ),
        );
        self.ui.add_space(pull);
    }

    pub fn column_head(&mut self, label: &str, value: &str) {
        self.labelled(RichText::new(label).weak(), None, |ui| {
            let head = ui.add(Label::new(RichText::new(value).weak()));
            need(ui, head.rect.width());
        });
    }

    pub fn row_note(&mut self, text: &str) {
        let pull = self.ui.spacing().item_spacing.y;
        self.ui.add_space(-pull);
        let top = self.ui.cursor().top();
        self.full(|ui| ui.add(Label::new(text).wrap()));
        super::page::mark(
            super::page::Mark::Note,
            Rect::from_x_y_ranges(
                self.ui.max_rect().x_range(),
                top..=self.ui.min_rect().bottom(),
            ),
        );
        self.ui.add_space(pull / 2.0);
    }

    pub fn under<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> R {
        let indent = if let Some(indent) = self.after_box {
            indent
        } else if self.stacked() || self.label_width <= 0.0 {
            0.0
        } else if self.in_page && !self.one_column && !self.text_only {
            settled_control_column(self.ui)
                .unwrap_or(self.label_width)
                .max(self.section_column)
                + self.ui.spacing().item_spacing.x
                - self.section_squeeze
        } else {
            self.label_width + self.ui.spacing().item_spacing.x
        };
        let rect = self.ui.available_rect_before_wrap();
        let inner = self.ui.scope_builder(
            UiBuilder::new().max_rect(rect.with_min_x(rect.left() + indent)),
            add,
        );
        if !self.stacked() && !self.text_only && self.after_box.is_none() {
            super::page::mark(
                super::page::Mark::Control,
                Rect::from_x_y_ranges(
                    (rect.left() + indent)..=rect.right(),
                    rect.top()..=inner.response.rect.bottom(),
                ),
            );
        }
        inner.inner
    }

    #[cfg(feature = "nds")]
    pub fn under_field<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> R {
        self.under(|ui| {
            let reset = crate::app::widgets::reset_slider::reset_width(ui)
                + crate::app::widgets::reset_slider::RESET_GAP;
            ui.set_max_width((ui.available_width() - reset).max(0.0));
            add(ui)
        })
    }

    pub fn under_value<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> R {
        let Some(value) = self.last_value.filter(|_| !self.stacked()) else {
            return self.under(add);
        };
        let rect = self.ui.available_rect_before_wrap();
        self.ui
            .scope_builder(
                UiBuilder::new().max_rect(Rect::from_x_y_ranges(value, rect.y_range())),
                add,
            )
            .inner
    }

    pub fn full<R>(&mut self, add: impl FnOnce(&mut Ui) -> R) -> R {
        let rect = self.ui.available_rect_before_wrap();
        self.ui
            .scope_builder(UiBuilder::new().max_rect(rect), add)
            .inner
    }

    fn labelled<R>(
        &mut self,
        label: impl Into<WidgetText>,
        hint: Option<&str>,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let height = if self.text_only {
            text_height()
        } else {
            self.ui.spacing().interact_size.y
        };
        self.labelled_at(label, hint, height, add)
    }

    fn labelled_at<R>(
        &mut self,
        label: impl Into<WidgetText>,
        hint: Option<&str>,
        height: f32,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let label: WidgetText = label.into();
        let measured = label
            .clone()
            .into_galley(
                self.ui,
                Some(TextWrapMode::Extend),
                f32::INFINITY,
                TextStyle::Body,
            )
            .size()
            .x;
        let mark = if hint.is_some() {
            crate::app::widgets::info_mark::width(self.ui)
        } else {
            0.0
        };
        self.widest = self.widest.max(measured + mark + self.nest);

        let word = widest_word(self.ui, &label);
        let floor = if mark > 0.0 && self.in_page {
            mark_floor(self.ui, &label, word, mark)
        } else {
            word + mark
        };
        let floor = floor + self.nest + LABEL_WRAP_SLACK;
        let row = self.ui.id().with(("value_need", label.text()));
        let settled = settled_need(self.ui, row);
        let need = settled.widest;
        let width = self.ui.available_width();
        let gap = self.ui.spacing().item_spacing.x;
        let beside_label = width - gap - self.select_label.max(measured + mark);
        let beside_words = width - gap - floor;
        let edge = settled.edge && self.select_need > 0.0;
        let select_share = settled.column_share(beside_label, beside_words, self.one_column);
        let shared = if settled.select {
            select_share.max(self.select_need)
        } else if edge {
            need.max(self.select_need)
        } else {
            need
        };
        let value_room = width - shared - gap;
        let line_room = width - if settled.select { select_share } else { shared } - gap;
        let column = if (need > 0.0 || edge) && !self.text_only {
            self.longest
        } else {
            self.label_width
        };
        let label_width = if self.one_column && !self.text_only {
            self.label_width
        } else if self.in_page && !self.text_only && !self.stacked() && need > 0.0 {
            shared_control_column(self.ui, column.min(value_room), floor.min(self.longest))
        } else if self.in_page && !self.text_only && !self.stacked() {
            settled_control_column(self.ui)
                .unwrap_or(self.label_width)
                .max(floor)
                .min(self.longest.max(self.label_width))
        } else {
            column.min(value_room.max(floor))
        };
        let snaps = self.in_page && !self.text_only && !self.stacked() && !self.one_column;
        let line = measured + mark + self.nest + LABEL_WRAP_SLACK;
        if snaps {
            self.section_widest = self.section_widest.max(label_width);
            let wraps = need > 0.0 && label_width < line;
            let fits = wraps && line <= value_room;
            self.line_widest = self.line_widest.max(if fits { line } else { label_width });
            let fits_own = wraps && line <= line_room;
            self.own_line_widest =
                self.own_line_widest
                    .max(if fits_own { line } else { label_width });
            if fits_own {
                self.own_lines.push(line);
            }
        }
        if snaps && need > 0.0 {
            self.widest_squeeze = self.widest_squeeze.max(label_width - value_room);
            self.least_room = self.least_room.min(value_room);
            self.least_line_room = self.least_line_room.min(line_room);
        }
        let own_line = snaps
            && need > 0.0
            && self.section_column == 0.0
            && label_width < line
            && line <= line_room;
        let label_width = if own_line {
            line
        } else if snaps {
            label_width.max(self.section_column)
        } else {
            label_width
        };
        let label_width = if self.in_page
            && mark > 0.0
            && label_width - self.nest < word + mark
            && tail_fit(self.ui, &label, label_width - self.nest, mark).is_none()
        {
            (word + mark + self.nest + LABEL_WRAP_SLACK).min(column.max(label_width))
        } else {
            label_width
        };
        let outer = self
            .ui
            .data(|d| d.get_temp::<Option<Id>>(row_key()))
            .flatten();
        self.ui.data_mut(|d| d.insert_temp(row_key(), Some(row)));
        let squeeze = if own_line {
            0.0
        } else if need > 0.0 && snaps {
            (label_width - value_room).clamp(0.0, gap / 2.0)
        } else {
            0.0
        };
        let squeeze = if snaps {
            squeeze.max(self.section_squeeze)
        } else {
            squeeze
        };
        #[cfg(test)]
        if snaps {
            DOCK_ROWS.with(|rows| {
                rows.borrow_mut().push(DockRow {
                    label: label.text().to_owned(),
                    section: self.section,
                    value_x: label_width - squeeze,
                    floor,
                    line,
                    shared: self.section_column > 0.0,
                    page: settled_control_column(self.ui).unwrap_or(0.0),
                });
            });
        }
        let inner = self.row_body(label, hint, label_width, squeeze, height, add);
        let now = settled_need(self.ui, row);
        if now.pass == self.ui.ctx().cumulative_pass_nr() {
            self.widest_need = self.widest_need.max(now.fixed - now.trailing);
        }
        if now.select {
            self.widest_select = self.widest_select.max(now.column_share(
                beside_label,
                beside_words,
                self.one_column,
            ));
            self.widest_dock_select =
                self.widest_dock_select
                    .max(now.column_share(beside_label, beside_words, false));
            self.widest_select_label = self.widest_select_label.max(measured + mark);
        }
        self.ui.data_mut(|d| d.insert_temp(row_key(), outer));
        check_need(self.ui, row, settled);
        inner
    }

    fn tag_columns(&mut self, tag: Option<&RowTag>) -> (f32, f32, bool) {
        let long = tag.map_or(0.0, |tag| tag.width(self.ui, false));
        self.widest_tag = self.widest_tag.max(long);
        let long = long.max(self.tag_column);
        let short = self.short_tags && tag.is_some_and(|tag| tag.tag.game_scope());
        let width = if short {
            tag_width(&short_galley(self.ui))
        } else {
            long
        };
        (width, long, short)
    }

    fn row_body<R>(
        &mut self,
        label: WidgetText,
        hint: Option<&str>,
        label_width: f32,
        squeeze: f32,
        height: f32,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let row_height = self.ui.spacing().interact_size.y.min(height);
        let top = self.ui.cursor().top();
        let text = label.text().to_owned();
        let in_page = self.in_page;
        let controls = !self.text_only;
        let nest = self.nest.min(label_width);
        let mark = if hint.is_some() {
            crate::app::widgets::info_mark::width(self.ui)
        } else {
            0.0
        };
        if self.stacked() {
            self.last_value = None;
            let inner = self.stacked_row(label, hint, height, row_height, add);
            self.mark_row(top);
            self.spot(&text, top);
            return inner;
        }

        let tag = self.tag.take();
        let (tag_width, long_width, short) = self.tag_columns(tag.as_ref());
        let game_tag = tag.as_ref().is_some_and(|tag| tag.tag.game_scope());
        let mut excess = 0.0_f32;
        let mut clicked = false;
        let mut value = Rangef::point(0.0);
        let inner = self
            .ui
            .scope(|ui| {
                ui.spacing_mut().interact_size.y = row_height;
                ui.horizontal(|ui| {
                    let start = ui.cursor().left();
                    ui.add_space(nest);
                    ui.allocate_ui_with_layout(
                        vec2(label_width - nest, height),
                        Layout::left_to_right(Align::Center),
                        |ui| {
                            let text_width = label_width - nest;
                            ui.set_width(text_width);
                            let tail = hint
                                .filter(|_| in_page)
                                .and_then(|_| tail_fit(ui, &label, text_width, mark));
                            match (hint, tail) {
                                (Some(hint), Some((wrap, tail))) => {
                                    let shown = ui
                                        .scope(|ui| {
                                            ui.set_max_width(wrap);
                                            let galley = label.into_galley(
                                                ui,
                                                Some(TextWrapMode::Wrap),
                                                ui.available_width(),
                                                TextStyle::Body,
                                            );
                                            ui.add(Label::new(rows_start_at_words(galley)))
                                        })
                                        .inner;
                                    let tail = tail.translate(shown.rect.min.to_vec2());
                                    let mut cell = ui.new_child(
                                        UiBuilder::new()
                                            .max_rect(Rect::from_min_size(
                                                pos2(
                                                    tail.right() + ui.spacing().item_spacing.x,
                                                    tail.top(),
                                                ),
                                                vec2(mark, tail.height()),
                                            ))
                                            .layout(Layout::left_to_right(Align::Center)),
                                    );
                                    info_mark_in_line(&mut cell, hint);
                                }
                                _ => {
                                    if in_page {
                                        ui.scope(|ui| {
                                            ui.set_max_width((text_width - mark).max(0.0));
                                            let galley = label.into_galley(
                                                ui,
                                                Some(TextWrapMode::Wrap),
                                                ui.available_width(),
                                                TextStyle::Body,
                                            );
                                            ui.add(Label::new(rows_start_at_words(galley)));
                                        });
                                    } else {
                                        ui.add(Label::new(label).truncate());
                                    }
                                    if let Some(hint) = hint {
                                        info_mark(ui, hint);
                                    }
                                }
                            }
                        },
                    );
                    ui.add_space(-squeeze);

                    let gap = ui.spacing().item_spacing.x;
                    let over = ui.cursor().left() - (start + label_width + gap);
                    if over > 0.0 {
                        ui.add_space(-over);
                    }
                    let column = if tag_width > 0.0 {
                        tag_width + gap
                    } else {
                        0.0
                    };
                    let room = ui.available_width();
                    let width = (room - column).max(0.0);
                    let left = ui.cursor().left();
                    value = Rangef::new(left, left + width);
                    let inner = ui
                        .allocate_ui_with_layout(
                            vec2(width, height),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                ui.set_max_width(width);
                                if controls {
                                    super::page::mark(super::page::Mark::Control, ui.max_rect());
                                }
                                if column > 0.0 {
                                    ui.set_min_width(width);
                                }
                                add(ui)
                            },
                        )
                        .inner;
                    if column > 0.0 {
                        need_more(ui, column);
                    }
                    if game_tag {
                        excess = control_need(ui) - (room - long_width - gap);
                    }
                    if let Some(tag) = &tag {
                        ui.allocate_ui_with_layout(
                            vec2(ui.available_width(), height),
                            Layout::left_to_right(Align::Center),
                            |ui| clicked = tag.show(ui, short),
                        );
                    }
                    inner
                })
                .inner
            })
            .inner;
        self.tag_clicked |= clicked;
        self.tag_excess = self.tag_excess.max(excess);
        self.last_value = Some(value);
        self.mark_row(top);
        self.spot(&text, top);
        inner
    }

    fn stacked_row<R>(
        &mut self,
        label: WidgetText,
        hint: Option<&str>,
        height: f32,
        row_height: f32,
        add: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let gap = self.ui.spacing().item_spacing.y;
        if self.stacked_rows > 0 {
            self.ui.add_space(gap);
        }
        self.stacked_rows += 1;
        let tag = self.tag.take();
        match (hint, tag) {
            (hint, Some(tag)) => {
                let mut clicked = false;
                self.ui.horizontal(|ui| {
                    ui.add(Label::new(label).wrap());
                    if let Some(hint) = hint {
                        info_mark(ui, hint);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        clicked = tag.show(ui, false);
                    });
                });
                self.tag_clicked |= clicked;
            }
            (Some(hint), None) => {
                self.ui.horizontal(|ui| {
                    ui.add(Label::new(label).wrap());
                    info_mark(ui, hint);
                });
            }
            (None, None) => {
                self.ui.add(Label::new(label).wrap());
            }
        }
        self.ui.add_space(-gap / 2.0);
        self.ui
            .scope(|ui| {
                ui.spacing_mut().interact_size.y = row_height;
                let width = ui.available_width();
                ui.allocate_ui_with_layout(
                    vec2(width, height),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_max_width(width);
                        add(ui)
                    },
                )
                .inner
            })
            .inner
    }
}

fn last_row(ui: &Ui, label: &WidgetText, width: f32) -> Option<(usize, Rect)> {
    let galley = rows_start_at_words(label.clone().into_galley(
        ui,
        Some(TextWrapMode::Wrap),
        width,
        TextStyle::Body,
    ));
    Some((galley.rows.len(), galley.rows.last()?.rect()))
}

fn tail_row(ui: &Ui, label: &WidgetText, width: f32, mark: f32) -> Option<Rect> {
    let (rows, last) = last_row(ui, label, width)?;
    (rows > 1 && last.right() + mark <= width).then_some(last)
}

fn widest_word(ui: &Ui, label: &WidgetText) -> f32 {
    label
        .text()
        .split_whitespace()
        .map(|word| {
            WidgetText::from(word)
                .into_galley(
                    ui,
                    Some(TextWrapMode::Extend),
                    f32::INFINITY,
                    TextStyle::Body,
                )
                .size()
                .x
        })
        .fold(0.0, f32::max)
}

fn tail_fit(ui: &Ui, label: &WidgetText, width: f32, mark: f32) -> Option<(f32, Rect)> {
    let (rows, last) = last_row(ui, label, width)?;
    if rows == 1 && last.right() + mark <= width {
        return None;
    }
    let least = widest_word(ui, label) + LABEL_WRAP_SLACK;
    let mut wrap = width;
    while wrap >= least {
        if let Some(tail) = tail_row(ui, label, wrap, mark) {
            return Some((wrap, tail));
        }
        wrap -= 1.0;
    }
    None
}

fn mark_floor(ui: &Ui, label: &WidgetText, word: f32, mark: f32) -> f32 {
    let mut width = word;
    while width < word + mark {
        let Some((rows, last)) = last_row(ui, label, width + LABEL_WRAP_SLACK) else {
            break;
        };
        if rows > 1 && last.right() + mark <= width + LABEL_WRAP_SLACK {
            return width;
        }
        width = (last.right() + mark).max(width + 0.5);
    }
    word + mark
}

fn text_height() -> f32 {
    egui::style::Spacing::default().interact_size.y
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_pass(ctx: &egui::Context, widths: &[f32]) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 400.0))),
            ..Default::default()
        };
        ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                Form::show(ui, "need_test", |form| {
                    form.row("Birthday", |ui| {
                        for width in widths {
                            need(ui, *width);
                        }
                    });
                });
            });
        })
    }

    #[test]
    fn a_hint_sits_under_its_row_as_large_as_the_row() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 400.0))),
            ..Default::default()
        };
        let mut texts = Vec::new();
        for _ in 0..3 {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    Form::show(ui, "hint_test", |form| {
                        let mut on = true;
                        form.toggle("Screenshot", &mut on);
                        form.hint("Off on this server.");
                        form.toggle("Every minute", &mut on);
                    });
                });
            });
            texts = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    Shape::Text(text) => Some((
                        text.galley.rect.translate(text.pos.to_vec2()),
                        text.galley.text().to_owned(),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>();
        }
        let find = |label: &str| {
            texts
                .iter()
                .find(|(_, text)| text == label)
                .unwrap_or_else(|| panic!("no {label:?} in {texts:?}"))
                .0
        };
        let row = find("Screenshot");
        let hint = find("Off on this server.");
        let next = find("Every minute");
        assert!(
            hint.top() >= row.bottom(),
            "hint {hint:?} above its row {row:?}"
        );
        assert!(
            hint.bottom() <= next.top(),
            "hint {hint:?} not before {next:?}"
        );
        assert!(
            (hint.left() - row.left()).abs() < 0.5,
            "hint {hint:?} not under its label {row:?}"
        );
        assert!(
            hint.top() - row.bottom() < next.top() - hint.bottom(),
            "hint {hint:?} is closer to {next:?} than to {row:?}"
        );
        assert!(
            hint.height() >= row.height() - 0.5,
            "hint {} pt high, row {} pt",
            hint.height(),
            row.height()
        );
    }

    #[test]
    fn the_select_column_settles_in_the_pass_after_its_rows_measure() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 400.0))),
            ..Default::default()
        };
        let output = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                Form::show(ui, "select_settle_test", |form| {
                    for (label, width) in [("Language", 90.0), ("Clock", 140.0)] {
                        form.row(label, |ui| select_need(ui, width));
                    }
                });
            });
        });
        assert_eq!(output.platform_output.num_completed_passes, 2);
        let reasons = output.platform_output.request_discard_reasons;
        assert!(
            reasons
                .iter()
                .all(|reason| !reason.reason.contains("select column")),
            "{reasons:?}"
        );
    }

    #[test]
    fn a_shrinking_select_leaves_its_label_one_line() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 400.0))),
            ..Default::default()
        };
        let label = "Game Boy Color boot ROM";
        let mut rows = Vec::new();
        for _ in 0..3 {
            let output = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    Form::show(ui, "shrink_test", |form| {
                        form.row("Model", |ui| select_need(ui, 100.0));
                        form.row("Boot ROM", |ui| shrinking_select_need(ui, 300.0));
                        form.row(label, |ui| shrinking_select_need(ui, 60.0));
                    });
                });
            });
            rows = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    Shape::Text(text) if text.galley.text() == label => {
                        Some(text.galley.rows.len())
                    }
                    _ => None,
                })
                .collect();
        }
        assert_eq!(rows, [1]);
    }

    #[test]
    fn a_long_label_beside_no_select_leaves_a_shrinking_select_whole() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 400.0))),
            ..Default::default()
        };
        let mut room = 0.0;
        for _ in 0..3 {
            let _ = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    Form::show(ui, "shrink_beside_toggle", |form| {
                        form.row("Boot ROM", |ui| {
                            room = ui.available_width();
                            shrinking_select_need(ui, 200.0);
                        });
                        form.row("A checkbox label long enough to crowd", |ui| need(ui, 20.0));
                    });
                });
            });
        }
        assert!(
            room >= 199.0,
            "the file select got {room} of the 200 it needs"
        );
    }

    #[test]
    fn a_column_head_starts_where_the_checks_below_it_start() {
        let ctx = egui::Context::default();
        let mut output = None;
        for _ in 0..4 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 400.0))),
                ..Default::default()
            };
            output = Some(ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    Form::show(ui, "head_test", |form| {
                        let mut on = true;
                        form.toggle("Saves on this network", &mut on);
                        form.column_head("Devices", "Sync saves");
                        form.row("Steam Deck", |ui| {
                            ui.add(crate::app::widgets::check::Check::new(&mut on, ""));
                            ui.label("Not syncing");
                        });
                    });
                });
            }));
        }
        let output = output.expect("a pass");
        let (head, head_top) = output
            .shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                Shape::Text(text) if text.galley.text() == "Sync saves" => {
                    Some((text.pos.x + text.galley.rect.left(), text.pos.y))
                }
                _ => None,
            })
            .expect("the head");
        let boxes: Vec<f32> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                Shape::Rect(rect)
                    if (rect.rect.width() - ctx.style().spacing.icon_width).abs() < 0.5
                        && rect.rect.top() > head_top =>
                {
                    Some(rect.rect.left())
                }
                _ => None,
            })
            .collect();
        assert!(!boxes.is_empty(), "no check boxes");
        for left in boxes {
            assert!((left - head).abs() < 0.5, "head {head}, check {left}");
        }
    }

    #[test]
    fn a_row_note_starts_on_the_label_edge() {
        let ctx = egui::Context::default();
        let mut output = None;
        for _ in 0..4 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(600.0, 400.0))),
                ..Default::default()
            };
            output = Some(ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    Form::show(ui, "note_test", |form| {
                        let mut on = true;
                        form.row("Clara's laptop", |ui| {
                            ui.add(crate::app::widgets::check::Check::new(&mut on, ""));
                        });
                        form.row_note("On that device, turn on \"Sync saves\"");
                    });
                });
            }));
        }
        let output = output.expect("a pass");
        let left = |wanted: &str| {
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    Shape::Text(text) if text.galley.text() == wanted => {
                        Some(text.pos.x + text.galley.rect.left())
                    }
                    _ => None,
                })
                .expect(wanted)
        };
        let name = left("Clara's laptop");
        let note = left("On that device, turn on \"Sync saves\"");
        assert!((name - note).abs() < 0.5, "name {name}, note {note}");
    }

    #[test]
    fn every_row_keeps_its_control_in_one_column_beside_tags_of_any_width() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 400.0))),
            ..Default::default()
        };
        let mut spans = Vec::new();
        for _ in 0..4 {
            spans.clear();
            let _ = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    Form::show(ui, "tag_column_test", |form| {
                        for (label, tag) in [
                            ("Grid lines", Some(Tag::Follows)),
                            ("Ghosting strength", Some(Tag::Own)),
                            ("HUD opacity", None),
                        ] {
                            let tag = tag.map(|tag| RowTag {
                                tag,
                                hover: String::new(),
                            });
                            form.with_tag(tag, |form| {
                                form.row(label, |ui| {
                                    need(ui, 100.0);
                                    let left = ui.max_rect().left();
                                    spans.push((left, ui.available_width()));
                                });
                            });
                        }
                    });
                });
            });
        }
        assert_eq!(spans.len(), 3);
        for (left, width) in &spans[1..] {
            assert!((left - spans[0].0).abs() < 0.5, "{spans:?}");
            assert!((width - spans[0].1).abs() < 0.5, "{spans:?}");
        }
    }

    #[test]
    fn every_tag_sits_in_one_column_whatever_its_control_takes() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 400.0))),
            ..Default::default()
        };
        let mut output = None;
        for _ in 0..4 {
            output = Some(ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    Form::show(ui, "tag_x_test", |form| {
                        for (label, tag, used) in [
                            ("LCD grid", Tag::Own, 18.0),
                            ("Grid lines", Tag::Follows, 120.0),
                            ("Ghosting", Tag::Follows, 18.0),
                        ] {
                            let tag = Some(RowTag {
                                tag,
                                hover: String::new(),
                            });
                            form.with_tag(tag, |form| {
                                form.row(label, |ui| {
                                    need(ui, 120.0);
                                    ui.allocate_exact_size(vec2(used, 18.0), Sense::hover());
                                });
                            });
                        }
                    });
                });
            }));
        }
        let mut lefts = Vec::new();
        let mut shapes: Vec<Shape> = output
            .expect("ran")
            .shapes
            .into_iter()
            .map(|clipped| clipped.shape)
            .collect();
        while let Some(shape) = shapes.pop() {
            match shape {
                Shape::Vec(inner) => shapes.extend(inner),
                Shape::Text(text) if text.galley.text().contains("game") => {
                    lefts.push(text.pos.x);
                }
                _ => {}
            }
        }
        assert_eq!(lefts.len(), 3, "{lefts:?}");
        for left in &lefts[1..] {
            assert!((left - lefts[0]).abs() < 0.5, "{lefts:?}");
        }
    }

    #[test]
    fn the_strip_under_a_select_lines_up_with_the_select() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(400.0, 400.0))),
            ..Default::default()
        };
        let mut select = None;
        let mut strip = None;
        for _ in 0..4 {
            let _ = ctx.run(input.clone(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    Form::show(ui, "strip_test", |form| {
                        form.row("A long label for a checkbox row", |ui| need(ui, 20.0));
                        form.with_tag(
                            Some(RowTag {
                                tag: Tag::Own,
                                hover: String::new(),
                            }),
                            |form| {
                                form.row("Palette", |ui| {
                                    select_need(ui, 120.0);
                                    select = Some(ui.max_rect().x_range());
                                });
                            },
                        );
                        form.under_value(|ui| strip = Some(ui.max_rect().x_range()));
                    });
                });
            });
        }
        let (select, strip) = (select.expect("select"), strip.expect("strip"));
        assert!((select.min - strip.min).abs() < 0.5, "{select:?} {strip:?}");
        assert!((select.max - strip.max).abs() < 0.5, "{select:?} {strip:?}");
    }

    #[test]
    fn a_row_with_two_needs_settles_after_one_pass() {
        let ctx = egui::Context::default();
        run_pass(&ctx, &[40.0, 90.0]);
        for _ in 0..3 {
            let output = run_pass(&ctx, &[40.0, 90.0]);
            assert!(
                output.platform_output.request_discard_reasons.is_empty(),
                "{:?}",
                output.platform_output.request_discard_reasons
            );
        }
    }

    #[test]
    fn a_row_asks_again_when_its_need_changes() {
        let ctx = egui::Context::default();
        run_pass(&ctx, &[40.0, 90.0]);
        run_pass(&ctx, &[40.0, 90.0]);
        let output = run_pass(&ctx, &[40.0, 60.0]);
        assert_eq!(output.platform_output.num_completed_passes, 2);
        let output = run_pass(&ctx, &[40.0, 60.0]);
        assert!(output.platform_output.request_discard_reasons.is_empty());
    }
}
