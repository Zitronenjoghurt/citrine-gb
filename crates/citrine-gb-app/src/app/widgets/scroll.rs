pub fn gutter(ui: &mut egui::Ui) {
    let scroll = &mut ui.spacing_mut().scroll;
    scroll.floating_allocated_width = scroll.bar_inner_margin + scroll.bar_width;
}

pub fn fixed_gutter(ui: &mut egui::Ui) -> f32 {
    let scroll = &mut ui.spacing_mut().scroll;
    scroll.floating_allocated_width = 0.0;
    scroll.bar_inner_margin + scroll.bar_width
}
