use egui::{Color32, FontId, RichText, Ui};

pub fn faint(text: impl Into<String>) -> RichText {
    RichText::new(text).small().weak()
}

pub fn one_line(
    ui: &Ui,
    text: &str,
    size: f32,
    color: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat::simple(FontId::proportional(size), color),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(1.0));
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}
