use egui::{Rect, Vec2, pos2};

pub mod cells;
pub mod chip;
pub mod editor;
pub mod online;
pub mod overlay;
pub mod paint;

pub const SIZES: std::ops::RangeInclusive<f32> = 0.75..=2.0;
pub const DEFAULT_OPACITY: f32 = 0.35;
pub const INSET: f32 = 12.0;
pub const TRACKER: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Value {
    Fps,
    Low,
    Speed,
    Graph,
    FrameTime,
    Late,
    Game,
    App,
    Gpu,
    SoundBuffer,
    SoundGaps,
    Jit,
    Resolution,
    Played,
    Refresh,
    Clock,
    Ping,
    Delay,
    Connection,
    Waits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Rate,
    Smoothness,
    Load,
    Sound,
    Game,
    Device,
    Online,
}

impl Group {
    pub const ALL: [Group; 7] = [
        Group::Rate,
        Group::Smoothness,
        Group::Load,
        Group::Sound,
        Group::Game,
        Group::Device,
        Group::Online,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Group::Rate => "Frame rate",
            Group::Smoothness => "Smoothness",
            Group::Load => "Load",
            Group::Sound => "Sound",
            Group::Game => "Game",
            Group::Device => "Device",
            Group::Online => "Online",
        }
    }
}

impl Value {
    pub const ALL: [Value; 20] = [
        Value::Fps,
        Value::Low,
        Value::Speed,
        Value::Graph,
        Value::FrameTime,
        Value::Late,
        Value::Game,
        Value::App,
        Value::Gpu,
        Value::SoundBuffer,
        Value::SoundGaps,
        Value::Jit,
        Value::Resolution,
        Value::Played,
        Value::Refresh,
        Value::Clock,
        Value::Ping,
        Value::Delay,
        Value::Connection,
        Value::Waits,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Value::Fps => "FPS",
            Value::Low => "1% low",
            Value::Speed => "Speed",
            Value::Graph => "Frame graph",
            Value::FrameTime => "Frame time",
            Value::Late => "Late frames",
            Value::Game => "Game",
            Value::App => "App",
            Value::Gpu => "3D GPU",
            Value::SoundBuffer => "Sound buffer",
            Value::SoundGaps => "Sound gaps",
            Value::Jit => "JIT",
            Value::Resolution => "3D resolution",
            Value::Played => "Played",
            Value::Refresh => "Refresh",
            Value::Clock => "Clock",
            Value::Ping => "Ping",
            Value::Delay => "Input delay",
            Value::Connection => "Connection",
            Value::Waits => "Waits",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Value::Fps => "Game frames shown each second",
            Value::Low => "Frames per second over the slowest 1% of the last 10 seconds",
            Value::Speed => "How fast the game runs next to the real console",
            Value::Graph => "The last 60 frames. Red bars are frames that came late",
            Value::FrameTime => "Time between two game frames",
            Value::Late => "Frames that came late in the last 5 seconds",
            Value::Game => "Time the console takes to make one frame",
            Value::App => "Time the app takes to draw one update",
            Value::Gpu => "Time the graphics card takes for the 3D picture",
            Value::SoundBuffer => "Sound waiting to play",
            Value::SoundGaps => "Silence from an empty sound buffer in the last 5 seconds",
            Value::Jit => "Whether the JIT recompiler runs this game",
            Value::Resolution => "The size the 3D picture is drawn at",
            Value::Played => "Time played since the game started",
            Value::Refresh => "How often the screen refreshes",
            Value::Clock => "The time on this device",
            Value::Ping => {
                "Time a message takes to the other player and back. Without a game together, to the server and back"
            }
            Value::Delay => {
                "How long the game holds each button press so both players see it on the same frame"
            }
            Value::Connection => {
                "Direct when the game runs straight between the players, Relay when it goes through the server"
            }
            Value::Waits => "Times the game paused for the other player in the last minute",
        }
    }

    pub fn group(self) -> Group {
        match self {
            Value::Fps | Value::Low | Value::Speed => Group::Rate,
            Value::Graph | Value::FrameTime | Value::Late => Group::Smoothness,
            Value::Game | Value::App | Value::Gpu => Group::Load,
            Value::SoundBuffer | Value::SoundGaps => Group::Sound,
            Value::Jit | Value::Resolution | Value::Played => Group::Game,
            Value::Refresh | Value::Clock => Group::Device,
            Value::Ping | Value::Delay | Value::Connection | Value::Waits => Group::Online,
        }
    }

    pub fn ds_only(self) -> bool {
        matches!(self, Value::Gpu | Value::Jit | Value::Resolution)
    }
}

pub fn addable(have: &[Value], ds: bool, group: Group) -> impl Iterator<Item = Value> + '_ {
    Value::ALL
        .into_iter()
        .filter(move |value| value.group() == group)
        .filter(move |value| ds || !value.ds_only())
        .filter(move |value| !have.contains(value))
}

