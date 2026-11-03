use super::*;
use eframe::App as _;

struct Phone {
    size: egui::Vec2,
    insets: [f32; 4],
}

const NATIVE: f32 = 3.5;

fn portrait() -> Phone {
    Phone {
        size: egui::vec2(1440.0, 3120.0),
        insets: [0.0, 130.0, 0.0, 70.0],
    }
}

fn landscape() -> Phone {
    Phone {
        size: egui::vec2(3120.0, 1440.0),
        insets: [130.0, 0.0, 0.0, 70.0],
    }
}

#[derive(Debug, PartialEq)]
struct Shown {
    points: f32,
    content: egui::Rect,
    landscape: bool,
}

fn phone_frame(app: &mut Citrine, ctx: &Context, phone: &Phone) -> Shown {
    let points = ctx.zoom_factor() * NATIVE;
    let [left, top, right, bottom] = phone.insets.map(|px| px / points);
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            phone.size / points,
        )),
        safe_area_insets: Some(egui::SafeAreaInsets(egui::epaint::MarginF32 {
            left,
            right,
            top,
            bottom,
        })),
        ..Default::default()
    };
    input
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .native_pixels_per_point = Some(NATIVE);
    app.raw_input_hook(ctx, &mut input);
    let _ = ctx.run(input, |ctx| {
        mobile::touch_spacing(ctx, app.is_mobile(ctx));
        app.render(ctx);
        let mobile = app.is_mobile(ctx);
        let Citrine {
            ui,
            audio,
            emulator,
            ..
        } = app;
        ui.settings.apply(ctx, audio, emulator, mobile);
    });
    Shown {
        points: ctx.pixels_per_point(),
        content: ctx.content_rect(),
        landscape: mobile::landscape(ctx),
    }
}

fn settles(app: &mut Citrine, ctx: &Context, phone: &Phone) -> Shown {
    for _ in 0..3 {
        phone_frame(app, ctx, phone);
    }
    let first = phone_frame(app, ctx, phone);
    for frame in 0..20 {
        assert_eq!(
            phone_frame(app, ctx, phone),
            first,
            "frame {frame} after the turn shows the same layout"
        );
    }
    first
}

#[test]
fn turning_the_phone_settles_on_one_layout_each_way() {
    let mut app = phone_app("rotation");
    let ctx = Context::default();
    let upright = settles(&mut app, &ctx, &portrait());
    assert!(!upright.landscape);
    let turned = settles(&mut app, &ctx, &landscape());
    assert!(turned.landscape);
    assert_eq!(turned.points, upright.points, "turning keeps the text size");
    assert!(turned.content.width() > turned.content.height());
    assert_eq!(settles(&mut app, &ctx, &portrait()), upright);
}
