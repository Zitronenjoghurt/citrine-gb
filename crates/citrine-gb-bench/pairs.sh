#!/bin/sh
set -eu
out=$1
head=$(cd "$2" && pwd)
base=${3:+$(cd "$3" && pwd)}
rounds=${ROUNDS:-10}
extra=${EXTRA:+--extra $EXTRA}
build() {
  if [ ! -d "$1/crates/citrine-gb-bench" ]; then
    cp -r "$head/crates/citrine-gb-bench" "$1/crates/"
    cp "$head/Cargo.lock" "$1/Cargo.lock"
  fi
  (cd "$1" && CARGO_TARGET_DIR="$1/target/bench-wall" cargo build -q --profile dist -p citrine-gb-bench)
  echo "$1/target/bench-wall/dist/citrine-gb-bench"
}
mkdir -p "$out"
head_bin=$(build "$head")
base_bin=${base:+$(build "$base")}
host="$(hostname) $(lscpu | awk -F: '/Model name/ {gsub(/^ +/, "", $2); print $2; exit}')"
for round in $(seq "$rounds"); do
  if [ -n "$base" ]; then "$base_bin" run $extra --out "$out/base.$round.json" > /dev/null || true; fi
  "$head_bin" run $extra --out "$out/head.$round.json" > /dev/null || true
done
for side in head ${base:+base}; do
  eval bin=\$${side}_bin
  set --
  for round in $(seq "$rounds"); do set -- "$@" --readings "$out/$side.$round.json"; done
  for id in $("$bin" list $extra); do
    set -- "$@" --peak "$id=$("$bin" one "$id" $extra 2>/dev/null | awk '{print $5}')"
  done
  "$bin" record $extra --out "$out/$side.json" "$@" --meta "host=$host" || true
done
