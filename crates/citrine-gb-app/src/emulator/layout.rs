use crate::app::ui_state::ds_screens::{
    Arrangement, Corner, DsScreens, MAX_GAP, Placement, ScreenContext,
};
use citrine_gb::console::{Screen, Touch, VideoSpec};
use egui::{Pos2, Rect, Vec2, pos2, vec2};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub screen: Screen,
    pub uv: Rect,
    pub rect: Rect,
    pub turns: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenLayout {
    video: VideoSpec,
    placement: Placement,
    corner: Corner,
    share: f32,
    gap: f32,
    turns: u8,
    only: Option<usize>,
    beside: bool,
    inset: f32,
}

impl From<VideoSpec> for ScreenLayout {
    fn from(video: VideoSpec) -> Self {
        Self::new(video, &DsScreens::default(), ScreenContext::Desktop)
    }
}

impl ScreenLayout {
    pub fn new(video: VideoSpec, screens: &DsScreens, context: ScreenContext) -> Self {
        Self {
            video,
            placement: screens.placement(context),
            corner: screens.corner,
            share: screens.small.share(),
            gap: f32::from(screens.gap.min(MAX_GAP)),
            turns: screens.rotation.quarter_turns(),
            only: None,
            beside: true,
            inset: 0.0,
        }
    }

    pub fn only(self, index: usize) -> Self {
        Self {
            only: Some(index),
            ..self
        }
    }

    pub fn for_area(self, area: Vec2) -> Self {
        if !self.big_and_small() {
            return self;
        }
        let fit = |layout: Self| {
            let size = layout.size();
            (area.x / size.x).min(area.y / size.y)
        };
        let beside = Self {
            beside: true,
            ..self
        };
        let above = Self {
            beside: false,
            ..self
        };
        if fit(beside) >= fit(above) {
            beside
        } else {
            above
        }
    }

    pub fn big_and_small(self) -> bool {
        self.dual()
            && self.only.is_none()
            && !self.placement.one_screen
            && self.placement.arrangement == Arrangement::BigAndSmall
    }

    pub fn share(self) -> f32 {
        self.share
    }

    pub fn with_share(self, share: f32) -> Self {
        Self { share, ..self }
    }

    pub fn sampled_at(self, scale: usize) -> Self {
        let inset = if self.video.screens.len() > 1 {
            0.5 / scale.max(1) as f32
        } else {
            0.0
        };
        Self { inset, ..self }
    }

    fn dual(self) -> bool {
        self.video.screens.len() > 1
    }

    fn boxes(self) -> (Vec2, Vec<(Screen, Rect)>) {
        let screens = self.video.screens;
        let size = |s: Screen| vec2(s.width as f32, s.height as f32);
        let at = |s: Screen, x: f32, y: f32| (s, Rect::from_min_size(pos2(x, y), size(s)));
        if !self.dual() {
            let frame = vec2(self.video.width as f32, self.video.height as f32);
            let boxes = screens
                .iter()
                .map(|&s| at(s, s.x as f32, s.y as f32))
                .collect();
            return (frame, boxes);
        }
        let (first, second) = if self.placement.swapped {
            (screens[1], screens[0])
        } else {
            (screens[0], screens[1])
        };
        let alone = match self.only {
            Some(index) => Some(screens[index.min(screens.len() - 1)]),
            None => self.placement.one_screen.then_some(first),
        };
        let gap = self.gap;
        let (a, b) = (size(first), size(second));
        let (frame, boxes) = match (alone, self.placement.arrangement) {
            (Some(only), _) => (size(only), vec![at(only, 0.0, 0.0)]),
            (None, Arrangement::Stacked) => {
                let frame = vec2(a.x.max(b.x), a.y + gap + b.y);
                (
                    frame,
                    vec![
                        at(first, (frame.x - a.x) / 2.0, 0.0),
                        at(second, (frame.x - b.x) / 2.0, a.y + gap),
                    ],
                )
            }
            (None, Arrangement::SideBySide) => {
                let frame = vec2(a.x + gap + b.x, a.y.max(b.y));
                (
                    frame,
                    vec![
                        at(first, 0.0, (frame.y - a.y) / 2.0),
                        at(second, a.x + gap, (frame.y - b.y) / 2.0),
                    ],
                )
            }
            (None, Arrangement::BigAndSmall) => {
                let small = b * (a.x * self.share / b.x);
                let (left, top) = (self.corner.left(), self.corner.top());
                let (frame, big_at, small_at) = if self.beside {
                    let frame = vec2(a.x + gap + small.x, a.y.max(small.y));
                    let big = pos2(if left { small.x + gap } else { 0.0 }, 0.0);
                    let little = pos2(
                        if left { 0.0 } else { a.x + gap },
                        if top { 0.0 } else { frame.y - small.y },
                    );
                    (frame, big, little)
                } else {
                    let frame = vec2(a.x.max(small.x), a.y + gap + small.y);
                    let big = pos2(0.0, if top { small.y + gap } else { 0.0 });
                    let little = pos2(
                        if left { 0.0 } else { frame.x - small.x },
                        if top { 0.0 } else { a.y + gap },
                    );
                    (frame, big, little)
                };
                (
                    frame,
                    vec![
                        (first, Rect::from_min_size(big_at, a)),
                        (second, Rect::from_min_size(small_at, small)),
                    ],
                )
            }
        };
        turned(frame, boxes, self.turns)
    }

