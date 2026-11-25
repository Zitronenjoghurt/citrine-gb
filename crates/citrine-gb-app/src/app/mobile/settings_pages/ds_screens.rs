use super::{Sheet, label, pick, slide};
use crate::app::mobile::kit::{List, Row};
use crate::app::mobile::sheet::Amount;
#[cfg(not(target_arch = "wasm32"))]
use crate::app::ui_state::ds_3d::{
    DISTANT_TEXTURES_HINT, Ds3d, RESOLUTION_HINT, SMOOTH_EDGES_HINT, TEXTURE_FILTER_HINT,
};
use crate::app::ui_state::ds_screens::{DsScreens, MAX_GAP, Placement, ScreenContext};
use crate::app::ui_state::settings_tree::rows::RowId;
use egui::Context;

pub(super) fn row(
    list: &mut List<'_>,
    screens: &DsScreens,
    placement: Placement,
    context: ScreenContext,
    row: RowId,
) -> Option<Sheet> {
    let shown = screens.shown_rows_with(placement, context);
    let opens = |list: &mut List<'_>, value: String, sheet: Sheet| {
        list.row(Row::new(label(row)).value(value))
            .clicked()
            .then_some(sheet)
    };
    match row {
        RowId::SmallCorner if shown.small => {
            opens(list, screens.corner.to_string(), Sheet::DsCorner)
        }
        RowId::SmallSize if shown.small => opens(list, screens.small.to_string(), Sheet::DsSmall),
        RowId::Gap if shown.gap => opens(list, screens.gap_label(), Sheet::DsGap),
        RowId::Rotation => opens(list, screens.rotation.to_string(), Sheet::DsRotation),
        _ => None,
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn three_d_row(list: &mut List<'_>, three_d: Ds3d, row: RowId) -> Option<Sheet> {
    let (entry, sheet) = match row {
        RowId::Resolution3d => (
            Row::new(label(row))
                .value(three_d.resolution.to_string())
                .info(RESOLUTION_HINT),
            Sheet::Ds3dResolution,
        ),
        RowId::SmoothEdges if three_d.upscaled() => (
            Row::new(label(row))
                .value(three_d.smooth_edges.to_string())
                .info(SMOOTH_EDGES_HINT),
            Sheet::DsSmoothEdges,
        ),
        RowId::TextureFilter if three_d.upscaled() => (
            Row::new(label(row))
                .value(three_d.texture_filter.to_string())
                .info(TEXTURE_FILTER_HINT),
            Sheet::DsTextureFilter,
        ),
        RowId::DistantTextures if three_d.upscaled() => (
            Row::new(label(row))
                .value(three_d.distant_textures.to_string())
                .info(DISTANT_TEXTURES_HINT),
            Sheet::DsDistantTextures,
        ),
        _ => return None,
    };
    list.row(entry).clicked().then_some(sheet)
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn three_d_sheet(ctx: &Context, open: Sheet, three_d: &mut Ds3d) -> (bool, bool) {
    match open {
        Sheet::Ds3dResolution => pick(
            ctx,
            label(RowId::Resolution3d),
            None,
            &mut three_d.resolution,
        ),
        Sheet::DsTextureFilter => pick(
            ctx,
            label(RowId::TextureFilter),
            None,
            &mut three_d.texture_filter,
        ),
        Sheet::DsDistantTextures => pick(
            ctx,
            label(RowId::DistantTextures),
            None,
            &mut three_d.distant_textures,
        ),
        _ => pick(
            ctx,
            label(RowId::SmoothEdges),
            None,
            &mut three_d.smooth_edges,
        ),
    }
}

pub(super) fn sheet(ctx: &Context, open: Sheet, screens: &mut DsScreens) -> (bool, bool) {
    match open {
        Sheet::DsCorner => pick(ctx, label(RowId::SmallCorner), None, &mut screens.corner),
        Sheet::DsSmall => pick(ctx, label(RowId::SmallSize), None, &mut screens.small),
        Sheet::DsRotation => pick(ctx, label(RowId::Rotation), None, &mut screens.rotation),
        _ => {
            let shown = slide(
                ctx,
                label(RowId::Gap),
                Some("In DS pixels."),
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
    }
}

fn gap_text(value: f64) -> String {
    if value < 0.5 {
        "None".to_owned()
    } else {
        format!("{value:.0}")
    }
}
