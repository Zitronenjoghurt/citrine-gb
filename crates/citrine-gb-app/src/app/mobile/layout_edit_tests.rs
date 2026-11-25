use crate::app::Citrine;
use crate::app::hud::{Corner, TRACKER};
use crate::app::layout_edit::ResetButton;
use crate::app::layout_sheet::{LayoutSheet, SheetPick};
use crate::app::tabs::Tab;
use crate::app::tests::{phone_app, text_containing, text_rect};
use crate::app::ui_state::ds_screens::Placement;
use crate::app::ui_state::hotkeys::HotkeyAction;
use crate::app::ui_state::layout_picks::{Pick, Place, Preset, Resolved};
use crate::app::ui_state::per_game::Scope;
use crate::app::ui_state::screen_layouts::{Item, LayoutConsole, LayoutView, Spot, Spots};
use crate::app::ui_state::settings::LayoutMode;
use crate::app::widgets::screens_menu::NEW_LAYOUT;
use citrine_gb::nds::Cpu;
use egui::{Context, Event, Pos2, Rect, Vec2, pos2, vec2};

const RCNT_EXTKEYIN: u32 = 0x0400_0134;
const PEN_UP: u32 = 1 << 6;
const PORTRAIT: Vec2 = vec2(412.0, 915.0);
const LANDSCAPE: Vec2 = vec2(915.0, 412.0);
const DESKTOP: Vec2 = vec2(1280.0, 800.0);

fn ds_app(name: &str) -> Citrine {
    let mut app = phone_app(name);
    app.emulator
        .load_nds(include_bytes!(
            "../../../fixtures/nds/blocksds-console-both-screens.nds"
        ))
        .unwrap();
    app.mobile.nav.show_game();
    app.emulator.running = true;
    app
}

fn frame(app: &mut Citrine, ctx: &Context, size: Vec2, events: Vec<Event>) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size / ctx.zoom_factor())),
        viewports: std::iter::once((
            egui::ViewportId::ROOT,
            egui::ViewportInfo {
                native_pixels_per_point: Some(1.0),
                ..Default::default()
            },
        ))
        .collect(),
        events,
        ..Default::default()
    };
    let output = ctx.run(input, |ctx| {
        let touch = app.is_mobile(ctx);
        super::touch_spacing(ctx, touch);
        app.render(ctx);
        app.layout_edit_keys(ctx);
        app.mobile_escape(ctx);
        app.handle_event_queue(touch);
        let Citrine {
            ui,
            audio,
            emulator,
            ..
        } = app;
        ui.settings.apply(ctx, audio, emulator, touch);
    });
    app.second_screens_input();
    output
}

fn settle(app: &mut Citrine, ctx: &Context, size: Vec2) -> egui::FullOutput {
    frame(app, ctx, size, vec![]);
    frame(app, ctx, size, vec![]);
    frame(app, ctx, size, vec![])
}

fn button(at: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

fn tap(app: &mut Citrine, ctx: &Context, size: Vec2, at: Pos2) {
    frame(app, ctx, size, vec![Event::PointerMoved(at)]);
    frame(app, ctx, size, vec![button(at, true)]);
    frame(app, ctx, size, vec![button(at, false)]);
    frame(app, ctx, size, vec![Event::PointerGone]);
}

fn tap_text(app: &mut Citrine, ctx: &Context, size: Vec2, wanted: &str) {
    let find = |output: &egui::FullOutput| {
        text_rect(output, wanted).or_else(|| text_containing(output, wanted))
    };
    let mut last = find(&settle(app, ctx, size));
    for _ in 0..60 {
        let now = find(&frame(app, ctx, size, vec![]));
        if now.is_some() && now == last {
            break;
        }
        last = now;
    }
    let at = last
        .unwrap_or_else(|| panic!("{wanted} is not on screen"))
        .center();
    tap(app, ctx, size, at);
}

fn drag(app: &mut Citrine, ctx: &Context, size: Vec2, from: Pos2, to: Pos2) {
    frame(app, ctx, size, vec![Event::PointerMoved(from)]);
    frame(app, ctx, size, vec![button(from, true)]);
    for step in 1..=6 {
        let at = from.lerp(to, step as f32 / 6.0);
        frame(app, ctx, size, vec![Event::PointerMoved(at)]);
    }
    frame(app, ctx, size, vec![button(to, false)]);
    frame(app, ctx, size, vec![Event::PointerGone]);
}

fn escape() -> Event {
    Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    }
}

fn touch(app: &Citrine) -> Rect {
    app.emulator
        .placed_touch()
        .expect("the touch screen is shown")
}

fn pen_down(app: &mut Citrine) -> bool {
    let nds = app.emulator.as_nds_mut().expect("ds");
    (nds.buses().peek_io(Cpu::Arm7, RCNT_EXTKEYIN) >> 16) & PEN_UP == 0
}

fn free_spot(app: &Citrine, rect: Rect) -> Pos2 {
    let covered = |at: Pos2| app.ui.touch_targets.iter().any(|t| t.contains(at));
    (1..10)
        .flat_map(|y| (1..10).map(move |x| (x as f32 / 10.0, y as f32 / 10.0)))
        .map(|(x, y)| rect.lerp_inside(vec2(x, y)))
        .filter(|at| !covered(*at))
        .min_by(|a, b| {
            let centre = rect.center();
            a.distance(centre).total_cmp(&b.distance(centre))
        })
        .expect("a spot on the touch screen no button covers")
}

fn open_sheet(app: &mut Citrine, ctx: &Context, size: Vec2) -> egui::FullOutput {
    settle(app, ctx, size);
    if app.mobile.hidden.on() {
        let eye = app.mobile.hidden.eye.expect("the eye shows");
        tap(app, ctx, size, eye.center());
        settle(app, ctx, size);
    }
    let icon = app.screens_icon();
    tap_text(app, ctx, size, icon);
    assert!(
        app.ui.layout_sheet.is_some(),
        "the Layout button opens the sheet"
    );
    settle(app, ctx, size)
}

fn edit_from_sheet(app: &mut Citrine, ctx: &Context, size: Vec2) {
    open_sheet(app, ctx, size);
    tap_text(app, ctx, size, "Edit layout");
    assert!(
        app.editing_layout(),
        "Edit layout in the sheet starts editing"
    );
    settle(app, ctx, size);
}

fn save_edit(app: &mut Citrine, ctx: &Context, size: Vec2) {
    tap_text(app, ctx, size, &format!("{} Save", crate::icons::CHECK));
    assert!(!app.naming_layout(), "Save asks for no name");
    assert!(!app.editing_layout(), "Save ends editing");
}

fn start_new_layout(app: &mut Citrine) {
    app.start_layout_edit_with(crate::app::layout_edit::EditStart {
        new: true,
        level: None,
    });
}

fn create_layout(app: &mut Citrine, ctx: &Context, size: Vec2) {
    tap_text(app, ctx, size, &format!("{} Create", crate::icons::CHECK));
    assert!(app.naming_layout(), "Create asks for a name");
    tap_text(app, ctx, size, "Create");
    assert!(!app.editing_layout(), "the named layout ends editing");
}

fn right_click(app: &mut Citrine, ctx: &Context, size: Vec2, at: Pos2) {
    let press = |pressed| Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Secondary,
        pressed,
        modifiers: Default::default(),
    };
    frame(app, ctx, size, vec![Event::PointerMoved(at)]);
    frame(app, ctx, size, vec![press(true)]);
    frame(app, ctx, size, vec![press(false)]);
    frame(app, ctx, size, vec![Event::PointerGone]);
}

fn ds_pick(app: &Citrine, place: Place) -> Resolved {
    app.ui.settings.layout_pick(LayoutConsole::Ds, place)
}

fn preset(preset: Preset) -> Pick {
    Pick::Preset(preset.placement())
}

fn touch_spot(
    app: &Citrine,
    view: LayoutView,
) -> Option<crate::app::ui_state::screen_layouts::Spot> {
    app.ui
        .settings
        .layouts
        .spot(LayoutConsole::Ds, view, Item::TouchScreen)
}

#[test]
fn a_dragged_touch_screen_stays_moved_and_takes_the_pen_after_done() {
    let mut app = ds_app("layout-move");
    let ctx = Context::default();
    edit_from_sheet(&mut app, &ctx, LANDSCAPE);
    settle(&mut app, &ctx, LANDSCAPE);
    assert!(!app.emulator.running, "the game pauses while editing");

    let before = touch(&app);
    let shift = vec2(-120.0, 0.0);
    drag(
        &mut app,
        &ctx,
        LANDSCAPE,
        before.center(),
        before.center() + shift,
    );
    settle(&mut app, &ctx, LANDSCAPE);
    let after = touch(&app);
    assert!(
        after.center().distance(before.center() + shift) <= 10.0,
        "{before:?} moved to {after:?}"
    );
    assert!(touch_spot(&app, LayoutView::Landscape).is_some());

    save_edit(&mut app, &ctx, LANDSCAPE);
    assert!(app.emulator.running, "the game resumes after Save");
    settle(&mut app, &ctx, LANDSCAPE);
    assert_eq!(touch(&app), after, "the moved place stays after Save");

    let at = free_spot(&app, after);
    frame(
        &mut app,
        &ctx,
        LANDSCAPE,
        vec![Event::PointerMoved(at), button(at, true)],
    );
    frame(&mut app, &ctx, LANDSCAPE, vec![]);
    assert!(
        pen_down(&mut app),
        "a press at {at:?} on the moved screen reaches the pen"
    );
    frame(&mut app, &ctx, LANDSCAPE, vec![button(at, false)]);
    frame(&mut app, &ctx, LANDSCAPE, vec![]);
    assert!(!pen_down(&mut app));
}

#[test]
fn a_corner_drag_resizes_the_selected_screen_and_keeps_its_shape() {
    let mut app = ds_app("layout-resize");
    let ctx = Context::default();
    edit_from_sheet(&mut app, &ctx, PORTRAIT);
    let before = touch(&app);
    tap(&mut app, &ctx, PORTRAIT, before.center());
    let corner = before.right_bottom();
    drag(&mut app, &ctx, PORTRAIT, corner, corner - vec2(80.0, 60.0));
    settle(&mut app, &ctx, PORTRAIT);
    let after = touch(&app);
    assert!(
        after.width() < before.width() - 40.0,
        "{before:?} became {after:?}"
    );
    assert!(
        (after.min - before.min).length() < 1.0,
        "the opposite corner holds"
    );
    let shape = |r: Rect| r.height() / r.width();
    assert!((shape(after) - shape(before)).abs() < 0.01);
}

#[test]
fn the_opacity_slider_sets_the_button_opacity() {
    let mut app = ds_app("layout-opacity");
    let ctx = Context::default();
    app.ui.settings.touch.opacity = 0.5;
    edit_from_sheet(&mut app, &ctx, PORTRAIT);
    let output = settle(&mut app, &ctx, PORTRAIT);
    assert!(text_rect(&output, "50%").is_some(), "the opacity percent");
    let word = text_rect(&output, "Opacity").expect("the slider's label");
    let y = word.bottom() + 17.0;
    drag(
        &mut app,
        &ctx,
        PORTRAIT,
        pos2(word.left() + 20.0, y),
        pos2(PORTRAIT.x - 4.0, y),
    );
    assert!(
        app.ui.settings.touch.opacity > 0.9,
        "{}",
        app.ui.settings.touch.opacity
    );
    assert!(app.editing_layout(), "the slider keeps edit mode open");
}

