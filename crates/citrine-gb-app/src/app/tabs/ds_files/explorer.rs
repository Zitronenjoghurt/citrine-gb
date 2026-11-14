use super::{DsFilesUi, group_color, group_icon, size_text};
use crate::app::ui_state::ui_theme;
use crate::icons;
use citrine_gb_nds_files::{FileTree, Group, NodeId, Row, Sort};
use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align, Align2, CornerRadius, FontId, Layout, Rect, RichText, Sense, Stroke, TextFormat, Ui,
    pos2, vec2,
};

const INDENT: f32 = 16.0;
const CARET: f32 = 18.0;
const ICON: f32 = 24.0;
const TYPE_COLUMN: f32 = 64.0;
const SIZE_COLUMN: f32 = 76.0;
const ID_COLUMN: f32 = 52.0;
const ID_MIN_WIDTH: f32 = 420.0;
const TYPE_MIN_WIDTH: f32 = 340.0;
const SIZE_MIN_WIDTH: f32 = 220.0;
const CHIP_TEXT: f32 = 11.0;

enum RowClick {
    Toggle,
    Select,
    Open,
}

pub(super) fn show(
    state: &mut DsFilesUi,
    ui: &mut Ui,
    phone: bool,
    spawner: &citrine_gb_threads::Spawner,
) -> Option<NodeId> {
    if state.carts.is_empty() {
        empty(state, ui, spawner);
        return None;
    }
    source(state, ui, spawner, phone);
    if let Some(problem) = &state.problem {
        ui.colored_label(ui.visuals().error_fg_color, problem);
    }
    search(state, ui, phone);
    filters(state, ui);
    if phone {
        phone_list(state, ui)
    } else {
        desktop_tree(state, ui)
    }
}

fn empty(state: &mut DsFilesUi, ui: &mut Ui, spawner: &citrine_gb_threads::Spawner) {
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_space((ui.available_height() / 2.0 - 70.0).max(12.0));
        ui.label(RichText::new(icons::FOLDER_OPEN).size(48.0).weak());
        ui.add_space(8.0);
        ui.label("Open a .nds file or start a DS game");
        ui.add_space(8.0);
        if ui
            .button(format!("{}  Open .nds file", icons::FOLDER))
            .clicked()
        {
            state.pick_file(spawner);
        }
        if let Some(problem) = &state.problem {
            ui.colored_label(ui.visuals().error_fg_color, problem);
        }
    });
}

fn source(state: &mut DsFilesUi, ui: &mut Ui, spawner: &citrine_gb_threads::Spawner, phone: bool) {
    if phone && (state.folder.is_some() || state.query.is_search()) {
        return;
    }
    let name = state
        .cart()
        .map(|cart| cart.name.clone())
        .unwrap_or_default();
    let mut chosen = None;
    let mut pick = false;
    ui.horizontal(|ui| {
        let label = format!("{}  {name}  {}", icons::GAME_CONTROLLER, icons::CARET_DOWN);
        ui.menu_button(label, |ui| {
            for (index, cart) in state.carts.iter().enumerate() {
                let text = if cart.from_game {
                    format!("{}  {}", icons::PLAY, cart.name)
                } else {
                    format!("{}  {}", icons::FILE, cart.name)
                };
                if ui
                    .add(egui::Button::selectable(index == state.current, text))
                    .clicked()
                {
                    chosen = Some(index);
                }
            }
            ui.separator();
            if ui
                .button(format!("{}  Open .nds file", icons::FOLDER))
                .clicked()
            {
                pick = true;
            }
        });
    });
    if let Some(index) = chosen {
        state.choose(index);
    }
    if pick {
        state.pick_file(spawner);
    }
    ui.add_space(4.0);
}

