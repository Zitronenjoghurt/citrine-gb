use super::{ENGINE_BYTES, Screens};
use crate::nds::video::{Engine, EngineKind, Layered, Line3d, Look, Memory, Shade, WIDTH};
use crate::nds::vram::{EngineView, Vram};
use citrine_gb_threads::{Claim, work};
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::Thread;

const WAKE_AT: usize = 64;
const SPINS: u32 = 64;

pub(super) struct Job {
    y: usize,
    kind: EngineKind,
    look: Look,
    top: bool,
    pub palette: [u8; ENGINE_BYTES],
    pub oam: [u8; ENGINE_BYTES],
    pub three_d: Line3d,
    pub with_3d: bool,
    pub layered: bool,
    row: [u32; WIDTH],
    line: Vec<Layered>,
    scroll: u16,
    master: Shade,
}

impl Job {
    fn blank(y: usize, kind: EngineKind, look: Look, top: bool) -> Box<Self> {
        Box::new(Self {
            y,
            kind,
            look,
            top,
            palette: [0; ENGINE_BYTES],
            oam: [0; ENGINE_BYTES],
            three_d: Line3d::default(),
            with_3d: false,
            layered: false,
            row: [0; WIDTH],
            line: Vec::new(),
            scroll: 0,
            master: Shade::default(),
        })
    }
}

type Parcel = (Box<Job>, Arc<Vram>);

struct Painter {
    engines: [Engine; 2],
}

impl Default for Painter {
    fn default() -> Self {
        Self {
            engines: [Engine::new(EngineKind::A), Engine::new(EngineKind::B)],
        }
    }
}

impl Painter {
    fn draw(&mut self, job: &mut Job, vram: &Vram) {
        let engine = &mut self.engines[job.kind as usize];
        let memory = Memory {
            vram: &EngineView::new(vram, job.kind),
            palette: &job.palette,
            oam: &job.oam,
            three_d: job.with_3d.then_some(&job.three_d),
        };
        engine.paint(&job.look, job.y, &memory, job.layered, &mut job.row);
        if job.layered {
            job.line.resize(WIDTH, Layered::default());
            job.scroll = engine.layered_line(&job.palette, &job.row, &mut job.line);
            job.master = engine.master_shade();
        }
    }
}

struct Shared {
    queue: Claim<VecDeque<Parcel>>,
    queued: AtomicUsize,
    stop: AtomicBool,
    done: Sender<Box<Job>>,
    sleeping: AtomicUsize,
    sleepers: Claim<Vec<Thread>>,
}

impl Shared {
    fn take(&self) -> Option<Parcel> {
        while self.queued.load(Ordering::Acquire) > 0 {
            let taken = self.queue.try_with(|queue| {
                let job = queue.pop_front();
                if job.is_some() {
                    self.queued.fetch_sub(1, Ordering::Release);
                }
                job
            });
            match taken {
                Some(job) => return job,
                None => std::hint::spin_loop(),
            }
        }
        None
    }

    fn put(&self, parcel: Parcel) {
        let mut parcel = Some(parcel);
        while self
            .queue
            .try_with(|queue| {
                if let Some(parcel) = parcel.take() {
                    queue.push_back(parcel);
                    self.queued.fetch_add(1, Ordering::SeqCst);
                }
            })
            .is_none()
        {
            std::hint::spin_loop();
        }
        if self.queued.load(Ordering::Relaxed) >= WAKE_AT {
            self.wake(false);
        }
    }

    fn wake(&self, all: bool) {
        if self.sleeping.load(Ordering::SeqCst) == 0 {
            return;
        }
        let mut woken = Vec::new();
        self.sleepers_with(|sleepers| {
            let keep = if all {
                0
            } else {
                sleepers.len().saturating_sub(1)
            };
            woken.extend(sleepers.drain(keep..));
        });
        woken.iter().for_each(Thread::unpark);
    }

    fn sleep(&self) {
        let me = std::thread::current();
        self.sleepers_with(|sleepers| sleepers.push(me.clone()));
        if self.queued.load(Ordering::SeqCst) < WAKE_AT && !self.stop.load(Ordering::SeqCst) {
            std::thread::park();
        }
        self.sleepers_with(|sleepers| sleepers.retain(|thread| thread.id() != me.id()));
    }

    fn sleepers_with(&self, f: impl FnOnce(&mut Vec<Thread>)) {
        let mut f = Some(f);
        while self
            .sleepers
            .try_with(|sleepers| {
                if let Some(f) = f.take() {
                    f(sleepers);
                }
                self.sleeping.store(sleepers.len(), Ordering::SeqCst);
            })
            .is_none()
        {
            std::hint::spin_loop();
        }
    }

    fn end(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.wake(true);
    }
}

