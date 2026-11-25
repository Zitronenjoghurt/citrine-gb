mod away;
mod clip;
pub mod command;
mod engine;
mod fifo;
pub mod frame;
mod light;
mod matrix;
mod pipeline;
mod polygon;
mod query;
mod texcoord;

pub use away::{Step, Worker};
use command::*;
use engine::Engine;
use fifo::Fifo;
use frame::Frame;
use matrix::Stack;
use pipeline::Pipelines;

pub const FIRST: usize = 0x400;
pub const LAST: usize = 0x6A3;

pub const GXFIFO_END: usize = 0x040;
const PORTS_END: usize = 0x200;
const GXSTAT: usize = 0x200;
const RAM_COUNT: usize = 0x204;
const DOT_DEPTH: usize = 0x210;
const VERTEX_COUNT_SHIFT: u32 = 16;
const POS_RESULT: usize = 0x220;
const POS_RESULT_END: usize = 0x230;
const VEC_RESULT: usize = 0x230;
const VEC_RESULT_Z: usize = 0x234;
const CLIPMTX_RESULT: usize = 0x240;
const CLIPMTX_RESULT_END: usize = 0x280;
const VECMTX_RESULT: usize = 0x280;
const VECMTX_RESULT_END: usize = 0x2A4;
const RENDER_FAR: u32 = 1 << 12;

const TEST_BUSY: u32 = 1 << 0;
const BOX_INSIDE: u32 = 1 << 1;
const POSITION_LEVEL_SHIFT: u32 = 8;
const PROJECTION_LEVEL: u32 = 1 << 13;
const STACK_BUSY: u32 = 1 << 14;
const STACK_ERROR: u32 = 1 << 15;
const LEVEL_SHIFT: u32 = 16;
const HALF_EMPTY: u32 = 1 << 25;
const EMPTY: u32 = 1 << 26;
const BUSY: u32 = 1 << 27;
const IRQ_SHIFT: u32 = 30;
const IRQ_HALF: u32 = 1;
const IRQ_EMPTY: u32 = 2;
const LIGHTS: u32 = 0xF;
const COLOR_MASK: u32 = 0x1F;

#[derive(Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Geometry {
    fifo: Fifo,
    params: Vec<u32>,
    command: u8,
    busy: u32,
    #[cfg_attr(feature = "serde", serde(default))]
    pipelines: Pipelines,
    irq_mode: u32,
    stack: Stack,
    pending_attributes: u32,
    attributes: u32,
    swap: bool,
    box_ran: bool,
    box_inside: bool,
    engine: Option<Box<Engine>>,
    #[cfg_attr(feature = "serde", serde(skip))]
    away: Away,
}

impl Clone for Geometry {
    fn clone(&self) -> Self {
        Self {
            fifo: self.fifo.clone(),
            params: self.params.clone(),
            command: self.command,
            busy: self.busy,
            pipelines: self.pipelines,
            irq_mode: self.irq_mode,
            stack: self.stack,
            pending_attributes: self.pending_attributes,
            attributes: self.attributes,
            swap: self.swap,
            box_ran: self.box_ran,
            box_inside: self.box_inside,
            engine: self.engine.clone(),
            away: self.away.clone(),
        }
    }

    fn clone_from(&mut self, source: &Self) {
        let Self {
            fifo,
            params,
            command,
            busy,
            pipelines,
            irq_mode,
            stack,
            pending_attributes,
            attributes,
            swap,
            box_ran,
            box_inside,
            engine,
            away,
        } = self;
        fifo.clone_from(&source.fifo);
        params.clone_from(&source.params);
        command.clone_from(&source.command);
        busy.clone_from(&source.busy);
        pipelines.clone_from(&source.pipelines);
        irq_mode.clone_from(&source.irq_mode);
        stack.clone_from(&source.stack);
        pending_attributes.clone_from(&source.pending_attributes);
        attributes.clone_from(&source.attributes);
        swap.clone_from(&source.swap);
        box_ran.clone_from(&source.box_ran);
        box_inside.clone_from(&source.box_inside);
        engine.clone_from(&source.engine);
        away.clone_from(&source.away);
    }
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            fifo: Fifo::default(),
            params: Vec::new(),
            command: 0,
            busy: 0,
            pipelines: Pipelines::default(),
            irq_mode: 0,
            stack: Stack::default(),
            pending_attributes: 0,
            attributes: 0,
            swap: false,
            box_ran: false,
            box_inside: false,
            engine: Some(Box::default()),
            away: Away::default(),
        }
    }
}

#[derive(Default)]
struct Away(Option<away::Link>);

impl Clone for Away {
    fn clone(&self) -> Self {
        Self(None)
    }
}

impl PartialEq for Away {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Away {}

impl std::hash::Hash for Away {
    fn hash<H: std::hash::Hasher>(&self, _: &mut H) {}
}

impl std::fmt::Debug for Away {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() { "away" } else { "home" })
    }
}

const HOME: &str = "the geometry engine is home between frames";

impl Geometry {
    pub fn read(&mut self, offset: usize) -> u32 {
        let offset = offset & !3;
        match offset {
            GXSTAT if self.box_ran => {
                self.box_ran = false;
                self.box_inside = self.with_engine(|engine| engine.box_inside());
                self.status()
            }
            RAM_COUNT
            | POS_RESULT..POS_RESULT_END
            | VEC_RESULT
            | VEC_RESULT_Z
            | CLIPMTX_RESULT..CLIPMTX_RESULT_END
            | VECMTX_RESULT..VECMTX_RESULT_END => self.with_engine(|engine| engine.read(offset)),
            _ => self.peek(offset),
        }
    }

