use super::{Sheet, label, opens, pick};
use crate::app::Citrine;
use crate::app::mobile::kit::{self, Row, SwitchRow};
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::ui_state::settings_tree::{GroupId, SettingsTab, Surface};
use crate::app::widgets::progress::ProgressBar;
use crate::app::widgets::settings::online::{self, CLOUD_HINT, JoinSheet, server_set};
use crate::app::widgets::settings::page::{Page, section_note};
use crate::app::widgets::settings::pages::sync::{
    ACCOUNT_LOCKED, DEVICES, GET, GETTING, KEEP_HINT, NETWORK_HINT, NO_DEVICES, SETTINGS_HINT,
    SIGN_IN, SYNC_WITH, facts, notes, placed, set_allowed, settings_sync_on, spoken, status,
    this_kind, title, title_line, turn_on_there, why,
};
use crate::online::cloud::Elsewhere;
use crate::progress::Progress;
use egui::{Align, Context, FontId, Layout, Rect, RichText, Sense, Ui, vec2};

const ROW_PAD: f32 = 8.0;
const NAME_EDGE: f32 = 12.0;

impl Citrine {
    pub(super) fn sync_page(&mut self, ui: &mut Ui) {
        let dev_mode = self.ui.settings.dev_mode;
        let signed_out = !self.ui.settings.online.is_joined();
        let mut open = None;
        let mut sign_in = false;
        let cloud_notes = notes(&self.ui.settings, Some(&self.online));
        let mut page = Page::begin(ui);
        for group in SettingsTab::Sync.groups_on(Surface::Phone, dev_mode) {
            page.tree_group(group, None, |ui| {
                kit::list(ui, |list| {
                    for row in placed(group, group.rows_on(Surface::Phone, dev_mode), signed_out) {
                        if let Some(sheet) = self.sync_row(list, row) {
                            open = Some(sheet);
                        }
                    }
                    if group == GroupId::Cloud && signed_out {
                        sign_in = list.row(Row::new(SIGN_IN)).clicked();
                    }
                });
                if group == GroupId::Cloud {
                    for text in &cloud_notes {
                        section_note(ui, true, *text);
                    }
                }
            });
        }
        page.end();
        self.open_sheet(open);
        if sign_in {
            self.sign_in_here();
        }
        if signed_out && self.mobile.join_sheet.is_some() {
            let now = web_time::Instant::now();
            online::track(
                &mut self.ui.settings,
                &mut self.online,
                &mut self.ui.join,
                now,
            );
            self.signed_out_sheet(ui);
        }
    }

    fn sign_in_here(&mut self) {
        let now = web_time::Instant::now();
        let s = &mut self.ui.settings;
        let sheet = if server_set(s) {
            JoinSheet::SignIn
        } else {
            JoinSheet::Server
        };
        online::open_sheet(s, &mut self.online, &mut self.ui.join, sheet, now);
        self.mobile.join_sheet = Some(sheet);
        self.release_touch();
    }

