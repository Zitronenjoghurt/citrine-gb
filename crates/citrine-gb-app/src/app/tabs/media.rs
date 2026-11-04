use crate::app::tabs::TabViewer;
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::widgets;
use crate::app::widgets::action::more_button;
use crate::icons;
use crate::media::{Frame, MediaId, MediaItem, MediaKind, format_length, format_size, local_stamp};
use egui::{ColorImage, Context, RichText, TextureHandle, TextureOptions, Ui, vec2};
use std::collections::HashMap;

pub const PAGE_SIZE: usize = 20;
const ROW_THUMB: egui::Vec2 = egui::Vec2::new(80.0, 72.0);
const TILE_MIN: f32 = 104.0;
const TILE_GAP: f32 = 4.0;
const TILE_RADIUS: u8 = 8;
const HIGHLIGHT_SECS: f64 = 2.0;
const HIGHLIGHT_FADE_SECS: f64 = 0.6;
#[cfg(not(target_arch = "wasm32"))]
const REFRESH_EVERY: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Default)]
pub struct Browse {
    pub kind: Option<MediaKind>,
    pub game: Option<String>,
    pub page: usize,
    pub bar_hidden: bool,
}

#[derive(Default)]
pub struct MediaTabState {
    textures: HashMap<MediaId, TextureHandle>,
    pub browse: Browse,
    pub confirm_delete: Option<MediaId>,
    pub send: Option<MediaId>,
    highlight: Option<(MediaId, Option<f64>)>,
    #[cfg(not(target_arch = "wasm32"))]
    refreshed_at: Option<web_time::Instant>,
}

impl MediaTabState {
    pub fn highlight(&mut self, id: MediaId) {
        self.highlight = Some((id, None));
    }

    pub fn highlighted(&self) -> Option<&MediaId> {
        self.highlight.as_ref().map(|(id, _)| id)
    }

    fn bring_highlight_into_view(&mut self, items: &[MediaItem]) {
        let Some((id, None)) = &self.highlight else {
            return;
        };
        let Some(item) = items.iter().find(|item| &item.id == id) else {
            return;
        };
        let browse = &mut self.browse;
        if !matches(item, browse.kind, browse.game.as_deref()) {
            browse.kind = None;
            browse.game = None;
        }
        let position = items
            .iter()
            .filter(|item| matches(item, browse.kind, browse.game.as_deref()))
            .position(|item| &item.id == id)
            .unwrap_or(0);
        browse.page = position / PAGE_SIZE;
    }

    fn highlight_row(&mut self, id: &MediaId, now: f64) -> Option<(f32, bool)> {
        let (highlighted, since) = self.highlight.as_mut()?;
        if highlighted != id {
            return None;
        }
        let first = since.is_none();
        let since = *since.get_or_insert(now);
        match highlight_alpha(now - since) {
            Some(alpha) => Some((alpha, first)),
            None => {
                self.highlight = None;
                None
            }
        }
    }

    pub fn texture(&mut self, ctx: &Context, item: &MediaItem) -> Option<egui::TextureId> {
        let thumb = item.thumbnail.as_ref()?;
        let handle = self.textures.entry(item.id.clone()).or_insert_with(|| {
            ctx.load_texture(
                format!("media:{}", item.id.as_str()),
                color_image(thumb),
                TextureOptions::NEAREST,
            )
        });
        Some(handle.id())
    }

    fn forget_missing(&mut self, items: &[MediaItem]) {
        self.textures
            .retain(|id, _| items.iter().any(|item| &item.id == id));
    }
}

fn color_image(frame: &Frame) -> ColorImage {
    ColorImage::from_rgba_unmultiplied([frame.width as usize, frame.height as usize], &frame.rgba)
}

pub fn matches(item: &MediaItem, kind: Option<MediaKind>, game: Option<&str>) -> bool {
    kind.is_none_or(|kind| item.info.kind == kind) && game.is_none_or(|game| item.info.game == game)
}

