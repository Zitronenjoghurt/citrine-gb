use super::Shown;
use crate::nds::geometry::{self, Worker};
use crate::nds::render::{Renderer, Stencil};
use crate::nds::video::{HEIGHT, Line3d};
use citrine_gb_threads::{Claim, work};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

const BANDS: usize = 8;
const BAND: usize = HEIGHT / BANDS;

static SERIALS: AtomicU64 = AtomicU64::new(1);

struct Shared {
    shown: Claim<Arc<Shown>>,
    renderers: Vec<Claim<Renderer>>,
    serial: AtomicU64,
    next: AtomicUsize,
    drawn: AtomicUsize,
    stencil: Claim<Option<Stencil>>,
    lines: Sender<(usize, Line3d)>,
    side_lines: AtomicU64,
    side_geometry: AtomicU64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Drew,
    Done,
}

impl Shared {
    fn step(&self, between: &mut dyn FnMut()) -> Step {
        let band = self.next.fetch_add(1, Ordering::AcqRel);
        if band >= BANDS {
            return Step::Done;
        }
        let serial = self.serial.load(Ordering::Relaxed);
        let shown = hold(&self.shown, |shown| Arc::clone(shown));
        let rows = band * BAND..(band + 1) * BAND;
        let mut end = loop {
            let drew = self.renderers.iter().find_map(|renderer| {
                renderer.try_with(|renderer| {
                    renderer.start_frame(shown.registers);
                    renderer.share_frame(serial);
                    renderer.render_band(
                        rows.clone(),
                        &shown.frame,
                        &shown.textures,
                        &shown.stencil,
                        |y, line| {
                            let _ = self.lines.send((y, line.clone()));
                            between();
                        },
                    )
                })
            });
            match drew {
                Some(end) => break end,
                None => std::hint::spin_loop(),
            }
        };
        drop(shown);
        if end.is_some() {
            hold(&self.stencil, |stencil| *stencil = end.take());
        }
        self.drawn.fetch_add(1, Ordering::AcqRel);
        Step::Drew
    }
}

fn hold<T, R>(claim: &Claim<T>, mut f: impl FnMut(&mut T) -> R) -> R {
    loop {
        if let Some(out) = claim.try_with(&mut f) {
            return out;
        }
        std::hint::spin_loop();
    }
}

enum Signal {
    Go,
    Geometry,
    Stop,
}

pub(super) struct Link {
    shared: Arc<Shared>,
    lines: Receiver<(usize, Line3d)>,
    go: Vec<Sender<Signal>>,
    got: [bool; HEIGHT],
    received: usize,
}

pub struct Task {
    shared: Arc<Shared>,
    go: Receiver<Signal>,
    geometry: Option<Worker>,
}

pub struct Ending(Vec<Sender<Signal>>);

impl Drop for Ending {
    fn drop(&mut self) {
        for go in &self.0 {
            let _ = go.send(Signal::Stop);
        }
    }
}

pub(super) fn lend(
    shown: Box<Shown>,
    mut renderers: Vec<Renderer>,
    tasks: usize,
) -> (Link, Vec<Task>, Ending, Box<dyn Fn() + Send>) {
    let tasks = tasks.max(1);
    renderers.resize_with(tasks + 1, Renderer::default);
    let (lines, received) = mpsc::channel();
    let shared = Arc::new(Shared {
        shown: Claim::new(Arc::new(*shown)),
        renderers: renderers.into_iter().map(Claim::new).collect(),
        serial: AtomicU64::new(0),
        next: AtomicUsize::new(BANDS),
        drawn: AtomicUsize::new(BANDS),
        stencil: Claim::new(None),
        lines,
        side_lines: AtomicU64::new(0),
        side_geometry: AtomicU64::new(0),
    });
    let (go, signals): (Vec<_>, Vec<_>) = (0..tasks).map(|_| mpsc::channel()).unzip();
    let nudge = go[0].clone();
    let link = Link {
        shared: Arc::clone(&shared),
        lines: received,
        go: go.clone(),
        got: [true; HEIGHT],
        received: HEIGHT,
    };
    let tasks = signals
        .into_iter()
        .map(|go| Task {
            shared: Arc::clone(&shared),
            go,
            geometry: None,
        })
        .collect();
    (
        link,
        tasks,
        Ending(go),
        Box::new(move || {
            let _ = nudge.send(Signal::Geometry);
        }),
    )
}