    fn sync_row(&mut self, list: &mut kit::List<'_>, row: RowId) -> Option<Sheet> {
        let s = &mut self.ui.settings;
        match row {
            RowId::CloudSaves => {
                let joined = s.online.is_joined();
                let mut on = joined && s.online.cloud_saves;
                if list.switch(
                    SwitchRow::new(label(row), &mut on)
                        .info(CLOUD_HINT)
                        .enabled(joined),
                ) {
                    s.online.cloud_saves = on;
                    s.dirty = true;
                }
                None
            }
            RowId::SyncSettings => {
                let can = settings_sync_on(s);
                let mut on = can && s.online.sync_settings;
                if list.switch(
                    SwitchRow::new(label(row), &mut on)
                        .info(SETTINGS_HINT)
                        .enabled(can),
                ) {
                    s.online.sync_settings = on;
                    s.dirty = true;
                }
                None
            }
            RowId::KeepSaves => opens(
                list,
                Row::new(label(row))
                    .info(KEEP_HINT)
                    .value(s.online.keep_saves.to_string()),
                Sheet::KeepSaves,
            ),
            RowId::NetworkSaves => {
                if list.switch(
                    SwitchRow::new(label(row), &mut s.online.network_saves).info(NETWORK_HINT),
                ) {
                    s.dirty = true;
                }
                None
            }
            RowId::NetworkDevices => {
                if !s.online.network_saves {
                    return None;
                }
                let devices = self.online.network_devices();
                if devices.is_empty() {
                    list.row(Row::new(NO_DEVICES).no_chevron().enabled(false));
                    return None;
                }
                let head = list.custom(devices_head);
                let me = self.online.device_name();
                let kind = this_kind(list.ui().ctx());
                pinned_rows(list, head, |list| {
                    for device in &devices {
                        let there = turn_on_there(device, &me, kind);
                        let spoken = spoken(device);
                        let mut allowed = device.allowed;
                        let mut row = SwitchRow::new(&device.name, &mut allowed)
                            .value(status(device))
                            .note(there.as_deref())
                            .spoken(&spoken)
                            .locked(device.account);
                        if device.account {
                            row = row.info(ACCOUNT_LOCKED);
                        }
                        if list.switch(row) {
                            set_allowed(s, device, allowed);
                        }
                    }
                });
                None
            }
            RowId::SavesElsewhere => {
                let games = self.online.saves_elsewhere();
                for game in &games {
                    let get = list.custom(|ui| {
                        ui.push_id(("elsewhere", game.game, &game.rom), |ui| {
                            elsewhere_row(ui, game)
                        })
                        .inner
                    });
                    if get {
                        self.online.fetch_elsewhere(game);
                    }
                }
                None
            }
            _ => None,
        }
    }

    pub(super) fn sync_sheet(&mut self, ctx: &Context) -> bool {
        let s = &mut self.ui.settings;
        let (changed, done) = pick(ctx, label(RowId::KeepSaves), None, &mut s.online.keep_saves);
        s.dirty |= changed;
        done
    }
}

fn devices_head(ui: &mut Ui) -> Rect {
    let size = ui.fonts_mut(|f| f.row_height(&FontId::proportional(kit::DETAIL_SIZE)));
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), size + 2.0 * ROW_PAD),
        Sense::hover(),
    );
    if ui.is_rect_visible(rect) {
        paint_head(ui, rect);
    }
    rect
}

