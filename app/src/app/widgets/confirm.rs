const OPEN_KEY: &str = "confirm_open";

pub fn confirm(ctx: &egui::Context, id: &str, message: &str, action: &str) -> Option<bool> {
    confirm_with_detail(ctx, id, message, None, action)
}

pub fn confirm_with_detail(
    ctx: &egui::Context,
    id: &str,
    message: &str,
    detail: Option<&str>,
    action: &str,
) -> Option<bool> {
    dialog(ctx, id, message, detail, action, None)
}

pub fn confirm_with_option(
    ctx: &egui::Context,
    id: &str,
    message: &str,
    action: &str,
    option: (&str, &mut bool),
) -> Option<bool> {
    dialog(ctx, id, message, None, action, Some(option))
}

fn dialog(
    ctx: &egui::Context,
    id: &str,
    message: &str,
    detail: Option<&str>,
    action: &str,
    option: Option<(&str, &mut bool)>,
) -> Option<bool> {
    let mut choice = None;
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(OPEN_KEY), pass));
    let modal = egui::Modal::new(egui::Id::new(id)).show(ctx, |ui| {
        ui.set_max_width(300.0);
        ui.label(message);
        if let Some(detail) = detail {
            ui.add(egui::Label::new(egui::RichText::new(detail).small()).truncate());
        }
        if let Some((label, checked)) = option {
            ui.checkbox(checked, label);
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .add(egui::Button::new(action).min_size(egui::vec2(80.0, 32.0)))
                .clicked()
            {
                choice = Some(true);
            }
            if ui
                .add(egui::Button::new("Cancel").min_size(egui::vec2(80.0, 32.0)))
                .clicked()
            {
                choice = Some(false);
            }
        });
    });
    if modal.should_close() && choice.is_none() {
        choice = Some(false);
    }
    choice
}

pub fn is_open(ctx: &egui::Context) -> bool {
    let pass = ctx.cumulative_pass_nr();
    ctx.data(|d| d.get_temp::<u64>(egui::Id::new(OPEN_KEY))) == Some(pass)
}