pub fn games(items: &[MediaItem]) -> Vec<String> {
    let mut games: Vec<String> = items.iter().map(|item| item.info.game.clone()).collect();
    games.sort_by_key(|game| game.to_lowercase());
    games.dedup();
    games
}

pub fn highlight_alpha(elapsed: f64) -> Option<f32> {
    (0.0..HIGHLIGHT_SECS)
        .contains(&elapsed)
        .then(|| ((HIGHLIGHT_SECS - elapsed) / HIGHLIGHT_FADE_SECS).min(1.0) as f32)
}

pub fn page_count(len: usize) -> usize {
    len.div_ceil(PAGE_SIZE).max(1)
}

pub fn game_name(game: &str) -> &str {
    if game.is_empty() {
        "Untitled game"
    } else {
        game
    }
}

pub fn facts_line(item: &MediaItem) -> String {
    let mut facts = vec![local_stamp(item.info.created_at, "%-d %b %Y, %H:%M")];
    if let Some(length) = item.info.length_ms {
        facts.push(format_length(length));
    }
    facts.push(format_size(item.info.bytes));
    facts.join(" · ")
}

fn kind_icon(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Screenshot => icons::CAMERA,
        MediaKind::Print => icons::PRINTER,
        MediaKind::Recording => icons::VIDEO_CAMERA,
    }
}

pub fn empty_line(
    mobile: bool,
    screenshot_key: Option<String>,
    record_key: Option<String>,
) -> String {
    if mobile {
        return format!(
            "While playing, tap {} More, then {} Screenshot or {} Record video.",
            icons::DOTS_THREE_VERTICAL,
            icons::CAMERA,
            icons::RECORD
        );
    }
    let press = |key: Option<String>, icon: &str| match key {
        Some(key) => format!("{key} or {icon}"),
        None => icon.to_string(),
    };
    format!(
        "Press {} for a screenshot, {} to record.",
        press(screenshot_key, icons::CAMERA),
        press(record_key, icons::RECORD)
    )
}

enum Action {
    Pass(MediaId, crate::media::clipboard::Way),
    Export(MediaId),
    #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
    Reveal(MediaId),
    AskDelete(MediaId),
    Preview(MediaId),
    Send(MediaId),
}

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    refresh(viewer);
    let can_send = can_send(viewer);
    let items = viewer.media.items();
    let state = &mut viewer.ui.media;
    state.forget_missing(items);
    state.bring_highlight_into_view(items);

    if items.is_empty() {
        let key = |action| viewer.ui.settings.hotkeys.key_name(action);
        let mobile = viewer.ui.settings.layout.is_touch(ui.ctx());
        ui.label(empty_line(
            mobile,
            key(HotkeyAction::Screenshot),
            key(HotkeyAction::ToggleRecording),
        ));
        return;
    }

    let mut action = None;
    let mut browse = std::mem::take(&mut state.browse);
    let touch = viewer.ui.settings.layout.is_touch(ui.ctx());
    browse.bar_hidden = touch;
    browse.show(ui, "media", items, None, |ui, page| {
        if touch {
            action = grid(ui, state, &page);
            return;
        }
        for item in page {
            if let Some(chosen) = row(ui, state, item, can_send) {
                action = Some(chosen);
            }
            ui.separator();
        }
    });
    state.browse = browse;

    if let Some(action) = action {
        apply(viewer, action);
    }
    if !viewer.ui.viewer.is_open() {
        confirm_delete(viewer, ui.ctx());
        send_to(viewer, ui.ctx());
    }
}

fn can_send(viewer: &TabViewer) -> bool {
    viewer.ui.settings.online.is_joined() && !viewer.online.friends().is_empty()
}

