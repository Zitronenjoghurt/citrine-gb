use crate::app::big_screen::Cover;
use crate::icons;
use egui::{
    Align2, Color32, FontId, Rect, Response, Sense, Stroke, TextStyle, TextWrapMode, Ui, Widget,
    WidgetText, pos2, vec2,
};

const PICTURE_SHAPE: f32 = 0.9;
const COVER_MARGIN: f32 = 5.0;
const NAME_GAP: f32 = 4.0;
const EMPTY_ICON: f32 = 24.0;
const CONSOLE_LIFT: f32 = 10.0;
const CONSOLE_GAP: f32 = 4.0;
const TAG_INSET: f32 = 4.0;
const TAG_PAD: f32 = 6.0;
const TAG_HEIGHT: f32 = 18.0;
const GONE_FADE: f32 = 0.4;
const BAR_HEIGHT: f32 = 2.0;
const PROGRESS_GAP: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
    Missing,
    NotConnected,
    NoAccess,
}

impl Tag {
    pub fn label(self) -> &'static str {
        match self {
            Tag::Missing => "File missing",
            Tag::NotConnected => "Not connected",
            Tag::NoAccess => "No access",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            Tag::Missing => icons::FILE_X,
            Tag::NotConnected => icons::HARD_DRIVE,
            Tag::NoAccess => icons::LOCK_SIMPLE,
        }
    }

    pub fn hint(self, path: &str) -> String {
        let state = match self {
            Tag::Missing => "File missing.",
            Tag::NotConnected => "Drive not connected.",
            Tag::NoAccess => "No access to its folder.",
        };
        format!("{state} Last seen at {path}")
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
    console: Option<&'a str>,
    tag: Option<(Tag, String)>,
    progress: Option<Progress>,
    selected: bool,
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
            selected: false,
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn cover(mut self, cover: Option<&'a Cover>) -> Self {
        self.cover = cover;
        self
    }

    pub fn console(mut self, console: Option<&'a str>) -> Self {
        self.console = console;
        self
    }

    pub fn tag(mut self, tag: Option<Tag>, path: &str) -> Self {
        self.tag = tag.map(|tag| (tag, tag.hint(path)));
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
        let name = text(ui, self.name, width, TextStyle::Body);
        let progress = self.progress.map(|progress| {
            let words = format!(
                "{} {} of {}",
                icons::TROPHY,
                progress.earned,
                progress.total
            );
            (progress, text(ui, &words, width, TextStyle::Small))
        });
        let progress_height = progress
            .as_ref()
            .map_or(0.0, |(_, galley)| PROGRESS_GAP + galley.size().y);
        let size = vec2(width, picture + NAME_GAP + name.size().y + progress_height);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, self.name));
        let response = match &self.tag {
            Some((_, hint)) => response.on_hover_text(hint),
            None if name.elided => response.on_hover_text(self.name),
            None => response,
        };
        if !ui.is_rect_visible(rect) {
            return response;
        }
        let gone = self.tag.is_some();
        let visuals = ui.visuals();
        let frame = Rect::from_min_size(rect.min, vec2(width, picture));
        let radius = visuals.widgets.noninteractive.corner_radius;
        let painter = ui.painter();
        painter.rect_filled(frame, radius, visuals.faint_bg_color);
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
                let lift = if self.console.is_some() {
                    CONSOLE_LIFT
                } else {
                    0.0
                };
                painter.text(
                    frame.center() - vec2(0.0, lift),
                    Align2::CENTER_CENTER,
                    icons::GAME_CONTROLLER,
                    FontId::proportional(EMPTY_ICON),
                    visuals.weak_text_color(),
                );
                if let Some(console) = self.console {
                    painter.text(
                        frame.center() + vec2(0.0, EMPTY_ICON * 0.5 + CONSOLE_GAP),
                        Align2::CENTER_CENTER,
                        console,
                        TextStyle::Small.resolve(ui.style()),
                        visuals.weak_text_color(),
                    );
                }
            }
        }
        let stroke = if response.hovered() || self.selected {
            Stroke::new(2.0_f32, visuals.selection.bg_fill)
        } else {
            visuals.widgets.noninteractive.bg_stroke
        };
        painter.rect_stroke(frame, radius, stroke, egui::StrokeKind::Outside);
        if let Some((tag, _)) = &self.tag {
            paint_tag(ui, frame, *tag);
        }
        if let Some((progress, _)) = &progress {
            let share = (progress.earned as f32 / progress.total as f32).clamp(0.0, 1.0);
            let bar = Rect::from_min_size(
                pos2(frame.left(), frame.bottom() - BAR_HEIGHT),
                vec2(frame.width() * share, BAR_HEIGHT),
            );
            ui.painter()
                .rect_filled(bar, 0.0, visuals.selection.bg_fill);
        }
        let color = if gone {
            visuals.weak_text_color()
        } else if response.hovered() {
            visuals.strong_text_color()
        } else {
            visuals.text_color()
        };
        let name_top = frame.bottom() + NAME_GAP;
        let name_height = name.size().y;
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
        Tag::NotConnected | Tag::NoAccess => {
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
