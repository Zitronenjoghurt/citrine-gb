use crate::app::big_screen::Cover;
use crate::app::ui_state::settings::kind_icon_or_cart;
use crate::icons;
use egui::{
    Align2, Color32, CornerRadius, FontId, Rect, Response, Sense, Stroke, TextStyle, TextWrapMode,
    Ui, Widget, WidgetText, pos2, vec2,
};

const PICTURE_SHAPE: f32 = 0.9;
const COVER_MARGIN: f32 = 5.0;
const NAME_GAP: f32 = 4.0;
pub const COUCH_NAME: f32 = 16.0;
pub const COUCH_NAME_GAP: f32 = crate::app::big_screen::ring::MARGIN;
const EMPTY_ICON: f32 = 24.0;
const COUCH_RADIUS: u8 = 12;
const TAG_INSET: f32 = 4.0;
const TAG_PAD: f32 = 6.0;
const TAG_HEIGHT: f32 = 18.0;
const GONE_FADE: f32 = 0.4;
const BAR_HEIGHT: f32 = 2.0;
const CLOUD_BADGE: f32 = 24.0;
const DOWNLOAD_BAR: f32 = 5.0;
const DOWNLOAD_INSET: f32 = 6.0;
const PROGRESS_GAP: f32 = 2.0;
pub const ARCHIVE_OFFLINE: &str = "Connect to download from the Game Archive";
const NAME_ROWS: usize = 2;

mod name_cut;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
    Missing,
    NotConnected,
    NoAccess,
    ArchiveOffline,
}

impl Tag {
    pub fn label(self) -> &'static str {
        match self {
            Tag::Missing => "File missing",
            Tag::NotConnected => "Not connected",
            Tag::ArchiveOffline => "Offline",
            Tag::NoAccess => "No access",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Tag::Missing => icons::FILE_X,
            Tag::NotConnected => icons::HARD_DRIVE,
            Tag::NoAccess => icons::LOCK_SIMPLE,
            Tag::ArchiveOffline => icons::CLOUD_SLASH,
        }
    }

    pub fn hint(self, path: &str) -> String {
        let state = match self {
            Tag::Missing => "File missing.",
            Tag::NotConnected => "Drive not connected.",
            Tag::NoAccess => "No access to its folder.",
            Tag::ArchiveOffline => return ARCHIVE_OFFLINE.to_owned(),
        };
        format!("{state} Last seen at {path}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Archive {
    Remote { bytes: u64 },
    Downloading { done: u64, total: u64 },
    Downloaded,
}

pub fn archive_badge(tag: Option<Tag>, archive: Option<Archive>) -> Option<&'static str> {
    match (tag, archive) {
        (Some(Tag::ArchiveOffline), _) => Some(icons::CLOUD_SLASH),
        (Some(_), _) | (None, None) => None,
        (None, Some(Archive::Remote { .. } | Archive::Downloading { .. })) => {
            Some(icons::CLOUD_ARROW_DOWN)
        }
        (None, Some(Archive::Downloaded)) => Some(icons::CLOUD_CHECK),
    }
}

impl Archive {
    pub fn hint(self) -> String {
        match self {
            Archive::Remote { bytes } => format!(
                "In the Game Archive. Downloads {} when you play it",
                crate::progress::size_words(bytes)
            ),
            Archive::Downloading { .. } => "Downloading from the Game Archive".to_owned(),
            Archive::Downloaded => "Downloaded from the Game Archive".to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub earned: u32,
    pub total: u32,
}

pub struct GameTile<'a> {
    name: &'a str,
    width: f32,
    cover: Option<&'a Cover>,
    console: Option<citrine_gb::console::ConsoleKind>,
    tag: Option<(Tag, String)>,
    progress: Option<Progress>,
    neighbours: &'a [&'a str],
    name_lines: usize,
    archive: Option<Archive>,
    selected: bool,
    couch: bool,
}

impl<'a> GameTile<'a> {
    pub fn new(name: &'a str, width: f32) -> Self {
        Self {
            name,
            width,
            cover: None,
            console: None,
            tag: None,
            progress: None,
            neighbours: &[],
            name_lines: NAME_ROWS,
            archive: None,
            selected: false,
            couch: false,
        }
    }

