use egui::RichText;

pub fn faint(text: impl Into<String>) -> RichText {
    RichText::new(text).small().weak()
}