fn search(state: &mut DsFilesUi, ui: &mut Ui, phone: bool) {
    let total = state.cart().map_or(0, |cart| cart.tree.file_count());
    let mut text = state.query.text.clone();
    let height = ui.spacing().interact_size.y + if phone { 12.0 } else { 6.0 };
    let visuals = ui.visuals().clone();
    egui::Frame::new()
        .fill(visuals.extreme_bg_color)
        .stroke(visuals.widgets.inactive.bg_stroke)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(8, 0))
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), height),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.label(RichText::new(icons::MAGNIFYING_GLASS).weak());
                    let side = height - 6.0;
                    let room = ui.available_width() - side * 2.0 - 8.0;
                    ui.add(
                        egui::TextEdit::singleline(&mut text)
                            .hint_text(if room >= 150.0 {
                                format!("Search {} files", thousands(total))
                            } else {
                                "Search".to_owned()
                            })
                            .frame(false)
                            .desired_width(room.max(40.0)),
                    );
                    if !text.is_empty()
                        && ui
                            .add(egui::Button::new(icons::X).frame(false))
                            .on_hover_text("Clear")
                            .clicked()
                    {
                        text.clear();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        filter_button(state, ui, phone, side);
                    });
                },
            );
        });
    state.set_query_text(text);
    if phone && state.filter_sheet {
        filter_sheet(state, ui);
    }
    ui.add_space(4.0);
}

fn filter_button(state: &mut DsFilesUi, ui: &mut Ui, phone: bool, side: f32) {
    let mut text = RichText::new(icons::FUNNEL).size(18.0);
    if !state.query.groups.is_empty() {
        text = text.color(ui_theme::brand(ui.visuals()));
    }
    if phone {
        let button = egui::Button::new(text)
            .frame(false)
            .min_size(egui::Vec2::splat(side));
        if ui.add(button).on_hover_text("Filter").clicked() {
            state.filter_sheet = true;
        }
        return;
    }
    ui.menu_button(text, |ui| filter_list(state, ui))
        .response
        .on_hover_text("Filter");
}

fn filter_list(state: &mut DsFilesUi, ui: &mut Ui) {
    let counts = state
        .cart()
        .map(|cart| cart.tree.group_counts())
        .unwrap_or_default();
    let dark = ui.visuals().dark_mode;
    ui.label(RichText::new("Show").weak());
    for group in Group::ALL {
        let mut on = state.query.groups.contains(&group);
        let count = counts.get(&group).copied().unwrap_or(0);
        let response = ui
            .horizontal(|ui| {
                let changed = ui.checkbox(&mut on, "").changed();
                ui.label(RichText::new(group_icon(group)).color(group_color(group, dark)));
                ui.label(group.label());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(thousands(count)).weak());
                });
                changed
            })
            .inner;
        if response {
            state.toggle_group(group);
        }
    }
}

fn filter_sheet(state: &mut DsFilesUi, ui: &mut Ui) {
    let id = egui::Id::new("ds_files_filter_sheet");
    let response = crate::app::mobile::sheet::show(ui.ctx(), id, "Filter", |ui| {
        filter_list(state, ui);
        ui.separator();
        ui.label(RichText::new("Sort by").weak());
        ui.horizontal(|ui| {
            for sort in [Sort::Name, Sort::Type, Sort::Size, Sort::Id] {
                if ui
                    .add(egui::Button::selectable(
                        state.query.sort == sort,
                        sort_label(sort),
                    ))
                    .clicked()
                {
                    pick_sort(state, sort);
                }
            }
        });
    });
    if response.closed {
        state.filter_sheet = false;
    }
}

fn filters(state: &mut DsFilesUi, ui: &mut Ui) {
    if !state.query.is_search() {
        return;
    }
    state.rows();
    let total = state.cart().map_or(0, |cart| cart.tree.file_count());
    let dark = ui.visuals().dark_mode;
    let mut removed = None;
    ui.horizontal(|ui| {
        for &group in &state.query.groups {
            let text = RichText::new(format!("{}  {}", group.label(), icons::X));
            let chip = egui::Button::new(text)
                .stroke(Stroke::new(1.0_f32, group_color(group, dark)))
                .corner_radius(CornerRadius::same(12));
            if ui.add(chip).on_hover_text("Remove this filter").clicked() {
                removed = Some(group);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!(
                    "{} of {}",
                    thousands(state.count),
                    thousands(total)
                ))
                .weak(),
            );
        });
    });
    if let Some(group) = removed {
        state.toggle_group(group);
    }
    ui.add_space(2.0);
}

