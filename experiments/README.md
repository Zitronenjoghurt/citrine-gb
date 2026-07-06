# Experiments

One-shot snapshots of the emulator's accuracy, collected with `make results`
(= `cargo run --release -p citrine-gb-lab --bin collect`). Each run captures:

1. **Mooneye suite** — every DMG acceptance/emulator-only ROM, executed in-process with the
   same Fibonacci-register convention as `lib/tests/mooneye.rs`.
2. **SameBoy frame diff** — every ROM in `roms/games/` and `roms/test/` (dmg-acid2 + blargg)
   compared against SameBoy for 3600 frames (cycle-aligned, greyscale-normalized, no PNG
   dumps), swept over alignment tolerances 0, 1, 2, 5 and 10. ROMs with a matching input
   recording in `roms/*.json` (matched by SHA-256) get an additional replayed run.

## Layout

Runs are versioned by date and commit and meant to be **committed**, so the emulator's
progress can be compared over time (`git diff` between two runs' CSVs works line-by-line):

```
experiments/runs/<YYYY-MM-DD>_<git-short-hash>/
  results.json               complete machine-readable record incl. metadata
  mooneye_tests.csv          one row per mooneye test (pass/fail + failure note)
  mooneye_summary.csv        pass/fail counts per category
  diff_results.csv           one row per (ROM, tolerance): match rate, divergence span,
                             mean/best/worst for exact, px_match, mse, nmse, psnr, ssim
  diff_pivot_match_rate.csv  ROM rows x tolerance columns (thesis-table-ready)
  diff_pivot_ssim.csv        ROM rows x tolerance columns
  raw/                       full per-frame reports (only with --per-frame; gitignored)
```

## Options

```
--frames N            frames per diff run (default 3600)
--tolerances 0,1,2    tolerance sweep (default 0,1,2,5,10)
--jobs N              parallel diff runs (default 3; each run uses ~3 threads)
--only SUBSTR         only diff ROMs whose file name contains SUBSTR
--skip-mooneye / --skip-diff
--per-frame           also write full per-frame reports to raw/ (for plots)
--out DIR             override the output directory
```

Interesting axes beyond the tolerance sweep (all supported by the lab CLI for one-off
follow-ups): `--align emission` vs `cycle`, `--raw` RGB vs greyscale normalization, longer
`--frames`, and additional input recordings.