fn send_to(viewer: &mut TabViewer, ctx: &Context) {
    let Some(id) = viewer.ui.media.send.clone() else {
        return;
    };
    let Some(answer) = crate::app::tabs::chat::pick_friend(ctx, viewer.online, "media_send") else {
        return;
    };
    viewer.ui.media.send = None;
    let Some(friend) = answer else {
        return;
    };
    let Some(item) = viewer.media.items().iter().find(|i| i.id == id).cloned() else {
        return;
    };
    let chat = &mut viewer.ui.friends.chat;
    chat.send_media(viewer.media, friend, &item);
    chat.open(friend);
    viewer.events.open_tab(crate::app::tabs::Tab::Friends);
}

fn refresh(viewer: &mut TabViewer) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let state = &mut viewer.ui.media;
        let due = state
            .refreshed_at
            .is_none_or(|at| at.elapsed() >= REFRESH_EVERY);
        if due {
            viewer.media.refresh();
            state.refreshed_at = Some(web_time::Instant::now());
        }
    }
    #[cfg(target_arch = "wasm32")]
    viewer.media.refresh();
}

impl Browse {
    pub fn show<'a>(
        &mut self,
        ui: &mut Ui,
        id: &str,
        items: &'a [MediaItem],
        max_height: Option<f32>,
        body: impl FnOnce(&mut Ui, Vec<&'a MediaItem>),
    ) {
        let games = games(items);
        if self.game.as_ref().is_some_and(|game| !games.contains(game)) {
            self.game = None;
        }
        if !self.bar_hidden {
            filter_bar(ui, self, id, &games);
        }

        let shown: Vec<&MediaItem> = items
            .iter()
            .filter(|item| matches(item, self.kind, self.game.as_deref()))
            .collect();
        let pages = page_count(shown.len());
        self.page = self.page.min(pages - 1);

        if !self.bar_hidden {
            ui.separator();
        }
        if shown.is_empty() {
            ui.small("Nothing matches this filter.");
        }

        let footer = if pages > 1 {
            ui.spacing().interact_size.y + ui.spacing().item_spacing.y * 2.0
        } else {
            0.0
        };
        let height = max_height.unwrap_or_else(|| ui.available_height());
        let page = shown
            .into_iter()
            .skip(self.page * PAGE_SIZE)
            .take(PAGE_SIZE)
            .collect();
        egui::ScrollArea::vertical()
            .id_salt((id, "list"))
            .max_height((height - footer).max(0.0))
            .auto_shrink([false, true])
            .show(ui, |ui| body(ui, page));

        if pages > 1 {
            pager(ui, &mut self.page, pages);
        }
    }
}

const FILTER_WIDTH: f32 = 200.0;

pub fn filter_button(ui: &mut Ui, browse: &Browse) -> bool {
    let mut text = RichText::new(icons::FUNNEL).size(20.0);
    if browse.kind.is_some() || browse.game.is_some() {
        text = text.color(crate::app::ui_state::ui_theme::brand(ui.visuals()));
    }
    let side = ui.spacing().interact_size.y;
    ui.add(
        egui::Button::new(text)
            .frame(false)
            .min_size(egui::Vec2::splat(side)),
    )
    .on_hover_text("Filter")
    .clicked()
}

pub fn kind_label(kind: Option<MediaKind>) -> &'static str {
    match kind {
        None => "All kinds",
        Some(MediaKind::Screenshot) => "Screenshots",
        Some(MediaKind::Recording) => "Recordings",
        Some(MediaKind::Print) => "Prints",
    }
}

pub fn day_label(day: jiff::civil::Date, today: jiff::civil::Date) -> String {
    if day == today {
        "Today".into()
    } else if today.yesterday().ok() == Some(day) {
        "Yesterday".into()
    } else if day.year() == today.year() {
        day.strftime("%-d %B").to_string()
    } else {
        day.strftime("%-d %B %Y").to_string()
    }
}

fn local_day(unix: u64) -> Option<jiff::civil::Date> {
    let timestamp = jiff::Timestamp::from_second(unix as i64).ok()?;
    Some(timestamp.to_zoned(jiff::tz::TimeZone::system()).date())
}

