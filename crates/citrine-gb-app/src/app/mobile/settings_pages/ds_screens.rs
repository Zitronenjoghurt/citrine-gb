use super::{Sheet, pick, slide};
use crate::app::mobile::kit::{List, Row, SwitchRow};
use crate::app::mobile::sheet::Amount;
use crate::app::ui_state::ds_3d::Ds3d;
#[cfg(not(target_arch = "wasm32"))]
use crate::app::ui_state::ds_3d::{RESOLUTION_HINT, SMOOTH_EDGES_HINT};
use crate::app::ui_state::ds_screens::{Arrangement, DsScreens, MAX_GAP, ScreenContext};
use egui::Context;

pub(super) fn phone_context(context: ScreenContext) -> ScreenContext {
    match context {
        ScreenContext::Desktop => ScreenContext::Portrait,
        other => other,
    }
}

pub(super) fn rows(
    list: &mut List<'_>,
    screens: &mut DsScreens,
    three_d: Ds3d,
    context: ScreenContext,
    open: &mut Option<Sheet>,
) -> bool {
    let shown = screens.shown_rows(context);
    let mut changed = false;
    let mut row = |list: &mut List<'_>, label: &str, value: String, sheet: Sheet| {
        if list.row(Row::new(label).value(value)).clicked() {
            *open = Some(sheet);
        }
    };
    if shown.placement {
        let placement = screens.placement(context);
        row(
            list,
            "Layout",
            placement.arrangement.to_string(),
            Sheet::DsLayout,
        );
        let placement = screens.placement_mut(context);
        changed |= list.switch(SwitchRow::new("Swap screens", &mut placement.swapped));
        let only = placement.only_label();
        changed |= list.switch(SwitchRow::new(only, &mut placement.one_screen));
    }
    if shown.small {
        row(
            list,
            "Small screen corner",
            screens.corner.to_string(),
            Sheet::DsCorner,
        );
        row(
            list,
            "Small screen size",
            screens.small.to_string(),
            Sheet::DsSmall,
        );
    }
    if shown.gap {
        row(list, "Gap, in DS pixels", screens.gap_label(), Sheet::DsGap);
    }
    row(
        list,
        "Rotation",
        screens.rotation.to_string(),
        Sheet::DsRotation,
    );
    #[cfg(not(target_arch = "wasm32"))]
    three_d_rows(list, three_d, open);
    #[cfg(target_arch = "wasm32")]
    let _ = three_d;
    changed
}

#[cfg(not(target_arch = "wasm32"))]
fn three_d_rows(list: &mut List<'_>, three_d: Ds3d, open: &mut Option<Sheet>) {
    let resolution = Row::new("3D resolution")
        .value(three_d.resolution.to_string())
        .info(RESOLUTION_HINT);
    if list.row(resolution).clicked() {
        *open = Some(Sheet::Ds3dResolution);
    }
    if three_d.upscaled() {
        let edges = Row::new("Smooth 3D edges")
            .value(three_d.smooth_edges.to_string())
            .info(SMOOTH_EDGES_HINT);
        if list.row(edges).clicked() {
            *open = Some(Sheet::DsSmoothEdges);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn three_d_sheet(ctx: &Context, open: Sheet, three_d: &mut Ds3d) -> (bool, bool) {
    match open {
        Sheet::Ds3dResolution => pick(ctx, "3D resolution", None, &mut three_d.resolution),
        Sheet::DsSmoothEdges => pick(ctx, "Smooth 3D edges", None, &mut three_d.smooth_edges),
        _ => (false, true),
    }
}

pub(super) fn sheet(
    ctx: &Context,
    open: Sheet,
    screens: &mut DsScreens,
    context: ScreenContext,
) -> (bool, bool) {
    match open {
        Sheet::DsLayout => {
            let mut arrangement = screens.placement(context).arrangement;
            let (_, done) = pick::<Arrangement>(ctx, "Layout", None, &mut arrangement);
            (screens.arrange(context, arrangement), done)
        }
        Sheet::DsCorner => pick(ctx, "Small screen corner", None, &mut screens.corner),
        Sheet::DsSmall => pick(ctx, "Small screen size", None, &mut screens.small),
        Sheet::DsRotation => pick(ctx, "Rotation", None, &mut screens.rotation),
        Sheet::DsGap => {
            let shown = slide(
                ctx,
                "Gap, in DS pixels",
                None,
                Amount {
                    value: &mut screens.gap,
                    range: 0..=MAX_GAP,
                    step: 1.0,
                    default: 0,
                    text: gap_text,
                },
            );
            (shown.inner.changed(), shown.closed)
        }
        _ => (false, true),
    }
}

fn gap_text(value: f64) -> String {
    if value < 0.5 {
        "None".to_owned()
    } else {
        format!("{value:.0}")
    }
}
