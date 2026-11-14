#!/bin/sh
set -eu
out=${1:-target/bench/ir}
profiles=${PROFILES:-dist release}
repeat=${REPEAT:-3}
jobs=${JOBS:-$(nproc)}
extra=${EXTRA:+--extra $EXTRA}
export CARGO_HOME=${CARGO_HOME_CACHE:-$PWD/target/bench-cargo}
mkdir -p "$out"
rustc -V > "$out/rustc.txt"
valgrind --version > "$out/valgrind.txt"
for profile in $profiles; do
  [ "${IDS:-}" = app ] && break
  CARGO_PROFILE_DIST_STRIP=false CARGO_TARGET_DIR=target/bench-$profile \
    cargo build -q --locked --profile "$profile" -p citrine-gb-bench
  bin=target/bench-$profile/$profile/citrine-gb-bench
  mkdir -p "$out/$profile"
  if [ -n "${IDS:-}" ]; then echo "$IDS" | tr " " "\n" > "$out/ids.txt"; else $bin list $extra > "$out/ids.txt"; fi
  for round in $(seq "$repeat"); do
    while read -r id; do echo "$id $round"; done < "$out/ids.txt"
  done | xargs -P "$jobs" -L 1 sh -c '
    valgrind --tool=callgrind --toggle-collect="citrine_gb_bench::run::measured*" \
      --compress-strings=no --compress-pos=no \
      --callgrind-out-file="$0/$3.$4.out" "$1" one "$3" $2 > "$0/$3.$4.log" 2>&1 \
      || echo "$3" >> "$0/failed.txt"' "$out/$profile" "$bin" "$extra"
done
if [ "${APP:-1}" = 1 ] && { [ -z "${IDS:-}" ] || [ "$IDS" = app ]; }; then
  for profile in $profiles; do mkdir -p "$out/$profile"; done
  for profile in $profiles; do
    CARGO_PROFILE_DIST_STRIP=false CARGO_TARGET_DIR=target/bench-$profile \
      cargo build -q --locked --profile "$profile" -p citrine-gb-app --example app_bench
    bin=target/bench-$profile/$profile/examples/app_bench
    for round in $(seq "$repeat"); do
      for case in start library paused; do
        valgrind --tool=callgrind --toggle-collect="*app_bench::measured*" \
          --compress-strings=no --compress-pos=no \
          --callgrind-out-file="$out/$profile/app-$case.$round.out" \
          "$bin" "$case" crates/citrine-gb-app/homebrew/tobu.gb > "$out/$profile/app-$case.$round.log" 2>&1 \
          || echo "app-$case" >> "$out/$profile/failed.txt"
      done
    done
  done
fi