fn paint_head(ui: &Ui, rect: Rect) {
    let color = kit::value_color(ui.visuals());
    let font = FontId::proportional(kit::DETAIL_SIZE);
    let painter = ui.painter();
    painter.text(
        egui::pos2(rect.left() + NAME_EDGE, rect.center().y),
        egui::Align2::LEFT_CENTER,
        DEVICES,
        font.clone(),
        color,
    );
    painter.text(
        egui::pos2(rect.right() - kit::VALUE_EDGE, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        SYNC_WITH,
        font,
        color,
    );
}

fn pinned_rows<R>(
    list: &mut kit::List<'_>,
    head: Rect,
    rows: impl FnOnce(&mut kit::List<'_>) -> R,
) -> R {
    let clip = list.ui().clip_rect();
    let key = list.ui().id().with("devices_end");
    let known = list.ui().data(|d| d.get_temp::<f32>(key));
    let end = known.map_or(f32::INFINITY, |gap| head.top() + gap);
    if let Some(pinned) = pinned_head(head, end, clip.top()) {
        list.ui().set_clip_rect(clip.with_min_y(pinned.bottom()));
    }
    let out = rows(list);
    list.ui().set_clip_rect(clip);
    let gap = list.ui().min_rect().bottom() - head.top();
    if known.is_none_or(|known| (known - gap).abs() > 0.5) {
        list.ui().data_mut(|d| d.insert_temp(key, gap));
        list.ui()
            .ctx()
            .request_discard("the devices head leaves with its last row");
    }
    let edges = list.edges().to_vec();
    pin_head(list.ui(), head, head.top() + gap, &edges);
    out
}

fn pin_head(ui: &mut Ui, head: Rect, end: f32, edges: &[f32]) {
    let top = ui.clip_rect().top();
    let fill = kit::card_fill(ui.visuals());
    if let Some(above) = cut_above(head, top) {
        ui.painter().rect_filled(above, 0.0, fill);
        let _ = ui.interact(above, ui.id().with("cut_above"), Sense::click());
    }
    let Some(rect) = pinned_head(head, end, top) else {
        return;
    };
    let cover = rect.with_max_y(head_reach(rect, edges, end));
    let focused = ui
        .ctx()
        .memory(|m| m.focused())
        .and_then(|id| ui.ctx().read_response(id))
        .map(|response| response.rect)
        .filter(|row| row.top() >= head.bottom() && row.top() < cover.bottom());
    if let Some(row) = focused {
        ui.scroll_to_rect(row.with_min_y(row.top() - head.height()), None);
    }
    let _ = ui.interact(cover, ui.id().with("pinned_head"), Sense::click());
    let corners = head_corners(cover, end);
    if cover.bottom() < end - 0.5 {
        ui.painter()
            .rect_filled(cover.with_max_y(cover.bottom() + 0.5), corners, fill);
        let inset = f32::from(corners.sw);
        ui.painter().hline(
            cover.left() + inset..=cover.right() - inset,
            cover.bottom(),
            kit::hairline(ui.visuals()),
        );
    } else {
        ui.painter().rect_filled(cover, corners, fill);
    }
    paint_head(ui, rect);
}

fn head_reach(rect: Rect, edges: &[f32], end: f32) -> f32 {
    let next = edges
        .iter()
        .copied()
        .find(|y| *y > rect.bottom())
        .unwrap_or(end)
        .min(end);
    if next - rect.bottom() < rect.height() {
        next
    } else {
        rect.bottom()
    }
}

fn cut_above(head: Rect, top: f32) -> Option<Rect> {
    (head.top() >= top && head.top() < top + head.height())
        .then(|| Rect::from_x_y_ranges(head.x_range(), top..=head.top() + 1.0))
}

fn head_corners(rect: Rect, end: f32) -> egui::CornerRadius {
    let card_end = end + f32::from(kit::CARD_PAD);
    let round = (f32::from(kit::RADIUS) - (card_end - rect.bottom())).clamp(0.0, 255.0) as u8;
    egui::CornerRadius {
        nw: 0,
        ne: 0,
        sw: round,
        se: round,
    }
}

fn pinned_head(head: Rect, end: f32, top: f32) -> Option<Rect> {
    if head.top() >= top || end <= top {
        return None;
    }
    let y = top.min(end - head.height());
    Some(Rect::from_min_size(egui::pos2(head.left(), y), head.size()))
}

fn elsewhere_row(ui: &mut Ui, game: &Elsewhere) -> bool {
    let mut get = false;
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 12,
            right: kit::VALUE_EDGE as i8,
            top: 0,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let line = |size: f32| ui.fonts_mut(|f| f.row_height(&FontId::proportional(size)));
            let text = line(kit::LABEL_SIZE) + line(kit::DETAIL_SIZE) + 2.0;
            let pill = ui.spacing().interact_size.y.max(36.0);
            let height = text.max(pill) + 2.0 * ROW_PAD;
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), height),
                Layout::right_to_left(Align::Center),
                |ui| {
                    if game.fetched.is_none() {
                        get = ui
                            .add_enabled_ui(game.rom.is_some(), |ui| kit::pill(ui, GET, false))
                            .inner
                            .clicked();
                    }
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                        ui.add_space((height - text) / 2.0);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        let visuals = ui.visuals();
                        let name = RichText::new(title(game))
                            .size(kit::LABEL_SIZE)
                            .color(visuals.strong_text_color());
                        let place = RichText::new(facts(game))
                            .size(kit::DETAIL_SIZE)
                            .color(kit::value_color(visuals));
                        title_line(ui, name, why(game));
                        ui.add(egui::Label::new(place).truncate());
                    });
                },
            );
            if let Some((done, total)) = game.fetched {
                ui.add(ProgressBar::new(GETTING, Progress::bytes(done, total)));
                ui.add_space(6.0);
            }
        });
    get
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_devices_head_stays_under_the_top_until_the_last_device_scrolls_away() {
        let head = Rect::from_min_size(egui::pos2(0.0, 100.0), vec2(300.0, 30.0));
        let end = 400.0;
        assert_eq!(pinned_head(head, end, 60.0), None);
        let pinned = pinned_head(head, end, 200.0).expect("pinned");
        assert_eq!(pinned.top(), 200.0);
        assert_eq!(pinned.size(), head.size());
        let leaving = pinned_head(head, end, 385.0).expect("still pinned");
        assert_eq!(leaving.bottom(), end);
        assert_eq!(pinned_head(head, end, 400.0), None);
    }

    const NOTE: &str = "On that device, turn on \"Saves on this network\"";
    const ABOVE: &str = "Saves on this network";

    struct DevicesFrame {
        pinned: Option<Rect>,
        reach: Option<f32>,
        flow: Rect,
        top: f32,
        end: f32,
        shapes: Vec<egui::epaint::ClippedShape>,
    }

    fn devices_frame(ctx: &Context, offset: f32) -> DevicesFrame {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(412.0, 500.0))),
            ..Default::default()
        };
        let mut pinned = None;
        let mut reach = None;
        let mut flow = Rect::NOTHING;
        let mut top = 0.0;
        let mut end = 0.0;
        let shapes = ctx
            .run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .vertical_scroll_offset(offset)
                        .show(ui, |ui| {
                            ui.add_space(100.0);
                            kit::list(ui, |list| {
                                let mut saves = true;
                                list.switch(SwitchRow::new(ABOVE, &mut saves));
                                let head = list.custom(devices_head);
                                pinned_rows(list, head, |list| {
                                    for i in 0..12 {
                                        let mut on = i % 2 == 0;
                                        list.switch(
                                            SwitchRow::new(&format!("Device {i}"), &mut on)
                                                .value(Some("Not syncing"))
                                                .note(Some(NOTE)),
                                        );
                                    }
                                });
                                end = list.ui().min_rect().bottom();
                                top = list.ui().clip_rect().top();
                                pinned = pinned_head(head, end, top);
                                reach = pinned.map(|p| head_reach(p, list.edges(), end));
                                flow = head;
                            });
                            ui.add_space(1000.0);
                        });
                });
            })
            .shapes;
        DevicesFrame {
            pinned,
            reach,
            flow,
            top,
            end,
            shapes,
        }
    }

    fn seen_text(frame: &DevicesFrame, wanted: &str) -> bool {
        frame.shapes.iter().any(|clipped| match &clipped.shape {
            egui::Shape::Text(text) => {
                text.galley.text() == wanted
                    && clipped
                        .shape
                        .visual_bounding_rect()
                        .intersect(clipped.clip_rect)
                        .is_positive()
            }
            _ => false,
        })
    }

    fn lines(frame: &DevicesFrame) -> Vec<f32> {
        frame
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::LineSegment { points, .. }
                    if points[0].y == points[1].y
                        && clipped
                            .shape
                            .visual_bounding_rect()
                            .intersect(clipped.clip_rect)
                            .is_positive() =>
                {
                    Some(points[0].y)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_pinned_head_draws_one_line_and_leaves_with_its_last_row() {
        let ctx = Context::default();
        let fill = kit::card_fill(&ctx.style().visuals);
        let mut lined = 0;
        let mut unlined = 0;
        let mut pushed = 0;
        let mut covered = 0;
        let mut labelled = 0;
        let mut gone = 0;
        for step in 0..400 {
            let offset = 110.0 + step as f32 * 2.5;
            devices_frame(&ctx, offset);
            let frame = devices_frame(&ctx, offset);
            let head = seen_text(&frame, DEVICES);
            if seen_text(&frame, "Device 11") && frame.flow.top() < frame.top {
                assert!(head, "at {offset}: the last row shows without its head");
                labelled += 1;
            }
            if frame.end <= frame.top {
                assert!(
                    !head,
                    "at {offset}: the head shows after its last row left at {}",
                    frame.end
                );
                gone += 1;
            }
            let (Some(pinned), Some(reach)) = (frame.pinned, frame.reach) else {
                continue;
            };
            if pinned.top() < frame.top - 0.01 {
                assert!(
                    (pinned.bottom() - frame.end).abs() < 0.01,
                    "at {offset}: pushed head ends at {}, its last row at {}",
                    pinned.bottom(),
                    frame.end
                );
                pushed += 1;
            }
            let mut lines = lines(&frame);
            lines.sort_by(f32::total_cmp);
            if let Some(next) = lines.iter().find(|y| **y > reach + 0.5) {
                assert!(
                    *next >= reach + pinned.height() - 0.01,
                    "at {offset}: a {} pt strip shows under the head, lines {lines:?}, head {pinned:?}, reach {reach}",
                    next - reach
                );
            }
            let edged = lines.iter().any(|y| (y - reach).abs() < 0.5);
            if reach < frame.end - 0.5 {
                assert!(edged, "at {offset}: no line under the pinned head");
                lined += 1;
            } else {
                assert!(
                    !edged,
                    "at {offset}: a line under the pushed head over the card's foot"
                );
                unlined += 1;
            }
            if reach <= pinned.bottom() + 0.5 {
                continue;
            }
            let cover = frame
                .shapes
                .iter()
                .rposition(|clipped| match &clipped.shape {
                    egui::Shape::Rect(rect) => {
                        rect.fill == fill
                            && (rect.rect.top() - pinned.top()).abs() < 0.01
                            && (reach - 0.01..=reach + 0.51).contains(&rect.rect.bottom())
                    }
                    _ => false,
                })
                .unwrap_or_else(|| panic!("at {offset}: no cover down to {reach}"));
            let band = Rect::from_x_y_ranges(pinned.x_range(), pinned.bottom() + 0.5..=reach - 0.5);
            for (at, clipped) in frame.shapes.iter().enumerate() {
                let seen = clipped
                    .shape
                    .visual_bounding_rect()
                    .intersect(clipped.clip_rect)
                    .intersect(band);
                if at > cover && seen.is_positive() && seen.left() < pinned.right() - 10.0 {
                    panic!("at {offset}: {:?} draws over the cover", clipped.shape);
                }
            }
            covered += 1;
        }
        assert!(
            lined > 0 && unlined > 0 && pushed > 0 && covered > 0 && labelled > 0 && gone > 0,
            "lined {lined}, unlined {unlined}, pushed {pushed}, covered {covered}, labelled {labelled}, gone {gone}"
        );
    }

    #[test]
    fn the_row_above_the_head_shows_no_sliver_over_its_top() {
        let ctx = Context::default();
        let fill = kit::card_fill(&ctx.style().visuals);
        let mut covered = 0;
        let mut open = 0;
        for step in 0..160 {
            let offset = step as f32;
            devices_frame(&ctx, offset);
            let frame = devices_frame(&ctx, offset);
            let flow = frame.flow;
            let strip = flow.top() - frame.top;
            if strip < 1.0 {
                continue;
            }
            let band = Rect::from_x_y_ranges(flow.x_range(), frame.top..=flow.top() - 0.5);
            let cover = frame
                .shapes
                .iter()
                .rposition(|clipped| match &clipped.shape {
                    egui::Shape::Rect(rect) => {
                        rect.fill == fill
                            && rect.rect.top() <= frame.top + 0.01
                            && (flow.top() + 0.5..=flow.top() + 1.5).contains(&rect.rect.bottom())
                            && rect.rect.x_range() == flow.x_range()
                    }
                    _ => false,
                });
            if strip >= flow.height() {
                assert_eq!(cover, None, "at {offset}: a {strip} pt strip was covered");
                open += 1;
                continue;
            }
            let cover = cover.unwrap_or_else(|| {
                panic!("at {offset}: the row above shows a {strip} pt sliver over the head")
            });
            for (at, clipped) in frame.shapes.iter().enumerate() {
                let seen = clipped
                    .shape
                    .visual_bounding_rect()
                    .intersect(clipped.clip_rect)
                    .intersect(band);
                if at > cover && seen.is_positive() && seen.left() < flow.right() - 10.0 {
                    panic!("at {offset}: {:?} draws over the cover", clipped.shape);
                }
            }
            covered += 1;
        }
        assert!(covered > 0 && open > 0, "covered {covered}, open {open}");
    }

    #[test]
    fn a_head_at_the_cards_foot_takes_its_corners_and_drops_its_line() {
        let card = f32::from(kit::RADIUS);
        let pad = f32::from(kit::CARD_PAD);
        let rect = Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(300.0, 30.0));
        let pushed = head_corners(rect, 30.0);
        assert_eq!(f32::from(pushed.sw), card - pad);
        assert_eq!(f32::from(pushed.se), card - pad);
        assert_eq!((pushed.nw, pushed.ne), (0, 0));
        assert_eq!(head_corners(rect, 30.0 + card), egui::CornerRadius::ZERO);
        let head = Rect::from_min_size(egui::pos2(0.0, 110.0), vec2(300.0, 30.0));
        assert_eq!(cut_above(head, 120.0), None);
        assert_eq!(cut_above(head, 80.0), None);
        let cut = cut_above(head, 90.0).expect("a 20 pt strip is covered");
        assert_eq!((cut.top(), cut.bottom()), (90.0, 111.0));
        assert_eq!(head_reach(rect, &[10.0, 45.0, 100.0], 200.0), 45.0);
        assert_eq!(head_reach(rect, &[10.0, 70.0], 200.0), 30.0);
        assert_eq!(head_reach(rect, &[], 50.0), 50.0);
        assert_eq!(head_reach(rect, &[], 30.0), 30.0);
    }

    #[test]
    fn no_device_row_draws_above_the_pinned_heads_bottom_edge() {
        let ctx = Context::default();
        let mut cut = 0;
        let mut seen_pinned = false;
        for step in 0..200 {
            let offset = 110.0 + step as f32 * 5.0;
            devices_frame(&ctx, offset);
            let frame = devices_frame(&ctx, offset);
            let Some(pinned) = frame.pinned else {
                assert!(!seen_pinned || frame.end <= frame.top, "at {offset}");
                if seen_pinned {
                    break;
                }
                continue;
            };
            seen_pinned = true;
            let edge = pinned.bottom();
            for clipped in frame.shapes {
                let row = match &clipped.shape {
                    egui::Shape::Text(text) => {
                        let text = text.galley.text();
                        text.starts_with("Device ") || text == NOTE || text == "Not syncing"
                    }
                    egui::Shape::Circle(_) => true,
                    _ => false,
                };
                let drawn = clipped.shape.visual_bounding_rect();
                let seen = drawn.intersect(clipped.clip_rect);
                if !row || !seen.is_positive() {
                    continue;
                }
                assert!(
                    seen.top() >= edge - 0.01,
                    "at {offset}: {drawn:?} shows from {} above the head's edge {edge}",
                    seen.top()
                );
                if drawn.top() < edge {
                    cut += 1;
                }
            }
        }
        assert!(cut > 0, "no row was cut at the head's edge");
    }
}