#[test]
fn reset_brings_back_the_default_and_undo_takes_it_back() {
    let mut app = ds_app("layout-reset");
    let ctx = Context::default();
    edit_from_sheet(&mut app, &ctx, LANDSCAPE);
    settle(&mut app, &ctx, LANDSCAPE);
    let default = touch(&app);
    assert_eq!(app.ui.layout_reset_button(), ResetButton::Nothing);
    drag(
        &mut app,
        &ctx,
        LANDSCAPE,
        default.center(),
        default.center() - vec2(120.0, 0.0),
    );
    settle(&mut app, &ctx, LANDSCAPE);
    let moved = touch(&app);
    assert_ne!(moved, default);
    assert_eq!(app.ui.layout_reset_button(), ResetButton::Reset);

    tap_text(&mut app, &ctx, LANDSCAPE, "Reset");
    settle(&mut app, &ctx, LANDSCAPE);
    assert_eq!(touch(&app), default);
    assert_eq!(touch_spot(&app, LayoutView::Landscape), None);
    assert_eq!(app.ui.layout_reset_button(), ResetButton::Undo);

    tap_text(&mut app, &ctx, LANDSCAPE, "Undo");
    settle(&mut app, &ctx, LANDSCAPE);
    assert_eq!(touch(&app), moved);
    assert!(touch_spot(&app, LayoutView::Landscape).is_some());
}

#[test]
fn each_orientation_keeps_its_own_layout() {
    let mut app = ds_app("layout-views");
    let ctx = Context::default();
    settle(&mut app, &ctx, PORTRAIT);
    let upright = touch(&app);
    edit_from_sheet(&mut app, &ctx, LANDSCAPE);
    settle(&mut app, &ctx, LANDSCAPE);
    let from = touch(&app);
    drag(
        &mut app,
        &ctx,
        LANDSCAPE,
        from.center(),
        from.center() - vec2(120.0, 0.0),
    );
    save_edit(&mut app, &ctx, LANDSCAPE);

    settle(&mut app, &ctx, PORTRAIT);
    assert_eq!(touch(&app), upright, "portrait keeps its default");
    assert_eq!(touch_spot(&app, LayoutView::Portrait), None);
    assert!(touch_spot(&app, LayoutView::Landscape).is_some());
    assert_eq!(
        app.ui
            .settings
            .layouts
            .spot(LayoutConsole::GameBoy, LayoutView::Landscape, Item::Screen),
        None
    );
}

#[test]
fn back_ends_editing_and_the_game_goes_on() {
    let mut app = ds_app("layout-back");
    let ctx = Context::default();
    edit_from_sheet(&mut app, &ctx, PORTRAIT);
    frame(&mut app, &ctx, PORTRAIT, vec![escape()]);
    settle(&mut app, &ctx, PORTRAIT);
    assert!(!app.editing_layout());
    assert!(app.emulator.running);
    assert!(app.mobile.nav.on_game(), "Back leaves the game in view");
}

#[test]
fn the_layout_sheet_opens_the_editor() {
    let mut app = ds_app("layout-sheet");
    let ctx = Context::default();
    edit_from_sheet(&mut app, &ctx, PORTRAIT);
    assert!(app.ui.layout_sheet.is_none(), "the sheet closes");
    assert_eq!(app.ui.layout_done_label(), "Save");
}

#[test]
fn swap_on_a_saved_layout_swaps_its_screens_and_keeps_it_picked() {
    let mut app = ds_app("layout-arrange");
    let ctx = Context::default();
    open_sheet(&mut app, &ctx, LANDSCAPE);
    tap_text(&mut app, &ctx, LANDSCAPE, NEW_LAYOUT);
    settle(&mut app, &ctx, LANDSCAPE);
    let from = touch(&app);
    drag(
        &mut app,
        &ctx,
        LANDSCAPE,
        from.center(),
        from.center() - vec2(120.0, 0.0),
    );
    create_layout(&mut app, &ctx, LANDSCAPE);
    settle(&mut app, &ctx, LANDSCAPE);
    let moved = touch_spot(&app, LayoutView::Landscape).expect("the moved touch screen");
    let picked = ds_pick(&app, Place::Landscape).pick;
    app.screens_action(crate::app::ui_state::ds_screens::ScreensAction::Swap);
    settle(&mut app, &ctx, LANDSCAPE);
    assert_eq!(ds_pick(&app, Place::Landscape).pick, picked, "still picked");
    assert_eq!(
        app.ui
            .settings
            .layouts
            .spot(LayoutConsole::Ds, LayoutView::Landscape, Item::Screen),
        Some(moved),
        "the top screen takes the touch screen's place"
    );
}

#[test]
fn desktop_moves_a_ds_screen_inside_the_game_pane() {
    let mut app = ds_app("layout-desktop");
    app.ui.settings.layout = LayoutMode::Desktop;
    app.open_tab(Tab::GameBoy);
    let ctx = Context::default();
    settle(&mut app, &ctx, DESKTOP);
    app.start_layout_edit();
    let output = settle(&mut app, &ctx, DESKTOP);
    assert!(app.editing_layout());
    assert!(!app.emulator.running, "the game pauses while editing");
    assert!(
        text_rect(&output, "Edit layout").is_some(),
        "the foot bar shows"
    );
    let before = touch(&app);
    let shift = vec2(80.0, 0.0);
    drag(
        &mut app,
        &ctx,
        DESKTOP,
        before.center(),
        before.center() + shift,
    );
    settle(&mut app, &ctx, DESKTOP);
    let after = touch(&app);
    assert!(
        after.center().distance(before.center() + shift) <= 10.0,
        "{before:?} moved to {after:?}"
    );
    assert!(touch_spot(&app, LayoutView::Desktop).is_some());
    save_edit(&mut app, &ctx, DESKTOP);
    assert!(!app.editing_layout());
    assert!(app.emulator.running, "the game resumes after Done");
    settle(&mut app, &ctx, DESKTOP);
    assert_eq!(touch(&app), after);
}

#[test]
fn desktop_escape_ends_editing() {
    let mut app = ds_app("layout-desktop-esc");
    app.ui.settings.layout = LayoutMode::Desktop;
    app.open_tab(Tab::GameBoy);
    let ctx = Context::default();
    settle(&mut app, &ctx, DESKTOP);
    app.start_layout_edit();
    settle(&mut app, &ctx, DESKTOP);
    frame(&mut app, &ctx, DESKTOP, vec![escape()]);
    assert!(!app.editing_layout());
}

fn hud_rect(app: &Citrine, id: u32) -> Rect {
    app.ui
        .hud
        .placed
        .iter()
        .find(|(placed, _)| *placed == id)
        .map(|(_, rect)| *rect)
        .expect("the HUD is drawn")
}

#[test]
fn edit_mode_moves_and_resizes_a_hud_and_reset_puts_it_back() {
    let mut app = ds_app("layout-hud");
    let ctx = Context::default();
    app.ui.settings.hud.shown = true;
    let id = app.ui.settings.hud.add();
    edit_from_sheet(&mut app, &ctx, PORTRAIT);
    settle(&mut app, &ctx, PORTRAIT);
    let start = hud_rect(&app, id);
    assert_eq!(app.ui.layout_reset_button(), ResetButton::Nothing);

    drag(
        &mut app,
        &ctx,
        PORTRAIT,
        start.center(),
        start.center() + vec2(150.0, 90.0),
    );
    settle(&mut app, &ctx, PORTRAIT);
    let moved = hud_rect(&app, id);
    assert!(
        (moved.center() - start.center() - vec2(150.0, 90.0)).length() < 12.0,
        "{start:?} -> {moved:?}"
    );
    assert!(app.ui.hud.drag.is_none(), "the HUD's own drag stays off");

    tap(&mut app, &ctx, PORTRAIT, moved.center());
    drag(
        &mut app,
        &ctx,
        PORTRAIT,
        moved.right_bottom() + vec2(3.0, 3.0),
        moved.right_bottom() + vec2(60.0, 40.0),
    );
    settle(&mut app, &ctx, PORTRAIT);
    let grown = app.ui.settings.hud.get(id).expect("hud").size;
    assert!(grown > 1.2, "a corner drag grows the HUD to {grown}");
    assert_eq!(app.ui.layout_reset_button(), ResetButton::Reset);

    tap_text(&mut app, &ctx, PORTRAIT, ResetButton::Reset.icon());
    settle(&mut app, &ctx, PORTRAIT);
    assert_eq!(hud_rect(&app, id), start);
    tap_text(&mut app, &ctx, PORTRAIT, ResetButton::Undo.icon());
    settle(&mut app, &ctx, PORTRAIT);
    let back = app.ui.settings.hud.get(id).expect("hud");
    assert_eq!(back.size, grown);
}

#[test]
fn edit_mode_moves_the_tracker_and_reset_puts_it_back_in_its_corner() {
    let mut app = ds_app("layout-tracker");
    let ctx = Context::default();
    app.achievements.boards.attempts = vec![citrine_gb_achievements::Attempt {
        board: 1,
        title: "Brock Speedrun".to_owned(),
        score: "12:38.40".to_owned(),
    }];
    edit_from_sheet(&mut app, &ctx, PORTRAIT);
    settle(&mut app, &ctx, PORTRAIT);
    let start = hud_rect(&app, TRACKER);
    let corner = app.ui.settings.hud.get(TRACKER).expect("tracker").corner;
    assert_eq!(corner, Corner::TopRight);
    assert_eq!(app.ui.layout_reset_button(), ResetButton::Nothing);

    drag(
        &mut app,
        &ctx,
        PORTRAIT,
        start.center(),
        start.center() + vec2(-100.0, 120.0),
    );
    settle(&mut app, &ctx, PORTRAIT);
    let moved = hud_rect(&app, TRACKER);
    assert!(
        (moved.center() - start.center() - vec2(-100.0, 120.0)).length() < 12.0,
        "{start:?} -> {moved:?}"
    );
    assert_eq!(app.ui.layout_reset_button(), ResetButton::Reset);

    tap_text(&mut app, &ctx, PORTRAIT, ResetButton::Reset.icon());
    settle(&mut app, &ctx, PORTRAIT);
    assert_eq!(hud_rect(&app, TRACKER), start);
    let back = app.ui.settings.hud.get(TRACKER).expect("tracker");
    assert_eq!(back.corner, Corner::TopRight);
}

fn top_screen(app: &Citrine) -> Rect {
    let touch = app.emulator.placed_touch();
    app.emulator
        .placed_screens()
        .iter()
        .copied()
        .find(|rect| Some(*rect) != touch)
        .expect("the top screen is shown")
}

fn screens_fit(app: &Citrine, size: Vec2) -> Option<String> {
    screens_fit_in(app, Rect::from_min_size(Pos2::ZERO, size))
}