    pub fn couch(mut self, couch: bool) -> Self {
        self.couch = couch;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn cover(mut self, cover: Option<&'a Cover>) -> Self {
        self.cover = cover;
        self
    }

    pub fn console(mut self, console: Option<citrine_gb::console::ConsoleKind>) -> Self {
        self.console = console;
        self
    }

    pub fn tag(mut self, tag: Option<Tag>, path: &str) -> Self {
        self.tag = tag.map(|tag| (tag, tag.hint(path)));
        self
    }

    pub fn neighbours(mut self, neighbours: &'a [&'a str]) -> Self {
        self.neighbours = neighbours;
        self
    }

    pub fn name_lines(mut self, lines: usize) -> Self {
        self.name_lines = lines;
        self
    }

    pub fn archive(mut self, archive: Option<Archive>) -> Self {
        self.archive = archive;
        self
    }

    pub fn progress(mut self, progress: Option<Progress>) -> Self {
        self.progress = progress.filter(|progress| progress.total > 0);
        self
    }
}

impl Widget for GameTile<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let width = self.width;
        let picture = width * PICTURE_SHAPE;
        let (name, name_slot, elided) = name_galley(
            ui,
            self.name,
            self.neighbours,
            self.name_lines,
            width,
            self.couch,
        );
        let name_gap = if self.couch { COUCH_NAME_GAP } else { NAME_GAP };
        let download = match self.archive {
            Some(Archive::Downloading { done, total }) if total > 0 => {
                Some((done as f64 / total as f64) as f32)
            }
            Some(Archive::Downloading { .. }) => Some(0.0),
            _ => None,
        };
        let progress = self.progress.map(|progress| {
            let words = format!(
                "{} {} of {}",
                icons::TROPHY,
                progress.earned,
                progress.total
            );
            let share = progress.earned as f32 / progress.total as f32;
            (share, text(ui, &words, width, TextStyle::Small))
        });
        let progress_height = progress
            .as_ref()
            .map_or(0.0, |(_, galley)| PROGRESS_GAP + galley.size().y);
        let size = vec2(width, picture + name_gap + name_slot + progress_height);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, self.name));
        let frame = Rect::from_min_size(rect.min, vec2(width, picture));
        let badge = cloud_badge(frame);
        let on_badge = response
            .hover_pos()
            .is_some_and(|spot| badge.expand(2.0).contains(spot));
        let hint = match (&self.tag, self.archive) {
            (Some((_, hint)), _) => Some((hint.clone(), rect)),
            (None, Some(archive @ (Archive::Remote { .. } | Archive::Downloaded))) if on_badge => {
                Some((archive.hint(), badge))
            }
            (None, Some(archive @ Archive::Downloading { .. })) => Some((archive.hint(), rect)),
            _ if elided => Some((self.name.to_owned(), rect)),
            _ => None,
        };
        if let Some((text, target)) = hint {
            crate::app::widgets::info_mark::hover_hint_at(&response, &text, target);
        }
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let gone = self.tag.is_some();
        let visuals = ui.visuals();
        let (radius, ground, rest) = if self.couch {
            (
                CornerRadius::same(COUCH_RADIUS),
                crate::app::mobile::kit::card_fill(visuals),
                Stroke::NONE,
            )
        } else {
            (
                visuals.widgets.noninteractive.corner_radius,
                visuals.faint_bg_color,
                visuals.widgets.noninteractive.bg_stroke,
            )
        };
        crate::app::big_screen::ring::mark(
            &response,
            crate::app::big_screen::ring::Mark::Ring {
                rect: frame,
                radius: radius.average(),
            },
        );
        let painter = ui.painter();
        painter.rect_filled(frame, radius, ground);
        match self.cover {
            Some(cover) => {
                let inner = if cover.boxed {
                    fit(frame.shrink(COVER_MARGIN), cover.size)
                } else {
                    frame
                };
                let tint = if gone {
                    Color32::WHITE.gamma_multiply(GONE_FADE)
                } else {
                    Color32::WHITE
                };
                egui::Image::new((cover.id(), cover.size))
                    .uv(cover.uv)
                    .tint(tint)
                    .corner_radius(radius)
                    .paint_at(ui, inner);
            }
            None => {
                painter.text(
                    frame.center(),
                    Align2::CENTER_CENTER,
                    kind_icon_or_cart(self.console),
                    FontId::proportional(EMPTY_ICON),
                    visuals.weak_text_color(),
                );
            }
        }
        let stroke = if self.selected {
            Stroke::new(2.0_f32, visuals.selection.bg_fill)
        } else if response.hovered() {
            Stroke::new(2.0_f32, visuals.widgets.hovered.bg_stroke.color)
        } else {
            rest
        };
        painter.rect_stroke(frame, radius, stroke, egui::StrokeKind::Outside);
        let tag = self.tag.as_ref().map(|(tag, _)| *tag);
        match (tag, archive_badge(tag, self.archive)) {
            (_, Some(icon)) => paint_cloud(ui, frame, icon),
            (Some(tag), None) => paint_tag(ui, frame, tag),
            (None, None) => {}
        }
        if let Some(share) = download {
            paint_download(ui, frame, share);
        }
        if let Some((share, _)) = &progress {
            let share = share.clamp(0.0, 1.0);
            let bar = Rect::from_min_size(
                pos2(frame.left(), frame.bottom() - BAR_HEIGHT),
                vec2(frame.width() * share, BAR_HEIGHT),
            );
            ui.painter()
                .rect_filled(bar, 0.0, visuals.selection.bg_fill);
        }
        let color = if gone {
            visuals.weak_text_color()
        } else if response.hovered() || self.couch {
            visuals.strong_text_color()
        } else {
            visuals.text_color()
        };
        let name_top = frame.bottom() + name_gap;
        let name_height = name_slot;
        ui.painter()
            .galley(pos2(rect.left(), name_top), name, color);
        if let Some((_, galley)) = progress {
            ui.painter().galley(
                pos2(rect.left(), name_top + name_height + PROGRESS_GAP),
                galley,
                visuals.weak_text_color(),
            );
        }
        response
    }
}

