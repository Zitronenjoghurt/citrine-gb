#[cfg(not(target_arch = "wasm32"))]
use crate::app::ui_state::ds_3d::{
    DISTANT_TEXTURES_HINT, DistantTextures, RESOLUTION_HINT, Resolution, SMOOTH_EDGES_HINT,
    SmoothEdges, TEXTURE_FILTER_HINT, TextureFilter,
};
use crate::app::ui_state::ds_screens::{Corner, MAX_GAP, Rotation, ScreenContext, SmallSize};
use crate::app::ui_state::layout_picks::Place;
use crate::app::ui_state::screen_layouts::LayoutConsole;
use crate::app::ui_state::settings::Settings;
use crate::app::ui_state::settings_tree::Surface;
use crate::app::ui_state::settings_tree::rows::RowId;
use crate::app::widgets::generic_select::GenericSelect;
use crate::app::widgets::reset_slider::ResetSlider;
use crate::app::widgets::settings::form::Form;
use egui::Widget;

fn label(row: RowId) -> &'static str {
    row.label(Surface::Desktop)
}

pub fn screens_row(form: &mut Form, s: &mut Settings, row: RowId) {
    let context = ScreenContext::Desktop;
    let placement = s.shown_layout(LayoutConsole::Ds, Place::Window).placement;
    let screens = &mut s.ds_screens;
    let shown = screens.shown_rows_with(placement, context);
    let mut changed = false;
    match row {
        RowId::TwoTabs if shown.separate_tabs => {
            changed |= form.toggle(label(row), &mut screens.separate_tabs);
        }
        RowId::SmallCorner if shown.small => {
            form.row(label(row), |ui| {
                changed |= GenericSelect::from_enum(&mut screens.corner, "ds_corner")
                    .default_value(Corner::default())
                    .ui(ui)
                    .changed();
            });
        }
        RowId::SmallSize if shown.small => {
            form.row(label(row), |ui| {
                changed |= GenericSelect::from_enum(&mut screens.small, "ds_small")
                    .default_value(SmallSize::default())
                    .ui(ui)
                    .changed();
            });
        }
        RowId::Gap if shown.gap => {
            form.row_with_hint(label(row), "In DS pixels.", |ui| {
                let response = ResetSlider::new(&mut screens.gap, 0..=MAX_GAP)
                    .default_value(0)
                    .ui(ui);
                changed |= response.drag_stopped() || (response.changed() && !response.dragged());
            });
        }
        RowId::Rotation => {
            form.row(label(row), |ui| {
                changed |= GenericSelect::from_enum(&mut screens.rotation, "ds_rotation")
                    .default_value(Rotation::default())
                    .ui(ui)
                    .changed();
            });
        }
        _ => {}
    }
    s.dirty |= changed;
}

#[cfg(not(target_arch = "wasm32"))]
pub fn three_d_row(form: &mut Form, s: &mut Settings, row: RowId) {
    let three_d = &mut s.ds_3d;
    let mut changed = false;
    match row {
        RowId::Resolution3d => {
            form.row_with_hint(label(row), RESOLUTION_HINT, |ui| {
                changed |= GenericSelect::from_enum(&mut three_d.resolution, "ds_3d_resolution")
                    .default_value(Resolution::default())
                    .ui(ui)
                    .changed();
            });
        }
        RowId::SmoothEdges if three_d.upscaled() => {
            form.row_with_hint(label(row), SMOOTH_EDGES_HINT, |ui| {
                changed |= GenericSelect::from_enum(&mut three_d.smooth_edges, "ds_3d_edges")
                    .default_value(SmoothEdges::default())
                    .ui(ui)
                    .changed();
            });
        }
        RowId::TextureFilter if three_d.upscaled() => {
            form.row_with_hint(label(row), TEXTURE_FILTER_HINT, |ui| {
                changed |=
                    GenericSelect::from_enum(&mut three_d.texture_filter, "ds_3d_texture_filter")
                        .default_value(TextureFilter::default())
                        .ui(ui)
                        .changed();
            });
        }
        RowId::DistantTextures if three_d.upscaled() => {
            form.row_with_hint(label(row), DISTANT_TEXTURES_HINT, |ui| {
                changed |= GenericSelect::from_enum(
                    &mut three_d.distant_textures,
                    "ds_3d_distant_textures",
                )
                .default_value(DistantTextures::default())
                .ui(ui)
                .changed();
            });
        }
        _ => {}
    }
    s.dirty |= changed;
}