fn screens_fit_in(app: &Citrine, window: Rect) -> Option<String> {
    let size = window.size();
    let top = top_screen(app);
    let touch = touch(app);
    eprintln!("{size:?}: top {top:?} touch {touch:?}");
    if let Some(out) = [top, touch]
        .into_iter()
        .find(|rect| !window.expand(0.5).contains_rect(*rect))
    {
        return Some(format!("{out:?} leaves the window {size:?}"));
    }
    top.shrink(0.5)
        .intersects(touch.shrink(0.5))
        .then(|| format!("at {size:?} the screens overlap: top {top:?}, touch {touch:?}"))
}

#[test]
fn desktop_moved_screens_stay_apart_when_the_pane_changes_size() {
    let mut app = ds_app("layout-desktop-resize");
    app.ui.settings.layout = LayoutMode::Desktop;
    app.open_tab(Tab::GameBoy);
    let ctx = Context::default();
    settle(&mut app, &ctx, DESKTOP);
    app.start_layout_edit();
    settle(&mut app, &ctx, DESKTOP);
    let before = touch(&app);
    drag(
        &mut app,
        &ctx,
        DESKTOP,
        before.center(),
        before.center() + vec2(60.0, 0.0),
    );
    save_edit(&mut app, &ctx, DESKTOP);
    let sizes = [
        DESKTOP,
        vec2(900.0, 800.0),
        vec2(700.0, 800.0),
        vec2(1920.0, 1080.0),
        vec2(1280.0, 520.0),
        DESKTOP,
    ];
    let broken: Vec<String> = sizes
        .into_iter()
        .filter_map(|size| {
            settle(&mut app, &ctx, size);
            screens_fit(&app, size)
        })
        .collect();
    assert!(broken.is_empty(), "{broken:#?}");
}

fn place_in_room(app: &Citrine) -> [f32; 4] {
    let room = app
        .emulator
        .spotted_room()
        .expect("the moved layout has a room");
    let (top, touch) = (top_screen(app), touch(app));
    assert!(
        touch.top() >= top.bottom() - 0.5,
        "the pair is stacked: {top:?} {touch:?}"
    );
    [
        (top.center().x - room.left()) / room.width(),
        (touch.center().x - room.left()) / room.width(),
        (top.top() - room.top()) / room.height(),
        (touch.bottom() - room.top()) / room.height(),
    ]
}

fn game_pane(app: &Citrine) -> Rect {
    let (_, node, _) = app.dock.find_tab(&Tab::GameBoy).expect("the game tab");
    app.dock.main_surface()[node]
        .rect()
        .expect("the game pane was laid out")
}

#[test]
fn desktop_moved_screens_hold_their_layout_when_the_dock_split_moves() {
    let mut app = ds_app("layout-desktop-split");
    app.ui.settings.layout = LayoutMode::Desktop;
    app.dock = egui_dock::DockState::new(vec![Tab::GameBoy]);
    app.dock
        .main_surface_mut()
        .split_right(egui_dock::NodeIndex::root(), 0.62, vec![Tab::Cheats]);
    let ctx = Context::default();
    settle(&mut app, &ctx, DESKTOP);
    start_new_layout(&mut app);
    settle(&mut app, &ctx, DESKTOP);
    let before = touch(&app);
    drag(
        &mut app,
        &ctx,
        DESKTOP,
        before.center(),
        before.center() + vec2(60.0, 0.0),
    );
    create_layout(&mut app, &ctx, DESKTOP);
    let made = ds_pick(&app, Place::Window).pick;
    let wide = place_in_room(&app);
    let mut broken = Vec::new();
    for fraction in [0.35, 0.8, 0.5, 0.62] {
        let (_, node, _) = app.dock.find_tab(&Tab::GameBoy).expect("the game tab");
        let parent = node.parent().expect("the game shares the window");
        let egui_dock::Node::Horizontal(split) = &mut app.dock.main_surface_mut()[parent] else {
            panic!("the game sits beside a pane");
        };
        split.fraction = fraction;
        settle(&mut app, &ctx, DESKTOP);
        let pane = game_pane(&app);
        broken.extend(screens_fit_in(&app, pane));
        let now = place_in_room(&app);
        if wide.iter().zip(now).any(|(a, b)| (a - b).abs() > 0.005) {
            broken.push(format!(
                "at {fraction} the pair left its place in the pane: {wide:?} -> {now:?}"
            ));
        }
    }
    assert!(broken.is_empty(), "{broken:#?}");
    assert_eq!(ds_pick(&app, Place::Window).pick, made);
}

#[test]
fn focus_mode_shows_the_desktop_layout_scaled_to_the_window() {
    let mut app = ds_app("layout-desktop-focus");
    app.ui.settings.layout = LayoutMode::Desktop;
    app.open_tab(Tab::GameBoy);
    let ctx = Context::default();
    settle(&mut app, &ctx, DESKTOP);
    start_new_layout(&mut app);
    settle(&mut app, &ctx, DESKTOP);
    let before = touch(&app);
    drag(
        &mut app,
        &ctx,
        DESKTOP,
        before.center(),
        before.center() + vec2(60.0, 0.0),
    );
    create_layout(&mut app, &ctx, DESKTOP);
    let made = ds_pick(&app, Place::Window).pick;
    app.ui.settings.set_layout_pick(
        LayoutConsole::Ds,
        Place::FullScreen,
        made,
        Scope::AllGames,
        false,
    );
    settle(&mut app, &ctx, DESKTOP);
    let top = top_screen(&app);
    let pane = place_in_room(&app);
    app.ui.focus_mode = true;
    settle(&mut app, &ctx, DESKTOP);
    let big_top = top_screen(&app);
    assert!(big_top.width() > top.width(), "{top:?} -> {big_top:?}");
    let full = place_in_room(&app);
    assert!(
        pane.iter().zip(full).all(|(a, b)| (a - b).abs() < 0.005),
        "{pane:?} -> {full:?}"
    );
    assert!(screens_fit(&app, DESKTOP).is_none());
}

#[test]
fn the_layout_button_opens_the_sheet_and_a_card_applies_in_this_place_only() {
    let mut app = ds_app("layout-sheet-card");
    let ctx = Context::default();
    let output = open_sheet(&mut app, &ctx, PORTRAIT);
    assert!(text_rect(&output, "Layout").is_some());
    assert!(
        text_containing(&output, "DS, portrait").is_none(),
        "the header names no mode"
    );
    tap_text(&mut app, &ctx, PORTRAIT, "Top only");
    settle(&mut app, &ctx, PORTRAIT);
    let portrait = ds_pick(&app, Place::Portrait);
    assert_eq!(portrait.pick, preset(Preset::TopOnly));
    assert!(!portrait.game && !portrait.every_mode);
    assert!(
        app.emulator.ds_screens.portrait.one_screen,
        "the game shows it"
    );
    assert!(
        !ds_pick(&app, Place::Landscape).chosen,
        "landscape keeps its own"
    );
    assert!(app.ui.layout_sheet.is_some(), "the sheet stays open");
}

#[test]
fn the_game_bar_picks_for_this_game_and_every_mode_reaches_landscape() {
    let mut app = ds_app("layout-sheet-scope");
    app.emulator.set_rom_key_for_tests("nds-sheet");
    let ctx = Context::default();
    open_sheet(&mut app, &ctx, PORTRAIT);
    let title = crate::app::widgets::settings::scope::game_title(&app.ui.settings).to_owned();
    tap_text(&mut app, &ctx, PORTRAIT, &title);
    tap_text(&mut app, &ctx, PORTRAIT, "Bottom only");
    let picked = ds_pick(&app, Place::Portrait);
    assert_eq!(picked.pick, preset(Preset::BottomOnly));
    assert!(picked.game, "the game bar wrote this game's pick");
    app.ui.settings.play_game(Some("nds-other"));
    assert!(
        !ds_pick(&app, Place::Portrait).chosen,
        "another game keeps the default"
    );
    app.ui.settings.play_game(Some("nds-sheet"));

    tap_text(&mut app, &ctx, PORTRAIT, "Every mode");
    tap_text(&mut app, &ctx, PORTRAIT, "Top only");
    let landscape = ds_pick(&app, Place::Landscape);
    assert_eq!(landscape.pick, preset(Preset::TopOnly));
    assert!(landscape.every_mode, "every mode applies in landscape too");
}

#[test]
fn a_games_own_pick_shows_on_both_sides_and_its_tag_removes_it() {
    use crate::app::widgets::settings::scope::Tag;
    let mut app = ds_app("layout-sheet-tag");
    app.emulator.set_rom_key_for_tests("nds-tag");
    let ctx = Context::default();
    settle(&mut app, &ctx, PORTRAIT);
    for (games, pick) in [
        (Scope::AllGames, Preset::Stacked),
        (Scope::ThisGame, Preset::TopOnly),
    ] {
        app.ui.settings.set_layout_pick(
            LayoutConsole::Ds,
            Place::Portrait,
            preset(pick),
            games,
            false,
        );
    }
    let output = open_sheet(&mut app, &ctx, PORTRAIT);
    let device = app.layout_device(&ctx);
    let tag = Tag::Pick {
        game: crate::app::widgets::settings::scope::game_title(&app.ui.settings).to_owned(),
        value: "Top only".to_owned(),
    };
    assert_eq!(app.ui.layout_facts(&device).tag, Some(tag.clone()));
    let words = format!(": Top only {}", crate::icons::X);
    let this_game = text_containing(&output, &words).expect("the game's tag");
    app.ui
        .pick_in_sheet(&device, SheetPick::Games(Scope::AllGames));
    let facts = app.ui.layout_facts(&device);
    assert_eq!(facts.tag, Some(tag), "one tag in both scopes");
    let chosen: Vec<_> = facts.cards.iter().filter(|card| card.chosen).collect();
    assert_eq!(chosen.len(), 1);
    assert_eq!(
        chosen[0].name, "Stacked",
        "the all games side shows its own pick"
    );
    let output = settle(&mut app, &ctx, PORTRAIT);
    let all_games = text_containing(&output, &words).expect("the tag in all games");
    assert!(
        (all_games.width() - this_game.width()).abs() < 0.5,
        "{all_games:?} against {this_game:?}"
    );
    tap(&mut app, &ctx, PORTRAIT, all_games.center());
    settle(&mut app, &ctx, PORTRAIT);
    let now = ds_pick(&app, Place::Portrait);
    assert_eq!(now.pick, preset(Preset::Stacked));
    assert!(!now.game, "the game follows all games again");
    assert_eq!(app.ui.layout_facts(&device).tag, None);
    app.ui
        .pick_in_sheet(&device, SheetPick::Games(Scope::ThisGame));
    assert_eq!(app.ui.layout_facts(&device).tag, Some(Tag::Follows));
}

#[test]
fn the_desktop_menu_shows_the_swap_key_and_no_card_key() {
    let mut app = ds_app("layout-desktop-keys");
    app.ui.settings.layout = LayoutMode::Desktop;
    app.open_tab(Tab::GameBoy);
    let ctx = Context::default();
    settle(&mut app, &ctx, DESKTOP);
    app.ui.layout_sheet = Some(LayoutSheet::default());
    let output = settle(&mut app, &ctx, DESKTOP);
    assert!(text_rect(&output, "Top only").is_some(), "the menu is open");
    let swap = text_containing(&output, "Swap screens").expect("the swap row");
    let device = app.layout_device(&ctx);
    let key = device.keys.swap.expect("swap has a key");
    let drawn = text_rect(&output, &key).expect("the swap key");
    assert!(
        (drawn.center().y - swap.center().y).abs() < 2.0,
        "{key} at {drawn:?}, the row at {swap:?}"
    );
    if let Some(one) = device.keys.one {
        assert!(text_rect(&output, &one).is_none(), "{one} is drawn");
    }
}

