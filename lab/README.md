# Citrine Lab

An experimentation harness for the Citrine Game Boy emulator. The first experiment compares
Citrine's frame output against the reference emulator **SameBoy**, frame by frame.

Everything is built around three traits so experiments can swap implementations freely:

- `FrameEmulator` (`src/emulator.rs`) — anything that can be stepped, fed cycle-timed input, and
  produce frames. Adapters: `CitrineEmulator`, `SameBoyEmulator` (`src/emulators/`).
- `FrameMetric` (`src/metric.rs`) — a scalar similarity measure. Built-ins: `ExactFrame` (`exact`),
  `ExactPixelRatio` (`px_match`), `Mse` (`mse`), `Nmse` (`nmse`, MSE normalized to 0..1), `Psnr`
  (`psnr`, in dB; identical frames reported as 100 dB), `Ssim` (`ssim`) (`src/metrics/`).
- `Reporter` (`src/report.rs`) — renders a comparison. Built-ins: console table, JSON, PNG-diff.

The comparison engine (`src/runner.rs`) and recording format
(`citrine_gb::recording`, shared with the app frontend) are emulator-agnostic.

## Building SameBoy

SameBoy is a git submodule at `lab/SameBoy`, compiled by the `sameboy-sys` crate's `build.rs`
(via the `cc` crate, with hand-written FFI — no bindgen/libclang needed).

```sh
git submodule update --init lab/SameBoy
cargo build -p citrine-gb-lab
```

## Running a comparison

```sh
cargo run --release -p citrine-gb-lab -- \
    --rom roms/test/dmg-acid2.gb \
    --boot-rom roms/boot/dmg_boot.bin \
    --frames 600 \
    --metrics exact,px_match,mse,nmse,psnr,ssim \
    --dump-divergences ./lab/diff \
    --json ./lab/report.json \
    --tolerance 2
```

- `--boot-rom` is **recommended**: SameBoy needs a boot ROM to run, and feeding the same one to
  both emulators makes the comparison start from an identical power-on state. Without it, SameBoy
  will not execute the cartridge. (The boot ROM is also fed to Citrine.)
- Output is normalized to a canonical greyscale by default so palette/theme choices don't count as
  differences; pass `--raw` to compare actual RGB output.
- SameBoy is the reference; Citrine is the candidate under test.
- `--dump-divergences` defaults to **no cap** — on a run where most frames differ that is three PNGs
  per frame (tens of thousands of files). Use `--max-dumps N` to bound it.
- `--align` controls how the two frame sequences are paired. `cycle` (default) pairs each reference
  frame with the candidate frame closest in emulated cycle count, cancelling the phase offset that
  boot/intro LCD on/off transitions introduce (this can grow to several whole frames — e.g. Citrine
  ends up ~4.3 frames ahead of SameBoy partway through Link's Awakening's intro, then holds steady);
  `emission` pairs the Nth emitted frame of each. Cycle pairing is the fair key for an accuracy
  benchmark — both emulators are fed identical input at identical cycles, so equal cycle counts mean
  equal emulated time.
- `--tolerance N` (cycle mode only) widens the match to a ±N-frame window: a frame counts as
  matching if any candidate within N frames of the nearest-cycle one matches, and each metric keeps
  its best score over the window. `0` (default) is strict. `1` absorbs the **irreducible sub-frame
  sampling skew** — two independent emulators latch their framebuffers a few hundred cycles apart,
  so a game update landing in that gap shows up one frame early/late through no fault of either
  emulator. On Link's Awakening, `--tolerance 1` recovers ~14% of frames (the genuine off-by-ones);
  `--tolerance 2` and beyond recover almost nothing, confirming the skew never exceeds one frame and
  that the remaining divergence is real rendering difference, not phase. Report both `0` and `1` for
  an honest picture: `0` is the strict bound, `1` is "correct to within one frame".

### Performance

The comparison streams: the two emulators run on their own threads while the main thread scores
each frame pair and discards it, so memory stays flat (a few MB) regardless of `--frames`. SameBoy
runs in turbo mode (its core otherwise `nanosleep`s to pin itself to real-time 60 fps — the single
biggest cost on large runs). A progress bar (`indicatif`) shows throughput and ETA. Expect roughly
~900 frames/sec for a 10k-frame run with all four metrics.

## Recording input

In the Citrine app, the **Debug Actions** tab has Start/Stop/Export recording controls. A recording
captures button events with **absolute** T-cycle timestamps (`gb.debugger.total_cycles * 4`, which
resets to 0 on ROM load) as JSON (`citrine_recording.json`). Because the lab replays from a fresh
load at cycle 0, those absolute cycles line up with the reconstructed state — so start recording
right after loading the ROM. Replay it here with `--recording`.