pub struct Task {
    shared: Arc<Shared>,
    painter: Painter,
}

impl Task {
    pub fn run(mut self) {
        let mut idle = 0;
        loop {
            if let Some((mut job, vram)) = self.shared.take() {
                work("2d lines", || {
                    self.painter.draw(&mut job, &vram);
                    drop(vram);
                    let _ = self.shared.done.send(job);
                });
                idle = 0;
            } else if self.shared.stop.load(Ordering::Acquire) {
                return;
            } else if idle < SPINS {
                idle += 1;
                std::hint::spin_loop();
            } else {
                self.shared.sleep();
                idle = 0;
            }
        }
    }
}

pub struct Ending(Arc<Shared>);

impl Drop for Ending {
    fn drop(&mut self) {
        self.0.end();
    }
}

struct Link {
    shared: Arc<Shared>,
    done: Receiver<Box<Job>>,
    spare: Vec<Box<Job>>,
    painter: Painter,
    shipped: usize,
    applied: usize,
    painted: u64,
}

impl Link {
    fn collect(&mut self, screens: &mut Screens) {
        while let Ok(job) = self.done.try_recv() {
            apply(&job, screens);
            self.spare.push(job);
            self.applied += 1;
            self.painted += 1;
        }
    }

    fn finish(&mut self, screens: &mut Screens) {
        if self.applied < self.shipped {
            self.shared.wake(true);
        }
        while self.applied < self.shipped {
            if let Some((mut job, vram)) = self.shared.take() {
                self.painter.draw(&mut job, &vram);
                apply(&job, screens);
                self.spare.push(job);
                self.applied += 1;
            } else {
                self.collect(screens);
                std::hint::spin_loop();
            }
        }
    }
}

fn apply(job: &Job, screens: &mut Screens) {
    screens.put_row(job.y, job.top, &job.row);
    if job.layered
        && let Some(composite) = screens.composite.as_mut()
    {
        composite.line_mut(job.y).copy_from_slice(&job.line);
        composite.end_line(job.y, Ok(job.scroll), job.master);
    }
}

#[derive(Default)]
struct Away(Option<Link>);

impl Clone for Away {
    fn clone(&self) -> Self {
        Self(None)
    }
}

#[derive(Clone, Default)]
pub(super) struct Lines {
    copy: Arc<Vram>,
    away: Away,
    painted: u64,
}

impl Lines {
    pub fn lend(&mut self, painters: usize) -> (Vec<Task>, Option<Ending>) {
        if painters == 0 {
            return (Vec::new(), None);
        }
        let (done, received) = mpsc::channel();
        let shared = Arc::new(Shared {
            queue: Claim::new(VecDeque::new()),
            queued: AtomicUsize::new(0),
            stop: AtomicBool::new(false),
            done,
            sleeping: AtomicUsize::new(0),
            sleepers: Claim::new(Vec::new()),
        });
        self.away.0 = Some(Link {
            shared: Arc::clone(&shared),
            done: received,
            spare: Vec::new(),
            painter: Painter::default(),
            shipped: 0,
            applied: 0,
            painted: 0,
        });
        let tasks = (0..painters)
            .map(|_| Task {
                shared: Arc::clone(&shared),
                painter: Painter::default(),
            })
            .collect();
        (tasks, Some(Ending(shared)))
    }

    pub fn painted(&self) -> u64 {
        self.painted
    }

    pub fn sync(&mut self, vram: &mut Vram, screens: &mut Screens) {
        let Some(link) = self.away.0.as_mut() else {
            return;
        };
        link.collect(screens);
        if vram.copy_is_stale(&self.copy) {
            link.finish(screens);
            vram.refresh(Arc::make_mut(&mut self.copy));
        }
    }

    pub fn job(&mut self, y: usize, kind: EngineKind, look: Look, top: bool) -> Option<Box<Job>> {
        let link = self.away.0.as_mut()?;
        Some(match link.spare.pop() {
            Some(mut job) => {
                job.y = y;
                job.kind = kind;
                job.look = look;
                job.top = top;
                job
            }
            None => Job::blank(y, kind, look, top),
        })
    }

    pub fn ship(&mut self, job: Box<Job>) {
        if let Some(link) = self.away.0.as_mut() {
            link.shared.put((job, Arc::clone(&self.copy)));
            link.shipped += 1;
        }
    }

    pub fn finish(&mut self, screens: &mut Screens) {
        if let Some(link) = self.away.0.as_mut() {
            link.finish(screens);
        }
    }

