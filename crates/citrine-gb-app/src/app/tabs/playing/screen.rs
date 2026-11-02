use super::PlayingUi;
use super::lines::size_line;
use crate::app::widgets::action::main_button;
use crate::icons;
use crate::play::PlaySession;
use egui::{Label, RichText, Ui};

const SCREEN_WIDTH: usize = 160;
const SCREEN_HEIGHT: usize = 144;
const MAX_SCREEN_SCALE: f32 = 3.0;

fn friend_texture(
    playing: &mut PlayingUi,
    ctx: &egui::Context,
    session: &PlaySession,
) -> Option<egui::TextureId> {
    if !session.sees_friend() {
        return None;
    }
    let console = session.console(false)?;
    let pass = ctx.cumulative_pass_nr();
    if playing.painted_pass != pass || playing.friend_screen.is_none() {
        playing.painted_pass = pass;
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [SCREEN_WIDTH, SCREEN_HEIGHT],
            console.frame().as_slice(),
        );
        match &mut playing.friend_screen {
            Some(texture) => texture.set(image, egui::TextureOptions::NEAREST),
            None => {
                playing.friend_screen =
                    Some(ctx.load_texture("friend_screen", image, egui::TextureOptions::NEAREST));
            }
        }
    }
    playing.friend_screen.as_ref().map(|t| t.id())
}

pub fn paint_friend_screen(
    playing: &mut PlayingUi,
    ui: &mut Ui,
    session: &PlaySession,
    area: egui::Rect,
) -> Option<egui::Rect> {
    let texture = friend_texture(playing, ui.ctx(), session)?;
    let scale = (area.width() / SCREEN_WIDTH as f32)
        .min(area.height() / SCREEN_HEIGHT as f32)
        .min(MAX_SCREEN_SCALE);
    let size = egui::vec2(SCREEN_WIDTH as f32, SCREEN_HEIGHT as f32) * scale;
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

pub fn arriving(ui: &mut Ui, area: egui::Rect, title: &str, name: &str, got: u32, total: u32) {
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
    child.add_space(inner.height() / 2.0 - 36.0);
    child.add(
        Label::new(format!(
            "{}  Getting {title} from {name}",
            icons::DOWNLOAD_SIMPLE
        ))
        .wrap(),
    );
    child.add_space(6.0);
    let progress = if total == 0 {
        0.0
    } else {
        got as f32 / total as f32
    };
    child.add(egui::ProgressBar::new(progress).desired_height(6.0));
    child.add_space(6.0);
    child.label(RichText::new(size_line(got, total)).weak());
}
