use super::Cart;
use crate::app::widgets::action::{action_height, icon_button};
use crate::audio::preview::{Playing, Preview, PreviewClock};
use crate::emulator::Emulator;
use crate::icons;
use crate::media::playback;
use citrine_gb_nds_files::{FileType, NodeId, Sdat, SoundKind, Source, Swar, Wave};
use citrine_gb_nds_sound::{Length, Player, sequence_length, wave_length};
use egui::{Align, Layout, RichText, Sense, Ui, WidgetText, vec2};
use std::sync::Arc;

const START_RATE: u32 = 48_000;
const NAME_WIDTH: f32 = 64.0;
const FACTS_WIDTH: f32 = 140.0;
const LOOP_WIDTH: f32 = 18.0;
const NO_BANK: &str = "Can't play: its instrument bank isn't in this sound archive.";
const NO_WAVES: &str = "Can't play: its wave archive isn't in this sound archive.";

fn cant_play(err: impl std::fmt::Display) -> String {
    format!("Can't play: {err}.")
}

impl Preview for Player {
    fn render(&mut self, frames: usize, sample_rate: u32, out: &mut Vec<f32>) {
        self.set_sample_rate(sample_rate);
        Player::render(self, frames, out);
    }

    fn is_done(&self) -> bool {
        Player::is_done(self)
    }

    fn position(&self) -> f64 {
        Player::position(self)
    }
}

struct Song {
    bank: Arc<[u8]>,
    archives: [Option<Arc<[u8]>>; 4],
    volume: u8,
}

struct WaveRow {
    index: Option<usize>,
    format: &'static str,
    rate: u16,
    length: Length,
}

enum Content {
    Song(Result<Song, String>),
    Waves(Vec<WaveRow>),
}

pub struct SoundView {
    content: Content,
    length: Option<Length>,
    playing: Option<(Option<usize>, PreviewClock)>,
}

impl SoundView {
    pub fn read(cart: &Cart, node: NodeId, kind: Option<FileType>, bytes: &[u8]) -> Option<Self> {
        let content = match kind? {
            FileType::Sseq => Content::Song(song(cart, node)),
            FileType::Swar => {
                let swar = Swar::parse(bytes).ok()?;
                Content::Waves(
                    (0..swar.len())
                        .filter_map(|i| Some(row(Some(i), &swar.wave(i).ok()?)))
                        .collect(),
                )
            }
            FileType::Swav => Content::Waves(vec![row(None, &Wave::parse_swav(bytes).ok()?)]),
            _ => return None,
        };
        let mut view = Self {
            content,
            length: None,
            playing: None,
        };
        if let Content::Song(Ok(_)) = &view.content {
            view.length = sequence_length(bytes).ok();
        }
        Some(view)
    }

    fn player(&self, bytes: &[u8], index: Option<usize>) -> Result<Player, String> {
        match &self.content {
            Content::Song(Ok(song)) => {
                Player::sequence(bytes, &song.bank, song.archives(), song.volume, START_RATE)
                    .map_err(cant_play)
            }
            Content::Song(Err(err)) => Err(err.clone()),
            Content::Waves(_) => {
                let wave = match index {
                    Some(index) => Swar::parse(bytes).and_then(|swar| swar.wave(index)),
                    None => Wave::parse_swav(bytes),
                };
                wave.map(|wave| Player::wave(&wave, START_RATE))
                    .map_err(cant_play)
            }
        }
    }

    fn now_playing(&self, emulator: &Emulator, index: Option<usize>) -> Option<&PreviewClock> {
        let (at, clock) = self.playing.as_ref()?;
        (*at == index && emulator.is_previewing(clock)).then_some(clock)
    }

    pub fn stop(&mut self, emulator: &mut Emulator) {
        if let Some((_, clock)) = self.playing.take()
            && emulator.is_previewing(&clock)
        {
            emulator.stop_preview();
        }
    }

    fn toggle(
        &mut self,
        bytes: &[u8],
        index: Option<usize>,
        emulator: &mut Emulator,
    ) -> Result<(), String> {
        if self.now_playing(emulator, index).is_some() {
            emulator.stop_preview();
            self.playing = None;
            return Ok(());
        }
        let (playing, clock) = Playing::new(Box::new(self.player(bytes, index)?));
        emulator.play_preview(playing);
        self.playing = Some((index, clock));
        Ok(())
    }
}

impl Song {
    fn archives(&self) -> [Option<&[u8]>; 4] {
        std::array::from_fn(|slot| self.archives[slot].as_deref())
    }
}

