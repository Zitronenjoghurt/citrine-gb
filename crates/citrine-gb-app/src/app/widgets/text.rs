use egui::{Align, Color32, FontId, Layout, Response, RichText, TextWrapMode, Ui, WidgetText};

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

pub fn name_then(ui: &mut Ui, name: &str, tail: &str, size: Option<f32>, reserve: f32) -> Response {
    let rich = |words: &str| match size {
        Some(size) => RichText::new(words).size(size),
        None => RichText::new(words),
    };
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let tail = WidgetText::from(rich(tail)).into_galley(
            ui,
            Some(TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Body,
        );
        let room = (ui.available_width() - tail.size().x - reserve).max(0.0);
        ui.scope(|ui| {
            ui.set_max_width(room);
            ui.add(egui::Label::new(rich(name)).truncate());
        });
        ui.label(tail)
    })
    .inner
}

pub fn balanced_width(widest: f32, rows: impl Fn(f32) -> usize) -> f32 {
    let wanted = rows(widest);
    if wanted < 2 {
        return widest;
    }
    let (mut short, mut long) = (widest / 2.0, widest);
    for _ in 0..12 {
        let mid = (short + long) / 2.0;
        if rows(mid) == wanted {
            long = mid;
        } else {
            short = mid;
        }
    }
    long
}

pub fn balanced_label(ui: &mut Ui, text: RichText) -> egui::Response {
    let widest = ui.available_width();
    let rows = |width: f32| {
        let mut job =
            std::sync::Arc::unwrap_or_clone(egui::WidgetText::from(text.clone()).into_layout_job(
                ui.style(),
                egui::FontSelection::Default,
                egui::Align::Center,
            ));
        job.wrap.max_width = width;
        ui.fonts_mut(|fonts| fonts.layout_job(job)).rows.len()
    };
    let width = (balanced_width(widest, rows) + 0.5).min(widest);
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Center),
        |ui| ui.add(egui::Label::new(text).wrap()),
    )
    .inner
}