fn paint_tag(ui: &Ui, frame: Rect, tag: Tag) {
    let visuals = ui.visuals();
    let (fill, ink) = match tag {
        Tag::Missing => (
            visuals.warn_fg_color.gamma_multiply(0.22),
            visuals.strong_text_color(),
        ),
        Tag::NotConnected | Tag::NoAccess | Tag::ArchiveOffline => {
            (visuals.widgets.inactive.bg_fill, visuals.text_color())
        }
    };
    let words = format!("{} {}", tag.icon(), tag.label());
    let galley = WidgetText::from(words).into_galley(
        ui,
        Some(TextWrapMode::Truncate),
        frame.width() - 2.0 * (TAG_INSET + TAG_PAD),
        TextStyle::Small,
    );
    let pill = Rect::from_min_size(
        frame.min + vec2(TAG_INSET, TAG_INSET),
        vec2(galley.size().x + 2.0 * TAG_PAD, TAG_HEIGHT),
    );
    let painter = ui.painter();
    painter.rect_filled(pill, TAG_HEIGHT / 2.0, visuals.panel_fill);
    painter.rect_filled(pill, TAG_HEIGHT / 2.0, fill);
    let top = pill.center().y - galley.size().y / 2.0;
    painter.galley(pos2(pill.left() + TAG_PAD, top), galley, ink);
}

fn name_font(ui: &Ui, couch: bool) -> FontId {
    if couch {
        FontId::proportional(COUCH_NAME)
    } else {
        TextStyle::Body.resolve(ui.style())
    }
}

pub fn name_lines(ui: &Ui, names: &[&str], width: f32, couch: bool) -> usize {
    let font = name_font(ui, couch);
    let fits = names.iter().all(|name| {
        let galley = ui.fonts_mut(|fonts| {
            fonts.layout_no_wrap((*name).to_owned(), font.clone(), Color32::PLACEHOLDER)
        });
        galley.size().x <= width
    });
    if fits { 1 } else { NAME_ROWS }
}

