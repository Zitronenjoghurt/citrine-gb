#[cfg(not(target_arch = "wasm32"))]
use crate::app::ui_state::ds_3d::{
    Ds3d, RESOLUTION_HINT, Resolution, SMOOTH_EDGES_HINT, SmoothEdges,
};
use crate::app::ui_state::ds_screens::{
    Arrangement, Corner, MAX_GAP, Rotation, ScreenContext, SmallSize,
};
use crate::app::ui_state::settings::Settings;
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::reset_slider::ResetSlider;
use crate::app::widgets::settings::form::Form;
use crate::app::widgets::settings::page::{Heading, Page};
use egui::Widget;

pub fn section(page: &mut Page, s: &mut Settings) {
    page.section(
        Heading::new(crate::icons::SQUARE_SPLIT_VERTICAL, "DS screens"),
        |ui| Form::show(ui, "ds_screens", |form| rows(form, s)),
    );
}

fn rows(form: &mut Form, s: &mut Settings) {
    let context = ScreenContext::Desktop;
    let screens = &mut s.ds_screens;
    let shown = screens.shown_rows(context);
    let mut changed = false;
    if shown.placement {
        let mut arrangement = screens.desktop.arrangement;
        form.row("Layout", |ui| {
            GenericSelect::from_enum(&mut arrangement, "ds_layout")
                .default_value(Arrangement::default())
                .ui(ui);
        });
        changed |= screens.arrange(context, arrangement);
        let placement = &mut screens.desktop;
        changed |= form.toggle("Swap screens", &mut placement.swapped);
        changed |= form.toggle(placement.only_label(), &mut placement.one_screen);
    }
    if shown.separate_tabs {
        changed |= form.toggle("Separate tabs", &mut screens.separate_tabs);
    }
    if shown.small {
        form.row("Small screen corner", |ui| {
            changed |= GenericSelect::from_enum(&mut screens.corner, "ds_corner")
                .default_value(Corner::default())
                .ui(ui)
                .changed();
        });
        form.row("Small screen size", |ui| {
            changed |= GenericSelect::from_enum(&mut screens.small, "ds_small")
                .default_value(SmallSize::default())
                .ui(ui)
                .changed();
        });
    }
    if shown.gap {
        form.row("Gap, in DS pixels", |ui| {
            let response = ResetSlider::new(&mut screens.gap, 0..=MAX_GAP)
                .default_value(0)
                .ui(ui);
            changed |= response.drag_stopped() || (response.changed() && !response.dragged());
        });
    }
    form.row("Rotation", |ui| {
        changed |= GenericSelect::from_enum(&mut screens.rotation, "ds_rotation")
            .default_value(Rotation::default())
            .ui(ui)
            .changed();
    });
    #[cfg(not(target_arch = "wasm32"))]
    {
        changed |= three_d_rows(form, &mut s.ds_3d);
    }
    s.dirty |= changed;
}

#[cfg(not(target_arch = "wasm32"))]
fn three_d_rows(form: &mut Form, three_d: &mut Ds3d) -> bool {
    let mut changed = false;
    form.row_with_hint("3D resolution", RESOLUTION_HINT, |ui| {
        changed |= GenericSelect::from_enum(&mut three_d.resolution, "ds_3d_resolution")
            .default_value(Resolution::default())
            .ui(ui)
            .changed();
    });
    if three_d.upscaled() {
        form.row_with_hint("Smooth 3D edges", SMOOTH_EDGES_HINT, |ui| {
            changed |= GenericSelect::from_enum(&mut three_d.smooth_edges, "ds_3d_edges")
                .default_value(SmoothEdges::default())
                .ui(ui)
                .changed();
        });
    }
    changed
}