#[test]
fn the_sheet_opens_on_the_level_the_pick_comes_from() {
    let mut app = ds_app("layout-sheet-level");
    app.ui.settings.set_layout_pick(
        LayoutConsole::Ds,
        Place::Portrait,
        preset(Preset::TopOnly),
        Scope::AllGames,
        true,
    );
    let ctx = Context::default();
    open_sheet(&mut app, &ctx, PORTRAIT);
    tap_text(&mut app, &ctx, PORTRAIT, "Stacked");
    let picks = app.ui.settings.layout_picks.ds;
    assert_eq!(picks.every_mode, Some(preset(Preset::Stacked)));
    assert_eq!(picks.portrait, None, "the card wrote the every mode level");
}

#[test]
fn new_then_create_makes_a_saved_card_listed_in_this_place_only() {
    let mut app = ds_app("layout-sheet-new");
    let ctx = Context::default();
    open_sheet(&mut app, &ctx, PORTRAIT);
    tap_text(&mut app, &ctx, PORTRAIT, NEW_LAYOUT);
    assert!(app.editing_layout());
    assert_eq!(app.ui.layout_done_label(), "Create");
    let before = touch(&app);
    drag(
        &mut app,
        &ctx,
        PORTRAIT,
        before.center(),
        before.center() + vec2(0.0, 60.0),
    );
    create_layout(&mut app, &ctx, PORTRAIT);
    let saved = app.ui.settings.saved_layouts.clone();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].name, "My layout");
    assert_eq!(saved[0].place, Place::Portrait);
    assert_eq!(
        ds_pick(&app, Place::Portrait).pick,
        Pick::Saved(saved[0].id)
    );
    let output = open_sheet(&mut app, &ctx, PORTRAIT);
    assert!(
        text_rect(&output, "My layout").is_some(),
        "the card shows here"
    );
    app.ui.layout_sheet = None;
    open_sheet(&mut app, &ctx, LANDSCAPE);
    let device = app.layout_device(&ctx);
    assert_eq!(device.here, Place::Landscape);
    let cards = app.ui.layout_facts(&device).cards;
    assert!(
        !cards.iter().any(|card| card.name == "My layout"),
        "no card in landscape: {cards:?}"
    );
}

#[test]
fn cancel_in_the_editor_leaves_no_card_and_no_move() {
    let mut app = ds_app("layout-sheet-cancel");
    let ctx = Context::default();
    open_sheet(&mut app, &ctx, PORTRAIT);
    tap_text(&mut app, &ctx, PORTRAIT, NEW_LAYOUT);
    let before = touch(&app);
    drag(
        &mut app,
        &ctx,
        PORTRAIT,
        before.center(),
        before.center() + vec2(0.0, 60.0),
    );
    tap_text(&mut app, &ctx, PORTRAIT, "Cancel");
    assert!(!app.editing_layout());
    assert!(app.ui.settings.saved_layouts.is_empty());
    assert_eq!(touch_spot(&app, LayoutView::Portrait), None);
    settle(&mut app, &ctx, PORTRAIT);
    assert_eq!(touch(&app), before);
}

#[test]
fn a_saved_card_renames_and_deletes_after_asking() {
    let mut app = ds_app("layout-sheet-rename");
    let ctx = Context::default();
    let id = app.ui.settings.save_layout(
        "My layout",
        LayoutConsole::Ds,
        Place::Portrait,
        Preset::Stacked.placement(),
        Default::default(),
    );
    app.ui.settings.set_layout_pick(
        LayoutConsole::Ds,
        Place::Portrait,
        Pick::Saved(id),
        Scope::AllGames,
        false,
    );
    let output = open_sheet(&mut app, &ctx, PORTRAIT);
    let card = text_rect(&output, "My layout").expect("the saved card");
    right_click(&mut app, &ctx, PORTRAIT, card.center());
    tap_text(&mut app, &ctx, PORTRAIT, "Rename");
    if let Some(sheet) = &mut app.ui.layout_sheet
        && let Some((_, name)) = &mut sheet.rename
    {
        *name = "Couch".to_owned();
    }
    tap_text(&mut app, &ctx, PORTRAIT, "Rename");
    assert_eq!(
        app.ui.settings.saved_layout(id).map(|s| s.name.as_str()),
        Some("Couch")
    );

    let output = settle(&mut app, &ctx, PORTRAIT);
    let card = text_rect(&output, "Couch").expect("the renamed card");
    right_click(&mut app, &ctx, PORTRAIT, card.center());
    tap_text(&mut app, &ctx, PORTRAIT, "Delete");
    assert!(
        app.ui.settings.saved_layout(id).is_some(),
        "Delete asks first"
    );
    let output = settle(&mut app, &ctx, PORTRAIT);
    assert!(text_rect(&output, "Delete Couch?").is_some());
    tap_text(&mut app, &ctx, PORTRAIT, "Delete");
    assert!(app.ui.settings.saved_layout(id).is_none());
    assert!(
        !ds_pick(&app, Place::Portrait).chosen,
        "the place falls back"
    );
}

#[test]
fn the_desktop_chip_opens_the_layout_popover_with_checkboxes() {
    let mut app = ds_app("layout-popover");
    app.ui.settings.layout = LayoutMode::Desktop;
    app.open_tab(Tab::GameBoy);
    let ctx = Context::default();
    let output = settle(&mut app, &ctx, DESKTOP);
    let chip = text_containing(&output, "Stacked").expect("the layout chip names the pick");
    tap(&mut app, &ctx, DESKTOP, chip.center());
    let output = settle(&mut app, &ctx, DESKTOP);
    assert!(app.ui.layout_sheet.is_some(), "the chip opens the popover");
    assert!(
        text_containing(&output, "DS, window").is_none(),
        "the header names no mode"
    );
    let swap_row = text_containing(&output, "Swap screens").expect("the swap row");
    tap(&mut app, &ctx, DESKTOP, swap_row.center());
    settle(&mut app, &ctx, DESKTOP);
    assert!(
        ds_pick(&app, Place::Window).pick
            == Pick::Preset(Placement {
                swapped: true,
                ..Preset::Stacked.placement()
            }),
        "the checkbox swaps the screens in a window"
    );
    assert!(!ds_pick(&app, Place::FullScreen).chosen);
    frame(&mut app, &ctx, DESKTOP, vec![escape()]);
    settle(&mut app, &ctx, DESKTOP);
    assert!(app.ui.layout_sheet.is_none(), "Esc closes the popover");
}

fn moved_touch(size: Vec2) -> (Spot, Spots) {
    let room = Rect::from_min_size(Pos2::ZERO, size);
    let spot = Spot::of(
        Rect::from_min_size(pos2(40.0, 520.0), vec2(256.0, 192.0)),
        room,
    );
    (spot, Spots::from([(Item::TouchScreen, spot)]))
}

#[test]
fn a_games_own_saved_layout_stays_out_of_a_game_without_a_pick() {
    let mut app = ds_app("layout-other-game");
    app.emulator.set_rom_key_for_tests("nds-a");
    let ctx = Context::default();
    settle(&mut app, &ctx, PORTRAIT);
    let (spot, spots) = moved_touch(PORTRAIT);
    let id = app.ui.settings.save_layout(
        "Mine",
        LayoutConsole::Ds,
        Place::Portrait,
        Placement::default(),
        spots.clone(),
    );
    app.ui.settings.set_layout_pick(
        LayoutConsole::Ds,
        Place::Portrait,
        Pick::Saved(id),
        Scope::ThisGame,
        false,
    );
    settle(&mut app, &ctx, PORTRAIT);
    assert_eq!(touch_spot(&app, LayoutView::Portrait), Some(spot));
    app.emulator.set_rom_key_for_tests("nds-b");
    settle(&mut app, &ctx, PORTRAIT);
    assert_eq!(touch_spot(&app, LayoutView::Portrait), None);
    app.ui
        .settings
        .layouts
        .restore(LayoutConsole::Ds, LayoutView::Portrait, spots);
    app.ui.layout_applied = None;
    settle(&mut app, &ctx, PORTRAIT);
    assert_eq!(
        touch_spot(&app, LayoutView::Portrait),
        None,
        "a fresh start in game B drops game A's spots"
    );
}

#[test]
fn desktop_escape_on_the_name_dialog_keeps_editing() {
    let mut app = ds_app("layout-desktop-name-esc");
    app.ui.settings.layout = LayoutMode::Desktop;
    app.open_tab(Tab::GameBoy);
    let ctx = Context::default();
    settle(&mut app, &ctx, DESKTOP);
    start_new_layout(&mut app);
    settle(&mut app, &ctx, DESKTOP);
    app.done_layout_edit();
    settle(&mut app, &ctx, DESKTOP);
    assert!(app.naming_layout(), "Create asks for a name");
    frame(&mut app, &ctx, DESKTOP, vec![escape()]);
    settle(&mut app, &ctx, DESKTOP);
    assert!(!app.naming_layout(), "Esc closes the name dialog");
    assert!(app.editing_layout(), "and the edit stays open");
    frame(&mut app, &ctx, DESKTOP, vec![escape()]);
    assert!(!app.editing_layout(), "a second Esc cancels the edit");
}

#[test]
fn layout_keys_and_picks_wait_while_editing_and_cancel_restores_all() {
    let mut app = ds_app("layout-keys-edit");
    let ctx = Context::default();
    app.ui.settings.touch.opacity = 0.6;
    edit_from_sheet(&mut app, &ctx, PORTRAIT);
    let before = touch(&app);
    drag(
        &mut app,
        &ctx,
        PORTRAIT,
        before.center(),
        before.center() + vec2(0.0, 60.0),
    );
    let moved = touch_spot(&app, LayoutView::Portrait).expect("the moved touch screen");
    let picks = app.ui.settings.layout_picks;
    for action in [
        HotkeyAction::SwapScreens,
        HotkeyAction::OneScreen,
        HotkeyAction::NextScreenLayout,
    ] {
        app.run_hotkey(action);
        settle(&mut app, &ctx, PORTRAIT);
    }
    assert_eq!(app.ui.settings.layout_picks, picks, "F2 changes no pick");
    assert!(app.editing_layout());
    app.ui.settings.set_layout_pick(
        LayoutConsole::Ds,
        Place::Portrait,
        preset(Preset::TopOnly),
        Scope::AllGames,
        false,
    );
    app.ui.settings.touch.opacity = 0.2;
    settle(&mut app, &ctx, PORTRAIT);
    assert_eq!(
        touch_spot(&app, LayoutView::Portrait),
        Some(moved),
        "a pick change waits for the edit"
    );
    assert!(!app.emulator.ds_screens.portrait.one_screen);
    app.ui
        .settings
        .clear_layout_pick(LayoutConsole::Ds, Place::Portrait, Scope::AllGames, false);
    tap_text(&mut app, &ctx, PORTRAIT, "Cancel");
    settle(&mut app, &ctx, PORTRAIT);
    assert!(!app.editing_layout());
    assert_eq!(touch_spot(&app, LayoutView::Portrait), None);
    assert_eq!(touch(&app), before);
    assert_eq!(app.ui.settings.touch.opacity, 0.6);
}