pub fn grid_columns(width: f32) -> usize {
    (((width + TILE_GAP) / (TILE_MIN + TILE_GAP)).floor() as usize).max(2)
}

fn grid(ui: &mut Ui, state: &mut MediaTabState, page: &[&MediaItem]) -> Option<Action> {
    let mut action = None;
    let today = jiff::Zoned::now().date();
    let columns = grid_columns(ui.available_width());
    let side = (ui.available_width() - TILE_GAP * (columns as f32 - 1.0)) / columns as f32;
    let size = vec2(side, (side * 0.9).round());
    ui.spacing_mut().item_spacing = vec2(TILE_GAP, 0.0);
    for day in page.chunk_by(|a, b| local_day(a.info.created_at) == local_day(b.info.created_at)) {
        let label = local_day(day[0].info.created_at)
            .map(|date| day_label(date, today))
            .unwrap_or_default();
        crate::app::mobile::kit::section_label(ui, &label);
        ui.add_space(4.0);
        for line in day.chunks(columns) {
            ui.horizontal(|ui| {
                for item in line {
                    if tile(ui, state, item, size) {
                        action = Some(Action::Preview(item.id.clone()));
                    }
                }
            });
            ui.add_space(TILE_GAP);
        }
    }
    ui.add_space(12.0);
    action
}

fn tile(ui: &mut Ui, state: &mut MediaTabState, item: &MediaItem, size: egui::Vec2) -> bool {
    let texture = state.texture(ui.ctx(), item);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let painter = ui.painter_at(rect);
    let radius = egui::CornerRadius::same(TILE_RADIUS);
    painter.rect_filled(rect, radius, ui.visuals().extreme_bg_color);
    match (texture, &item.thumbnail) {
        (Some(texture), Some(thumb)) => {
            let picture = vec2(thumb.width as f32, thumb.height as f32);
            let scale = (rect.width() / picture.x).min(rect.height() / picture.y);
            let shown = egui::Rect::from_center_size(rect.center(), picture * scale);
            egui::Image::new((texture, shown.size()))
                .corner_radius(radius)
                .paint_at(ui, shown);
        }
        _ => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                kind_icon(item.info.kind),
                egui::FontId::proportional(size.y * 0.3),
                ui.visuals().weak_text_color(),
            );
        }
    }
    if let Some(length) = item.info.length_ms {
        let text = format!("{} {}", icons::PLAY, format_length(length));
        let galley =
            painter.layout_no_wrap(text, egui::FontId::proportional(12.0), egui::Color32::WHITE);
        let pad = vec2(6.0, 3.0);
        let pill_size = galley.size() + pad * 2.0;
        let pill =
            egui::Rect::from_min_size(rect.right_bottom() - pill_size - vec2(6.0, 6.0), pill_size);
        painter.rect_filled(
            pill,
            pill_size.y / 2.0,
            egui::Color32::from_black_alpha(160),
        );
        painter.galley(pill.min + pad, galley, egui::Color32::WHITE);
    }
    let now = ui.input(|i| i.time);
    if let Some((alpha, first)) = state.highlight_row(&item.id, now) {
        let selection = ui.visuals().selection.bg_fill;
        painter.rect_stroke(
            rect.shrink(1.5),
            radius,
            egui::Stroke::new(3.0_f32, selection.gamma_multiply(alpha)),
            egui::StrokeKind::Inside,
        );
        if first {
            response.scroll_to_me(Some(egui::Align::Center));
        }
        ui.ctx().request_repaint();
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn filter_bar(ui: &mut Ui, state: &mut Browse, id: &str, games: &[String]) {
    let before = (state.kind, state.game.clone());
    ui.horizontal(|ui| {
        let width = ((ui.available_width() - ui.spacing().item_spacing.x) / 2.0).min(FILTER_WIDTH);
        let kind_text = kind_label(state.kind);
        egui::ComboBox::from_id_salt((id, "kind"))
            .selected_text(kind_text)
            .width(width)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.kind, None, "All kinds");
                ui.selectable_value(&mut state.kind, Some(MediaKind::Screenshot), "Screenshots");
                ui.selectable_value(&mut state.kind, Some(MediaKind::Recording), "Recordings");
                ui.selectable_value(&mut state.kind, Some(MediaKind::Print), "Prints");
            });
        let game_text = state
            .game
            .as_deref()
            .map_or("All games", game_name)
            .to_string();
        egui::ComboBox::from_id_salt((id, "game"))
            .selected_text(game_text)
            .truncate()
            .width(width)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.game, None, "All games");
                for game in games {
                    ui.selectable_value(&mut state.game, Some(game.clone()), game_name(game));
                }
            });
    });
    if before != (state.kind, state.game.clone()) {
        state.page = 0;
    }
}