fn song(cart: &Cart, node: NodeId) -> Result<Song, String> {
    let missing = || NO_BANK.to_owned();
    let Some(Source::Sound { sdat, file }) = cart.tree.node(node).source() else {
        return Err(missing());
    };
    let archive = cart.file(sdat).ok_or_else(missing)?;
    let sdat = Sdat::parse(&archive).map_err(cant_play)?;
    let sequence = sdat
        .entry_for_file(SoundKind::Sequence, file)
        .ok_or_else(missing)?;
    let bank = sequence
        .bank
        .and_then(|index| sdat.entry(SoundKind::Bank, index))
        .ok_or_else(missing)?;
    let load = |file: usize| sdat.file(file).ok().map(Arc::<[u8]>::from);
    let mut archives: [Option<Arc<[u8]>>; 4] = Default::default();
    for (slot, index) in bank.wave_archives.into_iter().enumerate() {
        let Some(index) = index else {
            continue;
        };
        let entry = sdat.entry(SoundKind::WaveArchive, index);
        archives[slot] = Some(
            entry
                .and_then(|entry| load(entry.file))
                .ok_or_else(|| NO_WAVES.to_owned())?,
        );
    }
    Ok(Song {
        bank: load(bank.file).ok_or_else(missing)?,
        archives,
        volume: sequence.volume,
    })
}

fn row(index: Option<usize>, wave: &Wave) -> WaveRow {
    WaveRow {
        index,
        format: wave.format.label(),
        rate: wave.sample_rate,
        length: wave_length(wave),
    }
}

pub fn show(
    view: &mut SoundView,
    bytes: &[u8],
    ui: &mut Ui,
    emulator: &mut Emulator,
    node: NodeId,
) {
    let mut toggled = None;
    egui::ScrollArea::vertical()
        .id_salt(("ds_sound", node))
        .auto_shrink(false)
        .show(ui, |ui| match &view.content {
            Content::Song(Err(err)) => {
                ui.weak(err);
            }
            Content::Song(Ok(_)) => {
                if bar(ui, view, emulator, None, None, view.length) {
                    toggled = Some(None);
                }
            }
            Content::Waves(rows) => {
                for wave in rows {
                    let name = wave
                        .index
                        .map_or_else(|| "Wave".to_owned(), |index| format!("Wave {index}"));
                    let facts = format!("{} · {} Hz", wave.format, wave.rate);
                    ui.push_id(("ds_wave", wave.index), |ui| {
                        if bar(
                            ui,
                            view,
                            emulator,
                            wave.index,
                            Some((name, facts)),
                            Some(wave.length),
                        ) {
                            toggled = Some(wave.index);
                        }
                    });
                }
            }
        });
    if let Some(index) = toggled
        && let Err(err) = view.toggle(bytes, index, emulator)
    {
        view.content = Content::Song(Err(err));
    }
    if view.playing.is_some() {
        ui.ctx().request_repaint();
    }
}

fn bar(
    ui: &mut Ui,
    view: &SoundView,
    emulator: &Emulator,
    index: Option<usize>,
    label: Option<(String, String)>,
    length: Option<Length>,
) -> bool {
    let playing = view.now_playing(emulator, index);
    let position = playing.map_or(0.0, PreviewClock::seconds);
    let mut clicked = false;
    ui.horizontal(|ui| {
        let side = action_height(ui);
        let (icon, hint) = if playing.is_some() {
            (icons::STOP, "Stop")
        } else {
            (icons::PLAY, "Play")
        };
        clicked = ui
            .add(icon_button(icon, side))
            .on_hover_text(hint)
            .clicked();
        if let Some((name, facts)) = label {
            cell(ui, NAME_WIDTH, side, name.into());
            cell(ui, FACTS_WIDTH, side, RichText::new(facts).weak().into());
        }
        ui.label(RichText::new(playback::clock(position)).monospace().small());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (slot, _) = ui.allocate_exact_size(vec2(LOOP_WIDTH, side), Sense::hover());
            if let Some(start) = length.and_then(|length| length.loop_start) {
                ui.put(slot, egui::Label::new(RichText::new(icons::REPEAT).weak()))
                    .on_hover_text(format!("Loops back to {}", playback::clock(start)));
            }
            if let Some(length) = length {
                ui.label(
                    RichText::new(playback::clock(length.seconds.ceil()))
                        .monospace()
                        .small(),
                );
            }
            let fraction = match length {
                Some(length) if length.seconds > 0.0 => (position / length.seconds).clamp(0.0, 1.0),
                _ => 0.0,
            };
            ui.add(egui::ProgressBar::new(fraction as f32).desired_height(4.0));
        });
    });
    clicked
}