    pub fn peek(&self, offset: usize) -> u32 {
        match offset & !3 {
            GXSTAT => self.status(),
            offset => self
                .engine
                .as_deref()
                .map_or(0, |engine| engine.read(offset)),
        }
    }

    pub fn write(&mut self, offset: usize, value: u32, mask: u32) {
        let offset = offset & !3;
        match offset {
            0..GXFIFO_END => self.fifo.write_packed(value & mask),
            GXFIFO_END..PORTS_END => self.fifo.write_port((offset >> 2) as u8, value & mask),
            GXSTAT => {
                if value & mask & STACK_ERROR != 0 {
                    self.stack.acknowledge_error();
                    self.post(engine::ACKNOWLEDGE_STACK, &[]);
                }
                if mask >> IRQ_SHIFT != 0 {
                    self.irq_mode =
                        (self.irq_mode & !(mask >> IRQ_SHIFT)) | ((value & mask) >> IRQ_SHIFT);
                }
            }
            DOT_DEPTH => self.post(engine::SET_DOT_DEPTH, &[value & mask]),
            _ => {}
        }
    }

    pub fn step(&mut self, cycles: u32) {
        let mut left = cycles;
        loop {
            if self.swap {
                return;
            }
            let spent = left.min(self.busy);
            self.busy -= spent;
            left -= spent;
            if self.busy > 0 {
                return;
            }
            let Some(entry) = self.fifo.pop() else {
                self.pipelines.idle(left);
                return;
            };
            self.busy = self.pipelines.charge(
                entry.command,
                entry.param,
                self.params.len(),
                self.stack.mode(),
                self.lights(),
            );
            if self.params.is_empty() && parameters(entry.command) <= 1 {
                self.command = entry.command;
                self.execute(entry.command, &[entry.param]);
                continue;
            }
            self.params.push(entry.param);
            if self.params.len() < parameters(entry.command).max(1) {
                continue;
            }
            let mut params = std::mem::take(&mut self.params);
            self.command = entry.command;
            self.execute(entry.command, &params);
            params.clear();
            self.params = params;
        }
    }

    pub fn write_packed(&mut self, value: u32) {
        self.fifo.write_packed(value);
    }

    pub fn write_packed_until_full(&mut self, words: &[u32]) -> usize {
        self.fifo.write_packed_until_full(words)
    }

    pub fn busy(&self) -> u32 {
        self.busy
    }

    pub fn running(&self) -> Option<u32> {
        if self.swap {
            return None;
        }
        if self.busy > 0 {
            return Some(self.busy);
        }
        let drain = self.pipelines.drain();
        (drain > 0 && self.fifo.is_empty()).then_some(drain)
    }

    pub fn next_seen(&self, limit: u32, dma_waits: impl Fn() -> bool) -> Option<u32> {
        self.running()?;
        let mut at = self.busy;
        let mut drain = self.fifo.drain();
        let mut pipelines = self.pipelines;
        let mut taken = self.params.len();
        let mut mode = self.stack.mode();
        let mut pending = self.pending_attributes;
        let mut lights = self.lights();
        let mut waits = None;
        loop {
            let before = drain.level();
            let Some(entry) = drain.next_entry() else {
                return Some(at + pipelines.drain());
            };
            let level = drain.level();
            let crossed = before >= fifo::HALF && level < fifo::HALF;
            let emptied = before > 0 && level == 0;
            if crossed || emptied || (level < fifo::HALF && *waits.get_or_insert_with(&dma_waits)) {
                return Some(at);
            }
            if at >= limit {
                return Some(at);
            }
            at += pipelines.charge(entry.command, entry.param, taken, mode, lights);
            taken += 1;
            if taken < parameters(entry.command).max(1) {
                continue;
            }
            taken = 0;
            match entry.command {
                MTX_MODE => mode = matrix::Mode::from_param(entry.param),
                POLYGON_ATTR => pending = entry.param,
                BEGIN_VTXS => lights = pending & LIGHTS,
                SWAP_BUFFERS => return None,
                _ => {}
            }
        }
    }

    pub fn swapping(&self) -> bool {
        self.swap
    }

    pub fn vblank(&mut self) {
        let swapped = std::mem::take(&mut self.swap);
        self.post(engine::VBLANK, &[]);
        if swapped {
            let built = self.with_frame(|frame| frame.polygons.len() as u32);
            self.pipelines.close_frame(built);
        }
    }

    pub fn with_frame<R>(&mut self, f: impl FnOnce(&Frame) -> R) -> R {
        self.with_engine(|engine| f(engine.frame()))
    }

    pub fn ram_overflow(&mut self) -> bool {
        self.with_engine(|engine| engine.overflow())
    }

    pub fn peek_ram_overflow(&self) -> bool {
        self.engine.as_deref().is_some_and(Engine::overflow)
    }

    pub fn acknowledge_ram_overflow(&mut self) {
        self.post(engine::ACKNOWLEDGE_OVERFLOW, &[]);
    }

    pub fn full(&self) -> bool {
        self.fifo.full()
    }

    pub fn irq(&self) -> bool {
        match self.irq_mode {
            IRQ_HALF => self.fifo.level() < fifo::HALF,
            IRQ_EMPTY => self.fifo.level() == 0,
            _ => false,
        }
    }