impl Link {
    pub(super) fn load(
        &mut self,
        lines: &mut [Line3d],
        settle: bool,
        load: impl FnOnce(&mut Shown),
    ) {
        self.wait_all(lines);
        let mut end = hold(&self.shared.stencil, Option::take);
        let mut load = Some(load);
        while load.is_some() {
            hold(&self.shared.shown, |shown| {
                if let Some(shown) = Arc::get_mut(shown) {
                    if let Some(end) = end.take() {
                        shown.stencil = end;
                    }
                    if settle {
                        hold(&self.shared.renderers[0], |renderer| {
                            super::settle(
                                renderer,
                                &shown.frame,
                                shown.registers,
                                &shown.textures,
                                &mut shown.stencil,
                            )
                        });
                    }
                    if let Some(load) = load.take() {
                        load(shown);
                    }
                }
            });
        }
    }

    pub(super) fn go(&mut self) {
        self.shared
            .serial
            .store(SERIALS.fetch_add(1, Ordering::Relaxed), Ordering::Relaxed);
        self.shared.drawn.store(0, Ordering::Release);
        self.shared.next.store(0, Ordering::Release);
        self.got = [false; HEIGHT];
        self.received = 0;
        for go in &self.go {
            let _ = go.send(Signal::Go);
        }
    }

    pub(super) fn wait_for(&mut self, y: usize, lines: &mut [Line3d]) {
        loop {
            self.receive(lines);
            if self.got[y] {
                return;
            }
            self.help(y);
        }
    }

    fn wait_all(&mut self, lines: &mut [Line3d]) {
        loop {
            self.receive(lines);
            if self.received == HEIGHT && self.shared.drawn.load(Ordering::Acquire) >= BANDS {
                return;
            }
            self.help(HEIGHT - 1);
        }
    }

    fn receive(&mut self, lines: &mut [Line3d]) {
        for (row, line) in self.lines.try_iter() {
            lines[row] = line;
            self.got[row] = true;
            self.received += 1;
        }
    }

    fn help(&self, y: usize) {
        if self.shared.next.load(Ordering::Acquire) * BAND <= y {
            self.shared.step(&mut || {});
        } else {
            std::hint::spin_loop();
        }
    }

    pub(super) fn come_home(
        mut self,
        lines: &mut [Line3d],
    ) -> (Box<Shown>, Vec<Renderer>, [u64; 2]) {
        self.wait_all(lines);
        let Self { shared, .. } = self;
        let shared = Arc::into_inner(shared).expect("the 3d task ends with its frame");
        let side = [
            shared.side_lines.into_inner(),
            shared.side_geometry.into_inner(),
        ];
        let mut shown = Arc::into_inner(shared.shown.into_inner())
            .expect("no band holds the frame once every band is drawn");
        if let Some(end) = shared.stencil.into_inner() {
            shown.stencil = end;
        }
        let renderers = shared
            .renderers
            .into_iter()
            .map(Claim::into_inner)
            .collect();
        (Box::new(shown), renderers, side)
    }
}

impl Task {
    pub(super) fn add_geometry(&mut self, worker: Worker) {
        self.geometry = Some(worker);
    }

    pub fn run(self) {
        let (mut drew, mut ran) = (0, 0);
        let worker = self.geometry.as_ref();
        let shape = || worker.map_or(geometry::Step::Idle, Worker::step);
        while let Ok(Signal::Go | Signal::Geometry) = self.go.recv() {
            work("3d", || {
                loop {
                    let lines = self.shared.step(&mut || {
                        ran += u64::from(shape() == geometry::Step::Ran);
                    });
                    let shapes = shape();
                    drew += if lines == Step::Drew { BAND as u64 } else { 0 };
                    ran += u64::from(shapes == geometry::Step::Ran);
                    match (lines, shapes) {
                        (Step::Done, geometry::Step::Idle) => break,
                        (Step::Drew, _) | (_, geometry::Step::Ran) => {}
                        _ => std::hint::spin_loop(),
                    }
                }
            });
        }
        self.shared.side_lines.fetch_add(drew, Ordering::Relaxed);
        self.shared.side_geometry.fetch_add(ran, Ordering::Relaxed);
    }
}
