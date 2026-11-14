use super::geometry::frame::Frame;
use super::geometry::{self, Geometry};
use super::render::texture::{Decoded, PALETTE_SIZE, TEXTURE_SIZE, TextureMemory};
use super::render::{self, Registers, Renderer, Stencil};
use super::video::{HEIGHT, Line3d};
use super::vram::{Purpose, Vram};
use std::sync::Arc;

pub mod raster;

pub const REGISTERS_FIRST: usize = 0x320;
pub const REGISTERS_LAST: usize = 0x3BF;
const RAM_OVERFLOW: u32 = 1 << 13;
const LOOK_AHEAD: u32 = 64;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ThreeD {
    geometry: Geometry,
    registers: Registers,
    latched: Registers,
    ahead: u32,
    shown: Option<Box<Shown>>,
    #[cfg_attr(feature = "serde", serde(skip, default = "blank_lines"))]
    lines: Box<[Line3d]>,
    #[cfg_attr(feature = "serde", serde(skip))]
    away: Away,
    #[cfg_attr(feature = "serde", serde(skip))]
    side_lines: u64,
    #[cfg_attr(feature = "serde", serde(skip))]
    side_geometry: u64,
    #[cfg_attr(feature = "serde", serde(skip))]
    swapped: bool,
    #[cfg_attr(feature = "serde", serde(skip))]
    drawn: Option<Drawn>,
    #[cfg_attr(feature = "serde", serde(skip))]
    later: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Drawn {
    registers: Registers,
    textures: u64,
}

impl Clone for ThreeD {
    fn clone(&self) -> Self {
        Self {
            geometry: self.geometry.clone(),
            registers: self.registers,
            latched: self.latched,
            ahead: self.ahead,
            shown: self.shown.clone(),
            lines: self.lines.clone(),
            away: Away::default(),
            side_lines: self.side_lines,
            side_geometry: self.side_geometry,
            swapped: self.swapped,
            drawn: self.drawn,
            later: self.later,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        let Self {
            geometry,
            registers,
            latched,
            ahead,
            shown,
            lines,
            away,
            side_lines,
            side_geometry,
            swapped,
            drawn,
            later,
        } = self;
        geometry.clone_from(&source.geometry);
        *registers = source.registers;
        *latched = source.latched;
        *ahead = source.ahead;
        shown.clone_from(&source.shown);
        lines.clone_from(&source.lines);
        away.clone_from(&source.away);
        *side_lines = source.side_lines;
        *side_geometry = source.side_geometry;
        *swapped = source.swapped;
        *drawn = source.drawn;
        *later = source.later;
    }
}

impl Default for ThreeD {
    fn default() -> Self {
        Self {
            geometry: Geometry::default(),
            registers: Registers::default(),
            latched: Registers::default(),
            ahead: 0,
            shown: Some(Box::default()),
            lines: blank_lines(),
            away: Away::default(),
            side_lines: 0,
            side_geometry: 0,
            swapped: false,
            drawn: None,
            later: false,
        }
    }
}

#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct Shown {
    frame: Frame,
    registers: Registers,
    textures: Textures,
    #[cfg_attr(feature = "serde", serde(skip))]
    renderer: Renderer,
    #[cfg_attr(feature = "serde", serde(skip))]
    stencil: Stencil,
    #[cfg_attr(feature = "serde", serde(skip))]
    textures_version: Option<u64>,
}

impl Clone for Shown {
    fn clone(&self) -> Self {
        Self {
            frame: self.frame.clone(),
            registers: self.registers,
            textures: self.textures.clone(),
            renderer: self.renderer.clone(),
            stencil: self.stencil.clone(),
            textures_version: self.textures_version,
        }
    }

    fn clone_from(&mut self, source: &Self) {
        let Self {
            frame,
            registers,
            textures,
            renderer,
            stencil,
            textures_version,
        } = self;
        frame.clone_from(&source.frame);
        *registers = source.registers;
        textures.clone_from(&source.textures);
        renderer.clone_from(&source.renderer);
        stencil.clone_from(&source.stencil);
        *textures_version = source.textures_version;
    }
}

impl Shown {
    #[cfg(feature = "persistence")]
    fn copy_saved_from(&mut self, source: &Shown) {
        let Self {
            frame,
            registers,
            textures,
            renderer: _,
            stencil: _,
            textures_version: _,
        } = self;
        frame.clone_from(&source.frame);
        *registers = source.registers;
        textures.clone_from(&source.textures);
    }

    fn load(&mut self, frame: &Frame, registers: Registers, vram: &Vram) {
        self.frame.clone_from(frame);
        self.registers = registers;
        let version = Some(vram.texture_version());
        if !self.frame.polygons.is_empty() && self.textures_version != version {
            self.textures.load(vram);
            self.textures_version = version;
        }
        if self.registers.textured() {
            self.textures.prepare(&self.frame);
        }
        self.renderer.start_frame(self.registers);
    }

    fn settle(&mut self) {
        settle(
            &mut self.renderer,
            &self.frame,
            self.registers,
            &self.textures,
            &mut self.stencil,
        );
    }

    fn draw(&mut self, lines: &mut [Line3d]) {
        let end = self.renderer.render_band(
            0..HEIGHT,
            &self.frame,
            &self.textures,
            &self.stencil,
            |y, line| lines[y].clone_from(line),
        );
        if let Some(end) = end {
            self.stencil = end;
        }
    }
}

fn settle(
    renderer: &mut Renderer,
    frame: &Frame,
    registers: Registers,
    textures: &Textures,
    stencil: &mut Stencil,
) {
    renderer.start_frame(registers);
    if let Some(end) = renderer.end_stencil(frame, textures, stencil) {
        *stencil = end;
    }
}

#[derive(Default)]
struct Away {
    link: Option<raster::Link>,
    renderers: Vec<Renderer>,
}

impl Clone for Away {
    fn clone(&self) -> Self {
        Self::default()
    }
}

const HOME: &str = "the 3d scene is home between frames";

fn blank_lines() -> Box<[Line3d]> {
    vec![Line3d::default(); HEIGHT].into_boxed_slice()
}

#[derive(Clone, Copy)]
pub struct Scene<'a> {
    pub frame: &'a Frame,
    pub registers: Registers,
    pub textures: &'a Textures,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone)]
pub struct Textures {
    #[cfg_attr(feature = "serde", serde(with = "shared_bytes"))]
    image: Arc<[u8]>,
    #[cfg_attr(feature = "serde", serde(with = "shared_bytes"))]
    palette: Arc<[u8]>,
    #[cfg_attr(feature = "serde", serde(skip))]
    decoded: Decoded,
}

#[cfg(feature = "serde")]
mod shared_bytes {
    use std::sync::Arc;

    pub fn serialize<S: serde::Serializer>(
        bytes: &Arc<[u8]>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        crate::utils::bytes::serialize(bytes, serializer)
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Arc<[u8]>, D::Error> {
        crate::utils::bytes::deserialize(deserializer).map(Arc::from)
    }
}

impl Default for Textures {
    fn default() -> Self {
        Self {
            image: vec![0; TEXTURE_SIZE as usize].into(),
            palette: vec![0; PALETTE_SIZE as usize].into(),
            decoded: Decoded::default(),
        }
    }
}

impl Textures {
    pub fn load(&mut self, vram: &Vram) {
        vram.copy_window(Purpose::Texture, Arc::make_mut(&mut self.image));
        vram.copy_window(Purpose::TexturePalette, Arc::make_mut(&mut self.palette));
        self.decoded.clear();
    }

    pub fn prepare(&mut self, frame: &Frame) {
        let mut decoded = std::mem::take(&mut self.decoded);
        for polygon in &frame.polygons {
            decoded.prepare(self, polygon.texture, polygon.palette);
        }
        self.decoded = decoded;
    }

    pub fn image(&self) -> &[u8] {
        &self.image
    }

    pub fn palette(&self) -> &[u8] {
        &self.palette
    }
}

impl TextureMemory for Textures {
    fn texture(&self, address: u32) -> u8 {
        self.image[(address & (TEXTURE_SIZE - 1)) as usize]
    }

    fn texture_palette(&self, address: u32) -> u8 {
        self.palette[(address % PALETTE_SIZE) as usize]
    }

    fn texture_bytes(&self) -> Option<(&[u8], &[u8])> {
        Some((&self.image, &self.palette))
    }

    fn decoded(&self, param: u32, palette: u16) -> Option<&[u16]> {
        self.decoded.get(param, palette)
    }

    fn texture_u16(&self, address: u32) -> u16 {
        let at = (address & (TEXTURE_SIZE - 1)) as usize;
        match self.image.get(at..at + 2) {
            Some(bytes) => u16::from_le_bytes([bytes[0], bytes[1]]),
            None => u16::from_le_bytes([self.image[at], self.image[0]]),
        }
    }

    fn texture_palette_u16(&self, address: u32) -> u16 {
        let at = (address % PALETTE_SIZE) as usize;
        match self.palette.get(at..at + 2) {
            Some(bytes) => u16::from_le_bytes([bytes[0], bytes[1]]),
            None => u16::from_le_bytes([self.palette[at], self.palette[0]]),
        }
    }
}

impl ThreeD {
    #[cfg(feature = "persistence")]
    pub(crate) fn copy_saved_from(&mut self, source: &ThreeD) {
        self.draw_now();
        let Self {
            geometry,
            registers,
            latched,
            ahead,
            shown,
            lines: _,
            away: _,
            side_lines: _,
            side_geometry: _,
            swapped: _,
            drawn: _,
            later: _,
        } = self;
        geometry.clone_from(&source.geometry);
        *registers = source.registers;
        *latched = source.latched;
        *ahead = source.ahead;
        match (shown.as_deref_mut(), source.shown.as_deref()) {
            (Some(to), Some(from)) => to.copy_saved_from(from),
            _ => shown.clone_from(&source.shown),
        }
    }

    pub fn geometry(&self) -> &Geometry {
        &self.geometry
    }

    pub fn registers(&self) -> &Registers {
        &self.registers
    }

    pub fn read(&mut self, offset: usize) -> u32 {
        match offset {
            render::DISP3DCNT => {
                let overflow = self.geometry.ram_overflow();
                self.display_control(overflow)
            }
            geometry::FIRST..=geometry::LAST => self.geometry.read(offset - geometry::FIRST),
            _ => self.registers.read(offset),
        }
    }

    pub fn peek(&self, offset: usize) -> u32 {
        match offset {
            render::DISP3DCNT => self.display_control(self.geometry.peek_ram_overflow()),
            geometry::FIRST..=geometry::LAST => self.geometry.peek(offset - geometry::FIRST),
            _ => self.registers.read(offset),
        }
    }

    fn display_control(&self, overflow: bool) -> u32 {
        let overflow = if overflow { RAM_OVERFLOW } else { 0 };
        self.registers.read(render::DISP3DCNT) | overflow
    }

    pub fn write(&mut self, offset: usize, value: u32, mask: u32) -> u32 {
        match offset {
            geometry::FIRST..=geometry::LAST => {
                self.geometry.write(offset - geometry::FIRST, value, mask);
                return self.drain_while_full();
            }
            render::DISP3DCNT if value & mask & RAM_OVERFLOW != 0 => {
                self.geometry.acknowledge_ram_overflow();
                self.registers.write(offset, value, mask);
            }
            _ => self.registers.write(offset, value, mask),
        }
        0
    }

    pub fn write_packed(&mut self, value: u32) -> u32 {
        self.geometry.write_packed(value);
        self.drain_while_full()
    }

    pub fn step(&mut self, cycles: u32) {
        let skipped = cycles.min(self.ahead);
        self.ahead -= skipped;
        if cycles > skipped {
            self.geometry.step(cycles - skipped);
        }
    }

    pub fn until_event(&self, dma_waits: impl Fn() -> bool) -> Option<u32> {
        let at = self.geometry.next_seen(LOOK_AHEAD, dma_waits)?;
        Some(self.ahead + at)
    }

    pub fn next_finish(&self) -> Option<u32> {
        Some(self.ahead + self.geometry.running()?)
    }

    pub fn finish_command(&mut self) {
        let Some(busy) = self.geometry.running() else {
            return;
        };
        self.geometry.step(busy);
        self.ahead += busy;
    }

    pub fn irq(&self) -> bool {
        self.geometry.irq()
    }

    pub fn dma_request(&self) -> bool {
        self.geometry.dma_request()
    }

    pub fn vblank(&mut self, rendering: bool) {
        self.swapped |= self.geometry.swapping();
        self.geometry.vblank();
        if rendering {
            self.latched = self.registers;
        }
    }

    pub fn render(&mut self, vram: &Vram, defer: bool) -> bool {
        let registers = self.latched;
        let drawn = Drawn {
            registers,
            textures: vram.texture_version(),
        };
        if !std::mem::take(&mut self.swapped) && self.drawn == Some(drawn) {
            if !defer {
                self.draw_now();
            }
            return false;
        }
        self.drawn = Some(drawn);
        let settle = std::mem::replace(&mut self.later, defer);
        let away = &mut self.away.link;
        let shown = &mut self.shown;
        let lines = &mut self.lines;
        self.geometry.with_frame(|frame| match away {
            Some(link) => {
                link.load(lines, settle, |shown| {
                    shown.load(frame, registers, vram);
                });
                if !defer {
                    link.go();
                }
            }
            None => {
                let shown = shown.as_deref_mut().expect(HOME);
                if settle {
                    shown.settle();
                }
                shown.load(frame, registers, vram);
                if !defer {
                    shown.draw(lines);
                }
            }
        });
        true
    }

    pub fn later(&self) -> bool {
        self.later
    }

    pub fn draw_now(&mut self) {
        if !std::mem::take(&mut self.later) {
            return;
        }
        match &mut self.away.link {
            Some(link) => link.go(),
            None => self.shown.as_deref_mut().expect(HOME).draw(&mut self.lines),
        }
    }

    pub fn line(&mut self, y: usize) -> &Line3d {
        self.draw_now();
        if let Some(link) = &mut self.away.link {
            link.wait_for(y, &mut self.lines);
        }
        &self.lines[y]
    }

    pub fn scene(&self) -> Scene<'_> {
        let shown = self.shown.as_deref().expect(HOME);
        Scene {
            frame: &shown.frame,
            registers: shown.registers,
            textures: &shown.textures,
        }
    }

    pub fn lend(&mut self, tasks: usize) -> (Vec<raster::Task>, raster::Ending) {
        self.come_home();
        let shown = self.shown.take().expect(HOME);
        let renderers = std::mem::take(&mut self.away.renderers);
        let (link, mut tasks, ending, nudge) = raster::lend(shown, renderers, tasks);
        tasks[0].add_geometry(self.geometry.lend(nudge));
        self.away.link = Some(link);
        (tasks, ending)
    }

    pub fn side_work(&self) -> [u64; 2] {
        [self.side_lines, self.side_geometry]
    }

    pub fn come_home(&mut self) {
        if let Some(link) = self.away.link.take() {
            let (shown, renderers, [lines, geometry]) = link.come_home(&mut self.lines);
            self.shown = Some(shown);
            self.away.renderers = renderers;
            self.side_lines += lines;
            self.side_geometry += geometry;
        }
        self.geometry.come_home();
    }

    fn drain_while_full(&mut self) -> u32 {
        let mut spent = 0;
        while self.geometry.full() {
            let busy = self.geometry.busy();
            self.geometry.step(busy);
            spent += busy;
            if busy == 0 && self.geometry.busy() == 0 {
                break;
            }
        }
        self.ahead += spent;
        spent
    }
}

#[cfg(test)]
mod tests {
    use super::super::bus::Buses;
    use super::super::dma::{DmaBus, Run};
    use super::super::geometry::command::*;
    use super::super::interrupt::Irq;
    use super::super::lcd::{FRAME_CYCLES, LINE_CYCLES, VISIBLE_LINES};
    use super::super::video::WIDTH;
    use super::super::video::color;
    use crate::gba::wait::Width;
    use citrine_gb_threads::{Mode, Side, Split, Threads};

    const IO: u32 = 0x0400_0000;
    const DISPCNT_3D_ON_BG0: u32 = 0x0001_0108;
    const POWER_ON: u32 = 0x820F;
    const GXSTAT: u32 = IO + 0x600;
    const GXFIFO: u32 = IO + 0x400;
    const CLEAR_DEPTH: u32 = IO + 0x354;
    const IE: u32 = IO + 0x210;
    const IF: u32 = IO + 0x214;
    const WHITE: u16 = 0x7FFF;

    fn send(buses: &mut Buses, command: u8, param: u32) {
        let port = GXFIFO + (u32::from(command) << 2);
        buses.arm9().write(port, Width::Word, param);
    }

    fn console() -> Buses {
        let mut buses = Buses::default();
        buses.arm9().write(IO + 0x304, Width::Half, POWER_ON);
        buses.arm9().write(IO, Width::Word, DISPCNT_3D_ON_BG0);
        buses.arm9().write(CLEAR_DEPTH, Width::Half, 0x7FFF);
        buses
    }

    fn white_quad(buses: &mut Buses) {
        send(buses, VIEWPORT, 0xBFFF_0000);
        send(buses, POLYGON_ATTR, 0x001F_0080);
        send(buses, BEGIN_VTXS, 1);
        send(buses, COLOR, u32::from(WHITE));
        for [x, y] in [
            [-0x800, -0x800],
            [0x800, -0x800],
            [0x800, 0x800],
            [-0x800, 0x800],
        ] {
            send(
                buses,
                VTX_16,
                u32::from(x as u16) | u32::from(y as u16) << 16,
            );
            send(buses, VTX_16, 0);
        }
        send(buses, SWAP_BUFFERS, 0);
    }

    fn top(buses: &Buses, x: usize, y: usize) -> u16 {
        color::narrow(buses.screens().top()[y * WIDTH + x])
    }

    fn level(buses: &mut Buses) -> u32 {
        (buses.arm9().read(GXSTAT, Width::Word) >> 16) & 0x1FF
    }

    #[test]
    fn a_quad_sent_through_the_arm9_bus_shows_on_engine_a_bg0_the_frame_after_its_swap() {
        let mut buses = console();
        white_quad(&mut buses);
        buses.elapse(FRAME_CYCLES);
        assert_ne!(top(&buses, 100, 100), WHITE);
        buses.elapse(FRAME_CYCLES);
        assert_eq!(top(&buses, 100, 100), WHITE);
        assert_eq!(top(&buses, 64, 48), WHITE);
        assert_ne!(top(&buses, 63, 48), WHITE);
        assert_ne!(top(&buses, 100, 144), WHITE);
        assert_eq!(buses.three_d_scene().frame.polygons.len(), 1);
    }

    #[test]
    fn a_console_with_the_rendering_engine_off_draws_no_3d() {
        let mut buses = console();
        buses
            .arm9()
            .write(IO + 0x304, Width::Half, POWER_ON & !(1 << 2));
        white_quad(&mut buses);
        buses.elapse(2 * FRAME_CYCLES);
        assert_ne!(top(&buses, 100, 100), WHITE);
    }

    #[test]
    fn gxstat_counts_queued_entries_until_the_engine_runs_them() {
        let mut buses = console();
        for _ in 0..10 {
            send(&mut buses, MTX_IDENTITY, 0);
        }
        assert_eq!(level(&mut buses), 6);
        buses.elapse(100);
        assert_eq!(level(&mut buses), 0);
    }

    #[test]
    fn the_gxfifo_irq_holds_while_the_fifo_is_less_than_half_full() {
        let mut buses = console();
        buses
            .arm9()
            .write(IE, Width::Word, Irq::GEOMETRY_FIFO.bits());
        buses.arm9().write(GXSTAT, Width::Word, 1 << 30);
        assert!(
            buses
                .interrupts(super::super::Cpu::Arm9)
                .flags()
                .contains(Irq::GEOMETRY_FIFO)
        );
        for _ in 0..200 {
            send(&mut buses, MTX_IDENTITY, 0);
        }
        buses
            .arm9()
            .write(IF, Width::Word, Irq::GEOMETRY_FIFO.bits());
        assert!(
            !buses
                .interrupts(super::super::Cpu::Arm9)
                .flags()
                .contains(Irq::GEOMETRY_FIFO)
        );
        buses.elapse(10_000);
        assert!(
            buses
                .interrupts(super::super::Cpu::Arm9)
                .flags()
                .contains(Irq::GEOMETRY_FIFO)
        );
    }

    #[test]
    fn a_write_to_a_full_fifo_stalls_the_arm9_until_an_entry_is_free() {
        let mut buses = console();
        let mut stalled = 0;
        for _ in 0..300 {
            send(&mut buses, MTX_IDENTITY, 0);
            stalled += buses.arm9().take_stall();
        }
        assert!(stalled > 0);
        assert!((250..256).contains(&level(&mut buses)));
        assert_eq!(buses.arm9().take_stall(), 0);
    }

    #[test]
    fn a_geometry_fifo_dma_moves_112_words_while_the_fifo_is_less_than_half_full() {
        let mut buses = console();
        for index in 0..300 {
            buses.arm9().write(
                0x0200_0000 + 4 * index,
                Width::Word,
                u32::from(MTX_IDENTITY),
            );
        }
        buses.arm9().write(IO + 0xD4, Width::Word, 0x0200_0000);
        buses.arm9().write(IO + 0xD8, Width::Word, GXFIFO);
        let control = 0x8000_0000 | 1 << 30 | 7 << 27 | 1 << 26 | 2 << 21 | 300;
        buses.arm9().write(IO + 0xDC, Width::Word, control);
        buses.elapse(100_000);
        assert_eq!(level(&mut buses), 0);
        assert!(
            buses
                .interrupts(super::super::Cpu::Arm9)
                .flags()
                .contains(Irq::DMA3)
        );
    }

    #[test]
    fn a_geometry_fifo_dma_runs_112_word_blocks_at_its_enabling_write_until_128_entries_wait() {
        let mut buses = console();
        for index in 0..300 {
            buses.arm9().write(
                0x0200_0000 + 4 * index,
                Width::Word,
                u32::from(MTX_IDENTITY),
            );
        }
        buses.arm9().write(IO + 0xD4, Width::Word, 0x0200_0000);
        buses.arm9().write(IO + 0xD8, Width::Word, GXFIFO);
        let control = 0x8000_0000 | 7 << 27 | 1 << 26 | 2 << 21 | 300;
        buses.arm9().write(IO + 0xDC, Width::Word, control);
        assert_eq!(level(&mut buses), 2 * 112 - 4);
    }

    #[test]
    fn a_fifo_dma_run_leaves_what_word_by_word_writes_leave() {
        let run = Run {
            source: 0x0200_0000,
            source_step: 4,
            dest: GXFIFO,
            dest_step: 0,
            wide: true,
            units: 112,
        };
        let mut states = Vec::new();
        for whole_run in [true, false] {
            let mut buses = console();
            buses.arm9().write(GXSTAT, Width::Word, 1 << 30);
            for _ in 0..100 {
                send(&mut buses, MTX_IDENTITY, 0);
            }
            for index in 0..run.units {
                let word = if index % 2 == 0 { 0x0020_1515 } else { 0x7FFF };
                buses
                    .arm9()
                    .write(0x0200_0000 + 4 * index, Width::Word, word);
            }
            buses
                .arm9()
                .write(IF, Width::Word, Irq::GEOMETRY_FIFO.bits());
            buses.arm9().take_stall();
            if whole_run {
                buses.arm9().copy(&run);
            } else {
                run.copy_each(&mut buses.arm9());
            }
            let stall = buses.arm9().take_stall();
            let flags = buses.interrupts(super::super::Cpu::Arm9).flags();
            states.push((level(&mut buses), stall, flags));
        }
        assert_eq!(states[0], states[1]);
        assert!(states[0].1 > 0);
    }

    #[test]
    fn disp3dcnt_sits_beside_the_engine_a_registers() {
        let mut buses = console();
        buses.arm9().write(IO + 0x60, Width::Half, 0x0008);
        buses.arm9().write(IO + 0x64, Width::Word, 0x0001_0000);
        assert_eq!(buses.arm9().read(IO + 0x60, Width::Half), 0x0008);
        assert_eq!(
            buses.arm9().read(IO + 0x64, Width::Word) & 0x0001_0000,
            0x0001_0000
        );
        assert_eq!(buses.arm9().read(IO, Width::Word), DISPCNT_3D_ON_BG0);
    }

    fn quads(buses: &mut Buses, n: i32) {
        shapes(buses, n);
        send(buses, SWAP_BUFFERS, 0);
    }

    fn shapes(buses: &mut Buses, n: i32) {
        send(buses, VIEWPORT, 0xBFFF_0000);
        for (layer, (color, alpha)) in [(0x001F, 31), (0x7C00 | n as u32, 12)]
            .into_iter()
            .enumerate()
        {
            send(buses, POLYGON_ATTR, 0x0000_0080 | alpha << 16);
            send(buses, BEGIN_VTXS, 1);
            send(buses, COLOR, color);
            let shift = n * 0x180 + layer as i32 * 0x300;
            for [x, y] in [
                [-0x900, -0x700],
                [0x500, -0x800],
                [0x700, 0x600],
                [-0x600, 0x900],
            ] {
                let (x, y) = ((x + shift) as u16, (y - shift / 2) as u16);
                send(buses, VTX_16, u32::from(x) | u32::from(y) << 16);
                send(buses, VTX_16, (layer as u32) << 8);
            }
        }
    }

    fn lent_frame(buses: &mut Buses, split: &mut Split<'_>, tasks: usize) {
        let (tasks, ending) = buses.lend_3d(tasks);
        let sides: Vec<(&str, Side<'_>)> = tasks
            .into_iter()
            .map(|task| ("3d", Box::new(move || task.run()) as Side<'_>))
            .collect();
        split.beside(sides, || {
            let _ending = ending;
            buses.elapse(FRAME_CYCLES);
        });
        buses.home_3d();
    }

    fn frames(mut frame: impl FnMut(&mut Buses)) -> Vec<Vec<u32>> {
        let mut buses = console();
        buses.elapse(LINE_CYCLES * u32::from(VISIBLE_LINES));
        (0..4)
            .map(|n| {
                quads(&mut buses, n);
                frame(&mut buses);
                let screens = buses.screens();
                screens
                    .top()
                    .iter()
                    .chain(screens.bottom())
                    .copied()
                    .collect()
            })
            .collect()
    }

    #[test]
    fn a_scene_lent_to_the_pool_or_drawn_by_the_caller_matches_the_inline_one() {
        let inline = frames(|buses| {
            buses.elapse(FRAME_CYCLES);
        });
        assert!(inline.windows(2).all(|pair| pair[0] != pair[1]));
        let threads = Threads::new(Mode::Threads);
        let pooled = threads.split(|split| frames(|buses| lent_frame(buses, split, 1)));
        assert_eq!(pooled, inline);
        let banded = threads.split(|split| frames(|buses| lent_frame(buses, split, 3)));
        assert_eq!(banded, inline);
        let caller = frames(|buses| lent_frame(buses, &mut Split::inline(), 3));
        assert_eq!(caller, inline);
    }

    #[test]
    fn a_task_started_late_draws_the_lines_the_caller_left_and_the_scene_comes_home() {
        let start = LINE_CYCLES * (u32::from(VISIBLE_LINES) + 1);
        let mut inline = console();
        let mut lent = console();
        for buses in [&mut inline, &mut lent] {
            quads(buses, 1);
            buses.elapse(start);
        }
        inline.elapse(FRAME_CYCLES);
        let (tasks, ending) = lent.lend_3d(2);
        lent.elapse(FRAME_CYCLES / 2);
        drop(ending);
        for task in tasks {
            task.run();
        }
        lent.home_3d();
        lent.elapse(FRAME_CYCLES - FRAME_CYCLES / 2);
        assert_eq!(lent.screens().top(), inline.screens().top());
        assert_eq!(lent.three_d_scene().frame.polygons.len(), 2);
    }

    fn word(buses: &mut Buses, address: u32) -> u32 {
        buses.arm9().read(address, Width::Word)
    }

    fn sends(buses: &mut Buses, command: u8, params: &[u32]) {
        for &param in params {
            send(buses, command, param);
        }
    }

    fn busy_frame(buses: &mut Buses, n: u32) -> Vec<u32> {
        let mut seen = Vec::new();
        for step in 0..6 {
            sends(buses, MTX_MODE, &[2]);
            sends(buses, MTX_PUSH, &[0]);
            sends(buses, MTX_SCALE, &[0x1000 + n * 0x80, 0x1000, 0x1000]);
            sends(buses, MTX_TRANS, &[0x40 * step, 0, 0]);
            seen.push(word(buses, GXSTAT));
            sends(
                buses,
                BOX_TEST,
                &[0xFF80_FF80 + step, 0x0100_0000, 0x0100_0100],
            );
            seen.push(word(buses, GXSTAT));
            buses.elapse(LINE_CYCLES);
            seen.push(word(buses, GXSTAT));
            sends(buses, POS_TEST, &[(0x100 * step) | (0x200 << 16), 0x80]);
            buses.elapse(LINE_CYCLES);
            for k in 0..4 {
                seen.push(word(buses, IO + 0x620 + 4 * k));
            }
            seen.push(word(buses, IO + 0x640 + 4 * (step % 16)));
            shapes(buses, (n + step) as i32);
            seen.push(word(buses, IO + 0x604));
            sends(buses, MTX_POP, &[1]);
            seen.push(word(buses, GXSTAT));
            buses.elapse(LINE_CYCLES * 10);
        }
        let mut stalled = 0;
        for _ in 0..300 {
            send(buses, MTX_IDENTITY, 0);
            stalled += buses.arm9().take_stall();
        }
        seen.push(stalled);
        seen.push(word(buses, GXSTAT));
        send(buses, SWAP_BUFFERS, 0);
        seen.push(word(buses, IO + 0x60));
        seen
    }

    fn busy_frames(mut frame: impl FnMut(&mut Buses, &mut dyn FnMut(&mut Buses))) -> Vec<Vec<u32>> {
        let mut buses = console();
        buses.elapse(LINE_CYCLES * u32::from(VISIBLE_LINES));
        (0..3)
            .map(|n| {
                let mut seen = Vec::new();
                frame(&mut buses, &mut |buses| {
                    seen = busy_frame(buses, n);
                    buses.elapse(FRAME_CYCLES);
                });
                let screens = buses.screens();
                seen.extend(screens.top().iter().chain(screens.bottom()));
                seen
            })
            .collect()
    }

    fn lent_work(buses: &mut Buses, split: &mut Split<'_>, work: &mut dyn FnMut(&mut Buses)) {
        let (tasks, ending) = buses.lend_3d(1);
        let task = tasks.into_iter().next().expect("one 3d task");
        split.beside(vec![("3d", Box::new(move || task.run()))], || {
            let _ending = ending;
            work(buses);
        });
        buses.home_3d();
    }

    #[test]
    fn geometry_sent_while_the_scene_is_lent_reads_and_draws_like_the_inline_engine() {
        let inline = busy_frames(|buses, work| work(buses));
        assert!(inline.windows(2).all(|pair| pair[0] != pair[1]));
        let threads = Threads::new(Mode::Threads);
        let pooled =
            threads.split(|split| busy_frames(|buses, work| lent_work(buses, split, work)));
        assert_eq!(pooled, inline);
        let caller = busy_frames(|buses, work| lent_work(buses, &mut Split::inline(), work));
        assert_eq!(caller, inline);
    }

    const VRAMCNT_A: u32 = IO + 0x240;
    const LCDC_A: u32 = 0x0680_0000;
    const TEXTURE_SLOT_0: u32 = 0x83;
    const RED: u16 = 0x001F;
    const GREEN: u16 = 0x03E0;

    fn texels(buses: &mut Buses, color: u16) {
        buses.arm9().write(VRAMCNT_A, Width::Byte, 0x80);
        for texel in 0..64 {
            buses
                .arm9()
                .write(LCDC_A + 2 * texel, Width::Half, u32::from(0x8000 | color));
        }
        buses.arm9().write(VRAMCNT_A, Width::Byte, TEXTURE_SLOT_0);
    }

    fn textured_quad(buses: &mut Buses) {
        buses.arm9().write(IO + 0x60, Width::Half, 1);
        send(buses, VIEWPORT, 0xBFFF_0000);
        send(buses, TEXIMAGE_PARAM, 7 << 26);
        send(buses, POLYGON_ATTR, 0x001F_0080);
        send(buses, BEGIN_VTXS, 1);
        send(buses, COLOR, u32::from(WHITE));
        for [x, y] in [
            [-0x800, -0x800],
            [0x800, -0x800],
            [0x800, 0x800],
            [-0x800, 0x800],
        ] {
            send(buses, TEXCOORD, 0);
            send(
                buses,
                VTX_16,
                u32::from(x as u16) | u32::from(y as u16) << 16,
            );
            send(buses, VTX_16, 0);
        }
        send(buses, SWAP_BUFFERS, 0);
    }

    #[test]
    fn a_texture_written_between_two_identical_lists_shows_two_frames_later_like_melonds() {
        let mut buses = console();
        texels(&mut buses, RED);
        textured_quad(&mut buses);
        buses.elapse(2 * FRAME_CYCLES);
        assert_eq!(top(&buses, 100, 100), RED);
        buses.elapse(FRAME_CYCLES);
        assert_eq!(top(&buses, 100, 100), RED);
        texels(&mut buses, GREEN);
        buses.elapse(2 * FRAME_CYCLES);
        assert_eq!(top(&buses, 100, 100), GREEN);
    }

    fn engine_frame(three_d: &mut super::ThreeD, vram: &super::Vram) {
        three_d.step(FRAME_CYCLES);
        three_d.vblank(true);
        three_d.step(FRAME_CYCLES);
        three_d.render(vram, false);
    }

    fn engine_quad(three_d: &mut super::ThreeD) {
        let send = |three_d: &mut super::ThreeD, command: u8, param: u32| {
            three_d.write(0x400 + (usize::from(command) << 2), param, u32::MAX);
        };
        send(three_d, VIEWPORT, 0xBFFF_0000);
        send(three_d, POLYGON_ATTR, 0x001F_0080);
        send(three_d, BEGIN_VTXS, 1);
        send(three_d, COLOR, u32::from(WHITE));
        for [x, y] in [
            [-0x800, -0x800],
            [0x800, -0x800],
            [0x800, 0x800],
            [-0x800, 0x800],
        ] {
            send(
                three_d,
                VTX_16,
                u32::from(x as u16) | u32::from(y as u16) << 16,
            );
            send(three_d, VTX_16, 0);
        }
        send(three_d, SWAP_BUFFERS, 0);
    }

    #[test]
    fn an_unchanged_scene_keeps_its_lines_and_any_change_draws_it_again_like_melonds() {
        let mut three_d = super::ThreeD::default();
        three_d.write(super::render::CLEAR_DEPTH, 0x7FFF, u32::MAX);
        let mut vram = super::Vram::default();
        let blank = super::Line3d::default();
        let drawn_again = |three_d: &mut super::ThreeD, vram: &super::Vram| {
            three_d.lines[100] = blank.clone();
            engine_frame(three_d, vram);
            three_d.lines[100] != blank
        };
        engine_quad(&mut three_d);
        assert!(drawn_again(&mut three_d, &vram));
        assert!(!drawn_again(&mut three_d, &vram));
        three_d.write(super::render::CLEAR_COLOR, 0x7C00, u32::MAX);
        assert!(drawn_again(&mut three_d, &vram));
        assert!(!drawn_again(&mut three_d, &vram));
        three_d.write(super::render::TOON_TABLE, 0x001F, u32::MAX);
        assert!(drawn_again(&mut three_d, &vram));
        vram.write_control(super::super::vram::Bank::A, TEXTURE_SLOT_0 as u8);
        assert!(drawn_again(&mut three_d, &vram));
        vram.write_control(super::super::vram::Bank::A, TEXTURE_SLOT_0 as u8);
        assert!(!drawn_again(&mut three_d, &vram));
        vram.write_control(super::super::vram::Bank::B, 0x80);
        assert!(!drawn_again(&mut three_d, &vram));
        engine_quad(&mut three_d);
        assert!(drawn_again(&mut three_d, &vram));
        assert!(!drawn_again(&mut three_d, &vram));
    }

    #[test]
    fn a_3d_engine_cloned_or_copied_into_draws_what_the_original_draws() {
        let vram = super::Vram::default();
        let mut original = super::ThreeD::default();
        original.write(super::render::CLEAR_DEPTH, 0x7FFF, u32::MAX);
        engine_quad(&mut original);
        engine_frame(&mut original, &vram);
        let mut clone = original.clone();
        let mut copied = super::ThreeD::default();
        copied.write(super::render::CLEAR_COLOR, 0x001F, u32::MAX);
        engine_quad(&mut copied);
        engine_frame(&mut copied, &vram);
        copied.clone_from(&original);
        let textures =
            |three_d: &super::ThreeD| three_d.shown.as_ref().unwrap().textures.image.clone();
        assert!(std::sync::Arc::ptr_eq(
            &textures(&original),
            &textures(&clone)
        ));
        for three_d in [&mut original, &mut clone, &mut copied] {
            three_d.write(super::render::CLEAR_COLOR, 0x7C00, u32::MAX);
            engine_quad(three_d);
            engine_frame(three_d, &vram);
        }
        assert!(original.lines == clone.lines);
        assert!(original.lines == copied.lines);
        assert!(original.lines[0] != super::Line3d::default());
    }

    const DISPCAPCNT: u32 = IO + 0x64;
    const RDLINES_COUNT: u32 = IO + 0x320;
    const CAPTURE_3D_WHOLE_SCREEN: u32 = 0x8130_0000;

    fn busy_readings(skip: bool) -> Vec<Vec<u32>> {
        let mut buses = console();
        buses.set_skip_draw(skip);
        buses.elapse(LINE_CYCLES * u32::from(VISIBLE_LINES));
        (0..3)
            .map(|n| {
                let mut seen = busy_frame(&mut buses, n);
                seen.push(word(&mut buses, RDLINES_COUNT));
                buses.elapse(FRAME_CYCLES);
                seen.push(word(&mut buses, GXSTAT));
                seen.push(word(&mut buses, RDLINES_COUNT));
                seen
            })
            .collect()
    }

    #[test]
    fn a_console_that_skips_drawing_reads_the_same_gxstat_and_rdlines_count() {
        assert_eq!(busy_readings(true), busy_readings(false));
    }

    fn captures(skip: bool) -> Vec<Vec<u32>> {
        let mut buses = console();
        buses.arm9().write(VRAMCNT_A, Width::Byte, 0x80);
        buses.set_skip_draw(skip);
        buses.elapse(LINE_CYCLES * u32::from(VISIBLE_LINES));
        [10, 40, 10, 40, 40]
            .into_iter()
            .enumerate()
            .map(|(n, late)| {
                quads(&mut buses, n as i32);
                buses.elapse(LINE_CYCLES * late);
                buses
                    .arm9()
                    .write(DISPCAPCNT, Width::Word, CAPTURE_3D_WHOLE_SCREEN);
                buses.elapse(FRAME_CYCLES - LINE_CYCLES * late);
                (0..WIDTH * super::HEIGHT / 2)
                    .map(|i| word(&mut buses, LCDC_A + 4 * i as u32))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn a_console_that_skips_drawing_captures_the_same_3d_into_vram() {
        let watched = captures(false);
        assert!(watched.windows(2).any(|pair| pair[0] != pair[1]));
        assert!(watched.iter().any(|frame| frame.iter().any(|&w| w != 0)));
        assert_eq!(captures(true), watched);
    }

    #[test]
    fn a_console_that_draws_again_shows_a_whole_picture_from_the_next_frame() {
        let watched = frames(|buses| {
            buses.elapse(FRAME_CYCLES);
        });
        let left = LINE_CYCLES * (u32::from(VISIBLE_LINES) - 100);
        let mut n = 0;
        let back = frames(|buses| {
            match n {
                0 => {
                    buses.set_skip_draw(true);
                    buses.elapse(FRAME_CYCLES);
                }
                1 => {
                    buses.elapse(FRAME_CYCLES - left);
                    buses.set_skip_draw(false);
                    buses.elapse(left);
                }
                _ => {
                    buses.elapse(FRAME_CYCLES);
                }
            }
            n += 1;
        });
        assert_ne!(back[1], watched[1]);
        assert_eq!(back[2..], watched[2..]);
    }
}