    pub fn dma_request(&self) -> bool {
        self.fifo.level() < fifo::HALF
    }

    pub fn lend(&mut self, nudge: Box<dyn Fn() + Send>) -> Worker {
        self.come_home();
        let engine = self.engine.take().expect(HOME);
        let (link, worker) = away::lend(engine, nudge);
        self.away.0 = Some(link);
        worker
    }

    pub fn come_home(&mut self) {
        if let Some(link) = self.away.0.take() {
            self.engine = Some(link.come_home());
        }
    }

    fn with_engine<R>(&mut self, f: impl FnOnce(&mut Engine) -> R) -> R {
        match &mut self.away.0 {
            Some(link) => link.with(f),
            None => f(self.engine.as_deref_mut().expect(HOME)),
        }
    }

    fn post(&mut self, command: u8, params: &[u32]) {
        match &mut self.away.0 {
            Some(link) => link.post(command, params),
            None => self
                .engine
                .as_deref_mut()
                .expect(HOME)
                .execute(command, params),
        }
    }

    fn status(&self) -> u32 {
        let box_inside = match self.engine.as_deref() {
            Some(engine) if self.box_ran => engine.box_inside(),
            _ => self.box_inside,
        };
        let level = self.fifo.level();
        let executing = |commands: &[u8]| {
            (self.busy > 0 && commands.contains(&self.command))
                || self.fifo.count(|command| commands.contains(&command)) > 0
        };
        let mut status = u32::from(self.stack.position_pointer() & 0x1F) << POSITION_LEVEL_SHIFT
            | (level as u32) << LEVEL_SHIFT
            | self.irq_mode << IRQ_SHIFT;
        if executing(&[BOX_TEST, POS_TEST, VEC_TEST]) {
            status |= TEST_BUSY;
        }
        if box_inside {
            status |= BOX_INSIDE;
        }
        if self.stack.projection_pointer() != 0 {
            status |= PROJECTION_LEVEL;
        }
        if executing(&[MTX_PUSH, MTX_POP]) {
            status |= STACK_BUSY;
        }
        if self.stack.error() {
            status |= STACK_ERROR;
        }
        if level < fifo::HALF {
            status |= HALF_EMPTY;
        }
        if level == 0 {
            status |= EMPTY;
        }
        if self.busy > 0
            || !self.fifo.is_empty()
            || !self.params.is_empty()
            || self.swap
            || self.pipelines.active()
        {
            status |= BUSY;
        }
        status
    }

    fn lights(&self) -> u32 {
        self.attributes & LIGHTS
    }