    pub fn size(self) -> Vec2 {
        self.boxes().0
    }

    pub fn video(self) -> VideoSpec {
        self.video
    }

    pub fn frame_size(self, scale: usize) -> (usize, usize) {
        let size = self.size() * scale.max(1) as f32;
        (size.x.round() as usize, size.y.round() as usize)
    }

    pub fn compose(self, source: &[u8], scale: usize, out: &mut Vec<u8>) {
        let scale = scale.max(1);
        let (width, height) = self.frame_size(scale);
        let stride = self.video.width * scale;
        out.clear();
        out.extend(std::iter::repeat_n([0, 0, 0, 255], width * height).flatten());
        let target = Rect::from_min_size(Pos2::ZERO, vec2(width as f32, height as f32));
        let span = |from: f32, to: f32, end: usize| {
            (from.round().max(0.0) as usize).min(end)..(to.round().max(0.0) as usize).min(end)
        };
        let crisp = Self { inset: 0.0, ..self };
        let pixel = |t: f32, size: usize| ((t * size as f32) as usize).min(size - 1);
        for placed in crisp.place(target) {
            let screen = placed.screen;
            let (screen_width, screen_height) = (screen.width * scale, screen.height * scale);
            for y in span(placed.rect.min.y, placed.rect.max.y, height) {
                for x in span(placed.rect.min.x, placed.rect.max.x, width) {
                    let at = pos2(x as f32 + 0.5, y as f32 + 0.5);
                    let along = upright((at - placed.rect.min) / placed.rect.size(), placed.turns);
                    let from_x = screen.x * scale + pixel(along.x, screen_width);
                    let from_y = screen.y * scale + pixel(along.y, screen_height);
                    let from = (from_y * stride + from_x) * 4;
                    let to = (y * width + x) * 4;
                    if let Some(rgba) = source.get(from..from + 4) {
                        out[to..to + 4].copy_from_slice(rgba);
                    }
                }
            }
        }
    }

    pub fn place(self, target: Rect) -> impl Iterator<Item = Placed> {
        let (frame, boxes) = self.boxes();
        let source = vec2(self.video.width as f32, self.video.height as f32);
        let scale = target.size() / frame;
        let turns = if self.dual() { self.turns } else { 0 };
        boxes.into_iter().map(move |(screen, at)| {
            let from = vec2(screen.x as f32, screen.y as f32);
            let end = from + vec2(screen.width as f32, screen.height as f32);
            Placed {
                screen,
                uv: Rect::from_min_max(
                    ((from + Vec2::splat(self.inset)) / source).to_pos2(),
                    ((end - Vec2::splat(self.inset)) / source).to_pos2(),
                ),
                rect: Rect::from_min_max(
                    target.min + at.min.to_vec2() * scale,
                    target.min + at.max.to_vec2() * scale,
                ),
                turns,
            }
        })
    }

    pub fn touch_at(self, target: Rect, pos: Pos2) -> Option<Touch> {
        let placed = self
            .place(target)
            .find(|p| p.screen.touch && p.rect.contains(pos))?;
        let along = upright((pos - placed.rect.min) / placed.rect.size(), placed.turns);
        let pixel = |t: f32, size: usize| {
            let at = (t * size as f32)
                .floor()
                .clamp(0.0, size.saturating_sub(1) as f32);
            u8::try_from(at as usize).ok()
        };
        Some(Touch {
            x: pixel(along.x, placed.screen.width)?,
            y: pixel(along.y, placed.screen.height)?,
        })
    }