#[test]
fn a_tap_outside_closes_the_sheet_and_escape_steps_back() {
    let mut app = ds_app("layout-sheet-outside");
    let ctx = Context::default();
    open_sheet(&mut app, &ctx, PORTRAIT);
    let other = Place::Landscape.row_label();
    tap_text(&mut app, &ctx, PORTRAIT, other);
    settle(&mut app, &ctx, PORTRAIT);
    let shown = |app: &Citrine| app.ui.layout_sheet.as_ref().map(|sheet| sheet.other);
    assert_eq!(shown(&app), Some(Some(Place::Landscape)));
    frame(&mut app, &ctx, PORTRAIT, vec![escape()]);
    settle(&mut app, &ctx, PORTRAIT);
    assert_eq!(shown(&app), Some(None), "Esc steps back to this place");
    tap_text(&mut app, &ctx, PORTRAIT, other);
    settle(&mut app, &ctx, PORTRAIT);
    assert_eq!(shown(&app), Some(Some(Place::Landscape)));
    tap(&mut app, &ctx, PORTRAIT, pos2(PORTRAIT.x / 2.0, 40.0));
    settle(&mut app, &ctx, PORTRAIT);
    assert_eq!(shown(&app), None, "a tap outside closes the whole sheet");
}

#[test]
fn swap_in_the_sheet_on_this_game_leaves_the_shared_layout_alone() {
    let mut app = ds_app("layout-sheet-swap-level");
    app.emulator.set_rom_key_for_tests("nds-swap");
    let ctx = Context::default();
    settle(&mut app, &ctx, PORTRAIT);
    let (_, spots) = moved_touch(PORTRAIT);
    let id = app.ui.settings.save_layout(
        "Thumbs",
        LayoutConsole::Ds,
        Place::Portrait,
        Placement::default(),
        spots,
    );
    app.ui.settings.set_layout_pick(
        LayoutConsole::Ds,
        Place::Portrait,
        Pick::Saved(id),
        Scope::AllGames,
        false,
    );
    settle(&mut app, &ctx, PORTRAIT);
    let shared = app.ui.settings.saved_layout(id).cloned();
    let device = app.layout_device(&ctx);
    app.ui.layout_sheet = Some(LayoutSheet::default());
    app.ui
        .pick_in_sheet(&device, SheetPick::Games(Scope::ThisGame));
    app.ui.pick_in_sheet(&device, SheetPick::Swap);
    assert_eq!(app.ui.settings.saved_layout(id).cloned(), shared);
    let picked = ds_pick(&app, Place::Portrait);
    assert!(picked.game, "the swap wrote this game's pick");
    assert_ne!(picked.pick, Pick::Saved(id));
    settle(&mut app, &ctx, PORTRAIT);
    assert!(app.emulator.ds_screens.portrait.swapped);
    app.ui.settings.play_game(Some("nds-other"));
    assert_eq!(ds_pick(&app, Place::Portrait).pick, Pick::Saved(id));
}

#[test]
fn portrait_card_labels_keep_a_gap_and_never_touch() {
    let mut app = ds_app("layout-card-labels");
    app.ui.settings.save_layout(
        "My layout",
        LayoutConsole::Ds,
        Place::Portrait,
        Preset::Stacked.placement(),
        Spots::default(),
    );
    let ctx = Context::default();
    let output = open_sheet(&mut app, &ctx, PORTRAIT);
    let names = ["Stacked", "Top only", "Bottom only", "My layout"];
    let mut rects: Vec<(&str, Rect)> = names
        .iter()
        .map(|name| {
            let rect = text_rect(&output, name).unwrap_or_else(|| panic!("no {name} label"));
            (*name, rect)
        })
        .collect();
    rects.sort_by(|a, b| {
        let line = a.1.top().round().total_cmp(&b.1.top().round());
        line.then(a.1.left().total_cmp(&b.1.left()))
    });
    for pair in rects.windows(2) {
        let ((left, a), (right, b)) = (pair[0], pair[1]);
        if (a.center().y - b.center().y).abs() > 2.0 {
            continue;
        }
        assert!(
            b.left() - a.right() >= crate::app::widgets::screens_menu::LABEL_GAP - 0.5,
            "{left} {a:?} and {right} {b:?} touch"
        );
    }
}

#[test]
fn the_landscape_game_sits_left_of_the_open_side_sheet() {
    let mut app = ds_app("layout-landscape-room");
    let ctx = Context::default();
    settle(&mut app, &ctx, LANDSCAPE);
    app.ui.layout_sheet = Some(LayoutSheet::default());
    settle(&mut app, &ctx, LANDSCAPE);
    let sheet = super::sheet::side_rect(&ctx);
    let screens = app.emulator.placed_screens().to_vec();
    assert!(!screens.is_empty(), "the game shows");
    for screen in &screens {
        assert!(
            screen.right() <= sheet.left() + 0.5,
            "{screen:?} reaches under the sheet at {sheet:?}"
        );
    }
    let widest = screens.iter().map(|s| s.width()).fold(0.0, f32::max);
    assert!(widest > 120.0, "{screens:?} are too small to see");
}

fn landscape_picks(app: &mut Citrine) -> Vec<(String, Pick)> {
    let room = Rect::from_min_size(Pos2::ZERO, LANDSCAPE);
    let spots = Spots::from([
        (
            Item::Screen,
            Spot::of(
                Rect::from_min_size(pos2(90.0, 20.0), vec2(256.0, 192.0)),
                room,
            ),
        ),
        (
            Item::TouchScreen,
            Spot::of(
                Rect::from_min_size(pos2(620.0, 200.0), vec2(256.0, 192.0)),
                room,
            ),
        ),
    ]);
    let id = app.ui.settings.save_layout(
        "Spread",
        LayoutConsole::Ds,
        Place::Landscape,
        Preset::SideBySide.placement(),
        spots,
    );
    let mut picks: Vec<(String, Pick)> = [
        Preset::Stacked,
        Preset::SideBySide,
        Preset::BigTop,
        Preset::BigBottom,
        Preset::TopOnly,
        Preset::BottomOnly,
    ]
    .into_iter()
    .map(|p| (p.label().to_owned(), preset(p)))
    .collect();
    picks.push(("saved".to_owned(), Pick::Saved(id)));
    picks
}

#[test]
fn every_landscape_pick_sits_left_of_the_open_side_sheet_and_returns_after() {
    let mut app = ds_app("layout-landscape-picks");
    let ctx = Context::default();
    for (name, pick) in landscape_picks(&mut app) {
        app.ui.settings.set_layout_pick(
            LayoutConsole::Ds,
            Place::Landscape,
            pick,
            Scope::AllGames,
            false,
        );
        settle(&mut app, &ctx, LANDSCAPE);
        let before = app.emulator.placed_screens().to_vec();
        let shown = if matches!(pick, Pick::Preset(p) if p.one_screen) {
            1
        } else {
            2
        };
        assert_eq!(before.len(), shown, "{name}: {before:?}");
        app.ui.layout_sheet = Some(LayoutSheet::default());
        settle(&mut app, &ctx, LANDSCAPE);
        let sheet = super::sheet::side_rect(&ctx);
        let screens = app.emulator.placed_screens().to_vec();
        assert!(!screens.is_empty(), "{name}: the game shows");
        for screen in &screens {
            assert!(
                screen.right() <= sheet.left() + 0.5,
                "{name}: {screen:?} reaches under the sheet at {sheet:?}"
            );
        }
        let widest = screens.iter().map(|s| s.width()).fold(0.0, f32::max);
        let least = match pick {
            Pick::Saved(_) => {
                let room = app
                    .emulator
                    .spotted_room()
                    .expect("the saved layout's room");
                256.0 * (room.size() / LANDSCAPE).min_elem() - 0.5
            }
            _ => 120.0,
        };
        assert!(widest >= least, "{name}: {screens:?} are too small to see");
        app.ui.layout_sheet = None;
        settle(&mut app, &ctx, LANDSCAPE);
        assert_eq!(
            app.emulator.placed_screens(),
            before.as_slice(),
            "{name}: the game goes back after the sheet closes"
        );
    }
}

#[test]
fn the_landscape_side_sheet_keeps_the_buttons_and_toolbar_beside_it() {
    let mut app = ds_app("layout-landscape-buttons");
    let ctx = Context::default();
    app.ui.settings.set_layout_pick(
        LayoutConsole::Ds,
        Place::Landscape,
        preset(Preset::BigBottom),
        Scope::AllGames,
        false,
    );
    let output = settle(&mut app, &ctx, LANDSCAPE);
    assert!(text_rect(&output, "START").is_some(), "the buttons show");
    app.ui.layout_sheet = Some(LayoutSheet::default());
    let output = settle(&mut app, &ctx, LANDSCAPE);
    let sheet = super::sheet::side_rect(&ctx);
    for label in ["START", "SELECT", "L", "R", "A", "B", "X", "Y"] {
        let button = text_rect(&output, label).unwrap_or_else(|| panic!("{label} rests"));
        assert!(
            button.right() <= sheet.left(),
            "{label} at {button:?} sits under the sheet at {sheet:?}"
        );
    }
    let more = text_rect(&output, crate::icons::DOTS_THREE_VERTICAL).expect("the toolbar");
    assert!(more.right() <= sheet.left(), "{more:?} under {sheet:?}");
    let screens = app.emulator.placed_screens().to_vec();
    for screen in &screens {
        assert!(
            screen.top() >= more.bottom(),
            "{screen:?} under the toolbar at {more:?}"
        );
    }
}

#[test]
fn the_editor_bar_keeps_create_inside_the_screen_with_even_margins() {
    for width in [360.0, 412.0] {
        let size = vec2(width, 800.0);
        let mut app = ds_app(&format!("layout-bar-{width}"));
        let ctx = Context::default();
        settle(&mut app, &ctx, size);
        app.start_layout_edit();
        let output = settle(&mut app, &ctx, size);
        let screen = ctx.content_rect();
        let label = format!("{} {}", crate::icons::CHECK, app.ui.layout_done_label());
        let done = text_rect(&output, &label).expect("the done pill");
        let inset = super::strip::STRIP_INSET;
        assert!(
            done.right() <= screen.right() - inset - 4.0,
            "at {width}: {done:?} touches the edge of {screen:?}"
        );
        let first = text_rect(&output, "Opacity").expect("the slider keeps its word");
        let row: Vec<(String, f32, f32)> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => {
                    let laid = text.galley.rect.translate(text.pos.to_vec2());
                    ((laid.center().y - first.center().y).abs() < 12.0)
                        .then(|| (text.galley.text().to_owned(), laid.left(), laid.right()))
                }
                _ => None,
            })
            .collect();
        let word = row
            .iter()
            .find(|(text, ..)| text.trim() == "Opacity")
            .map_or(f32::NAN, |(_, left, _)| *left);
        assert!(
            word >= inset - 0.01,
            "at {width}: Opacity laid out at {word} in {row:?}"
        );
        assert!(
            row.iter().any(|(text, ..)| text.trim().ends_with('%')),
            "at {width}: no opacity value in {row:?}"
        );
        let track = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if (shape.rect.height() - 6.0).abs() < 0.5
                        && shape.rect.top() > first.bottom()
                        && shape.rect.bottom() < first.bottom() + 40.0 =>
                {
                    Some(shape.rect.width())
                }
                _ => None,
            })
            .fold(0.0, f32::max);
        assert!(track >= 80.0, "at {width}: the track is {track} long");
    }
}