pub fn pager(ui: &mut Ui, page: &mut usize, pages: usize) {
    ui.separator();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                *page > 0,
                egui::Button::new(icons::CARET_LEFT)
                    .min_size(egui::vec2(crate::app::mobile::touch_square(ui), 0.0)),
            )
            .on_hover_text("Previous page")
            .clicked()
        {
            *page -= 1;
        }
        ui.label(format!("Page {} of {pages}", *page + 1));
        if ui
            .add_enabled(
                *page + 1 < pages,
                egui::Button::new(icons::CARET_RIGHT)
                    .min_size(egui::vec2(crate::app::mobile::touch_square(ui), 0.0)),
            )
            .on_hover_text("Next page")
            .clicked()
        {
            *page += 1;
        }
    });
}

pub fn thumbnail(
    ui: &mut Ui,
    texture: Option<egui::TextureId>,
    item: &MediaItem,
    box_size: egui::Vec2,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(box_size, egui::Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, ui.visuals().extreme_bg_color);
    match (texture, &item.thumbnail) {
        (Some(texture), Some(thumb)) => {
            let size = vec2(thumb.width as f32, thumb.height as f32);
            let scale = (rect.width() / size.x).min(rect.height() / size.y);
            let image_rect = egui::Rect::from_center_size(rect.center(), size * scale);
            painter.image(
                texture,
                image_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        _ => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                kind_icon(item.info.kind),
                egui::FontId::proportional(box_size.y * 0.35),
                ui.visuals().weak_text_color(),
            );
        }
    }
    if item.info.kind == MediaKind::Recording && texture.is_some() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            icons::PLAY_CIRCLE,
            egui::FontId::proportional(box_size.y * 0.35),
            egui::Color32::from_white_alpha(220),
        );
    }
    response
}

fn button_width(ui: &Ui, text: &str) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, egui::Color32::PLACEHOLDER)
        .size()
        .x
        + ui.spacing().button_padding.x * 2.0
}

fn buttons_that_fit(widths: &[f32], room: f32, gap: f32) -> usize {
    let mut used = 0.0;
    widths
        .iter()
        .take_while(|width| {
            used += *width + gap;
            used <= room
        })
        .count()
}