    fn execute(&mut self, command: u8, params: &[u32]) {
        let param = params[0];
        match command {
            MTX_MODE => self.stack.set_mode(param),
            MTX_PUSH => {
                self.stack.push();
            }
            MTX_POP => {
                self.stack.pop(param);
            }
            MTX_STORE => {
                self.stack.store(param);
            }
            MTX_RESTORE => {
                self.stack.restore(param);
            }
            POLYGON_ATTR => self.pending_attributes = param,
            BEGIN_VTXS => self.attributes = self.pending_attributes,
            SWAP_BUFFERS => self.swap = true,
            BOX_TEST => self.box_ran = true,
            _ => {}
        }
        self.post(command, params);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: u32 = 0xFFFF_FFFF;

    fn port(command: u8) -> usize {
        usize::from(command) << 2
    }

    fn send(geometry: &mut Geometry, command: u8, params: &[u32]) {
        if params.is_empty() {
            geometry.write(port(command), 0, ALL);
        }
        for &param in params {
            geometry.write(port(command), param, ALL);
        }
    }

    fn run(geometry: &mut Geometry) {
        geometry.step(u32::MAX);
    }

    fn time(geometry: &mut Geometry) -> u32 {
        geometry.step(0);
        let mut total = 0;
        while geometry.busy() > 0 {
            let busy = geometry.busy();
            total += busy;
            geometry.step(busy);
        }
        total
    }

    fn status(geometry: &mut Geometry) -> u32 {
        geometry.read(GXSTAT)
    }

    fn engine(geometry: &Geometry) -> &Engine {
        geometry.engine.as_deref().unwrap()
    }

    #[test]
    fn port_offsets_name_their_command() {
        assert_eq!(port(MTX_MODE), 0x040);
        assert_eq!(port(VEC_TEST), 0x1C8);
        assert_eq!(FIRST + port(SWAP_BUFFERS), 0x540);
    }

    #[test]
    fn idle_engine_reads_empty_and_not_busy() {
        let mut geometry = Geometry::default();
        let status = status(&mut geometry);
        assert_eq!(status & (EMPTY | HALF_EMPTY | BUSY), EMPTY | HALF_EMPTY);
    }

    #[test]
    fn a_queued_command_runs_on_step() {
        let mut geometry = Geometry::default();
        send(&mut geometry, MTX_MODE, &[1]);
        assert_ne!(status(&mut geometry) & BUSY, 0);
        geometry.step(0);
        assert_eq!(geometry.stack.mode(), matrix::Mode::Position);
    }

    #[test]
    fn the_next_seen_end_is_where_the_engine_goes_idle() {
        let mut geometry = Geometry::default();
        assert_eq!(geometry.next_seen(64, || false), None);
        send(&mut geometry, MTX_PUSH, &[]);
        send(&mut geometry, MTX_IDENTITY, &[]);
        geometry.step(0);
        assert_eq!(geometry.next_seen(64, || false), Some(17 + 19));
    }

    #[test]
    fn the_next_seen_end_is_where_the_fifo_falls_below_half() {
        let mut geometry = Geometry::default();
        for _ in 0..fifo::HALF + 10 {
            send(&mut geometry, MTX_IDENTITY, &[]);
        }
        geometry.step(0);
        let level = geometry.fifo.level();
        let at = geometry.next_seen(u32::MAX, || false).unwrap();
        geometry.step(at - 1);
        assert!(geometry.fifo.level() >= fifo::HALF);
        geometry.step(1);
        assert!(geometry.fifo.level() < fifo::HALF);
        assert!(level >= fifo::HALF);
    }

    #[test]
    fn a_waiting_fifo_dma_sees_every_end_below_half() {
        let mut geometry = Geometry::default();
        send(&mut geometry, MTX_PUSH, &[]);
        send(&mut geometry, MTX_IDENTITY, &[]);
        send(&mut geometry, MTX_IDENTITY, &[]);
        geometry.step(0);
        assert_eq!(geometry.next_seen(64, || true), Some(17));
        assert_eq!(geometry.next_seen(64, || false), Some(17 + 19 + 19));
    }

    #[test]
    fn the_next_seen_end_counts_a_cycle_per_light_and_the_normal_drain_like_melonds() {
        let mut geometry = Geometry::default();
        send(&mut geometry, MTX_PUSH, &[]);
        send(&mut geometry, POLYGON_ATTR, &[0xF]);
        send(&mut geometry, BEGIN_VTXS, &[0]);
        send(&mut geometry, NORMAL, &[0]);
        geometry.step(0);
        let at = geometry.next_seen(u32::MAX, || false).unwrap();
        assert_eq!(at, 17 + 1 + 1 + 1 + 4 + 7);
        geometry.step(at - 1);
        assert!(geometry.running().is_some());
        geometry.step(1);
        assert_eq!(geometry.running(), None);
    }

    #[test]
    fn a_swap_hides_the_ends_until_vblank() {
        let mut geometry = Geometry::default();
        send(&mut geometry, MTX_PUSH, &[]);
        send(&mut geometry, SWAP_BUFFERS, &[0]);
        send(&mut geometry, MTX_IDENTITY, &[]);
        geometry.step(0);
        assert_eq!(geometry.next_seen(u32::MAX, || false), None);
    }

    #[test]
    fn command_stays_busy_for_its_cycles() {
        let mut geometry = Geometry::default();
        send(&mut geometry, MTX_PUSH, &[]);
        send(&mut geometry, MTX_MODE, &[1]);
        geometry.step(0);
        assert_eq!(geometry.busy(), 17);
        geometry.step(16);
        assert_eq!(geometry.stack.mode(), matrix::Mode::Projection);
        geometry.step(1);
        assert_eq!(geometry.stack.mode(), matrix::Mode::Position);
        assert_eq!(geometry.busy(), 1);
        geometry.step(1);
        assert_eq!(status(&mut geometry) & BUSY, 0);
    }

    #[test]
    fn a_command_waits_for_all_its_parameters() {
        let mut geometry = Geometry::default();
        send(&mut geometry, MTX_MODE, &[1]);
        send(&mut geometry, MTX_TRANS, &[0x1000, 0x2000]);
        run(&mut geometry);
        assert_eq!(engine(&geometry).matrices.clip().0[12], 0);
        assert_ne!(status(&mut geometry) & BUSY, 0);
        send(&mut geometry, MTX_TRANS, &[0x3000]);
        run(&mut geometry);
        assert_eq!(
            &engine(&geometry).matrices.clip().0[12..15],
            &[0x1000, 0x2000, 0x3000]
        );
    }

    #[test]
    fn packed_writes_reach_the_matrices() {
        let mut geometry = Geometry::default();
        geometry.write(0, 0x0000_1B10, ALL);
        for param in [1, 0x2000, 0x2000, 0x2000] {
            geometry.write(0, param, ALL);
        }
        run(&mut geometry);
        assert_eq!(engine(&geometry).matrices.clip().0[0], 0x2000);
    }

    #[test]
    fn gxfifo_is_mirrored_over_16_words() {
        let mut geometry = Geometry::default();
        geometry.write(0x3C, 0x0000_0010, ALL);
        geometry.write(0x20, 3, ALL);
        run(&mut geometry);
        assert_eq!(geometry.stack.mode(), matrix::Mode::Texture);
    }

    #[test]
    fn simultaneous_multiply_takes_30_more_cycles() {
        let mut times = Vec::new();
        for mode in [1, 2] {
            let mut geometry = Geometry::default();
            send(&mut geometry, MTX_MODE, &[mode]);
            run(&mut geometry);
            send(
                &mut geometry,
                MTX_MULT_3X3,
                &[0x1000, 0, 0, 0, 0x1000, 0, 0, 0, 0x1000],
            );
            times.push(time(&mut geometry));
        }
        assert_eq!(times, [35, 65]);
    }

    #[test]
    fn status_reports_the_stack_levels() {
        let mut geometry = Geometry::default();
        send(&mut geometry, MTX_PUSH, &[]);
        send(&mut geometry, MTX_MODE, &[1]);
        send(&mut geometry, MTX_PUSH, &[]);
        send(&mut geometry, MTX_PUSH, &[]);
        run(&mut geometry);
        let status = status(&mut geometry);
        assert_eq!((status >> POSITION_LEVEL_SHIFT) & 0x1F, 2);
        assert_ne!(status & PROJECTION_LEVEL, 0);
    }

    #[test]
    fn status_shows_the_stack_busy_while_a_push_waits_or_runs() {
        let mut geometry = Geometry::default();
        send(&mut geometry, MTX_PUSH, &[]);
        assert_ne!(status(&mut geometry) & STACK_BUSY, 0);
        geometry.step(0);
        assert_ne!(status(&mut geometry) & STACK_BUSY, 0);
        run(&mut geometry);
        assert_eq!(status(&mut geometry) & STACK_BUSY, 0);
    }

    #[test]
    fn overflow_flag_shows_and_clears_by_writing_one() {
        let mut geometry = Geometry::default();
        send(&mut geometry, MTX_PUSH, &[]);
        send(&mut geometry, MTX_PUSH, &[]);
        run(&mut geometry);
        assert_ne!(status(&mut geometry) & STACK_ERROR, 0);
        geometry.write(GXSTAT, STACK_ERROR, ALL);
        assert_eq!(status(&mut geometry) & (STACK_ERROR | PROJECTION_LEVEL), 0);
    }

    #[test]
    fn status_counts_fifo_entries_past_the_pipe() {
        let mut geometry = Geometry::default();
        for _ in 0..134 {
            send(&mut geometry, MTX_MODE, &[1]);
        }
        let status = status(&mut geometry);
        assert_eq!((status >> LEVEL_SHIFT) & 0x1FF, 130);
        assert_eq!(status & (HALF_EMPTY | EMPTY), 0);
        assert!(!geometry.dma_request());
    }

    #[test]
    fn full_fifo_sets_bit_24() {
        let mut geometry = Geometry::default();
        for _ in 0..260 {
            send(&mut geometry, MTX_MODE, &[1]);
        }
        assert!(geometry.full());
        assert_ne!(status(&mut geometry) & (1 << 24), 0);
    }

    #[test]
    fn irq_modes_follow_the_fifo_level() {
        let mut geometry = Geometry::default();
        assert!(!geometry.irq());
        geometry.write(GXSTAT, IRQ_EMPTY << IRQ_SHIFT, ALL);
        assert!(geometry.irq());
        assert_eq!(status(&mut geometry) >> IRQ_SHIFT, IRQ_EMPTY);
        for _ in 0..5 {
            send(&mut geometry, MTX_MODE, &[1]);
        }
        assert!(!geometry.irq());
        geometry.write(GXSTAT, IRQ_HALF << IRQ_SHIFT, ALL);
        assert!(geometry.irq());
    }

    #[test]
    fn status_byte_write_leaves_the_irq_mode() {
        let mut geometry = Geometry::default();
        geometry.write(GXSTAT, IRQ_EMPTY << IRQ_SHIFT, ALL);
        geometry.write(GXSTAT, 0, 0xFFFF);
        assert_eq!(status(&mut geometry) >> IRQ_SHIFT, IRQ_EMPTY);
    }

    #[test]
    fn invalid_commands_are_ignored() {
        let mut geometry = Geometry::default();
        geometry.write(0, 0x0000_1003, ALL);
        geometry.write(0, 1, ALL);
        run(&mut geometry);
        assert_eq!(geometry.stack.mode(), matrix::Mode::Position);
    }

    const BOTH_SIDES_SOLID: u32 = 0x001F_00C0;
    const FULL_VIEW: u32 = 0xBFFF_0000;

    fn drawing(primitive: u32) -> Geometry {
        let mut geometry = Geometry::default();
        send(&mut geometry, VIEWPORT, &[FULL_VIEW]);
        send(&mut geometry, POLYGON_ATTR, &[BOTH_SIDES_SOLID]);
        send(&mut geometry, BEGIN_VTXS, &[primitive]);
        geometry
    }

    #[test]
    fn a_geometry_cloned_over_another_matches_its_source_and_runs_the_same() {
        let mut source = drawing(0);
        for xy in [0, 0x1000, 0x1000_0000] {
            send(&mut source, VTX_16, &[xy, 0]);
        }
        run(&mut source);
        send(&mut source, MTX_PUSH, &[]);
        let mut copy = drawing(1);
        for xy in [0x2000, 0x3000, 0x4000, 0x5000] {
            send(&mut copy, VTX_16, &[xy, 0]);
        }
        run(&mut copy);
        copy.clone_from(&source);
        assert_eq!(copy, source);
        assert_eq!(copy, source.clone());
        run(&mut source);
        run(&mut copy);
        assert_eq!(copy, source);
    }

    #[test]
    fn busy_stays_set_while_the_polygon_pipeline_drains_like_melonds() {
        let mut geometry = drawing(0);
        for xy in [0, 0x1000, 0x1000_0000] {
            send(&mut geometry, VTX_16, &[xy, 0]);
        }
        time(&mut geometry);
        assert_eq!(geometry.busy(), 0);
        assert_eq!(geometry.running(), Some(23));
        geometry.step(22);
        assert_ne!(status(&mut geometry) & BUSY, 0);
        geometry.step(1);
        assert_eq!(status(&mut geometry) & BUSY, 0);
        assert_eq!(geometry.running(), None);
    }

    #[test]
    fn a_frame_after_a_culled_frame_takes_melonds_culled_polygon_time() {
        let triangle = |geometry: &mut Geometry| {
            send(geometry, BEGIN_VTXS, &[0]);
            for xy in [0, 0x1000, 0x1000_0000] {
                send(geometry, VTX_16, &[xy, 0]);
            }
            send(geometry, BEGIN_VTXS, &[0]);
        };
        let times: Vec<u32> = [BOTH_SIDES_SOLID, BOTH_SIDES_SOLID & !0xC0]
            .map(|attributes| {
                let mut geometry = Geometry::default();
                send(&mut geometry, VIEWPORT, &[FULL_VIEW]);
                send(&mut geometry, POLYGON_ATTR, &[attributes]);
                triangle(&mut geometry);
                finish(&mut geometry);
                run(&mut geometry);
                triangle(&mut geometry);
                time(&mut geometry)
            })
            .to_vec();
        assert_eq!(times, [40, 22]);
    }

    fn finish(geometry: &mut Geometry) -> &Frame {
        send(geometry, SWAP_BUFFERS, &[0]);
        run(geometry);
        geometry.vblank();
        engine(geometry).frame()
    }

    fn positions(frame: &Frame) -> Vec<[i32; 4]> {
        frame
            .vertices
            .iter()
            .map(|vertex| vertex.position)
            .collect()
    }

    #[test]
    fn vertices_go_through_the_clip_matrix() {
        let mut geometry = drawing(0);
        send(&mut geometry, MTX_MODE, &[1]);
        send(&mut geometry, MTX_SCALE, &[0x2000, 0x2000, 0x2000]);
        send(&mut geometry, VTX_16, &[0x0000_0800, 0]);
        send(&mut geometry, VTX_16, &[0x0800_0000, 0]);
        send(&mut geometry, VTX_16, &[0, 0x0800]);
        let frame = finish(&mut geometry);
        assert_eq!(
            positions(frame),
            [
                [0x1000, 0, 0, 0x1000],
                [0, 0x1000, 0, 0x1000],
                [0, 0, 0x1000, 0x1000]
            ]
        );
    }

    #[test]
    fn vtx_10_has_a_six_bit_fraction_per_gbatek() {
        let mut geometry = drawing(0);
        send(&mut geometry, VTX_10, &[0x040 | 0x3C0 << 10 | 0x020 << 20]);
        send(&mut geometry, VTX_10, &[0]);
        send(&mut geometry, VTX_10, &[0x001]);
        let frame = finish(&mut geometry);
        assert_eq!(frame.vertices[0].position[..3], [0x1000, -0x1000, 0x0800]);
        assert_eq!(frame.vertices[2].position[0], 0x40);
    }

    #[test]
    fn partial_vertex_commands_keep_the_other_coordinate() {
        let mut geometry = drawing(1);
        send(&mut geometry, VTX_16, &[0x0200_0100, 0x0300]);
        send(&mut geometry, VTX_XY, &[0x0500_0400]);
        send(&mut geometry, VTX_XZ, &[0x0700_0600]);
        send(&mut geometry, VTX_YZ, &[0x0900_0800]);
        let frame = finish(&mut geometry);
        let xyz: Vec<[i32; 3]> = positions(frame)
            .iter()
            .map(|p| [p[0], p[1], p[2]])
            .collect();
        assert_eq!(
            xyz,
            [
                [0x100, 0x200, 0x300],
                [0x400, 0x500, 0x300],
                [0x600, 0x500, 0x700],
                [0x600, 0x800, 0x900]
            ]
        );
    }

    #[test]
    fn vtx_diff_adds_an_eighth_of_its_nine_bit_fraction_per_gbatek() {
        let mut geometry = drawing(0);
        send(&mut geometry, VTX_16, &[0x0800, 0]);
        send(
            &mut geometry,
            VTX_DIFF,
            &[0x3FF | 0x001 << 10 | 0x200 << 20],
        );
        send(&mut geometry, VTX_DIFF, &[0x1FF]);
        let frame = finish(&mut geometry);
        assert_eq!(frame.vertices[1].position[..3], [0x07FF, 1, -0x200]);
        assert_eq!(frame.vertices[2].position[..3], [0x09FE, 1, -0x200]);
    }

    #[test]
    fn color_and_texcoord_reach_the_vertex() {
        let mut geometry = drawing(0);
        send(&mut geometry, COLOR, &[31 | 1 << 5]);
        send(&mut geometry, TEXCOORD, &[0xFFF0_0020]);
        for _ in 0..3 {
            send(&mut geometry, VTX_16, &[0, 0]);
        }
        let frame = finish(&mut geometry);
        assert_eq!(frame.vertices[0].color, [63, 3, 0]);
        assert_eq!(frame.vertices[0].texcoord, [0x20, -0x10]);
    }

    #[test]
    fn swap_halts_the_engine_until_vblank_then_takes_melonds_325_cycles() {
        let mut geometry = drawing(0);
        for _ in 0..3 {
            send(&mut geometry, VTX_16, &[0, 0]);
        }
        send(&mut geometry, SWAP_BUFFERS, &[0]);
        send(&mut geometry, MTX_MODE, &[1]);
        run(&mut geometry);
        assert_eq!(geometry.stack.mode(), matrix::Mode::Projection);
        assert!(engine(&geometry).frame().polygons.is_empty());
        assert_ne!(status(&mut geometry) & BUSY, 0);
        geometry.vblank();
        assert_eq!(engine(&geometry).frame().polygons.len(), 1);
        geometry.step(324);
        assert_eq!(geometry.stack.mode(), matrix::Mode::Projection);
        geometry.step(1);
        assert_eq!(geometry.stack.mode(), matrix::Mode::Position);
    }

    #[test]
    fn vblank_without_a_swap_keeps_the_shown_frame() {
        let mut geometry = drawing(0);
        for _ in 0..3 {
            send(&mut geometry, VTX_16, &[0, 0]);
        }
        finish(&mut geometry);
        send(&mut geometry, BEGIN_VTXS, &[0]);
        for _ in 0..3 {
            send(&mut geometry, VTX_16, &[0, 0]);
        }
        run(&mut geometry);
        geometry.vblank();
        assert_eq!(engine(&geometry).frame().polygons.len(), 1);
    }

    #[test]
    fn normal_cycles_count_the_latched_lights_like_melonds() {
        let mut geometry = Geometry::default();
        send(&mut geometry, POLYGON_ATTR, &[0xF]);
        send(&mut geometry, NORMAL, &[0]);
        geometry.step(0);
        geometry.step(1);
        assert_eq!(geometry.busy(), 2);
        send(&mut geometry, BEGIN_VTXS, &[0]);
        run(&mut geometry);
        send(&mut geometry, NORMAL, &[0]);
        geometry.step(0);
        assert_eq!(geometry.busy(), 5);
    }

    #[test]
    fn normal_lights_the_next_vertices() {
        let mut geometry = drawing(0);
        send(&mut geometry, POLYGON_ATTR, &[BOTH_SIDES_SOLID | 1]);
        send(&mut geometry, BEGIN_VTXS, &[0]);
        send(&mut geometry, MTX_MODE, &[2]);
        send(&mut geometry, LIGHT_VECTOR, &[0x201 << 20]);
        send(&mut geometry, LIGHT_COLOR, &[0x7FFF]);
        send(&mut geometry, DIF_AMB, &[0x7FFF]);
        send(&mut geometry, COLOR, &[0]);
        send(&mut geometry, NORMAL, &[0x1FF << 20]);
        for _ in 0..3 {
            send(&mut geometry, VTX_16, &[0, 0]);
        }
        let frame = finish(&mut geometry);
        assert_eq!(frame.vertices[0].color, [59, 59, 59]);
    }

    #[test]
    fn normal_without_lights_leaves_the_emission_color() {
        let mut geometry = drawing(0);
        send(&mut geometry, COLOR, &[0x7FFF]);
        send(&mut geometry, SPE_EMI, &[3 << 16]);
        send(&mut geometry, NORMAL, &[0]);
        for _ in 0..3 {
            send(&mut geometry, VTX_16, &[0, 0]);
        }
        let frame = finish(&mut geometry);
        assert_eq!(frame.vertices[0].color, [7, 0, 0]);
    }

    #[test]
    fn diffuse_with_bit_15_sets_the_vertex_color() {
        let mut geometry = drawing(0);
        send(&mut geometry, DIF_AMB, &[0x8000 | 2 << 10]);
        for _ in 0..3 {
            send(&mut geometry, VTX_16, &[0, 0]);
        }
        let frame = finish(&mut geometry);
        assert_eq!(frame.vertices[0].color, [0, 0, 5]);
    }

    #[test]
    fn texcoord_mode_1_goes_through_the_texture_matrix_at_texcoord() {
        let mut geometry = drawing(0);
        send(&mut geometry, MTX_MODE, &[3]);
        send(&mut geometry, MTX_SCALE, &[0x2000, 0x3000, 0x1000]);
        send(&mut geometry, TEXIMAGE_PARAM, &[1 << 30]);
        send(&mut geometry, TEXCOORD, &[0x0010_0010]);
        send(&mut geometry, MTX_IDENTITY, &[]);
        for _ in 0..3 {
            send(&mut geometry, VTX_16, &[0, 0]);
        }
        let frame = finish(&mut geometry);
        assert_eq!(frame.vertices[0].texcoord, [0x20, 0x30]);
    }

    #[test]
    fn texcoord_mode_3_follows_each_vertex() {
        let mut geometry = drawing(0);
        send(&mut geometry, MTX_MODE, &[3]);
        send(&mut geometry, MTX_SCALE, &[0x1_0000, 0x1_0000, 0x1000]);
        send(&mut geometry, TEXIMAGE_PARAM, &[3 << 30]);
        send(&mut geometry, TEXCOORD, &[0x0010_0000]);
        send(&mut geometry, VTX_16, &[0x1000, 0]);
        send(&mut geometry, VTX_16, &[0x0800_0000, 0]);
        send(&mut geometry, VTX_16, &[0, 0]);
        let frame = finish(&mut geometry);
        let texcoords: Vec<[i16; 2]> = frame.vertices.iter().map(|v| v.texcoord).collect();
        assert_eq!(texcoords, [[16, 16], [0, 24], [0, 16]]);
    }

    #[test]
    fn texcoord_mode_2_takes_the_normal() {
        let mut geometry = drawing(0);
        send(&mut geometry, MTX_MODE, &[3]);
        send(
            &mut geometry,
            MTX_LOAD_4X4,
            &[
                0x10_0000, 0, 0, 0, 0, 0x1000, 0, 0, 0, 0, 0x1000, 0, 0, 0, 0, 0x1000,
            ],
        );
        send(&mut geometry, TEXIMAGE_PARAM, &[2 << 30]);
        send(&mut geometry, TEXCOORD, &[0x10]);
        send(&mut geometry, NORMAL, &[0x100]);
        for _ in 0..3 {
            send(&mut geometry, VTX_16, &[0, 0]);
        }
        let frame = finish(&mut geometry);
        assert_eq!(frame.vertices[0].texcoord, [0x90, 0]);
    }

    #[test]
    fn ram_count_reads_the_polygons_and_vertices_being_built() {
        let mut geometry = drawing(2);
        for index in 0..5 {
            send(&mut geometry, VTX_16, &[index * 0x100, 0]);
        }
        run(&mut geometry);
        assert_eq!(geometry.read(RAM_COUNT), 3 | 5 << 16);
        finish(&mut geometry);
        assert_eq!(geometry.read(RAM_COUNT), 0);
    }

    #[test]
    fn dot_depth_write_hides_far_one_dot_polygons() {
        let mut geometry = drawing(0);
        geometry.write(DOT_DEPTH, 0, ALL);
        for _ in 0..3 {
            send(&mut geometry, VTX_16, &[0, 0]);
        }
        let frame = finish(&mut geometry);
        assert!(frame.polygons.is_empty());
    }

    #[test]
    fn ram_overflow_flags_until_acknowledged() {
        let mut geometry = drawing(1);
        for _ in 0..1537 * 4 {
            send(&mut geometry, VTX_16, &[0x0800_0800, 0]);
            run(&mut geometry);
        }
        assert!(geometry.ram_overflow());
        geometry.acknowledge_ram_overflow();
        assert!(!geometry.ram_overflow());
    }

    #[test]
    fn clip_matrix_reads_position_times_projection() {
        let mut geometry = Geometry::default();
        send(&mut geometry, MTX_SCALE, &[0x2000, 0x3000, 0x1000]);
        send(&mut geometry, MTX_MODE, &[1]);
        send(&mut geometry, MTX_TRANS, &[0x1000, 0x1000, 0x1000]);
        run(&mut geometry);
        let clip: Vec<u32> = (0..16)
            .map(|k| geometry.read(CLIPMTX_RESULT + 4 * k))
            .collect();
        assert_eq!(
            clip,
            [
                0x2000, 0, 0, 0, 0, 0x3000, 0, 0, 0, 0, 0x1000, 0, 0x2000, 0x3000, 0x1000, 0x1000
            ]
        );
    }

    #[test]
    fn vector_matrix_reads_as_three_by_three_per_gbatek() {
        let mut geometry = Geometry::default();
        send(&mut geometry, MTX_MODE, &[2]);
        let cells: Vec<u32> = (1..=16).collect();
        send(&mut geometry, MTX_LOAD_4X4, &cells);
        run(&mut geometry);
        let vector: Vec<u32> = (0..9)
            .map(|k| geometry.read(VECMTX_RESULT + 4 * k))
            .collect();
        assert_eq!(vector, [1, 2, 3, 5, 6, 7, 9, 10, 11]);
    }

    #[test]
    fn position_test_reads_back_and_moves_the_vertex_registers_per_gbatek() {
        let mut geometry = drawing(0);
        send(&mut geometry, MTX_MODE, &[1]);
        send(&mut geometry, MTX_TRANS, &[0, 0, 0xFFFF_F000]);
        send(&mut geometry, POS_TEST, &[0x0800_1000, 0x0400]);
        run(&mut geometry);
        let result: Vec<u32> = (0..4).map(|k| geometry.read(POS_RESULT + 4 * k)).collect();
        assert_eq!(result, [0x1000, 0x0800, 0xFFFF_F400, 0x1000]);
        send(&mut geometry, VTX_XY, &[0]);
        send(&mut geometry, VTX_XY, &[0]);
        send(&mut geometry, VTX_XY, &[0x10]);
        let frame = finish(&mut geometry);
        assert_eq!(frame.vertices[0].position[2], -0x0C00);
    }

    #[test]
    fn vector_test_reads_three_halfwords() {
        let mut geometry = Geometry::default();
        send(
            &mut geometry,
            VEC_TEST,
            &[0x100 | 0x3C0 << 10 | 0x040 << 20],
        );
        run(&mut geometry);
        assert_eq!(geometry.read(VEC_RESULT), 0x0800 | 0xFE00 << 16);
        assert_eq!(geometry.read(VEC_RESULT_Z), 0x0200);
    }

    #[test]
    fn box_test_sets_the_status_result_and_busy_bits() {
        let mut geometry = Geometry::default();
        send(
            &mut geometry,
            BOX_TEST,
            &[0xF800_F800, 0x1000_F800, 0x1000_1000],
        );
        assert_ne!(status(&mut geometry) & TEST_BUSY, 0);
        geometry.step(2);
        assert_ne!(status(&mut geometry) & TEST_BUSY, 0);
        assert_ne!(status(&mut geometry) & BOX_INSIDE, 0);
        run(&mut geometry);
        assert_eq!(status(&mut geometry) & TEST_BUSY, 0);
        send(
            &mut geometry,
            BOX_TEST,
            &[0x0000_3000, 0x0800_0000, 0x0800_0800],
        );
        run(&mut geometry);
        assert_eq!(status(&mut geometry) & BOX_INSIDE, 0);
    }
}