fn name_galley(
    ui: &Ui,
    name: &str,
    neighbours: &[&str],
    lines: usize,
    width: f32,
    couch: bool,
) -> (std::sync::Arc<egui::Galley>, f32, bool) {
    let font = name_font(ui, couch);
    let row = ui.fonts_mut(|fonts| fonts.row_height(&font));
    let measure = |words: &str| {
        ui.fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(words.to_owned(), font.clone(), Color32::PLACEHOLDER)
                .size()
                .x
        })
    };
    let key = egui::Id::new(("tile_name", name, neighbours, width.to_bits(), couch));
    let shown = match ui.data(|data| data.get_temp::<String>(key)) {
        Some(shown) => shown,
        None => {
            let shown = name_cut::cut(name, neighbours, width, measure);
            ui.data_mut(|data| data.insert_temp(key, shown.clone()));
            shown
        }
    };
    let galley = ui
        .fonts_mut(|fonts| fonts.layout_no_wrap(shown.clone(), font.clone(), Color32::PLACEHOLDER));
    let slot = galley.size().y.max(row * lines as f32);
    (galley, slot, shown.replace('\n', " ") != name)
}

fn paint_cloud(ui: &Ui, frame: Rect, icon: &str) {
    let visuals = ui.visuals();
    let galley = WidgetText::from(icon).into_galley(
        ui,
        Some(TextWrapMode::Extend),
        f32::INFINITY,
        TextStyle::Body,
    );
    let badge = cloud_badge(frame);
    let painter = ui.painter();
    painter.rect_filled(badge, CLOUD_BADGE / 2.0, visuals.panel_fill);
    painter.rect_stroke(
        badge,
        CLOUD_BADGE / 2.0,
        visuals.widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    painter.galley(
        badge.center() - galley.size() / 2.0,
        galley,
        visuals.text_color(),
    );
}

fn paint_download(ui: &Ui, frame: Rect, share: f32) {
    let visuals = ui.visuals();
    let track = Rect::from_min_max(
        pos2(
            frame.left() + DOWNLOAD_INSET,
            frame.bottom() - DOWNLOAD_INSET - DOWNLOAD_BAR,
        ),
        pos2(
            frame.right() - DOWNLOAD_INSET,
            frame.bottom() - DOWNLOAD_INSET,
        ),
    );
    let radius = DOWNLOAD_BAR / 2.0;
    let painter = ui.painter();
    painter.rect_filled(track, radius, visuals.panel_fill);
    painter.rect_stroke(
        track,
        radius,
        visuals.widgets.noninteractive.bg_stroke,
        egui::StrokeKind::Outside,
    );
    let filled = (track.width() * share.clamp(0.0, 1.0)).max(DOWNLOAD_BAR);
    painter.rect_filled(
        Rect::from_min_size(track.min, vec2(filled, DOWNLOAD_BAR)),
        radius,
        crate::app::ui_state::ui_theme::brand(visuals),
    );
}

fn cloud_badge(frame: Rect) -> Rect {
    Rect::from_min_size(
        frame.min + vec2(TAG_INSET, TAG_INSET),
        vec2(CLOUD_BADGE, CLOUD_BADGE),
    )
}

fn text(ui: &Ui, words: &str, width: f32, style: TextStyle) -> std::sync::Arc<egui::Galley> {
    WidgetText::from(words).into_galley(ui, Some(TextWrapMode::Truncate), width, style)
}

fn fit(room: Rect, size: egui::Vec2) -> Rect {
    if size.x <= 0.0 || size.y <= 0.0 {
        return room;
    }
    let scale = (room.width() / size.x).min(room.height() / size.y);
    Rect::from_center_size(room.center(), size * scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui_state::settings::kind_name;
    use citrine_gb::console::ConsoleKind;

    fn painted(console: Option<ConsoleKind>) -> Vec<String> {
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add(GameTile::new("Tobu Tobu Girl", 160.0).console(console));
            });
        });
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_tile_without_a_cover_shows_its_console_icon_alone() {
        for kind in [
            ConsoleKind::Gb,
            ConsoleKind::Gbc,
            ConsoleKind::Gba,
            ConsoleKind::Nds,
        ] {
            let texts = painted(Some(kind));
            assert!(texts.iter().any(|t| t == kind_icon_or_cart(Some(kind))));
            assert!(!texts.iter().any(|t| t == icons::CARTRIDGE), "{kind:?}");
            assert!(!texts.iter().any(|t| t == kind_name(kind)), "{kind:?}");
        }
        assert!(painted(None).iter().any(|t| t == icons::CARTRIDGE));
    }
}