    pub fn point(self, ui: &egui::Ui, target: Rect) -> Option<Option<Touch>> {
        if !self.placed_touch(target) {
            return None;
        }
        let response = ui.interact(
            target,
            ui.id().with("touch_screen"),
            egui::Sense::click_and_drag(),
        );
        let touch = response
            .is_pointer_button_down_on()
            .then(|| response.interact_pointer_pos())
            .flatten()
            .and_then(|pos| self.touch_at(target, pos));
        Some(touch)
    }

    fn placed_touch(self, target: Rect) -> bool {
        self.place(target).any(|p| p.screen.touch)
    }

    pub fn paint(
        self,
        ui: &egui::Ui,
        texture: egui::TextureId,
        upscaled: Option<(egui::TextureId, usize)>,
        target: Rect,
    ) {
        for placed in self.place(target) {
            let (texture, uv) = match upscaled {
                Some((upscaled, y)) if placed.screen.x == 0 && placed.screen.y == y => {
                    (upscaled, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)))
                }
                _ => (texture, placed.uv),
            };
            let upright = if placed.turns % 2 == 1 {
                Rect::from_center_size(
                    placed.rect.center(),
                    vec2(placed.rect.height(), placed.rect.width()),
                )
            } else {
                placed.rect
            };
            let image =
                egui::Image::new(egui::load::SizedTexture::new(texture, upright.size())).uv(uv);
            let image = if placed.turns % 4 == 0 {
                image
            } else {
                image.rotate(
                    f32::from(placed.turns) * std::f32::consts::FRAC_PI_2,
                    Vec2::splat(0.5),
                )
            };
            image.paint_at(ui, upright);
        }
    }
}

fn upright(along: Vec2, turns: u8) -> Vec2 {
    match turns % 4 {
        1 => vec2(along.y, 1.0 - along.x),
        2 => vec2(1.0 - along.x, 1.0 - along.y),
        3 => vec2(1.0 - along.y, along.x),
        _ => along,
    }
}

