use super::PlayingUi;
use super::lines::member_name;
use crate::app::widgets::action::main_button;
use crate::app::widgets::avatar;
use crate::app::widgets::progress::ProgressBar;
use crate::icons;
use crate::online::Online;
use crate::play::{Play, PlaySession, Room};
use crate::progress::Progress;
use citrine_gb::console::Console;
use citrine_gb_net::OnlineId;
use egui::{Label, Ui};

const SCREEN_WIDTH: usize = 160;
const SCREEN_HEIGHT: usize = 144;
const MAX_SCREEN_SCALE: f32 = 3.0;

pub fn friend_console<'a>(play: &'a Play, session: &'a PlaySession) -> Option<&'a dyn Console> {
    play.room().and_then(Room::watched_console).or_else(|| {
        session
            .sees_friend()
            .then(|| session.console(false))
            .flatten()
    })
}

pub fn watched_friend(play: &Play, session: &PlaySession) -> OnlineId {
    play.room()
        .filter(|r| r.watched_console().is_some())
        .and_then(Room::watching)
        .unwrap_or(session.friend())
}

pub fn watched_name(play: &Play, online: &Online, session: &PlaySession) -> String {
    member_name(play, online, watched_friend(play, session))
}

fn friend_texture(
    playing: &mut PlayingUi,
    ctx: &egui::Context,
    console: &dyn Console,
) -> Option<(egui::TextureId, egui::Vec2)> {
    let video = console.video();
    let pass = ctx.cumulative_pass_nr();
    if playing.painted_pass != pass || playing.friend_screen.is_none() {
        playing.painted_pass = pass;
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [video.width, video.height],
            Console::frame(console),
        );
        match &mut playing.friend_screen {
            Some(texture) => texture.set(image, egui::TextureOptions::NEAREST),
            None => {
                playing.friend_screen =
                    Some(ctx.load_texture("friend_screen", image, egui::TextureOptions::NEAREST));
            }
        }
    }
    let size = egui::vec2(video.width as f32, video.height as f32);
    playing.friend_screen.as_ref().map(|t| (t.id(), size))
}

pub fn paint_friend_screen(
    playing: &mut PlayingUi,
    ui: &mut Ui,
    console: Option<&dyn Console>,
    area: egui::Rect,
) -> Option<egui::Rect> {
    let (texture, native) = friend_texture(playing, ui.ctx(), console?)?;
    let scale = (area.width() / native.x)
        .min(area.height() / native.y)
        .min(MAX_SCREEN_SCALE);
    let size = native * scale;
    let rect = egui::Rect::from_center_size(area.center(), size);
    egui::Image::new(egui::load::SizedTexture::new(texture, size)).paint_at(ui, rect);
    Some(rect)
}