fn cell(ui: &mut Ui, width: f32, height: f32, text: WidgetText) {
    ui.allocate_ui_with_layout(
        vec2(width, height),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_size(vec2(width, height));
            ui.add(egui::Label::new(text).truncate());
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use citrine_gb_nds_files::fixtures::sdat::{FixtureSound, build};
    use citrine_gb_nds_files::fixtures::{bank, nds, sseq, wave};
    use citrine_gb_nds_files::{Instrument, NoteDef, Voice, WaveFormat};

    fn sound(kind: SoundKind, name: &'static str, links: [u16; 4], bytes: Vec<u8>) -> FixtureSound {
        FixtureSound {
            kind,
            name: Some(name),
            links,
            bytes,
        }
    }

    fn cart() -> Cart {
        cart_with_waves(0)
    }

    fn cart_with_waves(wave_archive: u16) -> Cart {
        let note = NoteDef {
            voice: Voice::Pcm {
                wave: 0,
                archive: 0,
            },
            base_note: 60,
            attack: 127,
            decay: 127,
            sustain: 127,
            release: 100,
            pan: 64,
        };
        let waves = wave::build_swar(&[
            wave::build_info(WaveFormat::Pcm8, true, 0, &[0x40, 0xC0, 0x40, 0xC0]),
            wave::build_info(WaveFormat::Adpcm, false, 0, &[0, 0, 0, 0, 0x77, 0x77]),
        ]);
        let none = [0xFFFF; 4];
        let archive = build(&[
            sound(
                SoundKind::Sequence,
                "SEQ_TOWN",
                [0, 0, 0, 0],
                sseq::build(&[0x3C, 0x7F, 48, 0x94, 0, 0, 0]),
            ),
            sound(
                SoundKind::Bank,
                "BANK_TOWN",
                [wave_archive, 0xFFFF, 0xFFFF, 0xFFFF],
                bank::build(&[Some(Instrument::Single(note))]),
            ),
            sound(SoundKind::WaveArchive, "WAVE_TOWN", none, waves),
        ]);
        let bytes = nds::build(b"CPUE", &[("data/sound.sdat", archive)]);
        Cart::read("Test".into(), Arc::from(bytes), false).unwrap()
    }

    fn view(cart: &Cart, path: &[&str]) -> (SoundView, Arc<[u8]>) {
        let node = cart.tree.find(path).unwrap();
        let bytes = cart.file(node).unwrap();
        let kind = cart.tree.node(node).file_type();
        (SoundView::read(cart, node, kind, &bytes).unwrap(), bytes)
    }

    #[test]
    fn a_sequence_finds_its_bank_and_waves_and_plays_until_stopped() {
        let cart = cart();
        let (mut view, bytes) = view(&cart, &["data", "sound.sdat", "Sequences", "SEQ_TOWN"]);
        let length = view.length.unwrap();
        assert!(length.loop_start.is_some_and(|start| start < 0.01));
        let (mut playing, clock) = Playing::new(Box::new(view.player(&bytes, None).unwrap()));
        let mut out = Vec::new();
        assert!(playing.render(4800, 48_000, &mut out));
        assert!(out.iter().any(|s| s.abs() > 0.01));
        assert!(clock.seconds() > 0.0, "{}", clock.seconds());
        let mut emulator = Emulator::default();
        view.toggle(&bytes, None, &mut emulator).unwrap();
        assert!(view.now_playing(&emulator, None).is_some());
        view.toggle(&bytes, None, &mut emulator).unwrap();
        assert!(view.now_playing(&emulator, None).is_none());
        view.toggle(&bytes, None, &mut emulator).unwrap();
        let clock = view.now_playing(&emulator, None).unwrap().clone();
        view.stop(&mut emulator);
        assert!(!emulator.is_previewing(&clock));
    }

    #[test]
    fn a_wave_archive_lists_each_wave_and_plays_one_at_a_time() {
        let cart = cart();
        let (mut view, bytes) = view(&cart, &["data", "sound.sdat", "Wave archives", "WAVE_TOWN"]);
        let Content::Waves(rows) = &view.content else {
            panic!("not a wave list");
        };
        let facts: Vec<_> = rows.iter().map(|r| (r.index, r.format, r.rate)).collect();
        assert_eq!(
            facts,
            [(Some(0), "8-bit PCM", 16_000), (Some(1), "ADPCM", 16_000)]
        );
        let mut emulator = Emulator::default();
        view.toggle(&bytes, Some(0), &mut emulator).unwrap();
        view.toggle(&bytes, Some(1), &mut emulator).unwrap();
        assert!(view.now_playing(&emulator, Some(0)).is_none());
        assert!(view.now_playing(&emulator, Some(1)).is_some());
    }

    #[test]
    fn a_sequence_whose_wave_archive_is_missing_says_so() {
        let cart = cart_with_waves(7);
        let (view, _) = view(&cart, &["data", "sound.sdat", "Sequences", "SEQ_TOWN"]);
        let Content::Song(Err(err)) = &view.content else {
            panic!("expected a missing wave archive");
        };
        assert_eq!(err, NO_WAVES);
    }

    #[test]
    fn a_sequence_without_its_bank_says_so() {
        let bytes = nds::build(
            b"CPUE",
            &[("data/song.sseq", sseq::build(&[0x3C, 0x7F, 48, 0xFF]))],
        );
        let cart = Cart::read("Test".into(), Arc::from(bytes), false).unwrap();
        let (view, _) = view(&cart, &["data", "song.sseq"]);
        let Content::Song(Err(err)) = &view.content else {
            panic!("expected a missing bank");
        };
        assert_eq!(err, NO_BANK);
    }
}
