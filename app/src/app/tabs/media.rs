use crate::app::tabs::TabViewer;
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::widgets;
use crate::icons;
use crate::media::{Frame, MediaId, MediaItem, MediaKind, format_length, format_size, local_stamp};
use egui::{ColorImage, Context, TextureHandle, TextureOptions, Ui, vec2};
use std::collections::HashMap;

pub const PAGE_SIZE: usize = 20;
const ROW_THUMB: egui::Vec2 = egui::Vec2::new(80.0, 72.0);
const HIGHLIGHT_SECS: f64 = 2.0;
const HIGHLIGHT_FADE_SECS: f64 = 0.6;
#[cfg(not(target_arch = "wasm32"))]
const REFRESH_EVERY: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Default)]
pub struct MediaTabState {
    textures: HashMap<MediaId, TextureHandle>,
    pub kind: Option<MediaKind>,
    pub game: Option<String>,
    pub page: usize,
    pub confirm_delete: Option<MediaId>,
    pub preview: Option<MediaId>,
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
        if !matches(item, self.kind, self.game.as_deref()) {
            self.kind = None;
            self.game = None;
        }
        let position = items
            .iter()
            .filter(|item| matches(item, self.kind, self.game.as_deref()))
            .position(|item| &item.id == id)
            .unwrap_or(0);
        self.page = position / PAGE_SIZE;
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

    fn texture(&mut self, ctx: &Context, item: &MediaItem) -> Option<egui::TextureId> {
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
            "Tap {} Screenshot or {} Record video in the menu.",
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
    Export(MediaId),
    #[cfg(not(target_arch = "wasm32"))]
    Reveal(MediaId),
    AskDelete(MediaId),
    Preview(MediaId),
}

pub fn show(viewer: &mut TabViewer, ui: &mut Ui) {
    refresh(viewer);
    let items = viewer.media.items();
    let state = &mut viewer.ui.media;
    state.forget_missing(items);
    state.bring_highlight_into_view(items);

    if items.is_empty() {
        let key = |action| viewer.ui.settings.hotkeys.key_name(action);
        let mobile = crate::app::mobile_layout(viewer.ui.settings.layout, ui.ctx());
        ui.label(empty_line(
            mobile,
            key(HotkeyAction::Screenshot),
            key(HotkeyAction::ToggleRecording),
        ));
        return;
    }

    let games = games(items);
    if state
        .game
        .as_ref()
        .is_some_and(|game| !games.contains(game))
    {
        state.game = None;
    }
    filter_bar(ui, state, &games);

    let shown: Vec<&MediaItem> = items
        .iter()
        .filter(|item| matches(item, state.kind, state.game.as_deref()))
        .collect();
    let pages = page_count(shown.len());
    state.page = state.page.min(pages - 1);

    ui.separator();
    let mut action = None;
    if shown.is_empty() {
        ui.small("Nothing matches this filter.");
    }

    let footer = if pages > 1 {
        ui.spacing().interact_size.y + ui.spacing().item_spacing.y * 2.0
    } else {
        0.0
    };
    egui::ScrollArea::vertical()
        .max_height((ui.available_height() - footer).max(0.0))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            let start = state.page * PAGE_SIZE;
            for item in shown.iter().skip(start).take(PAGE_SIZE) {
                if let Some(chosen) = row(ui, state, item) {
                    action = Some(chosen);
                }
                ui.separator();
            }
        });

    if pages > 1 {
        pager(ui, &mut state.page, pages);
    }

    if let Some(action) = action {
        apply(viewer, action);
    }
    confirm_delete(viewer, ui.ctx());
    preview(viewer, ui.ctx());
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

fn filter_bar(ui: &mut Ui, state: &mut MediaTabState, games: &[String]) {
    let before = (state.kind, state.game.clone());
    ui.horizontal(|ui| {
        let kind_text = match state.kind {
            None => "All kinds",
            Some(MediaKind::Screenshot) => "Screenshots",
            Some(MediaKind::Recording) => "Recordings",
        };
        egui::ComboBox::from_id_salt("media_kind")
            .selected_text(kind_text)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.kind, None, "All kinds");
                ui.selectable_value(&mut state.kind, Some(MediaKind::Screenshot), "Screenshots");
                ui.selectable_value(&mut state.kind, Some(MediaKind::Recording), "Recordings");
            });
        let game_text = state
            .game
            .as_deref()
            .map_or("All games", game_name)
            .to_string();
        egui::ComboBox::from_id_salt("media_game")
            .selected_text(game_text)
            .truncate()
            .width(ui.available_width())
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

fn pager(ui: &mut Ui, page: &mut usize, pages: usize) {
    ui.separator();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(*page > 0, egui::Button::new(icons::CARET_LEFT))
            .on_hover_text("Previous page")
            .clicked()
        {
            *page -= 1;
        }
        ui.label(format!("Page {} of {pages}", *page + 1));
        if ui
            .add_enabled(*page + 1 < pages, egui::Button::new(icons::CARET_RIGHT))
            .on_hover_text("Next page")
            .clicked()
        {
            *page += 1;
        }
    });
}