fn desktop_tree(state: &mut DsFilesUi, ui: &mut Ui) -> Option<NodeId> {
    let wide = ui.available_width() >= ID_MIN_WIDTH;
    header(state, ui, wide);
    let height = ui.spacing().interact_size.y + 8.0;
    let rows: Vec<Row> = state.rows().to_vec();
    let cart = state.cart().cloned()?;
    let mut click = None;
    egui::ScrollArea::vertical()
        .id_salt("ds_files_tree")
        .auto_shrink(false)
        .show_rows(ui, height, rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in &rows[range] {
                let selected = state.selected == Some(row.id);
                if let Some(action) = tree_row(ui, &cart.tree, *row, selected, wide, height) {
                    click = Some((row.id, action));
                }
            }
        });
    let (node, action) = click?;
    match action {
        RowClick::Toggle => {
            state.selected = Some(node);
            state.toggle_open(node);
            None
        }
        RowClick::Select => {
            state.selected = Some(node);
            None
        }
        RowClick::Open => {
            state.selected = Some(node);
            Some(node)
        }
    }
}

fn header(state: &mut DsFilesUi, ui: &mut Ui, wide: bool) {
    let height = ui.spacing().interact_size.y;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let mut right = rect.right() - 8.0;
    let mut columns = Vec::new();
    if wide {
        columns.push((
            Sort::Id,
            "ID",
            Rect::from_min_max(
                pos2(right - ID_COLUMN, rect.top()),
                pos2(right, rect.bottom()),
            ),
            Align2::RIGHT_CENTER,
        ));
        right -= ID_COLUMN;
    }
    if rect.width() >= SIZE_MIN_WIDTH {
        columns.push((
            Sort::Size,
            "Size",
            Rect::from_min_max(
                pos2(right - SIZE_COLUMN, rect.top()),
                pos2(right, rect.bottom()),
            ),
            Align2::RIGHT_CENTER,
        ));
        right -= SIZE_COLUMN;
    }
    if rect.width() >= TYPE_MIN_WIDTH {
        columns.push((
            Sort::Type,
            "Type",
            Rect::from_min_max(
                pos2(right - TYPE_COLUMN - 8.0, rect.top()),
                pos2(right - 8.0, rect.bottom()),
            ),
            Align2::LEFT_CENTER,
        ));
        right -= TYPE_COLUMN + 8.0;
    }
    columns.push((
        Sort::Name,
        "Name",
        Rect::from_min_max(
            pos2(rect.left() + CARET + ICON + 6.0, rect.top()),
            pos2(right, rect.bottom()),
        ),
        Align2::LEFT_CENTER,
    ));
    let weak = ui.visuals().weak_text_color();
    let font = FontId::proportional(12.0);
    for (sort, label, column, align) in columns {
        let response = ui.interact(
            column,
            ui.id().with(("ds_files_sort", label)),
            Sense::click(),
        );
        let mut text = label.to_owned();
        if state.query.sort == sort {
            let arrow = if state.query.descending {
                icons::CARET_DOWN
            } else {
                icons::CARET_UP
            };
            text = format!("{text} {arrow}");
        }
        let color = if response.hovered() {
            ui.visuals().text_color()
        } else {
            weak
        };
        let at = align.pos_in_rect(&column);
        ui.painter().text(at, align, text, font.clone(), color);
        if response.clicked() {
            pick_sort(state, sort);
        }
    }
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color),
    );
}

fn pick_sort(state: &mut DsFilesUi, sort: Sort) {
    if state.query.sort == sort {
        state.query.descending = !state.query.descending;
    } else {
        state.query.sort = sort;
        state.query.descending = false;
    }
    state.changed();
}

fn sort_label(sort: Sort) -> &'static str {
    match sort {
        Sort::Name => "Name",
        Sort::Type => "Type",
        Sort::Size => "Size",
        Sort::Id => "ID",
    }
}