    pub fn come_home(&mut self, screens: &mut Screens) {
        if let Some(mut link) = self.away.0.take() {
            link.finish(screens);
            self.painted += link.painted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::Buses;
    use crate::gba::wait::Width;
    use crate::nds::lcd::{FRAME_CYCLES, LINE_CYCLES, LINES, VISIBLE_LINES};
    use crate::nds::video::{Fallback, HEIGHT, Layered, Shade};
    use citrine_gb_threads::{Mode, Side, Split, Threads};

    const POWCNT: u32 = 0x0400_0304;
    const POWER_SWAPPED: u32 = 0x8203;
    const POWER_B_OFF: u32 = 0x8003;
    const VRAMCNT: u32 = 0x0400_0240;
    const VRAMCNT_I: u32 = 0x0400_0249;
    const ENGINE_B: u32 = 0x1000;
    const DISPCNT: u32 = 0x0400_0000;
    const DISPLAY_LAYERS: u32 = 0x4001_1310;
    const THREE_D: u32 = 1 << 3;
    const BG0CNT: u32 = 0x0400_0008;
    const BG1CNT: u32 = 0x0400_000A;
    const BG0HOFS: u32 = 0x0400_0010;
    const DISPCAPCNT: u32 = 0x0400_0064;
    const CAPTURE_GRAPHICS_TO_D: u32 = 1 << 31 | 3 << 16 | 3 << 20 | 16;
    const MASTER_BRIGHT: u32 = 0x0400_006C;
    const BLDCNT: u32 = 0x0400_0050;
    const PALETTE: u32 = 0x0500_0000;
    const OAM: u32 = 0x0700_0000;
    const BG_A: u32 = 0x0600_0000;
    const BG_B: u32 = 0x0620_0000;
    const OBJ_A: u32 = 0x0640_0000;
    const OBJ_B: u32 = 0x0660_0000;
    const LCDC_E: u32 = 0x0688_0000;
    const VBLANK_CYCLES: u32 = LINE_CYCLES * (LINES - VISIBLE_LINES) as u32;
    const THIRD: u32 = LINE_CYCLES * VISIBLE_LINES as u32 / 3;

    struct Noise(u32);

    impl Noise {
        fn next(&mut self) -> u32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 17;
            self.0 ^= self.0 << 5;
            self.0
        }

        fn fill(&mut self, buses: &mut Buses, start: u32, bytes: u32) {
            for at in (start..start + bytes).step_by(4) {
                let value = self.next();
                buses.arm9().write(at, Width::Word, value);
            }
        }

        fn scatter(&mut self, buses: &mut Buses, start: u32, bytes: u32, count: usize) {
            for _ in 0..count {
                let at = (start + self.next() % bytes) & !3;
                let value = self.next();
                buses.arm9().write(at, Width::Word, value);
            }
        }
    }

    fn write(buses: &mut Buses, at: u32, width: Width, value: u32) {
        buses.arm9().write(at, width, value);
    }

    fn scene() -> Buses {
        let mut buses = Buses::default();
        let mut noise = Noise(0x2545_F491);
        write(&mut buses, POWCNT, Width::Half, POWER_SWAPPED);
        write(&mut buses, VRAMCNT, Width::Word, 0x8084_8281);
        write(&mut buses, VRAMCNT + 4, Width::Byte, 0x80);
        write(&mut buses, VRAMCNT_I, Width::Byte, 0x82);
        noise.fill(&mut buses, LCDC_E, 0x8000);
        write(&mut buses, VRAMCNT + 4, Width::Byte, 0x84);
        for engine in [0, ENGINE_B] {
            write(&mut buses, DISPCNT + engine, Width::Word, DISPLAY_LAYERS);
            write(&mut buses, BG0CNT + engine, Width::Half, 0x0100);
            write(&mut buses, BG1CNT + engine, Width::Half, 0x0285);
        }
        noise.fill(&mut buses, BG_A, 0x1_0000);
        noise.fill(&mut buses, BG_B, 0x1_0000);
        noise.fill(&mut buses, OBJ_A, 0x8000);
        noise.fill(&mut buses, OBJ_B, 0x4000);
        noise.fill(&mut buses, PALETTE, 0x800);
        noise.fill(&mut buses, OAM, 0x800);
        for sprite in (16..256).map(|sprite| OAM + sprite * 8) {
            write(&mut buses, sprite, Width::Half, 1 << 9);
        }
        buses.record_composite(true);
        buses.elapse(LINE_CYCLES * u32::from(VISIBLE_LINES));
        buses
    }

    fn change(buses: &mut Buses, noise: &mut Noise, frame: u32, third: u32) {
        noise.scatter(buses, BG_A, 0x1_0000, 64);
        noise.scatter(buses, BG_B, 0x1_0000, 64);
        noise.scatter(buses, PALETTE, 0x800, 16);
        let scroll = noise.next();
        write(buses, BG0HOFS, Width::Half, scroll);
        write(buses, BG0HOFS + ENGINE_B, Width::Half, scroll >> 16);
        if frame == 2 {
            let control = if third == 0 { 0x80 } else { 0x84 };
            write(buses, VRAMCNT + 2, Width::Byte, control);
        }
        if frame == 4 {
            let master = if third == 0 { 0x8010 } else { 0x4003 };
            write(buses, MASTER_BRIGHT + ENGINE_B, Width::Half, master);
        }
    }

    fn frame(buses: &mut Buses, frame: u32) {
        let mut noise = Noise(0x9E37_79B9 ^ frame);
        match frame {
            1 => write(buses, DISPCAPCNT, Width::Word, CAPTURE_GRAPHICS_TO_D),
            3 => write(buses, POWCNT, Width::Half, POWER_B_OFF),
            4 => {
                write(buses, POWCNT, Width::Half, POWER_SWAPPED);
                write(buses, MASTER_BRIGHT, Width::Half, 0x4008);
                write(buses, BLDCNT, Width::Half, 0x0241);
                write(buses, DISPCNT, Width::Word, DISPLAY_LAYERS | THREE_D);
            }
            _ => {}
        }
        buses.elapse(VBLANK_CYCLES);
        buses.elapse(THIRD);
        change(buses, &mut noise, frame, 0);
        buses.elapse(THIRD);
        change(buses, &mut noise, frame, 1);
        buses.elapse(THIRD);
    }

    #[derive(Debug, PartialEq)]
    struct Shot {
        top: Vec<u32>,
        bottom: Vec<u32>,
        layered: Vec<Layered>,
        scrolls: Vec<u16>,
        masters: Vec<Shade>,
        fallback: Option<Fallback>,
        shows_3d: bool,
    }

    fn frames(mut run: impl FnMut(&mut Buses, u32)) -> Vec<Shot> {
        let mut buses = scene();
        (0..5)
            .map(|n| {
                run(&mut buses, n);
                let screens = buses.screens();
                let composite = screens.composite().unwrap();
                Shot {
                    top: screens.top().to_vec(),
                    bottom: screens.bottom().to_vec(),
                    layered: composite.pixels().to_vec(),
                    scrolls: (0..HEIGHT).map(|y| composite.scroll(y)).collect(),
                    masters: (0..HEIGHT).map(|y| composite.master(y)).collect(),
                    fallback: composite.fallback(),
                    shows_3d: composite.shows_3d(),
                }
            })
            .collect()
    }

    fn lent_frame(buses: &mut Buses, n: u32, split: &mut Split<'_>, painters: usize) {
        let (tasks, ending) = buses.lend_lines(painters);
        let sides: Vec<(&str, Side<'_>)> = tasks
            .into_iter()
            .map(|task| ("2d lines", Box::new(move || task.run()) as Side<'_>))
            .collect();
        split.beside(sides, || {
            let _ending = ending;
            frame(buses, n);
            buses.finish_lines();
        });
        buses.home_lines();
    }

    #[test]
    fn lines_drawn_on_the_pool_or_by_the_caller_match_the_inline_ones() {
        let inline = frames(frame);
        assert!(inline.windows(2).all(|pair| pair[0] != pair[1]));
        let threads = Threads::new(Mode::Threads);
        let pooled = threads.split(|split| frames(|buses, n| lent_frame(buses, n, split, 2)));
        assert_eq!(pooled, inline);
        let one = threads.split(|split| frames(|buses, n| lent_frame(buses, n, split, 1)));
        assert_eq!(one, inline);
        let caller = frames(|buses, n| lent_frame(buses, n, &mut Split::inline(), 2));
        assert_eq!(caller, inline);
    }

    #[test]
    fn a_frame_left_early_comes_home_with_every_line_drawn() {
        let mut inline = scene();
        let mut lent = scene();
        inline.elapse(VBLANK_CYCLES + THIRD * 3);
        let (tasks, ending) = lent.lend_lines(1);
        lent.elapse(VBLANK_CYCLES + THIRD * 2);
        drop(ending);
        tasks.into_iter().for_each(super::Task::run);
        lent.home_lines();
        lent.elapse(THIRD);
        assert_eq!(lent.screens().top(), inline.screens().top());
        assert_eq!(lent.screens().bottom(), inline.screens().bottom());
    }

    #[test]
    fn sleep_blanks_the_lines_still_waiting_for_a_painter() {
        let mut lent = scene();
        let (tasks, ending) = lent.lend_lines(1);
        lent.elapse(VBLANK_CYCLES + THIRD * 2);
        assert!(lent.sleep(FRAME_CYCLES));
        drop(ending);
        tasks.into_iter().for_each(super::Task::run);
        lent.home_lines();
        assert!(lent.screens().top().iter().all(|&pixel| pixel == 0));
        assert!(lent.screens().bottom().iter().all(|&pixel| pixel == 0));
    }
}