fn thumbnail(
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

fn row(ui: &mut Ui, state: &mut MediaTabState, item: &MediaItem) -> Option<Action> {
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
            ui.horizontal(|ui| {
                if ui
                    .button(format!("{} Export", icons::DOWNLOAD_SIMPLE))
                    .clicked()
                {
                    action = Some(Action::Export(item.id.clone()));
                }
                #[cfg(not(target_arch = "wasm32"))]
                if ui
                    .button(format!(
                        "{} {}",
                        icons::FOLDER_OPEN,
                        crate::media::reveal::LABEL
                    ))
                    .clicked()
                {
                    action = Some(Action::Reveal(item.id.clone()));
                }
                if ui.button(icons::TRASH).on_hover_text("Delete").clicked() {
                    action = Some(Action::AskDelete(item.id.clone()));
                }
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
        Action::Export(id) => crate::media::export(viewer.media, &id, viewer.files.save_tx.clone()),
        #[cfg(not(target_arch = "wasm32"))]
        Action::Reveal(id) => {
            if let Err(err) = crate::media::reveal::reveal(&viewer.media.path(&id)) {
                viewer
                    .events
                    .notify_error(format!("Could not open the folder: {err}"));
            }
        }
        Action::AskDelete(id) => viewer.ui.media.confirm_delete = Some(id),
        Action::Preview(id) => viewer.ui.media.preview = Some(id),
    }
}

pub fn delete_question(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Screenshot => "Delete this screenshot? It can't be undone.",
        MediaKind::Recording => "Delete this recording? It can't be undone.",
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
    if viewer.ui.media.preview.as_ref() == Some(&id) {
        viewer.ui.media.preview = None;
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

fn preview(viewer: &mut TabViewer, ctx: &Context) {
    if viewer.ui.media.confirm_delete.is_some() {
        return;
    }
    let Some(id) = viewer.ui.media.preview.clone() else {
        return;
    };
    let Some(item) = viewer.media.items().iter().find(|i| i.id == id).cloned() else {
        viewer.ui.media.preview = None;
        return;
    };
    let mut action = None;
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("media_preview")).show(ctx, |ui| {
        let screen = ctx.content_rect();
        let width = (screen.width() - 64.0)
            .min((screen.height() - 200.0) / 0.9)
            .clamp(160.0, 480.0);
        ui.set_width(width);
        let texture = viewer.ui.media.texture(ctx, &item);
        thumbnail(ui, texture, &item, vec2(width, width * 0.9));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(kind_icon(item.info.kind));
            ui.add(
                egui::Label::new(egui::RichText::new(game_name(&item.info.game)).strong())
                    .truncate(),
            );
        });
        ui.add(egui::Label::new(egui::RichText::new(detail_line(&item)).small()).truncate());
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::Button::new(format!("{} Export", icons::DOWNLOAD_SIMPLE))
                        .min_size(vec2(80.0, 32.0)),
                )
                .clicked()
            {
                action = Some(Action::Export(id.clone()));
            }
            #[cfg(not(target_arch = "wasm32"))]
            if ui
                .add(
                    egui::Button::new(format!(
                        "{} {}",
                        icons::FOLDER_OPEN,
                        crate::media::reveal::LABEL
                    ))
                    .min_size(vec2(80.0, 32.0)),
                )
                .clicked()
            {
                action = Some(Action::Reveal(id.clone()));
            }
            if ui
                .add(egui::Button::new("Close").min_size(vec2(80.0, 32.0)))
                .clicked()
            {
                close = true;
            }
        });
    });
    if modal.should_close() || close {
        viewer.ui.media.preview = None;
    }
    if let Some(action) = action {
        apply(viewer, action);
    }
}

pub fn detail_line(item: &MediaItem) -> String {
    let mut facts = vec![local_stamp(item.info.created_at, "%-d %b %Y, %H:%M")];
    if let Some(length) = item.info.length_ms {
        facts.push(format_length(length));
    }
    if item.info.width > 0 && item.info.height > 0 {
        facts.push(format!("{} × {}", item.info.width, item.info.height));
    }
    if let Some(model) = item.info.model {
        facts.push(
            match model {
                citrine_gb::gb::GbModel::Dmg => "Game Boy",
                citrine_gb::gb::GbModel::Cgb => "Game Boy Color",
            }
            .to_string(),
        );
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
            model: GbModel::Cgb,
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
            MediaKind::Screenshot => store.add_screenshot(new, b"png").unwrap(),
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
            kind: Some(MediaKind::Screenshot),
            page: 3,
            ..Default::default()
        };
        state.highlight(clip.id.clone());
        state.bring_highlight_into_view(&[clip.clone(), shot]);
        assert_eq!((state.kind, state.page), (None, 0));
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
        assert!(detail_line(&clip).contains("160 × 144 · Game Boy Color"));
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
        assert!(phone.ends_with(" Record video in the menu."), "got {phone}");
        assert!(!phone.contains("F12"));
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