#[test]
fn swap_shows_only_where_the_card_does_not_name_the_big_screen() {
    let mut app = ds_app("layout-swap-shows");
    let ctx = Context::default();
    settle(&mut app, &ctx, PORTRAIT);
    let device = app.layout_device(&ctx);
    for (pick, shows) in [
        (Preset::Stacked, true),
        (Preset::TopOnly, false),
        (Preset::BottomOnly, false),
    ] {
        app.ui.settings.set_layout_pick(
            LayoutConsole::Ds,
            Place::Portrait,
            preset(pick),
            Scope::AllGames,
            false,
        );
        app.ui.layout_sheet = None;
        let swapped = app.ui.layout_facts(&device).swapped;
        assert_eq!(swapped.is_some(), shows, "{pick:?}");
    }
    app.ui.settings.set_layout_pick(
        LayoutConsole::Ds,
        Place::Landscape,
        preset(Preset::BigBottom),
        Scope::AllGames,
        false,
    );
    app.ui.layout_sheet = Some(LayoutSheet::default());
    app.ui
        .pick_in_sheet(&device, SheetPick::Place(Place::Landscape));
    let output = settle(&mut app, &ctx, PORTRAIT);
    assert!(
        text_rect(&output, "Big bottom").is_some(),
        "landscape shows"
    );
    assert!(
        text_containing(&output, "Swap screens").is_none(),
        "Big bottom already names the big screen"
    );
}

#[test]
fn the_desktop_menu_lists_new_under_the_cards_and_lines_up_its_bars() {
    let mut app = ds_app("layout-desktop-rows");
    app.emulator.set_rom_key_for_tests("nds-rows");
    app.ui.settings.layout = LayoutMode::Desktop;
    app.open_tab(Tab::GameBoy);
    let ctx = Context::default();
    settle(&mut app, &ctx, DESKTOP);
    app.ui.layout_sheet = Some(LayoutSheet::default());
    let output = settle(&mut app, &ctx, DESKTOP);
    assert!(text_rect(&output, "New").is_none(), "no New card");
    let new = text_containing(&output, NEW_LAYOUT).expect("the New layout row");
    let edit = text_containing(&output, "Edit layout").expect("the edit row");
    let cards = ["Stacked", "Big bottom", "Top only"]
        .map(|name| text_rect(&output, name).expect(name).bottom())
        .into_iter()
        .fold(0.0, f32::max);
    assert!(new.top() > cards && new.bottom() <= edit.top(), "{new:?}");
    let title = crate::app::widgets::settings::scope::game_title(&app.ui.settings).to_owned();
    let window = text_rect(&output, "Window").expect("the mode's button");
    let game = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text().contains(title.as_str()) => {
                Some(text.galley.rect.translate(text.pos.to_vec2()))
            }
            _ => None,
        })
        .find(|rect| rect.bottom() < window.top() && rect.bottom() > window.top() - 40.0)
        .expect("the game's button");
    assert!(
        (game.left() - window.left()).abs() < 2.0,
        "{game:?} against {window:?}"
    );
    tap(&mut app, &ctx, DESKTOP, new.center());
    assert!(app.editing_layout(), "New layout starts the editor");
}

fn switch_tracks(ctx: &Context, output: &egui::FullOutput, inside: Rect) -> Vec<Rect> {
    let off = super::kit::switch_knob(&ctx.style().visuals);
    let knobs: Vec<Pos2> = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Circle(circle)
                if circle.fill == egui::Color32::WHITE || circle.fill == off =>
            {
                Some(circle.center)
            }
            _ => None,
        })
        .collect();
    output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if inside.contains_rect(shape.rect)
                    && knobs.iter().any(|knob| shape.rect.contains(*knob)) =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
        .collect()
}

fn side_sheet_at_its_end(app: &mut Citrine, ctx: &Context) -> egui::FullOutput {
    let side = super::sheet::side_rect(ctx);
    let wheel = Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: vec2(0.0, -2000.0),
        modifiers: Default::default(),
    };
    frame(
        app,
        ctx,
        LANDSCAPE,
        vec![Event::PointerMoved(side.center())],
    );
    frame(app, ctx, LANDSCAPE, vec![wheel]);
    for _ in 0..40 {
        frame(app, ctx, LANDSCAPE, vec![]);
    }
    frame(app, ctx, LANDSCAPE, vec![])
}

#[test]
fn the_landscape_sheet_switches_keep_the_left_gap_on_the_right() {
    let mut app = ds_app("layout-side-gaps");
    let ctx = Context::default();
    open_sheet(&mut app, &ctx, LANDSCAPE);
    let side = super::sheet::side_rect(&ctx);
    let output = side_sheet_at_its_end(&mut app, &ctx);
    let icon = text_rect(&output, crate::icons::LAPTOP).expect("the lid row's icon");
    let tracks = switch_tracks(&ctx, &output, side);
    assert!(!tracks.is_empty(), "no switch in {side:?}");
    let left = icon.center().x - super::kit::COMPACT_ICON / 2.0 - side.left();
    for track in &tracks {
        let right = side.right() - track.right();
        assert!(
            (right - left).abs() < 2.0,
            "{track:?}: right gap {right} against left gap {left}"
        );
    }
    let bars: Vec<Rect> = output
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if side.contains_rect(shape.rect)
                    && shape.rect.width() <= 12.0
                    && shape.rect.height() >= 16.0
                    && shape.rect.left() > side.center().x
                    && shape.fill.a() > 0 =>
            {
                Some(shape.rect)
            }
            _ => None,
        })
        .collect();
    assert!(!bars.is_empty(), "no scroll bar in {side:?}");
    let content = side.right() - super::kit::SIDE;
    for bar in bars {
        assert!(
            bar.left() >= content,
            "{bar:?} covers the rows up to {content}"
        );
    }
}

#[test]
fn the_name_dialog_dims_the_bottom_bar_and_the_bar_does_not_navigate() {
    let mut app = ds_app("layout-name-scrim");
    let ctx = Context::default();
    open_sheet(&mut app, &ctx, PORTRAIT);
    tap_text(&mut app, &ctx, PORTRAIT, "Top only");
    settle(&mut app, &ctx, PORTRAIT);
    tap_text(&mut app, &ctx, PORTRAIT, NEW_LAYOUT);
    tap_text(
        &mut app,
        &ctx,
        PORTRAIT,
        &format!("{} Create", crate::icons::CHECK),
    );
    assert!(app.naming_layout(), "Create asks for a name");
    let output = settle(&mut app, &ctx, PORTRAIT);
    let screen = ctx.content_rect();
    let scrim = super::kit::scrim(&ctx.style().visuals);
    let covered = output.shapes.iter().any(|clipped| {
        matches!(crate::app::widgets::confirm::flat_fill(&clipped.shape),
            Some((rect, fill)) if fill == scrim && rect.contains_rect(screen))
    });
    assert!(covered, "the scrim covers {screen:?}");
    let media = text_rect(&output, "Media").expect("the bar's Media");
    let question =
        text_rect(&output, crate::app::layout_edit::NAME_QUESTION).expect("the question");
    let sheet = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(shape)
                if shape.corner_radius.nw == 20
                    && shape.corner_radius.sw == 0
                    && shape.rect.contains_rect(question) =>
            {
                Some(shape.rect)
            }
            _ => None,
        });
    let sheet = sheet.expect("the name sheet");
    assert!(
        sheet.top() < media.top() && (sheet.bottom() - screen.bottom()).abs() < 1.0,
        "the sheet {sheet:?} sits at the foot of {screen:?}, over the bar"
    );
    tap(&mut app, &ctx, PORTRAIT, media.center());
    settle(&mut app, &ctx, PORTRAIT);
    assert!(app.mobile.nav.on_game(), "the bar took the tap");
    assert!(app.editing_layout(), "the editor stays open");
}

#[test]
fn every_card_shows_whole_and_new_is_a_row_at_412_360_and_in_landscape() {
    let small = vec2(360.0, 800.0);
    for (size, place) in [
        (PORTRAIT, Place::Portrait),
        (small, Place::Portrait),
        (LANDSCAPE, Place::Landscape),
    ] {
        let mut app = ds_app("layout-cards-whole");
        app.ui.settings.save_layout(
            "My layout",
            LayoutConsole::Ds,
            place,
            Preset::Stacked.placement(),
            Spots::new(),
        );
        let ctx = Context::default();
        let output = open_sheet(&mut app, &ctx, size);
        let (left, right) = if place == Place::Landscape {
            let side = super::sheet::side_rect(&ctx);
            (side.left(), side.right())
        } else {
            (0.0, ctx.content_rect().width())
        };
        let names = place
            .presets()
            .iter()
            .map(|preset| preset.label())
            .chain(["My layout"]);
        for name in names.clone() {
            let label = text_rect(&output, name).unwrap_or_else(|| panic!("{size:?}: {name}"));
            assert!(
                label.left() >= left + super::kit::SIDE - 0.5
                    && label.right() <= right - super::kit::SIDE + 0.5,
                "{size:?}: {name} at {label:?} leaves {left}..{right}"
            );
        }
        let output = if place == Place::Landscape {
            side_sheet_at_its_end(&mut app, &ctx)
        } else {
            output
        };
        assert!(text_rect(&output, "New").is_none(), "{size:?}: a New card");
        let new = text_containing(&output, NEW_LAYOUT).expect("the New layout row");
        let edit = text_containing(&output, "Edit layout").expect("the edit row");
        let names: Vec<&str> = names.collect();
        let cards = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if names.contains(&text.galley.text().trim()) => {
                    Some(text.visual_bounding_rect())
                }
                _ => None,
            })
            .filter(|label| label.bottom() <= edit.top())
            .map(|label| label.bottom())
            .fold(0.0, f32::max);
        assert!(
            new.top() > cards && new.bottom() <= edit.top(),
            "{size:?}: {new:?} between the cards at {cards} and {edit:?}"
        );
    }
}