pub fn note_over(ui: &mut Ui, area: egui::Rect, line: &str, button: Option<&str>) -> bool {
    let width = area
        .width()
        .min(area.height() * SCREEN_WIDTH as f32 / SCREEN_HEIGHT as f32);
    let size = egui::vec2(width, width * SCREEN_HEIGHT as f32 / SCREEN_WIDTH as f32);
    let rect = egui::Rect::from_center_size(area.center(), size);
    ui.painter()
        .rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
    let inner = rect.shrink2(egui::vec2(width * 0.12, 0.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    let lift = if button.is_some() { 30.0 } else { 12.0 };
    child.add_space(inner.height() / 2.0 - lift);
    child.add(Label::new(line).wrap());
    let Some(text) = button else {
        return false;
    };
    child.add_space(10.0);
    child
        .add(main_button(
            &child,
            format!("{}  {text}", icons::FOLDER_OPEN),
        ))
        .clicked()
}

pub fn arriving(ui: &mut Ui, area: egui::Rect, label: &str, progress: Progress) {
    let width = area
        .width()
        .min(area.height() * SCREEN_WIDTH as f32 / SCREEN_HEIGHT as f32);
    let size = egui::vec2(width, width * SCREEN_HEIGHT as f32 / SCREEN_WIDTH as f32);
    let rect = egui::Rect::from_center_size(area.center(), size);
    ui.painter()
        .rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
    let inner = rect.shrink2(egui::vec2(width * 0.15, 0.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::top_down(egui::Align::Center)),
    );
    child.add_space(inner.height() / 2.0 - 16.0);
    child.add(ProgressBar::new(label, progress));
}

pub struct WatchCaption<'a> {
    pub label: &'a str,
    pub name: &'a str,
    pub seed: u64,
    pub own: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchTap {
    Swap,
    Profile,
}

const CAPTION_GAP: f32 = 4.0;

pub fn watch_thumbnail(ui: &mut Ui, small: egui::Rect, caption: &WatchCaption) -> Option<WatchTap> {
    let color = ui.visuals().weak_text_color();
    let font = egui::FontId::proportional(super::room_bar::PHONE_LINE);
    let galley = ui
        .painter()
        .layout_no_wrap(caption.label.to_owned(), font, color);
    let side = (galley.size().y + 6.0).round();
    let width = side + CAPTION_GAP + galley.size().x;
    let face = egui::Rect::from_min_size(
        egui::pos2(small.center().x - width / 2.0, small.bottom() + 3.0),
        egui::Vec2::splat(side),
    );
    avatar::paint(ui, face, caption.name, caption.seed, None);
    let text = egui::pos2(
        face.right() + CAPTION_GAP,
        face.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(text, galley, color);
    let line = egui::Rect::from_min_size(face.min, egui::vec2(width, side));
    let thumb = ui.interact(small, egui::Id::new("friend_small"), egui::Sense::click());
    thumb.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Swap screens"));
    if thumb.clicked() {
        return Some(WatchTap::Swap);
    }
    if caption.own {
        return None;
    }
    let target = egui::Rect::from_min_size(
        egui::pos2(line.left() - 8.0, line.top() - 2.0),
        egui::vec2(line.width() + 16.0, (line.height() + 4.0).max(32.0)),
    );
    let name = ui.interact(
        target,
        egui::Id::new("friend_caption"),
        egui::Sense::click(),
    );
    name.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, caption.label));
    name.clicked().then_some(WatchTap::Profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tap_at(point: Option<egui::Pos2>, own: bool) -> (Option<WatchTap>, Vec<String>) {
        let ctx = egui::Context::default();
        let small = egui::Rect::from_min_size(egui::pos2(140.0, 300.0), egui::vec2(120.0, 80.0));
        let caption = WatchCaption {
            label: "Clara",
            name: "Clara Weiss",
            seed: 7,
            own,
        };
        let mut answer = None;
        let mut texts = Vec::new();
        let frames: Vec<Vec<egui::Event>> = match point {
            Some(at) => vec![
                vec![egui::Event::PointerMoved(at)],
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                }],
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            ],
            None => vec![Vec::new()],
        };
        for events in frames {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(412.0, 915.0),
                )),
                events,
                ..Default::default()
            };
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if let Some(tap) = watch_thumbnail(ui, small, &caption) {
                        answer = Some(tap);
                    }
                });
            });
            texts = output
                .shapes
                .into_iter()
                .filter_map(|clipped| match clipped.shape {
                    egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                    _ => None,
                })
                .collect();
        }
        (answer, texts)
    }

    #[test]
    fn the_caption_shows_the_watched_face_beside_the_name() {
        let (_, texts) = tap_at(None, false);
        assert!(texts.contains(&"Clara".to_owned()), "{texts:?}");
        assert!(
            texts.contains(&citrine_gb_net::initials("Clara Weiss")),
            "{texts:?}"
        );
    }

    #[test]
    fn a_tap_on_the_thumbnail_swaps_the_screens() {
        let (tap, _) = tap_at(Some(egui::pos2(200.0, 340.0)), false);
        assert_eq!(tap, Some(WatchTap::Swap));
    }

    #[test]
    fn a_tap_on_the_name_opens_the_profile_but_not_your_own() {
        let (tap, _) = tap_at(Some(egui::pos2(205.0, 392.0)), false);
        assert_eq!(tap, Some(WatchTap::Profile));
        let (tap, _) = tap_at(Some(egui::pos2(205.0, 392.0)), true);
        assert_eq!(tap, None);
    }

    #[test]
    fn the_caption_is_as_big_as_the_phone_status_line() {
        let ctx = egui::Context::default();
        let small = egui::Rect::from_min_size(egui::pos2(140.0, 300.0), egui::vec2(120.0, 80.0));
        let caption = WatchCaption {
            label: "Clara",
            name: "Clara Weiss",
            seed: 7,
            own: false,
        };
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let _ = watch_thumbnail(ui, small, &caption);
            });
        });
        let size = output
            .shapes
            .into_iter()
            .find_map(|clipped| match clipped.shape {
                egui::Shape::Text(text) if text.galley.text() == "Clara" => {
                    Some(text.galley.job.sections[0].format.font_id.size)
                }
                _ => None,
            })
            .expect("no caption");
        assert!(size >= super::super::room_bar::PHONE_LINE, "{size}");
    }

    #[test]
    fn the_name_takes_a_finger_sized_tap_below_the_thumbnail() {
        let (tap, _) = tap_at(Some(egui::pos2(205.0, 411.0)), false);
        assert_eq!(tap, Some(WatchTap::Profile));
        let (tap, _) = tap_at(Some(egui::pos2(205.0, 379.0)), false);
        assert_eq!(tap, Some(WatchTap::Swap));
    }
}