fn row(ui: &mut Ui, state: &mut MediaTabState, item: &MediaItem, can_send: bool) -> Option<Action> {
    let mut action = None;
    let texture = state.texture(ui.ctx(), item);
    let background = ui.painter().add(egui::Shape::Noop);
    let response = ui.horizontal(|ui| {
        if thumbnail(ui, texture, item, ROW_THUMB)
            .on_hover_text("Open larger")
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            action = Some(Action::Preview(item.id.clone()));
        }
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(kind_icon(item.info.kind))
                    .on_hover_text(item.info.kind.label());
                ui.add(
                    egui::Label::new(egui::RichText::new(game_name(&item.info.game)).strong())
                        .truncate(),
                )
                .on_hover_text(item.id.file_name());
            });
            ui.add(egui::Label::new(egui::RichText::new(facts_line(item)).small()).truncate());
            let mut buttons = Vec::new();
            buttons.push((
                icons::DOWNLOAD_SIMPLE,
                "Export",
                "Export",
                Action::Export(item.id.clone()),
            ));
            if let Some(way) = crate::media::clipboard::way(item.info.kind) {
                buttons.push((
                    way.icon(),
                    way.label(),
                    way.hint(item.info.kind),
                    Action::Pass(item.id.clone(), way),
                ));
            }
            if can_send {
                buttons.push((
                    icons::PAPER_PLANE_TILT,
                    "Send",
                    "Send to a friend",
                    Action::Send(item.id.clone()),
                ));
            }
            #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
            buttons.push((
                icons::FOLDER_OPEN,
                crate::media::reveal::LABEL,
                crate::media::reveal::LABEL,
                Action::Reveal(item.id.clone()),
            ));
            let texts: Vec<String> = buttons
                .iter()
                .map(|(icon, label, _, _)| format!("{icon} {label}"))
                .collect();
            let side = crate::app::mobile::touch_square(ui);
            let gap = ui.spacing().item_spacing.x;
            let more_width = button_width(ui, icons::DOTS_THREE).max(side);
            let widths: Vec<f32> = texts.iter().map(|text| button_width(ui, text)).collect();
            let shown = buttons_that_fit(&widths, ui.available_width() - more_width, gap);
            let mut buttons = buttons.into_iter().zip(texts);
            ui.horizontal(|ui| {
                for ((_, label, hint, act), text) in buttons.by_ref().take(shown) {
                    let button = ui.button(text);
                    let button = if hint == label {
                        button
                    } else {
                        button.on_hover_text(hint)
                    };
                    if button.clicked() {
                        action = Some(act);
                    }
                }
                let more = more_button(side);
                egui::containers::menu::MenuButton::from_button(more)
                    .ui(ui, |ui| {
                        for ((icon, _, hint, act), _) in buttons {
                            if ui.button(format!("{icon} {hint}")).clicked() {
                                action = Some(act);
                            }
                        }
                        if ui.button(format!("{} Delete", icons::TRASH)).clicked() {
                            action = Some(Action::AskDelete(item.id.clone()));
                        }
                    })
                    .0
                    .on_hover_text("More");
            });
        });
    });
    let now = ui.input(|i| i.time);
    if let Some((alpha, first)) = state.highlight_row(&item.id, now) {
        let rect = response
            .response
            .rect
            .expand2(vec2(4.0, ui.spacing().item_spacing.y));
        let selection = ui.visuals().selection;
        ui.painter().set(
            background,
            egui::Shape::rect_filled(rect, 4.0, selection.bg_fill.gamma_multiply(0.5 * alpha)),
        );
        if first {
            response.response.scroll_to_me(Some(egui::Align::Center));
        }
        ui.ctx().request_repaint();
    }
    action
}

fn apply(viewer: &mut TabViewer, action: Action) {
    match action {
        Action::Pass(id, way) => {
            crate::media::clipboard::run(way, viewer.media, &id, viewer.files.copy_tx.clone())
        }
        Action::Export(id) => crate::media::export(viewer.media, &id, viewer.files.save_tx.clone()),
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        Action::Reveal(id) => {
            if let Err(err) = crate::media::reveal::reveal(&viewer.media.path(&id)) {
                viewer
                    .events
                    .notify_error(format!("Could not open the folder: {err}"));
            }
        }
        Action::AskDelete(id) => viewer.ui.media.confirm_delete = Some(id),
        Action::Preview(id) => viewer.ui.viewer.open(
            crate::app::viewer::Source::Media,
            crate::app::viewer::ItemKey::Media(id),
        ),
        Action::Send(id) => viewer.ui.media.send = Some(id),
    }
}