fn turned(frame: Vec2, boxes: Vec<(Screen, Rect)>, turns: u8) -> (Vec2, Vec<(Screen, Rect)>) {
    let turn = |r: Rect| match turns % 4 {
        1 => Rect::from_min_size(
            pos2(frame.y - r.max.y, r.min.x),
            vec2(r.height(), r.width()),
        ),
        2 => Rect::from_min_size(pos2(frame.x - r.max.x, frame.y - r.max.y), r.size()),
        3 => Rect::from_min_size(
            pos2(r.min.y, frame.x - r.max.x),
            vec2(r.height(), r.width()),
        ),
        _ => r,
    };
    let frame_turned = if turns % 2 == 1 {
        vec2(frame.y, frame.x)
    } else {
        frame
    };
    (
        frame_turned,
        boxes.into_iter().map(|(s, r)| (s, turn(r))).collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ui_state::ds_screens::{Rotation, SmallSize};
    use citrine_gb::console::{Console, ConsoleKind};
    use citrine_gb::error::GbResult;
    use citrine_gb::persistence::battery_save::BatterySave;
    use citrine_gb::persistence::state::StateBody;
    use citrine_gb::rom::id::RomId;

    const TWO_SCREENS: [Screen; 2] = [
        Screen::whole(256, 192),
        Screen {
            x: 0,
            y: 192,
            width: 256,
            height: 192,
            touch: true,
        },
    ];

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(pos2(x, y), vec2(w, h))
    }

    fn arranged(video: VideoSpec, arrangement: Arrangement) -> ScreenLayout {
        with(video, |s| s.desktop.arrangement = arrangement)
    }

    fn with(video: VideoSpec, change: impl FnOnce(&mut DsScreens)) -> ScreenLayout {
        let mut screens = DsScreens::default();
        change(&mut screens);
        ScreenLayout::new(video, &screens, ScreenContext::Desktop)
    }

    fn rects(layout: ScreenLayout, target: Rect) -> Vec<(Rect, Rect)> {
        layout.place(target).map(|p| (p.uv, p.rect)).collect()
    }

    #[test]
    fn one_screen_fills_the_target_with_the_whole_frame() {
        for kind in [ConsoleKind::Gb, ConsoleKind::Gba] {
            for arrangement in [Arrangement::Stacked, Arrangement::SideBySide] {
                let layout = arranged(kind.video(), arrangement);
                let target = rect(13.5, 7.25, 641.0, 577.5);
                assert_eq!(rects(layout, target), [(rect(0.0, 0.0, 1.0, 1.0), target)]);
            }
        }
        assert_eq!(
            ScreenLayout::from(ConsoleKind::Gba.video()).size(),
            vec2(240.0, 160.0)
        );
    }

    #[test]
    fn a_console_with_two_screens_stacks_them_top_over_bottom() {
        let layout = arranged(VideoSpec::of(&TWO_SCREENS), Arrangement::Stacked);
        assert_eq!(layout.size(), vec2(256.0, 384.0));
        assert_eq!(
            rects(layout, rect(100.0, 0.0, 512.0, 768.0)),
            [
                (rect(0.0, 0.0, 1.0, 0.5), rect(100.0, 0.0, 512.0, 384.0)),
                (rect(0.0, 0.5, 1.0, 0.5), rect(100.0, 384.0, 512.0, 384.0)),
            ]
        );
    }

    #[test]
    fn a_console_with_two_screens_lays_them_side_by_side() {
        let layout = arranged(VideoSpec::of(&TWO_SCREENS), Arrangement::SideBySide);
        assert_eq!(layout.size(), vec2(512.0, 192.0));
        let placed: Vec<_> = layout.place(rect(0.0, 50.0, 1024.0, 384.0)).collect();
        assert_eq!(
            placed.iter().map(|p| (p.uv, p.rect)).collect::<Vec<_>>(),
            [
                (rect(0.0, 0.0, 1.0, 0.5), rect(0.0, 50.0, 512.0, 384.0)),
                (rect(0.0, 0.5, 1.0, 0.5), rect(512.0, 50.0, 512.0, 384.0)),
            ]
        );
        assert!(!placed[0].screen.touch && placed[1].screen.touch);
    }

    struct Echo {
        video: VideoSpec,
        touch: Option<Touch>,
    }

    impl Console for Echo {
        fn kind(&self) -> ConsoleKind {
            ConsoleKind::Gba
        }
        fn video(&self) -> VideoSpec {
            self.video
        }
        fn timing(&self) -> citrine_gb::console::Timing {
            ConsoleKind::Gba.timing()
        }
        fn run_frame(&mut self) {}
        fn frame(&self) -> &[u8] {
            &[]
        }
        fn take_audio(&mut self, _out: &mut Vec<f32>) {}
        fn sample_rate(&self) -> u32 {
            48_000
        }
        fn set_sample_rate(&mut self, _rate: u32) {}
        fn keys(&self) -> citrine_gb::console::Keys {
            citrine_gb::console::Keys::empty()
        }
        fn set_keys(&mut self, _keys: citrine_gb::console::Keys) {}
        fn set_touch(&mut self, touch: Option<Touch>) {
            self.touch = touch;
        }
        fn soft_reset(&mut self) {}
        fn audio_channels(&self) -> &'static [&'static str] {
            &[]
        }
        fn set_channel_enabled(&mut self, _index: usize, _enabled: bool) {}
        fn rom_bytes(&self) -> std::sync::Arc<[u8]> {
            std::sync::Arc::from([])
        }
        fn title(&self) -> String {
            String::new()
        }
        fn has_battery(&self) -> bool {
            false
        }
        fn has_clock(&self) -> bool {
            false
        }
        fn battery(&mut self, _force: bool, _now: u64) -> Option<BatterySave> {
            None
        }
        fn battery_from_file(&self, _data: &[u8]) -> BatterySave {
            unimplemented!()
        }
        fn put_battery(&mut self, _save: BatterySave, _now: u64) {}
        fn save_state(&mut self, _now: u64) -> GbResult<Vec<u8>> {
            unimplemented!()
        }
        fn state_body(&mut self, _now: u64) -> GbResult<StateBody> {
            unimplemented!()
        }
        fn rom_id(&self) -> RomId {
            unimplemented!()
        }
    }

    fn press_at(
        layout: ScreenLayout,
        target: Rect,
        at: Pos2,
        echo: &mut Echo,
    ) -> Vec<Option<Touch>> {
        let ctx = egui::Context::default();
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        let frames = [
            vec![],
            vec![egui::Event::PointerMoved(at), button(true)],
            vec![egui::Event::PointerMoved(at + vec2(4.0, 2.0))],
            vec![button(false)],
        ];
        let mut seen = Vec::new();
        for events in frames {
            let input = egui::RawInput {
                screen_rect: Some(rect(0.0, 0.0, 1280.0, 800.0)),
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if let Some(touch) = layout.point(ui, target) {
                        echo.set_touch(touch);
                    }
                });
            });
            seen.push(echo.touch);
        }
        seen
    }

    #[test]
    fn a_press_on_the_touch_screen_reaches_the_console_as_its_pixel() {
        let video = VideoSpec::of(&TWO_SCREENS);
        let mut echo = Echo { video, touch: None };
        let stacked = arranged(video, Arrangement::Stacked);
        assert_eq!(
            press_at(
                stacked,
                rect(0.0, 0.0, 512.0, 768.0),
                pos2(100.5, 500.25),
                &mut echo
            ),
            [
                None,
                Some(Touch { x: 50, y: 58 }),
                Some(Touch { x: 52, y: 59 }),
                None
            ]
        );
        let beside = arranged(video, Arrangement::SideBySide);
        let target = rect(100.0, 200.0, 1024.0, 384.0);
        assert_eq!(
            press_at(beside, target, pos2(800.0, 300.0), &mut echo)[1],
            Some(Touch { x: 94, y: 50 })
        );
        assert_eq!(
            press_at(beside, target, pos2(300.0, 300.0), &mut echo)[1],
            None
        );
    }

    #[test]
    fn the_last_touch_pixel_is_the_screen_edge() {
        let layout = arranged(VideoSpec::of(&TWO_SCREENS), Arrangement::Stacked);
        let target = rect(0.0, 0.0, 256.0, 384.0);
        assert_eq!(
            layout.touch_at(target, pos2(255.99, 383.99)),
            Some(Touch { x: 255, y: 191 })
        );
        assert_eq!(layout.touch_at(target, pos2(10.0, 10.0)), None);
    }

    #[test]
    fn game_boy_and_gba_screens_take_no_touches() {
        for kind in [ConsoleKind::Gb, ConsoleKind::Gbc, ConsoleKind::Gba] {
            let video = kind.video();
            let mut echo = Echo { video, touch: None };
            let target = rect(0.0, 0.0, 640.0, 576.0);
            assert_eq!(
                press_at(video.into(), target, pos2(320.0, 300.0), &mut echo),
                [None; 4]
            );
            assert_eq!(
                ScreenLayout::from(video).touch_at(target, pos2(320.0, 300.0)),
                None
            );
        }
    }

    #[test]
    fn linear_sampling_keeps_each_screen_off_its_neighbour() {
        let two = arranged(VideoSpec::of(&TWO_SCREENS), Arrangement::SideBySide);
        let uvs: Vec<Rect> = two
            .sampled_at(2)
            .place(rect(0.0, 0.0, 512.0, 192.0))
            .map(|p| p.uv)
            .collect();
        let uv = |x0: f32, y0: f32, x1: f32, y1: f32| {
            Rect::from_min_max(pos2(x0 / 256.0, y0 / 384.0), pos2(x1 / 256.0, y1 / 384.0))
        };
        assert_eq!(uvs[0], uv(0.25, 0.25, 255.75, 191.75));
        assert_eq!(uvs[1], uv(0.25, 192.25, 255.75, 383.75));
        let one = ScreenLayout::from(ConsoleKind::Gb.video());
        assert_eq!(one.sampled_at(3), one);
    }

    fn close(a: Rect, b: Rect) -> bool {
        (a.min - b.min).length() < 0.01 && (a.max - b.max).length() < 0.01
    }

    fn placed(layout: ScreenLayout) -> Vec<(bool, Rect)> {
        let size = layout.size();
        layout
            .place(Rect::from_min_size(Pos2::ZERO, size))
            .map(|p| (p.screen.touch, p.rect))
            .collect()
    }

    fn ds() -> VideoSpec {
        VideoSpec::of(&TWO_SCREENS)
    }

    fn one_to_one(layout: ScreenLayout) -> Rect {
        Rect::from_min_size(Pos2::ZERO, layout.size())
    }

    #[test]
    fn swapped_stacked_puts_the_bottom_screen_on_top() {
        let layout = with(ds(), |s| s.desktop.swapped = true);
        assert_eq!(layout.size(), vec2(256.0, 384.0));
        assert_eq!(
            placed(layout),
            [
                (true, rect(0.0, 0.0, 256.0, 192.0)),
                (false, rect(0.0, 192.0, 256.0, 192.0))
            ]
        );
    }

    #[test]
    fn the_gap_sits_between_the_screens_in_ds_pixels() {
        let stacked = with(ds(), |s| s.gap = 16);
        assert_eq!(stacked.size(), vec2(256.0, 400.0));
        assert_eq!(placed(stacked)[1].1, rect(0.0, 208.0, 256.0, 192.0));
        let beside = with(ds(), |s| {
            s.gap = 16;
            s.desktop.arrangement = Arrangement::SideBySide;
        });
        assert_eq!(beside.size(), vec2(528.0, 192.0));
        assert_eq!(placed(beside)[1].1, rect(272.0, 0.0, 256.0, 192.0));
    }

    #[test]
    fn big_and_small_puts_the_small_screen_in_each_corner_beside_the_big_one() {
        let third = 256.0 / 3.0;
        let cases = [
            (Corner::TopLeft, (third, 0.0), (0.0, 0.0)),
            (Corner::TopRight, (0.0, 0.0), (256.0, 0.0)),
            (Corner::BottomLeft, (third, 0.0), (0.0, 128.0)),
            (Corner::BottomRight, (0.0, 0.0), (256.0, 128.0)),
        ];
        for (corner, big, small) in cases {
            let layout = with(ds(), |s| {
                s.desktop.arrangement = Arrangement::BigAndSmall;
                s.corner = corner;
            })
            .for_area(vec2(1200.0, 600.0));
            assert!((layout.size() - vec2(256.0 + third, 192.0)).length() < 0.01);
            let got = placed(layout);
            assert!(!got[0].0 && got[1].0, "{corner:?}");
            assert!(
                close(got[0].1, rect(big.0, big.1, 256.0, 192.0)),
                "{corner:?}"
            );
            assert!(
                close(got[1].1, rect(small.0, small.1, third, 64.0)),
                "{corner:?}"
            );
        }
    }

    #[test]
    fn big_and_small_goes_above_or_below_in_a_tall_area() {
        let third = 256.0 / 3.0;
        let cases = [
            (Corner::TopLeft, 64.0, (0.0, 0.0)),
            (Corner::TopRight, 64.0, (256.0 - third, 0.0)),
            (Corner::BottomLeft, 0.0, (0.0, 192.0)),
            (Corner::BottomRight, 0.0, (256.0 - third, 192.0)),
        ];
        for (corner, big_y, small) in cases {
            let layout = with(ds(), |s| {
                s.desktop.arrangement = Arrangement::BigAndSmall;
                s.desktop.swapped = true;
                s.corner = corner;
            })
            .for_area(vec2(412.0, 437.0));
            assert_eq!(layout.size(), vec2(256.0, 256.0));
            let got = placed(layout);
            assert!(got[0].0 && !got[1].0, "{corner:?}");
            assert!(
                close(got[0].1, rect(0.0, big_y, 256.0, 192.0)),
                "{corner:?}"
            );
            assert!(
                close(got[1].1, rect(small.0, small.1, third, 64.0)),
                "{corner:?}"
            );
        }
    }

    #[test]
    fn the_small_screen_size_is_a_share_of_the_big_ones_width() {
        for (small, width) in [
            (SmallSize::Quarter, 64.0),
            (SmallSize::Third, 256.0 / 3.0),
            (SmallSize::Half, 128.0),
        ] {
            let layout = with(ds(), |s| {
                s.desktop.arrangement = Arrangement::BigAndSmall;
                s.small = small;
            });
            assert!((placed(layout)[1].1.width() - width).abs() < 0.01);
        }
    }

    #[test]
    fn one_screen_shows_the_first_screen_alone() {
        let top = with(ds(), |s| s.desktop.one_screen = true);
        assert_eq!(top.size(), vec2(256.0, 192.0));
        assert_eq!(placed(top), [(false, rect(0.0, 0.0, 256.0, 192.0))]);
        let bottom = with(ds(), |s| {
            s.desktop.one_screen = true;
            s.desktop.swapped = true;
            s.desktop.arrangement = Arrangement::SideBySide;
        });
        assert_eq!(placed(bottom), [(true, rect(0.0, 0.0, 256.0, 192.0))]);
    }

    #[test]
    fn a_separate_tab_shows_its_own_screen_whatever_the_placement() {
        let swapped = with(ds(), |s| {
            s.desktop.swapped = true;
            s.desktop.arrangement = Arrangement::BigAndSmall;
        });
        assert_eq!(
            placed(swapped.only(0)),
            [(false, rect(0.0, 0.0, 256.0, 192.0))]
        );
        assert_eq!(
            placed(swapped.only(1)),
            [(true, rect(0.0, 0.0, 256.0, 192.0))]
        );
    }

    #[test]
    fn turning_moves_the_screens_with_the_whole_frame() {
        let right = with(ds(), |s| s.rotation = Rotation::Right);
        assert_eq!(right.size(), vec2(384.0, 256.0));
        assert_eq!(
            placed(right),
            [
                (false, rect(192.0, 0.0, 192.0, 256.0)),
                (true, rect(0.0, 0.0, 192.0, 256.0))
            ]
        );
        let left = with(ds(), |s| s.rotation = Rotation::Left);
        assert_eq!(
            placed(left),
            [
                (false, rect(0.0, 0.0, 192.0, 256.0)),
                (true, rect(192.0, 0.0, 192.0, 256.0))
            ]
        );
        let down = with(ds(), |s| s.rotation = Rotation::UpsideDown);
        assert_eq!(
            placed(down),
            [
                (false, rect(0.0, 192.0, 256.0, 192.0)),
                (true, rect(0.0, 0.0, 256.0, 192.0))
            ]
        );
    }

    #[test]
    fn game_boy_and_gba_ignore_the_ds_screen_settings() {
        for kind in [ConsoleKind::Gb, ConsoleKind::Gba] {
            let video = kind.video();
            let changed = with(video, |s| {
                s.desktop.arrangement = Arrangement::BigAndSmall;
                s.desktop.swapped = true;
                s.desktop.one_screen = true;
                s.rotation = Rotation::Right;
                s.gap = 40;
            });
            assert_eq!(changed.size(), ScreenLayout::from(video).size());
            let target = rect(0.0, 0.0, 480.0, 320.0);
            assert_eq!(
                changed.place(target).map(|p| p.turns).collect::<Vec<_>>(),
                [0]
            );
        }
    }

    #[test]
    fn touch_reaches_the_bottom_screen_in_every_placement() {
        let touch =
            |layout: ScreenLayout, x: f32, y: f32| layout.touch_at(one_to_one(layout), pos2(x, y));
        let swapped = with(ds(), |s| s.desktop.swapped = true);
        assert_eq!(touch(swapped, 10.5, 20.5), Some(Touch { x: 10, y: 20 }));
        assert_eq!(touch(swapped, 10.5, 200.5), None);

        let beside = with(ds(), |s| {
            s.gap = 16;
            s.desktop.arrangement = Arrangement::SideBySide;
        });
        assert_eq!(touch(beside, 302.5, 40.5), Some(Touch { x: 30, y: 40 }));
        assert_eq!(touch(beside, 260.0, 40.0), None);

        let small = with(ds(), |s| s.desktop.arrangement = Arrangement::BigAndSmall)
            .for_area(vec2(1200.0, 600.0));
        let half_pixel = 1.0 / 6.0;
        let centre = 256.0 + 256.0 / 6.0 + half_pixel;
        assert_eq!(
            touch(small, centre, 160.0 + half_pixel),
            Some(Touch { x: 128, y: 96 })
        );
        assert_eq!(touch(small, 100.0, 100.0), None);

        let big = with(ds(), |s| {
            s.desktop.arrangement = Arrangement::BigAndSmall;
            s.desktop.swapped = true;
            s.corner = Corner::TopLeft;
        })
        .for_area(vec2(1200.0, 600.0));
        let left = 256.0 / 3.0;
        assert_eq!(
            touch(big, left + 100.5, 50.5),
            Some(Touch { x: 100, y: 50 })
        );
        assert_eq!(touch(big, 10.0, 10.0), None);

        let bottom_only = with(ds(), |s| {
            s.desktop.one_screen = true;
            s.desktop.swapped = true;
        });
        assert_eq!(touch(bottom_only, 5.5, 6.5), Some(Touch { x: 5, y: 6 }));
        let top_only = with(ds(), |s| s.desktop.one_screen = true);
        assert_eq!(touch(top_only, 5.5, 6.5), None);

        let tab = with(ds(), |s| s.desktop.swapped = true).only(1);
        assert_eq!(touch(tab, 5.5, 6.5), Some(Touch { x: 5, y: 6 }));
        assert_eq!(touch(tab.only(0), 5.5, 6.5), None);
    }

    #[test]
    fn touch_on_a_turned_screen_reaches_the_pixel_under_the_finger() {
        let touch = |rotation, x: f32, y: f32| {
            let layout = with(ds(), |s| s.rotation = rotation);
            layout.touch_at(one_to_one(layout), pos2(x, y))
        };
        assert_eq!(
            touch(Rotation::Right, 50.5, 10.5),
            Some(Touch { x: 10, y: 141 })
        );
        assert_eq!(
            touch(Rotation::UpsideDown, 10.5, 20.5),
            Some(Touch { x: 245, y: 171 })
        );
        assert_eq!(
            touch(Rotation::Left, 242.5, 10.5),
            Some(Touch { x: 245, y: 50 })
        );
        assert_eq!(touch(Rotation::Right, 250.0, 10.0), None);
        assert_eq!(touch(Rotation::Left, 10.0, 10.0), None);
    }

    #[test]
    fn a_turned_touch_screen_maps_its_corners_to_its_corners() {
        for rotation in [Rotation::Right, Rotation::UpsideDown, Rotation::Left] {
            let layout = with(ds(), |s| {
                s.rotation = rotation;
                s.desktop.one_screen = true;
                s.desktop.swapped = true;
            });
            let target = one_to_one(layout);
            let mut corners: Vec<_> = [
                target.left_top(),
                target.right_top(),
                target.left_bottom(),
                target.right_bottom(),
            ]
            .into_iter()
            .map(|c| {
                let inside = c + (target.center() - c).normalized() * 0.1;
                layout.touch_at(target, inside).map(|t| (t.x, t.y))
            })
            .collect();
            corners.sort();
            assert_eq!(
                corners,
                [
                    Some((0, 0)),
                    Some((0, 191)),
                    Some((255, 0)),
                    Some((255, 191))
                ],
                "{rotation:?}"
            );
        }
    }

    fn two_colours() -> Vec<u8> {
        two_colours_at(1)
    }

    fn two_colours_at(scale: usize) -> Vec<u8> {
        let mut frame = vec![0; 256 * 384 * 4 * scale * scale];
        for (index, pixel) in frame.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let shade = if index < 256 * 192 * scale * scale {
                10
            } else {
                200
            };
            pixel.copy_from_slice(&[shade, shade, shade, 255]);
        }
        frame
    }

    fn shade_at(out: &[u8], width: usize, x: usize, y: usize) -> u8 {
        out[(y * width + x) * 4]
    }

    #[test]
    fn a_picture_of_two_screens_takes_the_size_and_order_shown() {
        for (name, change, size, first, last) in [
            ("stacked", None, (256, 384), 10, 200),
            (
                "swapped",
                Some(Placement {
                    swapped: true,
                    ..Placement::default()
                }),
                (256, 384),
                200,
                10,
            ),
            (
                "side by side",
                Some(Placement {
                    arrangement: Arrangement::SideBySide,
                    ..Placement::default()
                }),
                (512, 192),
                10,
                200,
            ),
            (
                "one screen",
                Some(Placement {
                    one_screen: true,
                    ..Placement::default()
                }),
                (256, 192),
                10,
                10,
            ),
        ] {
            let layout = with(ds(), |s| {
                if let Some(placement) = change {
                    s.desktop = placement;
                }
            });
            for scale in [1, 2] {
                let frame = two_colours_at(scale);
                let mut out = Vec::new();
                layout.compose(&frame, scale, &mut out);
                let (width, height) = layout.frame_size(scale);
                assert_eq!((width, height), (size.0 * scale, size.1 * scale), "{name}");
                assert_eq!(out.len(), width * height * 4, "{name}");
                assert_eq!(shade_at(&out, width, 0, 0), first, "{name}");
                assert_eq!(shade_at(&out, width, width - 1, height - 1), last, "{name}");
            }
        }
    }

    #[test]
    fn a_picture_keeps_the_gap_the_small_screen_and_the_turn() {
        let frame = two_colours();
        let gapped = with(ds(), |s| s.gap = 8);
        let mut out = Vec::new();
        gapped.compose(&frame, 1, &mut out);
        assert_eq!(gapped.frame_size(1), (256, 392));
        assert_eq!(shade_at(&out, 256, 100, 195), 0);
        assert_eq!(shade_at(&out, 256, 100, 391), 200);

        let big = with(ds(), |s| {
            s.desktop.arrangement = Arrangement::BigAndSmall;
            s.small = SmallSize::Half;
        });
        assert_eq!(big.frame_size(1), (384, 192));
        big.compose(&frame, 1, &mut out);
        assert_eq!(out.len(), 384 * 192 * 4);

        let turned = with(ds(), |s| s.rotation = Rotation::Right);
        assert_eq!(turned.frame_size(1), (384, 256));
        turned.compose(&frame, 1, &mut out);
        assert_eq!(shade_at(&out, 384, 383, 0), 10);
        assert_eq!(shade_at(&out, 384, 0, 0), 200);
    }
}