#[test]
fn portrait_cards_keep_equal_outer_gaps_at_412_and_360() {
    for saved in [true, false] {
        for size in [PORTRAIT, vec2(360.0, 800.0)] {
            let mut app = ds_app("layout-card-spread");
            if saved {
                app.ui.settings.save_layout(
                    "My layout",
                    LayoutConsole::Ds,
                    Place::Portrait,
                    Preset::Stacked.placement(),
                    Spots::new(),
                );
            }
            let ctx = Context::default();
            let output = open_sheet(&mut app, &ctx, size);
            let label = text_rect(&output, "Stacked").expect("the first card");
            let new = text_containing(&output, NEW_LAYOUT).expect("the New layout row");
            let pictures: Vec<egui::Rect> = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(shape)
                        if shape.rect.bottom() <= label.top() + 1.0
                            && shape.rect.bottom() >= label.top() - 12.0
                            && shape.rect.height() > shape.rect.width() =>
                    {
                        Some(shape.rect)
                    }
                    _ => None,
                })
                .collect();
            let line = output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::LineSegment { points, .. }
                        if points[0].y == points[1].y
                            && points[0].y > label.bottom()
                            && points[0].y < new.top() =>
                    {
                        Some((points[0].x.min(points[1].x), points[0].x.max(points[1].x)))
                    }
                    _ => None,
                })
                .expect("the line under the cards");
            let first = pictures
                .iter()
                .map(|rect| rect.left())
                .fold(f32::MAX, f32::min);
            let last = pictures.iter().map(|rect| rect.right()).fold(0.0, f32::max);
            let left = first - line.0;
            let right = line.1 - last;
            assert!(
                (left - right).abs() < 1.5,
                "{size:?} saved {saved}: {left} on the left, {right} on the right, line {line:?}, cards {pictures:?}"
            );
            let tops: Vec<f32> = pictures.iter().map(|rect| rect.top()).collect();
            assert!(
                tops.iter().all(|top| (top - tops[0]).abs() < 1.0),
                "{size:?}: one line, {tops:?}"
            );
        }
    }
}

#[test]
fn new_and_edit_on_the_other_shape_wait_for_the_turn() {
    for (new, row) in [(true, NEW_LAYOUT), (false, "Edit layout")] {
        let mut app = ds_app("layout-turn");
        let ctx = Context::default();
        open_sheet(&mut app, &ctx, PORTRAIT);
        tap_text(&mut app, &ctx, PORTRAIT, "In landscape");
        tap_text(&mut app, &ctx, PORTRAIT, row);
        settle(&mut app, &ctx, PORTRAIT);
        let notice = crate::app::layout_sheet::turn_notice(Place::Landscape, new);
        assert!(
            app.toasts.is_open(notice),
            "{row}: {:?}",
            app.toasts.texts()
        );
        assert!(!app.editing_layout(), "{row}: the editor waits");
        settle(&mut app, &ctx, PORTRAIT);
        assert!(app.toasts.is_open(notice), "{row}: the notice stays");
        settle(&mut app, &ctx, LANDSCAPE);
        let edit = app
            .ui
            .layout_edit
            .as_ref()
            .expect("turning opens the editor");
        assert_eq!(edit.place, Place::Landscape, "{row}");
        assert_eq!(edit.start.new, new, "{row}");
        assert!(!app.toasts.is_open(notice), "{row}: the notice closes");
    }
}

#[test]
fn closing_the_turn_notice_stops_the_wait() {
    let mut app = ds_app("layout-turn-closed");
    let ctx = Context::default();
    open_sheet(&mut app, &ctx, PORTRAIT);
    tap_text(&mut app, &ctx, PORTRAIT, "In landscape");
    tap_text(&mut app, &ctx, PORTRAIT, NEW_LAYOUT);
    let notice = crate::app::layout_sheet::turn_notice(Place::Landscape, true);
    app.toasts.close(notice);
    settle(&mut app, &ctx, PORTRAIT);
    settle(&mut app, &ctx, LANDSCAPE);
    assert!(!app.editing_layout(), "a closed notice opens no editor");
    assert!(app.ui.layout_turn.is_none());
}

#[test]
fn the_landscape_row_shows_a_sideways_phone() {
    let turns = |output: &egui::FullOutput| -> Vec<f32> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.text() == crate::icons::DEVICE_MOBILE => {
                    Some(text.angle)
                }
                _ => None,
            })
            .collect()
    };
    let mut app = ds_app("layout-turned-icon");
    let ctx = Context::default();
    let output = open_sheet(&mut app, &ctx, PORTRAIT);
    let quarter = std::f32::consts::FRAC_PI_2;
    let angles = turns(&output);
    assert!(
        angles.len() == 1 && (angles[0] - quarter).abs() < 1e-3,
        "In landscape: {angles:?}"
    );
    let mut app = ds_app("layout-upright-icon");
    open_sheet(&mut app, &ctx, LANDSCAPE);
    let output = side_sheet_at_its_end(&mut app, &ctx);
    assert_eq!(turns(&output), vec![0.0], "In portrait");
}

#[test]
fn the_opacity_word_lines_up_with_the_pills_in_their_ink() {
    for size in [PORTRAIT, vec2(360.0, 800.0)] {
        let mut app = ds_app("layout-opacity-word");
        let ctx = Context::default();
        edit_from_sheet(&mut app, &ctx, size);
        let output = settle(&mut app, &ctx, size);
        let text = |wanted: &str| {
            output
                .shapes
                .iter()
                .find_map(|clipped| match &clipped.shape {
                    egui::Shape::Text(text) if text.galley.text().trim() == wanted => Some((
                        text.galley.rect.translate(text.pos.to_vec2()),
                        text.galley.job.sections[0].format.color,
                    )),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{size:?}: {wanted}"))
        };
        let (word, ink) = text("Opacity");
        let (cancel, pill_ink) = text(crate::app::layout_edit::CANCEL);
        let pill = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape)
                    if shape.rect.contains_rect(cancel)
                        && shape.rect.height() >= 30.0
                        && shape.rect.height() <= 60.0 =>
                {
                    Some(shape.rect)
                }
                _ => None,
            })
            .min_by(|a, b| a.area().total_cmp(&b.area()))
            .expect("the Cancel pill");
        assert!(
            (word.top() - pill.top()).abs() <= 1.5,
            "{size:?}: Opacity at {word:?}, the pill at {pill:?}"
        );
        assert_eq!(ink, pill_ink, "{size:?}: the word's ink");
    }
}

#[test]
fn the_editor_bar_keeps_the_same_gap_above_its_pills_at_412_and_360() {
    let gaps: Vec<f32> = [PORTRAIT, vec2(360.0, 800.0)]
        .into_iter()
        .map(|size| {
            let mut app = ds_app("layout-bar-gap");
            let ctx = Context::default();
            edit_from_sheet(&mut app, &ctx, size);
            let output = settle(&mut app, &ctx, size);
            let cancel = text_rect(&output, crate::app::layout_edit::CANCEL).expect("Cancel");
            let pill = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(shape)
                        if shape.rect.contains_rect(cancel)
                            && shape.rect.height() >= 30.0
                            && shape.rect.height() <= 60.0 =>
                    {
                        Some(shape.rect)
                    }
                    _ => None,
                })
                .min_by(|a, b| a.area().total_cmp(&b.area()))
                .expect("the Cancel pill");
            pill.top() - ctx.content_rect().top()
        })
        .collect();
    assert!(
        gaps.iter().all(|gap| *gap >= 2.4),
        "gaps above the pills: {gaps:?}"
    );
    assert!((gaps[0] - gaps[1]).abs() <= 0.5, "412 and 360: {gaps:?}");
}

#[test]
fn the_screens_stay_in_place_between_play_and_the_editor_at_412_and_360() {
    for size in [PORTRAIT, vec2(360.0, 800.0)] {
        let mut app = ds_app("layout-editor-screens");
        app.emulator.set_rom_key_for_tests("nds-screens");
        let ctx = Context::default();
        settle(&mut app, &ctx, size);
        let play = app.emulator.placed_screens().to_vec();
        assert_eq!(play.len(), 2, "{size:?}: both screens in play");
        edit_from_sheet(&mut app, &ctx, size);
        settle(&mut app, &ctx, size);
        assert!(
            app.mobile.hidden.on(),
            "{size:?}: the editor keeps the frame hidden"
        );
        assert_eq!(
            app.emulator.placed_screens(),
            play.as_slice(),
            "{size:?}: the editor moves the screens"
        );
        tap_text(&mut app, &ctx, size, crate::app::layout_edit::CANCEL);
        settle(&mut app, &ctx, size);
        assert!(!app.editing_layout());
        assert_eq!(
            app.emulator.placed_screens(),
            play.as_slice(),
            "{size:?}: Cancel moves the screens"
        );
    }
}

#[test]
fn the_desktop_menu_never_leaves_one_card_on_its_last_line() {
    let mut app = ds_app("layout-desktop-grid");
    app.emulator.set_rom_key_for_tests("nds-grid");
    app.ui.settings.layout = LayoutMode::Desktop;
    app.ui.settings.save_layout(
        "My layout",
        LayoutConsole::Ds,
        Place::Window,
        Preset::Stacked.placement(),
        Spots::new(),
    );
    app.open_tab(Tab::GameBoy);
    let ctx = Context::default();
    settle(&mut app, &ctx, DESKTOP);
    app.ui.layout_sheet = Some(LayoutSheet::default());
    let output = settle(&mut app, &ctx, DESKTOP);
    let names = Place::Window
        .presets()
        .iter()
        .map(|preset| preset.label())
        .chain(["My layout"]);
    let mut lines: Vec<(f32, usize)> = Vec::new();
    for name in names {
        let top = text_rect(&output, name).expect(name).top();
        match lines.iter_mut().find(|(at, _)| (at - top).abs() < 2.0) {
            Some((_, count)) => *count += 1,
            None => lines.push((top, 1)),
        }
    }
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(
        lines.len() > 1 && lines.last().is_some_and(|(_, count)| *count > 1),
        "{lines:?}"
    );
    for count in 1..=12 {
        let columns = crate::app::widgets::screens_menu::grid_columns(count);
        assert!(
            count <= columns || count % columns != 1,
            "{count} cards in {columns} columns"
        );
    }
}

fn default_faults(app: &Citrine, ctx: &Context, size: Vec2) -> Vec<String> {
    let screen = ctx.content_rect();
    let touch = touch(app);
    let mut faults = Vec::new();
    for shown in app.emulator.placed_screens() {
        if !screen.contains_rect(shown.shrink(0.5)) {
            faults.push(format!("at {size:?} {shown:?} leaves {screen:?}"));
        }
    }
    if app.emulator.placed_screens().len() != 2 {
        faults.push(format!("at {size:?} both screens show"));
    }
    for target in &app.ui.touch_targets {
        let over = target.intersect(touch);
        if over.width() > 0.5 && over.height() > 0.5 {
            faults.push(format!(
                "at {size:?} {target:?} covers the touch screen {touch:?}"
            ));
        }
    }
    faults
}

#[test]
fn a_ds_game_opens_playable_in_both_orientations() {
    let mut faults = Vec::new();
    for size in [PORTRAIT, LANDSCAPE, vec2(360.0, 800.0), vec2(800.0, 360.0)] {
        let mut app = ds_app("layout-defaults");
        let ctx = Context::default();
        settle(&mut app, &ctx, size);
        faults.extend(default_faults(&app, &ctx, size));
    }
    assert!(faults.is_empty(), "{faults:#?}");
}