pub fn shown_values(values: &[Value], ds: bool) -> impl Iterator<Item = Value> + '_ {
    values
        .iter()
        .copied()
        .filter(move |value| ds || !value.ds_only())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Direction {
    #[default]
    Row,
    Column,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Corner {
    #[default]
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    fn left(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::BottomLeft)
    }

    fn top(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::TopRight)
    }

    fn of(left: bool, top: bool) -> Corner {
        match (left, top) {
            (true, true) => Corner::TopLeft,
            (false, true) => Corner::TopRight,
            (true, false) => Corner::BottomLeft,
            (false, false) => Corner::BottomRight,
        }
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Hud {
    pub id: u32,
    pub direction: Direction,
    pub values: Vec<Value>,
    pub corner: Corner,
    pub offset: [f32; 2],
    pub size: f32,
}

impl Default for Hud {
    fn default() -> Self {
        Self {
            id: 0,
            direction: Direction::Row,
            values: vec![Value::Fps, Value::Graph, Value::Low],
            corner: Corner::TopLeft,
            offset: [INSET, INSET],
            size: 1.0,
        }
    }
}

impl Hud {
    pub fn scale(&self) -> f32 {
        self.size.clamp(*SIZES.start(), *SIZES.end())
    }

    pub fn rect(&self, area: Rect, size: Vec2) -> Rect {
        let [dx, dy] = self.offset;
        let x = if self.corner.left() {
            area.left() + dx
        } else {
            area.right() - dx - size.x
        };
        let y = if self.corner.top() {
            area.top() + dy
        } else {
            area.bottom() - dy - size.y
        };
        let x = x.min(area.right() - size.x).max(area.left());
        let y = y.min(area.bottom() - size.y).max(area.top());
        Rect::from_min_size(pos2(x, y), size)
    }

    pub fn clear_of(&self, rect: Rect, taken: &[Rect], area: Rect) -> Rect {
        let gap = INSET / 2.0;
        let mut rect = rect;
        for _ in 0..taken.len() {
            let Some(hit) = taken
                .iter()
                .find(|other| other.expand(gap / 2.0).intersects(rect))
            else {
                break;
            };
            let y = if self.corner.top() {
                hit.bottom() + gap
            } else {
                hit.top() - gap - rect.height()
            };
            if y < area.top() || y + rect.height() > area.bottom() {
                break;
            }
            rect = rect.translate(egui::vec2(0.0, y - rect.top()));
        }
        rect
    }

    pub fn place(&mut self, area: Rect, rect: Rect) {
        let left = rect.center().x <= area.center().x;
        let top = rect.center().y <= area.center().y;
        self.corner = Corner::of(left, top);
        let dx = if left {
            rect.left() - area.left()
        } else {
            area.right() - rect.right()
        };
        let dy = if top {
            rect.top() - area.top()
        } else {
            area.bottom() - rect.bottom()
        };
        self.offset = [dx.max(0.0).round(), dy.max(0.0).round()];
    }

    pub fn summary(&self, ds: bool) -> String {
        let names: Vec<&str> = shown_values(&self.values, ds).map(Value::name).collect();
        if names.is_empty() {
            "No values".into()
        } else {
            names.join(", ")
        }
    }

    pub fn move_value(&mut self, from: usize, to: usize) {
        if from >= self.values.len() || to >= self.values.len() || from == to {
            return;
        }
        let value = self.values.remove(from);
        self.values.insert(to, value);
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct HudSettings {
    pub shown: bool,
    pub opacity: f32,
    pub huds: Vec<Hud>,
    pub tracker: Option<Hud>,
}

impl Default for HudSettings {
    fn default() -> Self {
        Self {
            shown: true,
            opacity: DEFAULT_OPACITY,
            huds: Vec::new(),
            tracker: None,
        }
    }
}

impl HudSettings {
    pub fn opacity(&self) -> f32 {
        self.opacity.clamp(0.0, 1.0)
    }

    pub fn add(&mut self) -> u32 {
        let id = self
            .huds
            .iter()
            .map(|hud| hud.id)
            .max()
            .map_or(1, |id| id + 1);
        let below = self
            .huds
            .iter()
            .filter(|hud| hud.corner == Corner::TopLeft)
            .map(|hud| hud.offset[1] + 40.0)
            .fold(INSET, f32::max);
        self.huds.push(Hud {
            id,
            offset: [INSET, below],
            ..Hud::default()
        });
        id
    }

    pub fn get(&self, id: u32) -> Option<&Hud> {
        if id == TRACKER {
            return self.tracker.as_ref();
        }
        self.huds.iter().find(|hud| hud.id == id)
    }

    pub fn get_mut(&mut self, id: u32) -> Option<&mut Hud> {
        if id == TRACKER {
            return Some(self.tracker());
        }
        self.huds.iter_mut().find(|hud| hud.id == id)
    }

    pub fn tracker(&mut self) -> &mut Hud {
        let corner = self.free_corner();
        self.tracker.get_or_insert_with(|| Hud {
            id: TRACKER,
            values: Vec::new(),
            corner,
            ..Hud::default()
        })
    }

    pub fn home_corner(&self, id: u32) -> Corner {
        if id == TRACKER {
            self.free_corner()
        } else {
            Corner::TopLeft
        }
    }

    fn free_corner(&self) -> Corner {
        [
            Corner::TopRight,
            Corner::BottomRight,
            Corner::BottomLeft,
            Corner::TopLeft,
        ]
        .into_iter()
        .find(|corner| self.huds.iter().all(|hud| hud.corner != *corner))
        .unwrap_or(Corner::TopRight)
    }

    pub fn remove(&mut self, id: u32) -> Option<(usize, Hud)> {
        let index = self.huds.iter().position(|hud| hud.id == id)?;
        Some((index, self.huds.remove(index)))
    }

    pub fn restore(&mut self, index: usize, hud: Hud) {
        if self.get(hud.id).is_some() {
            return;
        }
        let index = index.min(self.huds.len());
        self.huds.insert(index, hud);
    }

    pub fn visible(&self, trackers: bool) -> bool {
        self.shown && (trackers || !self.huds.is_empty())
    }
}

#[derive(Default)]
pub struct HudState {
    pub editor: Option<u32>,
    pub drag: Option<overlay::Drag>,
    pub cells: cells::Cache,
    pub placed: Vec<(u32, egui::Rect)>,
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut egui::Ui,
    place: &overlay::Place,
    state: &mut crate::app::ui_state::UiState,
    emulator: &crate::emulator::Emulator,
    stats: &crate::stats::Stats,
    play: &crate::play::Play,
    online: &mut crate::online::Online,
    trackers: &[(String, String)],
) -> overlay::Outcome {
    if !state.settings.hud.visible(!trackers.is_empty()) {
        state.hud.drag = None;
        state.hud.editor = None;
        state.hud.placed.clear();
        return overlay::Outcome::default();
    }
    let facts = cells::gather(
        cells::Sources {
            frame_log: &mut state.frame_log,
            load: state.performance.view(),
            emulator,
            stats,
            play,
            online,
        },
        &mut state.hud.cells,
        &state.settings.hud,
    );
    let outcome = overlay::show(
        ui,
        place,
        &facts,
        trackers,
        &mut state.settings.hud,
        &mut state.hud,
    );
    if outcome.changed {
        state.settings.dirty = true;
    }
    outcome
}

impl crate::app::Citrine {
    pub(super) fn game_hud(
        &mut self,
        ui: &mut egui::Ui,
        area: egui::Rect,
        picture: Option<egui::Rect>,
        touch: bool,
        keep_below: Option<f32>,
    ) {
        let engine = self.play.session().is_some_and(|s| s.runs_engine());
        if !self.emulator.shows_console() && !engine {
            return;
        }
        let place = overlay::Place {
            area,
            picture,
            screens: self.emulator.placed_screens().to_vec(),
            touch,
            editing: self.editing_layout(),
            keep_below,
        };
        let boards = &self.achievements.boards;
        let trackers = if place.editing {
            boards.layout_lines()
        } else {
            boards.tracker_lines()
        };
        let outcome = show(
            ui,
            &place,
            &mut self.ui,
            &self.emulator,
            &self.stats,
            &self.play,
            &mut self.online,
            &trackers,
        );
        if let Some(id) = outcome.remove {
            self.remove_hud(id);
        }
        if let Some(id) = outcome.edit_sheet {
            self.mobile.hud_sheet = Some(crate::app::mobile::hud::HudSheet::Edit(id));
        }
    }

    pub(super) fn remove_hud(&mut self, id: u32) {
        let Some((index, hud)) = self.ui.settings.hud.remove(id) else {
            return;
        };
        self.ui.settings.dirty = true;
        if self.ui.hud.editor == Some(id) {
            self.ui.hud.editor = None;
        }
        self.toasts.info("HUD removed").button(
            "Undo",
            crate::app::events::AppEvent::RestoreHud { index, hud },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::editor::{self, drop_index};
    use super::overlay::{Place, snap};
    use super::*;
    use egui::{Rect, pos2, vec2};

    fn area(width: f32, height: f32) -> Rect {
        Rect::from_min_size(pos2(0.0, 0.0), vec2(width, height))
    }

    fn facts() -> cells::Facts {
        cells::Facts {
            readout: None,
            speed: None,
            strip: Vec::new(),
            gpu: cells::Gpu::Unmeasured,
            ds: true,
            jit: false,
            resolution: String::new(),
            played: None,
            clock: String::new(),
            link: online::Link::default(),
        }
    }

    fn restart(settings: &HudSettings) -> HudSettings {
        serde_json::from_str(&serde_json::to_string(settings).unwrap()).unwrap()
    }

    #[test]
    fn a_dragged_hud_keeps_its_place_across_a_restart_and_a_resize() {
        let mut settings = HudSettings::default();
        let id = settings.add();
        let size = vec2(120.0, 30.0);
        let small = area(800.0, 600.0);
        let dropped = Rect::from_min_size(pos2(650.0, 540.0), size);
        settings.get_mut(id).unwrap().place(small, dropped);
        let settings = restart(&settings);
        let hud = settings.get(id).unwrap();
        assert_eq!(hud.corner, Corner::BottomRight);
        assert_eq!(hud.rect(small, size), dropped);
        let big = area(1200.0, 900.0);
        let moved = hud.rect(big, size);
        assert_eq!(big.right() - moved.right(), small.right() - dropped.right());
        assert_eq!(
            big.bottom() - moved.bottom(),
            small.bottom() - dropped.bottom()
        );
    }

    #[test]
    fn a_hud_never_leaves_a_shrunk_area() {
        let hud = Hud {
            offset: [500.0, 400.0],
            ..Hud::default()
        };
        let room = area(300.0, 200.0);
        let rect = hud.rect(room, vec2(120.0, 30.0));
        assert!(room.contains_rect(rect));
    }

    #[test]
    fn a_reorder_and_a_size_survive_a_restart() {
        let mut settings = HudSettings::default();
        let id = settings.add();
        let hud = settings.get_mut(id).unwrap();
        hud.move_value(0, 2);
        hud.size = 1.5;
        hud.direction = Direction::Column;
        let settings = restart(&settings);
        let hud = settings.get(id).unwrap();
        assert_eq!(hud.values, vec![Value::Graph, Value::Low, Value::Fps]);
        assert_eq!(hud.scale(), 1.5);
        assert_eq!(hud.direction, Direction::Column);
    }

    #[test]
    fn dropping_a_value_lands_where_the_line_showed() {
        let mut hud = Hud {
            values: vec![Value::Fps, Value::Graph, Value::Low, Value::Speed],
            ..Hud::default()
        };
        hud.move_value(0, drop_index(0, 2, false));
        assert_eq!(
            hud.values,
            vec![Value::Graph, Value::Fps, Value::Low, Value::Speed]
        );
        hud.move_value(3, drop_index(3, 0, false));
        assert_eq!(
            hud.values,
            vec![Value::Speed, Value::Graph, Value::Fps, Value::Low]
        );
        hud.move_value(0, drop_index(0, 3, true));
        assert_eq!(
            hud.values,
            vec![Value::Graph, Value::Fps, Value::Low, Value::Speed]
        );
    }

    #[test]
    fn a_game_boy_game_never_offers_ds_only_values() {
        let offered: Vec<Value> = Group::ALL
            .into_iter()
            .flat_map(|group| addable(&[], false, group).collect::<Vec<_>>())
            .collect();
        assert!(offered.iter().all(|value| !value.ds_only()));
        assert!(offered.contains(&Value::Fps));
        let values = [Value::Fps, Value::Gpu, Value::Jit];
        let shown: Vec<Value> = shown_values(&values, false).collect();
        assert_eq!(shown, vec![Value::Fps]);
        let ds: Vec<Value> = Group::ALL
            .into_iter()
            .flat_map(|group| addable(&[], true, group).collect::<Vec<_>>())
            .collect();
        assert_eq!(ds.len(), Value::ALL.len());
    }

    #[test]
    fn a_hud_snaps_to_the_picture_edge_with_one_inset() {
        let place = Place {
            area: area(800.0, 600.0),
            picture: Some(Rect::from_min_size(pos2(200.0, 100.0), vec2(400.0, 360.0))),
            screens: Vec::new(),
            touch: false,
            editing: false,
            keep_below: None,
        };
        let near = Rect::from_min_size(pos2(205.0 + INSET, 300.0), vec2(100.0, 30.0));
        let (snapped, guides) = snap(near, &place);
        assert_eq!(snapped.left(), 200.0 + INSET);
        assert_eq!(guides[0], Some(200.0 + INSET));
        assert_eq!(guides[1], None);
        let far = Rect::from_min_size(pos2(300.0, 300.0), vec2(100.0, 30.0));
        assert_eq!(snap(far, &place).0, far);
    }

    #[test]
    fn a_new_hud_sits_below_the_last_one() {
        let mut settings = HudSettings::default();
        let first = settings.add();
        let second = settings.add();
        assert_ne!(first, second);
        assert!(settings.get(second).unwrap().offset[1] > settings.get(first).unwrap().offset[1]);
    }

    #[test]
    fn undo_puts_a_removed_hud_back_in_its_place() {
        let mut settings = HudSettings::default();
        let ids = [settings.add(), settings.add(), settings.add()];
        let (index, hud) = settings.remove(ids[1]).unwrap();
        assert_eq!(settings.huds.len(), 2);
        settings.restore(index, hud.clone());
        settings.restore(index, hud);
        let order: Vec<u32> = settings.huds.iter().map(|hud| hud.id).collect();
        assert_eq!(order, ids);
    }

    #[test]
    fn sizes_and_card_stay_in_range() {
        let hud = Hud {
            size: 9.0,
            ..Hud::default()
        };
        assert_eq!(hud.scale(), *SIZES.end());
        let settings = HudSettings {
            opacity: -1.0,
            ..HudSettings::default()
        };
        assert_eq!(settings.opacity(), 0.0);
    }

    #[test]
    fn a_hud_that_would_cover_another_moves_below_it() {
        let area = area(412.0, 600.0);
        let first = Rect::from_min_size(pos2(12.0, 12.0), vec2(330.0, 30.0));
        let hud = Hud {
            corner: Corner::TopRight,
            ..Hud::default()
        };
        let wanted = Rect::from_min_size(pos2(300.0, 12.0), vec2(100.0, 120.0));

        let shown = hud.clear_of(wanted, &[first], area);

        assert!(!shown.intersects(first), "{shown:?}");
        assert_eq!(shown.left(), wanted.left());
        assert!(shown.top() <= first.bottom() + INSET);
        assert_eq!(hud.clear_of(wanted, &[], area), wanted);
    }

    #[test]
    fn a_hud_with_no_room_below_stays_inside_the_area() {
        let area = area(412.0, 150.0);
        let first = Rect::from_min_size(pos2(12.0, 12.0), vec2(330.0, 30.0));
        let wanted = Rect::from_min_size(pos2(300.0, 12.0), vec2(100.0, 120.0));

        let shown = Hud::default().clear_of(wanted, &[first], area);

        assert!(area.contains_rect(shown));
    }

    #[test]
    fn an_unmeasured_gpu_shows_the_dash_not_off() {
        let shown = cells::cell(Value::Gpu, &facts());
        assert_eq!(shown.value, cells::MISSING);
        assert_eq!(shown.unit, "");
    }

    #[test]
    fn the_text_outline_grows_with_the_hud_and_leaves_no_gap() {
        assert_eq!(paint::outline_rings(1.0).collect::<Vec<_>>(), vec![1.0]);
        assert_eq!(paint::outline_rings(0.75).collect::<Vec<_>>(), vec![1.0]);
        for scale in [1.25, 1.5, 2.0] {
            let rings: Vec<f32> = paint::outline_rings(scale).collect();
            assert_eq!(*rings.last().unwrap(), scale);
            let mut inner = 0.0;
            for ring in rings {
                assert!(ring - inner <= 1.0, "{scale}: gap from {inner} to {ring}");
                inner = ring;
            }
        }
    }

    fn text_rect(shapes: &[egui::epaint::ClippedShape], text: &str) -> Option<Rect> {
        shapes.iter().find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(shape) if shape.galley.text().contains(text) => {
                Some(shape.visual_bounding_rect())
            }
            _ => None,
        })
    }

    #[test]
    fn the_add_a_value_menu_keeps_its_size_frame_after_frame() {
        let ctx = egui::Context::default();
        crate::app::ui_state::ui_theme::UiTheme::Dark.apply(&ctx);
        let screen = area(1707.0, 960.0);
        let mut hud = Hud {
            values: Value::ALL[..9].to_vec(),
            ..Hud::default()
        };
        let mut opacity = DEFAULT_OPACITY;
        let facts = facts();
        let mut open = true;
        let mut hover = None;
        let mut widths = Vec::new();
        for _ in 0..40 {
            let mut input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            input.events.extend(hover.map(egui::Event::PointerMoved));
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let anchor = ui.button("HUD");
                    editor::popup(ui, &anchor, &mut hud, &mut opacity, &facts, &mut open);
                });
            });
            hover = hover
                .or_else(|| text_rect(&output.shapes, "Add a value").map(|rect| rect.center()));
            let Some(row) = text_rect(&output.shapes, "Sound buffer") else {
                continue;
            };
            let editor = text_rect(&output.shapes, "Add a value").expect("the editor closed");
            let [menu, editor] = [row, editor].map(|text| {
                ctx.memory(|memory| {
                    memory
                        .areas()
                        .visible_layer_ids()
                        .into_iter()
                        .filter_map(|layer| memory.area_rect(layer.id))
                        .filter(|rect| rect.contains(text.center()))
                        .min_by(|a, b| a.area().total_cmp(&b.area()))
                })
                .expect("no menu area around the text")
            });
            for rect in [menu, editor] {
                assert!(
                    screen.contains_rect(rect),
                    "the menu {rect:?} runs past the screen"
                );
                assert!(
                    rect.width() <= 2.0 * 150.0 + 60.0,
                    "a menu grew to {} pt",
                    rect.width()
                );
            }
            widths.push([menu.width(), editor.width()]);
        }
        assert!(
            widths.len() >= 30,
            "the menu opened for {} frames",
            widths.len()
        );
        assert!(
            widths.windows(2).all(|pair| pair[0]
                .iter()
                .zip(pair[1])
                .all(|(a, b)| (a - b).abs() < 0.5)),
            "a menu changed width: {widths:?}"
        );
    }

    #[test]
    fn the_hud_chip_drops_its_word_then_itself_before_it_leaves_its_pane() {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(area(560.0, 400.0)),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let full = chip::width(ui, &chip::label());
                let compact = chip::width(ui, &chip::compact_label());
                assert!(compact < full);
                assert_eq!(chip::label_for(ui, full), Some(chip::label()));
                assert_eq!(chip::label_for(ui, full - 1.0), Some(chip::compact_label()));
                assert_eq!(chip::label_for(ui, compact - 1.0), None);
            });
        });
    }

    #[test]
    fn the_hud_chip_menu_keeps_its_size_frame_after_frame() {
        let ctx = egui::Context::default();
        crate::app::ui_state::ui_theme::UiTheme::Dark.apply(&ctx);
        let screen = area(1707.0, 960.0);
        let mut settings = HudSettings::default();
        for _ in 0..3 {
            let id = settings.add();
            settings.get_mut(id).unwrap().values = Value::ALL.to_vec();
        }
        let chip = egui::Id::new("hud_chip");
        egui::Popup::open_id(&ctx, chip.with("menu"));
        let mut widths = Vec::new();
        for _ in 0..40 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let after = Rect::from_min_size(pos2(200.0, 0.0), vec2(80.0, 30.0));
                    chip::button(
                        ui,
                        chip,
                        after,
                        chip::label(),
                        "HUDs",
                        chip::MENU_WIDTH,
                        |ui| chip::list(ui, &settings, Some("H"), true),
                    );
                });
            });
            let menu = ctx
                .memory(|memory| memory.area_rect(chip.with("menu")))
                .expect("the chip menu is closed");
            assert!(
                screen.contains_rect(menu),
                "the menu {menu:?} runs past the screen"
            );
            widths.push(menu.width());
        }
        assert!(
            widths
                .windows(2)
                .all(|pair| (pair[0] - pair[1]).abs() < 0.5),
            "the menu changed width: {widths:?}"
        );
        assert!(
            widths[0] <= chip::MENU_WIDTH + 140.0,
            "the menu is {} pt",
            widths[0]
        );
    }

    fn ds_place(screens: [Rect; 2]) -> Place {
        Place {
            area: area(1200.0, 900.0),
            picture: Some(screens[0].union(screens[1])),
            screens: screens.to_vec(),
            touch: false,
            editing: false,
            keep_below: None,
        }
    }

    fn hud_at(centre: egui::Pos2) -> Rect {
        Rect::from_center_size(centre, vec2(100.0, 30.0))
    }

    #[test]
    fn a_hud_snaps_its_centre_to_the_picture_centre() {
        let place = Place {
            area: area(800.0, 600.0),
            picture: Some(Rect::from_min_size(pos2(200.0, 100.0), vec2(400.0, 360.0))),
            screens: Vec::new(),
            touch: false,
            editing: false,
            keep_below: None,
        };
        let (snapped, guides) = snap(hud_at(pos2(405.0, 274.0)), &place);
        assert_eq!(snapped.center(), pos2(400.0, 280.0));
        assert_eq!(guides, [Some(400.0), Some(280.0)]);
        let off = hud_at(pos2(420.0, 300.0));
        assert_eq!(snap(off, &place), (off, [None, None]));
    }

    #[test]
    fn a_hud_snaps_to_the_centre_of_each_stacked_screen() {
        let top = Rect::from_min_size(pos2(344.0, 66.0), vec2(512.0, 384.0));
        let bottom = Rect::from_min_size(pos2(344.0, 450.0), vec2(512.0, 384.0));
        let place = ds_place([top, bottom]);
        for screen in [top, bottom] {
            let near = hud_at(screen.center() + vec2(-6.0, 5.0));
            let (snapped, guides) = snap(near, &place);
            assert_eq!(snapped.center(), screen.center());
            assert_eq!(guides, [Some(screen.center().x), Some(screen.center().y)]);
        }
    }

    #[test]
    fn a_hud_snaps_to_the_centre_of_each_screen_side_by_side() {
        let left = Rect::from_min_size(pos2(88.0, 258.0), vec2(512.0, 384.0));
        let right = Rect::from_min_size(pos2(600.0, 258.0), vec2(512.0, 384.0));
        let place = ds_place([left, right]);
        for screen in [left, right] {
            let near = hud_at(screen.center() + vec2(7.0, -4.0));
            let (snapped, guides) = snap(near, &place);
            assert_eq!(snapped.center(), screen.center());
            assert_eq!(guides, [Some(screen.center().x), Some(screen.center().y)]);
        }
    }

    fn busy_facts() -> cells::Facts {
        use std::time::Duration;
        cells::Facts {
            readout: Some(crate::frame_log::Readout {
                at: Duration::from_secs(5),
                fps: 59.8,
                rate: 59.8,
                late: 122,
                game: Some(Duration::from_micros(1_200)),
                app: Duration::from_micros(3_400),
                repeated: 0.0,
                repeated_late: 0.0,
                dropped: 0.0,
                low: Some(21.5),
                refresh: Some(60.0),
                audio_queued: Some(Duration::from_millis(40)),
                audio_underrun: Duration::ZERO,
            }),
            speed: Some(100.0),
            strip: Vec::new(),
            gpu: cells::Gpu::Timed(Duration::from_micros(27_300)),
            ..facts()
        }
    }

    fn drawn(pane: Rect, settings: &mut HudSettings) -> Vec<egui::epaint::ClippedShape> {
        drawn_with(pane, settings, &[]).0
    }

    fn drawn_with(
        pane: Rect,
        settings: &mut HudSettings,
        trackers: &[(String, String)],
    ) -> (Vec<egui::epaint::ClippedShape>, bool) {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(pos2(0.0, 0.0), pane.max.to_vec2() + vec2(40.0, 40.0));
        let facts = busy_facts();
        let mut state = HudState::default();
        let place = Place {
            area: pane,
            picture: None,
            screens: Vec::new(),
            touch: false,
            editing: false,
            keep_below: None,
        };
        let mut shapes = Vec::new();
        let mut changed = false;
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            };
            shapes = ctx
                .run(input, |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            changed |=
                                overlay::show(ui, &place, &facts, trackers, settings, &mut state)
                                    .changed;
                        });
                })
                .shapes;
        }
        (shapes, changed)
    }

    fn attempt(title: &str, score: &str) -> (String, String) {
        (title.into(), score.into())
    }

    fn drawn_text(shapes: &[egui::epaint::ClippedShape], text: &str) -> bool {
        shapes.iter().any(|clipped| {
            matches!(&clipped.shape, egui::Shape::Text(shape) if shape.galley.text() == text)
        })
    }

    #[test]
    fn the_tracker_makes_itself_in_a_corner_no_hud_uses() {
        let pane = area(760.0, 700.0);
        let mut settings = HudSettings::default();
        settings.add();
        let (shapes, changed) =
            drawn_with(pane, &mut settings, &[attempt("Fastest lap", "0:42.10")]);
        assert!(changed);
        let tracker = settings.tracker.as_ref().expect("the tracker is made");
        assert_eq!(tracker.corner, Corner::TopRight);
        let title = label_rect(&shapes, "Fastest lap");
        assert!(title.center().x > pane.center().x);
        assert!(title.center().y < pane.center().y);
        settings.huds[0].corner = Corner::TopRight;
        settings.tracker = None;
        drawn_with(pane, &mut settings, &[attempt("Fastest lap", "0:42.10")]);
        assert_eq!(settings.tracker.unwrap().corner, Corner::BottomRight);
    }

    #[test]
    fn two_attempts_stack_in_one_block() {
        let pane = area(760.0, 700.0);
        let mut settings = HudSettings::default();
        let trackers = [attempt("Fastest lap", "0:42.10"), attempt("Coins", "120")];
        let (shapes, _) = drawn_with(pane, &mut settings, &trackers);
        let first = label_rect(&shapes, "Fastest lap");
        let second = label_rect(&shapes, "Coins");
        assert!(second.top() >= first.bottom());
        assert!((second.left() - first.left()).abs() < 1.0);
        let first_score = label_rect(&shapes, "0:42.10");
        let second_score = label_rect(&shapes, "120");
        assert!((first_score.right() - second_score.right()).abs() < 1.0);
    }

    #[test]
    fn a_dragged_tracker_keeps_its_place_across_a_restart() {
        let pane = area(800.0, 600.0);
        let mut settings = HudSettings::default();
        let size = vec2(160.0, 40.0);
        let dropped = Rect::from_min_size(pos2(40.0, 500.0), size);
        settings.get_mut(TRACKER).unwrap().place(pane, dropped);
        let settings = restart(&settings);
        let tracker = settings.get(TRACKER).unwrap();
        assert_eq!(tracker.corner, Corner::BottomLeft);
        assert_eq!(tracker.rect(pane, size), dropped);
    }

    #[test]
    fn the_tracker_is_gone_once_no_attempt_runs() {
        let pane = area(760.0, 700.0);
        let mut settings = HudSettings::default();
        let (shapes, _) = drawn_with(pane, &mut settings, &[attempt("Fastest lap", "0:42.10")]);
        assert!(drawn_text(&shapes, "Fastest lap"));
        let (shapes, changed) = drawn_with(pane, &mut settings, &[]);
        assert!(!changed);
        assert!(!drawn_text(&shapes, "Fastest lap"));
        assert!(!drawn_text(&shapes, "0:42.10"));
        assert!(settings.tracker.is_some());
    }

    fn label_rect(shapes: &[egui::epaint::ClippedShape], label: &str) -> Rect {
        shapes
            .iter()
            .find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) if shape.galley.text().starts_with(label) => {
                    Some(shape.visual_bounding_rect())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{label} is not drawn"))
    }

    fn assert_inside(pane: Rect, shapes: &[egui::epaint::ClippedShape], what: &str) {
        for clipped in shapes {
            let rect = clipped.shape.visual_bounding_rect();
            if rect.is_positive() {
                assert!(
                    pane.expand(0.5).contains_rect(rect),
                    "{what}: {rect:?} runs past the pane {pane:?}"
                );
            }
        }
    }

    fn nine(direction: Direction, size: f32) -> HudSettings {
        let mut settings = HudSettings::default();
        let id = settings.add();
        let hud = settings.get_mut(id).unwrap();
        hud.values = Value::ALL[..9].to_vec();
        hud.direction = direction;
        hud.size = size;
        settings
    }

    #[test]
    fn a_hud_of_nine_values_wraps_inside_the_pane_at_every_size() {
        let panes = [
            (
                "desktop game tab",
                Rect::from_min_size(pos2(8.0, 60.0), vec2(760.0, 700.0)),
            ),
            (
                "phone",
                Rect::from_min_size(pos2(0.0, 48.0), vec2(412.0, 360.0)),
            ),
            (
                "phone sideways",
                Rect::from_min_size(pos2(0.0, 0.0), vec2(915.0, 200.0)),
            ),
        ];
        for (name, pane) in panes {
            for direction in [Direction::Row, Direction::Column] {
                for size in [*SIZES.start(), 1.0, 1.5, *SIZES.end()] {
                    let what = format!("{direction:?} at {size} in the {name}");
                    let shapes = drawn(pane, &mut nine(direction, size));
                    assert_inside(pane, &shapes, &what);
                    let labels: Vec<Rect> = Value::ALL[..9]
                        .iter()
                        .filter(|value| **value != Value::Graph)
                        .map(|value| label_rect(&shapes, value.name()))
                        .collect();
                    for pair in labels.windows(2) {
                        let [before, after] = [pair[0], pair[1]];
                        let next = match direction {
                            Direction::Row => {
                                (after.center().y - before.center().y).abs() < before.height() / 2.0
                                    && after.left() > before.right()
                                    || after.top() > before.bottom()
                            }
                            Direction::Column => {
                                (after.left() - before.left()).abs() < 4.0
                                    && after.top() > before.bottom()
                                    || after.left() > before.right()
                            }
                        };
                        assert!(next, "{what}: {after:?} is out of order after {before:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_hud_stays_inside_a_pane_that_shrinks() {
        let big = Rect::from_min_size(pos2(0.0, 0.0), vec2(1280.0, 800.0));
        let small = Rect::from_min_size(pos2(120.0, 90.0), vec2(412.0, 300.0));
        for direction in [Direction::Row, Direction::Column] {
            for size in [1.0, *SIZES.end()] {
                let mut settings = nine(direction, size);
                let id = settings.huds[0].id;
                let dropped = Rect::from_min_size(pos2(900.0, 700.0), vec2(300.0, 60.0));
                settings.get_mut(id).unwrap().place(big, dropped);
                assert_inside(big, &drawn(big, &mut settings), "before the shrink");
                let what = format!("{direction:?} at {size} after the shrink");
                assert_inside(small, &drawn(small, &mut settings), &what);
            }
        }
    }

    const ONLINE: [Value; 4] = [Value::Ping, Value::Delay, Value::Connection, Value::Waits];

    fn shown(value: Value, link: online::Link) -> String {
        let shown = cells::cell(value, &cells::Facts { link, ..facts() });
        match shown.unit.as_ref() {
            "" => shown.value,
            unit => format!("{} {unit}", shown.value),
        }
    }

    #[test]
    fn without_a_session_the_online_values_show_the_dash() {
        for value in ONLINE {
            assert_eq!(shown(value, online::Link::default()), cells::MISSING);
        }
    }

    #[test]
    fn the_online_values_read_in_words() {
        let link = online::Link {
            ping: online::Ping::Partner(Some(34)),
            delay: Some(online::Delay { frames: 4, ms: 67 }),
            path: Some(citrine_gb_net::LinkPath::Direct),
            waits: Some(3),
        };
        assert_eq!(shown(Value::Ping, link.clone()), "34 ms");
        assert_eq!(shown(Value::Delay, link.clone()), "4 frames, 67 ms");
        assert_eq!(shown(Value::Connection, link.clone()), "Direct");
        assert_eq!(shown(Value::Waits, link.clone()), "3 in 1 min");
        let one = online::Link {
            delay: Some(online::Delay { frames: 1, ms: 17 }),
            path: Some(citrine_gb_net::LinkPath::Relay),
            ..link
        };
        assert_eq!(shown(Value::Delay, one.clone()), "1 frame, 17 ms");
        assert_eq!(shown(Value::Connection, one), "Relay");
    }

    #[test]
    fn a_group_shows_the_ping_of_each_partner() {
        let link = online::Link {
            ping: online::Ping::Partners(vec![("Mia".into(), Some(34)), ("Jonas".into(), None)]),
            ..online::Link::default()
        };
        assert_eq!(shown(Value::Ping, link), "Mia 34, Jonas - ms");
    }

    #[test]
    fn signed_in_without_a_session_the_ping_names_the_server() {
        let link = online::Link {
            ping: online::Ping::Server(41),
            ..online::Link::default()
        };
        assert_eq!(shown(Value::Ping, link), "41 ms to the server");
    }

    #[test]
    fn every_console_offers_the_online_group() {
        for ds in [false, true] {
            let offered: Vec<Value> = addable(&[], ds, Group::Online).collect();
            assert_eq!(offered, ONLINE);
        }
    }

    fn texts(output: &egui::FullOutput) -> Vec<String> {
        output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_phone_playing_a_borrowed_game_shows_the_online_values() {
        use crate::play::MyGame;
        use crate::play::tests::{Pair, homebrew, snake};
        let mut pair = Pair::new(false, 0, 0, homebrew());
        let session = pair.invite(snake());
        pair.accept(session, MyGame::Own(snake()));
        pair.play_frames(60);
        let mut app = crate::app::tests::phone_app("hud-online-borrowed");
        app.mobile.nav.show_game();
        let id = app.ui.settings.hud.add();
        app.ui.settings.hud.get_mut(id).unwrap().values = vec![Value::Connection];
        assert!(!app.emulator.has_game());
        std::mem::swap(&mut *app.play, &mut pair.host.play);
        std::mem::swap(&mut app.online, &mut pair.host.online);
        let ctx = egui::Context::default();
        let mut shown = Vec::new();
        for _ in 0..3 {
            shown = texts(&crate::app::tests::play_frame(&mut app, &ctx, Vec::new()));
        }
        std::mem::swap(&mut *app.play, &mut pair.host.play);
        std::mem::swap(&mut app.online, &mut pair.host.online);
        assert!(
            shown.iter().any(|text| text == "ConnectionRelay"),
            "{shown:?}"
        );
    }
}
