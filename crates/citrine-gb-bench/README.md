# citrine-gb-bench

Benchmarks for the GB, GBC, GBA and DS cores and the app. Every run also checks the output, so a faster but wrong core fails instead of scoring.

## Workloads

`workloads.toml` lists them. Each one names a console, a kind, a ROM and a frame count:

- `solo`: one console on a scripted input from a fixed seed. On the DS this is the whole system, both CPUs and both screens, running a blocksds example; `script = "touch"` also moves the pen
- `pair`, `link`, `rollback`: two consoles in a Play together session, unplugged, on the cable, or with remote input arriving late so the session rolls back and runs frames again
- `state`: a save state and load every 30 frames, checked by state hash
- `ds3d`, `ds2d`: the DS 3D engine and 2D engine A drawing fixed scenes through their register interfaces

ROMs come from the app's homebrew folder or the test ROM ledger (`tests/ledger/manifest.toml`), checked against their SHA-256. The DS examples are built from source with docker by `make ledger-nds`, which `make bench` and `make bench-ir` run first.

## Checks

For every workload, `expected.toml` holds the frame hashes at a third, two thirds and the end, the audio hash, the state hash and a few counts (samples, cable transfers, rollbacks, polygons, DS cycles). A run that differs from them fails and gives no number.

`bless` rewrites the expected values. Bless in a commit of its own, and only for an accuracy change that the ledger and the lab confirm. A speed change never changes a hash.

## Readings

- Instruction counts (Ir) from callgrind are the main reading. They come from a pinned image (`Dockerfile`: Debian digest and Rust version) and are the same on any x86 machine. Both the dist (desktop, Android) and release (web) profiles are counted.
- glibc picks its memset, memcpy and maths code by CPU, and sizes the `rep stosb` threshold from the cache sizes. Without a pin, gb-tobu counted 0.5% more on a Ryzen 5950X than on an i5-3470, all of it in memset. The image sets `GLIBC_TUNABLES` to turn off the AVX, FMA and ERMS paths and to fix the cache sizes and thresholds, so both machines give the same count to the instruction.
- Wall time is the second reading. `pairs.sh` runs base and head in turns on one machine, so the pairs share its load.
- Wall time catches what counts miss: cache misses, threads, the GPU, wasm in a browser, SIMD width and frame pacing.

## Commands

```
make bench                         run and check every workload, wall time
make bench-ir                      callgrind counts in the pinned image (PROFILES, REPEAT, JOBS, IDS, IDS=app runs only the app, APP=0 skips it)
make bench-wall BASE_TREE=../base  base and head wall time in turns
make bench-record                  merge counts and wall time into target/bench/reading.json
make bench-compare BASE=crates/citrine-gb-bench/baselines/0.12.0-x86_64.json
make bench-flame ID=gb-tobu        flame graph from the first callgrind run, BASE_IR for a differential one
```

`EXTRA=<dir>` and `--extra <dir>` run a second set of workloads from `<dir>/workloads.toml` and `<dir>/expected.toml` instead of the public one.

## What counts as a change

- Ir: the median moves by more than three times the spread of the repeat runs, and by at least 0.3%.
- Wall time: the middle half of the base and head pair ratios lies on one side of 1, and the median ratio moves by at least 1%.
- Each console's headline is the geometric mean of its workloads, and any single workload past its bound counts on its own.

## Profiling the app

Build the app with `--features profile` for the puffin profiler window. Ctrl+Shift+P (Cmd on macOS) shows or hides it.