fn tree_row(
    ui: &mut Ui,
    tree: &FileTree,
    row: Row,
    selected: bool,
    wide: bool,
    height: f32,
) -> Option<RowClick> {
    let node = tree.node(row.id);
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    if !ui.is_rect_visible(rect) {
        return None;
    }
    let visuals = ui.visuals();
    let dark = visuals.dark_mode;
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 4.0, ui_theme::brand(visuals).gamma_multiply(0.25));
    } else if response.hovered() {
        painter.add(crate::app::widgets::row::row_highlight(
            ui,
            rect.shrink(2.0),
        ));
    }
    let text = visuals.text_color();
    let weak = visuals.weak_text_color();
    let body = FontId::proportional(14.0);
    let mut x = rect.left() + 4.0 + f32::from(row.depth) * INDENT;
    let caret = Rect::from_min_size(pos2(x, rect.top()), vec2(CARET, height));
    if node.has_children() {
        let arrow = if row.open {
            icons::CARET_DOWN
        } else {
            icons::CARET_RIGHT
        };
        painter.text(
            caret.center(),
            Align2::CENTER_CENTER,
            arrow,
            FontId::proportional(12.0),
            weak,
        );
    }
    x += CARET;
    let (icon, color) = match node.file_type() {
        None if row.open => (icons::FOLDER_OPEN, weak),
        None => (icons::FOLDER, weak),
        Some(kind) => (group_icon(kind.group()), group_color(kind.group(), dark)),
    };
    painter.text(
        pos2(x + ICON / 2.0, rect.center().y),
        Align2::CENTER_CENTER,
        icon,
        FontId::proportional(15.0),
        color,
    );
    x += ICON + 6.0;

    let mut right = rect.right() - 8.0;
    if wide {
        if let Some(id) = node.cart_id() {
            painter.text(
                pos2(right, rect.center().y),
                Align2::RIGHT_CENTER,
                id.to_string(),
                FontId::proportional(12.0),
                weak,
            );
        }
        right -= ID_COLUMN;
    }
    if rect.width() >= SIZE_MIN_WIDTH {
        painter.text(
            pos2(right, rect.center().y),
            Align2::RIGHT_CENTER,
            size_text(node.size()),
            FontId::proportional(13.0),
            text,
        );
        right -= SIZE_COLUMN;
    }
    if rect.width() >= TYPE_MIN_WIDTH
        && let Some(kind) = node.file_type()
    {
        let color = group_color(kind.group(), dark);
        let galley = painter.layout_no_wrap(
            kind.label().to_owned(),
            FontId::proportional(CHIP_TEXT),
            color,
        );
        let chip = Rect::from_min_size(
            pos2(right - TYPE_COLUMN - 8.0, rect.center().y - 9.0),
            vec2(galley.size().x + 12.0, 18.0),
        );
        painter.rect_filled(
            chip,
            9.0,
            color.gamma_multiply(if dark { 0.22 } else { 0.14 }),
        );
        painter.galley(
            pos2(chip.left() + 6.0, chip.center().y - galley.size().y / 2.0),
            galley,
            color,
        );
    }
    if rect.width() >= TYPE_MIN_WIDTH {
        right -= TYPE_COLUMN + 8.0;
    }

    let mut job = LayoutJob::single_section(node.name().to_owned(), TextFormat::simple(body, text));
    job.wrap = TextWrapping::truncate_at_width((right - x - 4.0).max(10.0));
    let galley = painter.layout_job(job);
    painter.galley(
        pos2(x, rect.center().y - galley.size().y / 2.0),
        galley,
        text,
    );

    if response.double_clicked() {
        if node.is_folder() {
            return None;
        }
        return Some(RowClick::Open);
    }
    if response.clicked() {
        let on_caret = response
            .interact_pointer_pos()
            .is_some_and(|at| caret.contains(at));
        if node.has_children() && (on_caret || node.is_folder()) {
            return Some(RowClick::Toggle);
        }
        return Some(RowClick::Select);
    }
    None
}