#[test]
fn a_ds_game_in_landscape_opens_big_top_bare_and_the_eye_brings_the_frame_back() {
    for size in [LANDSCAPE, vec2(800.0, 360.0)] {
        let mut app = ds_app("layout-big-top");
        let ctx = Context::default();
        settle(&mut app, &ctx, size);
        assert!(app.mobile.hidden.on(), "at {size:?} the frame gives way");
        let touch = touch(&app);
        let top = top_screen(&app);
        assert!(top.width() > touch.width() * 1.5, "{top:?} {touch:?}");
        assert!(top.left() < touch.left() && (top.top() - touch.top()).abs() < 1.0);
        let eye = app.mobile.hidden.eye.expect("the eye shows");
        for taken in app
            .emulator
            .placed_screens()
            .iter()
            .chain(&app.ui.touch_targets)
        {
            assert!(
                !eye.intersects(*taken),
                "at {size:?} {eye:?} over {taken:?}"
            );
        }
        tap(&mut app, &ctx, size, eye.center());
        settle(&mut app, &ctx, size);
        assert!(!app.mobile.hidden.on(), "the eye shows the frame");
        settle(&mut app, &ctx, PORTRAIT);
        settle(&mut app, &ctx, size);
        assert!(!app.mobile.hidden.on(), "the frame stays after a turn");
    }
}

#[test]
fn a_ds_game_in_portrait_opens_stacked_rows_bare_and_the_eye_brings_the_frame_back() {
    for size in [PORTRAIT, vec2(360.0, 800.0)] {
        let mut app = ds_app("layout-stacked-rows");
        let ctx = Context::default();
        settle(&mut app, &ctx, size);
        assert!(app.mobile.hidden.on(), "at {size:?} the frame gives way");
        let touch = touch(&app);
        let top = top_screen(&app);
        assert!(top.width() > size.x * 0.9, "at {size:?} {top:?}");
        assert!(
            (top.width() - touch.width()).abs() < 1.0,
            "{top:?} {touch:?}"
        );
        assert!(
            (top.bottom() - touch.top()).abs() < 1.0,
            "{top:?} {touch:?}"
        );
        for button in &app.ui.touch_targets {
            assert!(
                button.top() >= touch.bottom() - 1.0,
                "at {size:?} {button:?} over the screens"
            );
        }
        let eye = app.mobile.hidden.eye.expect("the eye shows");
        for taken in app
            .emulator
            .placed_screens()
            .iter()
            .chain(&app.ui.touch_targets)
        {
            assert!(
                !eye.intersects(*taken),
                "at {size:?} {eye:?} over {taken:?}"
            );
        }
        tap(&mut app, &ctx, size, eye.center());
        settle(&mut app, &ctx, size);
        assert!(!app.mobile.hidden.on(), "the eye shows the frame");
    }
}

#[test]
fn every_built_in_keeps_the_buttons_off_the_touch_screen() {
    let mut faults = Vec::new();
    for (size, place) in [(PORTRAIT, Place::Portrait), (LANDSCAPE, Place::Landscape)] {
        for built_in in place
            .presets()
            .iter()
            .filter(|p| !matches!(p, Preset::TopOnly | Preset::BottomOnly))
        {
            let mut app = ds_app("layout-built-ins");
            app.ui.settings.set_layout_pick(
                LayoutConsole::Ds,
                place,
                preset(*built_in),
                Scope::AllGames,
                false,
            );
            let ctx = Context::default();
            settle(&mut app, &ctx, size);
            faults.extend(
                default_faults(&app, &ctx, size)
                    .into_iter()
                    .map(|fault| format!("{built_in:?}: {fault}")),
            );
        }
    }
    assert!(faults.is_empty(), "{faults:#?}");
}

fn restarted(app: &Citrine, name: &str) -> Citrine {
    let stored = serde_json::to_string(&app.ui.settings).expect("saves");
    let mut again = ds_app(name);
    again.ui.settings = serde_json::from_str(&stored).expect("loads");
    again
}

#[test]
fn edit_saves_the_built_in_in_place_and_it_stays_after_a_restart() {
    let mut app = ds_app("layout-edit-in-place");
    let ctx = Context::default();
    let shown = ds_pick(&app, Place::Portrait).pick;
    edit_from_sheet(&mut app, &ctx, PORTRAIT);
    assert_eq!(app.ui.layout_done_label(), "Save");
    let before = touch(&app);
    drag(
        &mut app,
        &ctx,
        PORTRAIT,
        before.center(),
        before.center() + vec2(0.0, -40.0),
    );
    settle(&mut app, &ctx, PORTRAIT);
    let moved = touch(&app);
    assert_ne!(moved, before, "the drag moves the touch screen");
    save_edit(&mut app, &ctx, PORTRAIT);
    assert_eq!(ds_pick(&app, Place::Portrait).pick, shown, "the pick stays");
    assert_eq!(
        app.ui
            .settings
            .saved_layouts_in(LayoutConsole::Ds, Place::Portrait)
            .count(),
        0,
        "no new layout card"
    );
    let builtin = crate::app::ui_state::layout_picks::Builtin::of(shown).expect("a built-in");
    let kept = app
        .ui
        .settings
        .builtin_edit(LayoutConsole::Ds, Place::Portrait, builtin)
        .expect("the change is kept with the built-in");
    assert_eq!(kept.name, builtin.label(), "under the built-in's name");
    settle(&mut app, &ctx, PORTRAIT);
    assert_eq!(touch(&app), moved);

    let mut app = restarted(&app, "layout-edit-in-place-again");
    settle(&mut app, &ctx, PORTRAIT);
    assert_eq!(touch(&app), moved, "the change is there after a restart");
}

#[test]
fn reset_on_an_edited_built_in_asks_and_brings_it_back() {
    let mut app = ds_app("layout-reset-built-in");
    app.mobile.hidden.keep_shown();
    let ctx = Context::default();
    settle(&mut app, &ctx, PORTRAIT);
    let plain = touch(&app);
    let shown = ds_pick(&app, Place::Portrait).pick;
    let name = app.ui.settings.pick_name(shown);
    edit_from_sheet(&mut app, &ctx, PORTRAIT);
    drag(
        &mut app,
        &ctx,
        PORTRAIT,
        plain.center(),
        plain.center() + vec2(0.0, -40.0),
    );
    save_edit(&mut app, &ctx, PORTRAIT);
    settle(&mut app, &ctx, PORTRAIT);
    assert_ne!(touch(&app), plain);

    let output = open_sheet(&mut app, &ctx, PORTRAIT);
    let card = text_rect(&output, &name).expect("the built-in's card");
    right_click(&mut app, &ctx, PORTRAIT, card.center());
    tap_text(&mut app, &ctx, PORTRAIT, screens_menu_reset());
    let output = settle(&mut app, &ctx, PORTRAIT);
    let question = crate::app::layout_sheet::reset_question(&name);
    let first = question.split("? ").next().expect("a question");
    assert!(
        text_containing(&output, first).is_some(),
        "Reset asks first"
    );
    let builtin = crate::app::ui_state::layout_picks::Builtin::of(shown).expect("a built-in");
    assert!(
        app.ui
            .settings
            .builtin_edit(LayoutConsole::Ds, Place::Portrait, builtin)
            .is_some()
    );
    tap_text(&mut app, &ctx, PORTRAIT, screens_menu_reset());
    assert!(
        app.ui
            .settings
            .builtin_edit(LayoutConsole::Ds, Place::Portrait, builtin)
            .is_none(),
        "Reset drops the change"
    );
    app.ui.layout_sheet = None;
    settle(&mut app, &ctx, PORTRAIT);
    assert_eq!(touch(&app), plain, "the built-in is back");
}

fn screens_menu_reset() -> &'static str {
    crate::app::widgets::screens_menu::RESET
}

#[test]
fn reset_in_the_editor_then_save_brings_the_built_in_back() {
    let mut app = ds_app("layout-editor-reset");
    let ctx = Context::default();
    settle(&mut app, &ctx, LANDSCAPE);
    let plain = touch(&app);
    edit_from_sheet(&mut app, &ctx, LANDSCAPE);
    drag(
        &mut app,
        &ctx,
        LANDSCAPE,
        plain.center(),
        plain.center() - vec2(60.0, 0.0),
    );
    save_edit(&mut app, &ctx, LANDSCAPE);
    edit_from_sheet(&mut app, &ctx, LANDSCAPE);
    assert_eq!(app.ui.layout_reset_button(), ResetButton::Reset);
    app.ui.press_layout_reset();
    save_edit(&mut app, &ctx, LANDSCAPE);
    let shown = ds_pick(&app, Place::Landscape).pick;
    let builtin = crate::app::ui_state::layout_picks::Builtin::of(shown).expect("a built-in");
    assert!(
        app.ui
            .settings
            .builtin_edit(LayoutConsole::Ds, Place::Landscape, builtin)
            .is_none()
    );
    settle(&mut app, &ctx, LANDSCAPE);
    assert_eq!(touch(&app), plain);
}

#[test]
fn swap_on_an_edited_built_in_trades_the_moved_screens() {
    let mut app = ds_app("layout-swap-built-in");
    let ctx = Context::default();
    edit_from_sheet(&mut app, &ctx, PORTRAIT);
    let from = touch(&app);
    drag(
        &mut app,
        &ctx,
        PORTRAIT,
        from.center(),
        from.center() + vec2(0.0, -40.0),
    );
    save_edit(&mut app, &ctx, PORTRAIT);
    settle(&mut app, &ctx, PORTRAIT);
    let moved = touch(&app);
    let top = top_screen(&app);
    app.screens_action(crate::app::ui_state::ds_screens::ScreensAction::Swap);
    settle(&mut app, &ctx, PORTRAIT);
    assert_eq!(touch(&app), top, "the touch screen takes the top's place");
    assert_eq!(
        top_screen(&app),
        moved,
        "and the top screen the moved place"
    );
}

#[test]
fn game_boy_and_gba_edits_save_in_place_too() {
    for (name, rom) in [
        (
            "ucity.gbc",
            &include_bytes!("../../../homebrew/ucity.gbc")[..],
        ),
        (
            "pentris.gba",
            &include_bytes!("../../../homebrew/gba/pentris.gba.br")[..],
        ),
    ] {
        let mut app = phone_app(&format!("layout-standard-{name}"));
        let data = crate::storage::decompress(rom).unwrap_or_else(|| rom.to_vec());
        app.events.load_rom_data(data);
        let ctx = Context::default();
        settle(&mut app, &ctx, PORTRAIT);
        app.resume_game();
        settle(&mut app, &ctx, PORTRAIT);
        let console = LayoutConsole::of(app.emulator.played_kind());
        assert_ne!(console, LayoutConsole::Ds, "{name}");
        app.start_layout_edit_with(crate::app::layout_edit::EditStart::CURRENT);
        assert!(app.editing_layout(), "{name}");
        assert_eq!(app.ui.layout_done_label(), "Save", "{name}");
        let room = ctx.content_rect();
        let spot = Spot::of(
            Rect::from_min_size(room.min + vec2(20.0, 80.0), vec2(200.0, 180.0)),
            room,
        );
        app.ui
            .settings
            .layouts
            .set(console, LayoutView::Portrait, Item::Screen, spot);
        app.done_layout_edit();
        assert!(!app.naming_layout(), "{name}: no name dialog");
        assert!(!app.editing_layout(), "{name}");
        let kept = app
            .ui
            .settings
            .builtin_edit(
                console,
                Place::Portrait,
                crate::app::ui_state::layout_picks::Builtin::Standard,
            )
            .unwrap_or_else(|| panic!("{name}: Standard keeps the change"));
        assert_eq!(kept.spots.get(&Item::Screen), Some(&spot), "{name}");
        assert_eq!(
            app.ui.settings.layout_pick(console, Place::Portrait).pick,
            Pick::Standard,
            "{name}"
        );
    }
}