pub fn delete_question(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Screenshot => "Delete this screenshot? It can't be undone.",
        MediaKind::Recording => "Delete this recording? It can't be undone.",
        MediaKind::Print => "Delete this print? It can't be undone.",
    }
}

fn confirm_delete(viewer: &mut TabViewer, ctx: &Context) {
    let Some(id) = viewer.ui.media.confirm_delete.clone() else {
        return;
    };
    let Some(item) = viewer.media.items().iter().find(|i| i.id == id).cloned() else {
        viewer.ui.media.confirm_delete = None;
        return;
    };
    let choice = widgets::confirm::confirm_with_detail(
        ctx,
        "media_delete_confirm",
        delete_question(item.info.kind),
        Some(item.id.file_name()),
        "Delete",
    );
    let Some(confirmed) = choice else {
        return;
    };
    viewer.ui.media.confirm_delete = None;
    if !confirmed {
        return;
    }
    match viewer.media.delete(&id) {
        Ok(()) => viewer
            .events
            .notify(format!("Deleted {}", item.info.kind.label().to_lowercase())),
        Err(err) => viewer
            .events
            .notify_error(format!("Could not delete it: {err}")),
    }
}

pub fn detail_line(item: &MediaItem) -> String {
    let mut facts = vec![local_stamp(item.info.created_at, "%-d %b %Y, %H:%M")];
    if let Some(length) = item.info.length_ms {
        facts.push(format_length(length));
    }
    if item.info.width > 0 && item.info.height > 0 {
        facts.push(format!("{} × {} px", item.info.width, item.info.height));
    }
    if let Some(model) = item.info.model {
        facts.push(crate::app::ui_state::settings::console_name(model).to_string());
    }
    facts.push(format_size(item.info.bytes));
    facts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{MediaStore, NewMedia};
    use citrine_gb::gb::GbModel;

    fn item(kind: MediaKind, game: &str, created_at: u64) -> MediaItem {
        let new = NewMedia {
            kind,
            game: game.to_string(),
            rom_key: None,
            model: Some(GbModel::Cgb),
            created_at,
            width: 160,
            height: 144,
            length_ms: (kind == MediaKind::Recording).then_some(65_000),
            frame: Frame {
                width: 1,
                height: 1,
                rgba: vec![0; 4],
            },
        };
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("citrine-media-tab-{n}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut store = MediaStore::at(root.clone());
        let item = match kind {
            MediaKind::Screenshot | MediaKind::Print => store.add_screenshot(new, b"png").unwrap(),
            MediaKind::Recording => {
                let temp = store.temp_path().unwrap();
                std::fs::write(&temp, vec![0; 2048]).unwrap();
                store.add_recording(&temp, new).unwrap()
            }
        };
        let _ = std::fs::remove_dir_all(root);
        item
    }

    #[test]
    fn row_buttons_that_do_not_fit_go_into_the_more_menu() {
        assert_eq!(buttons_that_fit(&[80.0, 70.0, 90.0], 300.0, 4.0), 3);
        assert_eq!(buttons_that_fit(&[80.0, 70.0, 90.0], 160.0, 4.0), 2);
        assert_eq!(buttons_that_fit(&[80.0, 70.0, 90.0], 60.0, 4.0), 0);
    }

    #[test]
    fn the_filter_narrows_by_kind_and_game() {
        let shot = item(MediaKind::Screenshot, "TETRIS", 1);
        let clip = item(MediaKind::Recording, "ZELDA", 2);
        assert!(matches(&shot, None, None));
        assert!(matches(&shot, Some(MediaKind::Screenshot), Some("TETRIS")));
        assert!(!matches(&shot, Some(MediaKind::Recording), None));
        assert!(!matches(&clip, None, Some("TETRIS")));
        assert_eq!(games(&[clip, shot]), vec!["TETRIS", "ZELDA"]);
    }

    #[test]
    fn a_new_item_glows_for_two_seconds_and_clears_a_filter_that_hides_it() {
        assert_eq!(highlight_alpha(0.0), Some(1.0));
        assert_eq!(highlight_alpha(1.0), Some(1.0));
        assert!(highlight_alpha(1.8).is_some_and(|alpha| alpha < 0.5));
        assert_eq!(highlight_alpha(2.0), None);

        let clip = item(MediaKind::Recording, "ZELDA", 2);
        let shot = item(MediaKind::Screenshot, "TETRIS", 1);
        let mut state = MediaTabState {
            browse: Browse {
                kind: Some(MediaKind::Screenshot),
                game: None,
                page: 3,
                bar_hidden: false,
            },
            ..Default::default()
        };
        state.highlight(clip.id.clone());
        state.bring_highlight_into_view(&[clip.clone(), shot]);
        assert_eq!((state.browse.kind, state.browse.page), (None, 0));
        assert_eq!(state.highlight_row(&clip.id, 10.0), Some((1.0, true)));
        assert_eq!(state.highlight_row(&clip.id, 10.5), Some((1.0, false)));
        assert_eq!(state.highlight_row(&clip.id, 12.0), None);
        assert_eq!(state.highlighted(), None);
    }

    #[test]
    fn pages_hold_twenty() {
        assert_eq!(page_count(0), 1);
        assert_eq!(page_count(20), 1);
        assert_eq!(page_count(21), 2);
    }

    #[test]
    fn a_recording_row_shows_its_length_and_size() {
        let clip = item(MediaKind::Recording, "ZELDA", 2);
        let line = facts_line(&clip);
        assert!(line.contains(" · 1:05 · 2 KB"), "got {line}");
        let shot = item(MediaKind::Screenshot, "TETRIS", 1);
        assert!(facts_line(&shot).ends_with(" · 3 B"));
        assert!(detail_line(&clip).contains("160 × 144 px · Game Boy Color"));
    }

    #[test]
    fn the_empty_list_says_how_to_make_the_first_one() {
        let line = empty_line(
            false,
            Some("F12".to_string()),
            Some("Shift+F12".to_string()),
        );
        assert!(line.starts_with("Press F12 or "), "got {line}");
        assert!(
            line.contains(" for a screenshot, Shift+F12 or "),
            "got {line}"
        );
        assert!(line.ends_with(" to record."));
        let phone = empty_line(true, Some("F12".to_string()), Some("Shift+F12".to_string()));
        assert!(phone.contains(" Screenshot or "), "got {phone}");
        assert!(phone.contains(" More, then "), "got {phone}");
        assert!(phone.ends_with(" Record video."), "got {phone}");
        assert!(!phone.contains("F12"));
    }

    #[test]
    fn days_read_today_yesterday_then_the_date() {
        let today = jiff::civil::date(2026, 11, 2);
        assert_eq!(day_label(today, today), "Today");
        assert_eq!(
            day_label(jiff::civil::date(2026, 11, 1), today),
            "Yesterday"
        );
        assert_eq!(
            day_label(jiff::civil::date(2026, 10, 28), today),
            "28 October"
        );
        assert_eq!(
            day_label(jiff::civil::date(2025, 12, 31), today),
            "31 December 2025"
        );
    }

    #[test]
    fn the_grid_keeps_three_tiles_on_a_phone() {
        assert_eq!(grid_columns(332.0), 3);
        assert_eq!(grid_columns(384.0), 3);
        assert_eq!(grid_columns(560.0), 5);
        assert_eq!(grid_columns(150.0), 2);
    }

    #[test]
    fn an_untitled_game_still_has_a_name() {
        assert_eq!(game_name(""), "Untitled game");
        assert_eq!(game_name("TETRIS"), "TETRIS");
    }

    #[test]
    fn the_delete_question_names_the_kind() {
        assert!(delete_question(MediaKind::Recording).starts_with("Delete this recording?"));
    }
}