fn phone_list(state: &mut DsFilesUi, ui: &mut Ui) -> Option<NodeId> {
    let cart = state.cart().cloned()?;
    let tree = &cart.tree;
    let search = state.query.is_search();
    let nodes = if search {
        tree.found(&state.query)
    } else {
        tree.listing(state.folder.unwrap_or(0), &state.query)
    };
    if !search && let Some(folder) = state.folder {
        ui.label(RichText::new(tree.path(folder).join(" › ")).weak().small());
    }
    let height = ui.spacing().interact_size.y + 28.0;
    let mut tapped = None;
    egui::ScrollArea::vertical()
        .id_salt("ds_files_phone")
        .auto_shrink(false)
        .show_rows(ui, height, nodes.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for &id in &nodes[range] {
                if phone_row(ui, tree, id, search, height) {
                    tapped = Some(id);
                }
            }
        });
    let node = tapped?;
    if !tree.node(node).has_children() {
        return Some(node);
    }
    if search {
        state.set_query_text(String::new());
        state.query.groups.clear();
        state.changed();
    }
    state.folder = Some(node);
    None
}

fn phone_row(ui: &mut Ui, tree: &FileTree, id: NodeId, search: bool, height: f32) -> bool {
    let node = tree.node(id);
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    if !ui.is_rect_visible(rect) {
        return false;
    }
    let visuals = ui.visuals();
    let dark = visuals.dark_mode;
    let painter = ui.painter();
    if response.is_pointer_button_down_on() {
        painter.add(crate::app::widgets::row::row_highlight(
            ui,
            rect.shrink(2.0),
        ));
    }
    let text = visuals.text_color();
    let weak = visuals.weak_text_color();
    let (icon, color) = match node.file_type() {
        None => (icons::FOLDER, weak),
        Some(kind) => (group_icon(kind.group()), group_color(kind.group(), dark)),
    };
    painter.text(
        pos2(rect.left() + 18.0, rect.center().y),
        Align2::CENTER_CENTER,
        icon,
        FontId::proportional(18.0),
        color,
    );
    let x = rect.left() + 40.0;
    let chevron = node.has_children() && !search;
    let right = rect.right() - if chevron { 28.0 } else { 8.0 };
    let mut job = LayoutJob::single_section(
        node.name().to_owned(),
        TextFormat::simple(FontId::proportional(16.0), text),
    );
    job.wrap = TextWrapping::truncate_at_width(right - x);
    let name = painter.layout_job(job);
    let top = rect.center().y - 19.0;
    painter.galley(pos2(x, top), name, text);
    let mut sub_x = x;
    if let Some(kind) = node.file_type() {
        let color = group_color(kind.group(), dark);
        let galley = painter.layout_no_wrap(
            kind.label().to_owned(),
            FontId::proportional(CHIP_TEXT),
            color,
        );
        let chip = Rect::from_min_size(pos2(sub_x, top + 22.0), vec2(galley.size().x + 12.0, 18.0));
        painter.rect_filled(
            chip,
            9.0,
            color.gamma_multiply(if dark { 0.22 } else { 0.14 }),
        );
        painter.galley(
            pos2(chip.left() + 6.0, chip.center().y - galley.size().y / 2.0),
            galley,
            color,
        );
        sub_x = chip.right() + 8.0;
    }
    let detail = if search {
        let path = tree.path(id);
        path[..path.len() - 1].join(" › ")
    } else {
        size_text(node.size())
    };
    let mut job =
        LayoutJob::single_section(detail, TextFormat::simple(FontId::proportional(12.0), weak));
    job.wrap = TextWrapping::truncate_at_width((right - sub_x).max(10.0));
    let galley = painter.layout_job(job);
    painter.galley(
        pos2(sub_x, top + 31.0 - galley.size().y / 2.0),
        galley,
        weak,
    );
    if chevron {
        painter.text(
            pos2(rect.right() - 14.0, rect.center().y),
            Align2::CENTER_CENTER,
            icons::CARET_RIGHT,
            FontId::proportional(14.0),
            weak,
        );
    }
    painter.hline(
        rect.x_range().shrink(8.0),
        rect.bottom(),
        Stroke::new(
            1.0_f32,
            visuals
                .widgets
                .noninteractive
                .bg_stroke
                .color
                .gamma_multiply(0.5),
        ),
    );
    response.clicked()
}

pub fn thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_get_commas() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(14_212), "14,212");
        assert_eq!(thousands(1_234_567), "1,234,567");
    }
}
