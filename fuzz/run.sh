#!/usr/bin/env bash
# Security fuzz run. From the repository root:
#   fuzz/run.sh [--corpus DIR] [--out DIR] [--seconds N] [--state-runs FROM:TO] PART [PART]
# Parts: targets (every cargo-fuzz target for N seconds, default 120, with its
# corpus in DIR/<target>), audit (cargo audit of both lock files), state (the
# damaged state tests, runs FROM to TO of each, default 0:5000; a range splits
# one seed over several jobs) and diff (the JIT differential, 30000 programs).
# state and diff in one call share one build. Each part prints one line
# "fuzz: <part> ok|FAIL <detail>"; the exit status is 1 when any part fails.
# Logs, crash inputs and DIR/<part>.fail (the lines worth reading) go to --out.
set -u

CORPUS=fuzz/corpus
OUT=fuzz/out
SECONDS_PER_TARGET=120
STATE_RUNS=0:5000
PARTS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --corpus) CORPUS=$2; shift 2 ;;
    --out) OUT=$2; shift 2 ;;
    --seconds) SECONDS_PER_TARGET=$2; shift 2 ;;
    --state-runs) STATE_RUNS=$2; shift 2 ;;
    targets|audit|state|diff) PARTS+=("$1"); shift ;;
    *) echo "fuzz/run.sh: unknown argument $1" >&2; exit 2 ;;
  esac
done
[ ${#PARTS[@]} -gt 0 ] || { echo "fuzz/run.sh: name at least one part" >&2; exit 2; }
case "$STATE_RUNS" in
  *[!0-9:]*|:*|*:|*:*:*) echo "fuzz/run.sh: --state-runs takes FROM:TO" >&2; exit 2 ;;
  *:*) ;;
  *) echo "fuzz/run.sh: --state-runs takes FROM:TO" >&2; exit 2 ;;
esac
STATE_FROM=${STATE_RUNS%:*}
STATE_TO=${STATE_RUNS#*:}
[ "$STATE_FROM" -lt "$STATE_TO" ] || { echo "fuzz/run.sh: --state-runs FROM must be below TO" >&2; exit 2; }
[ -f fuzz/Cargo.toml ] || { echo "fuzz/run.sh: run it from the repository root" >&2; exit 2; }

ROOT=$PWD
case "$CORPUS" in /*) ;; *) CORPUS=$ROOT/$CORPUS ;; esac
case "$OUT" in /*) ;; *) OUT=$ROOT/$OUT ;; esac
mkdir -p "$OUT" "$CORPUS"
TOOLS=$ROOT/fuzz/.tools-$(uname -s)-$(uname -m)
export PATH=$TOOLS/bin:$PATH
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 RUSTUP_PERMIT_COPY_RENAME=1
case $(uname -s) in Darwin) export CARGO_BUILD_JOBS=3 ;; *) export CARGO_BUILD_JOBS=4 ;; esac
FAILED=0

report() {
  local part=$1 verdict=$2
  shift 2
  echo "fuzz: $part $verdict $*" | sed "s|$ROOT/||g"
  [ "$verdict" = ok ] || FAILED=1
}

toolchain() {
  rustup toolchain install "$1" --profile minimal >/dev/null 2>&1
  if ! cargo "+$1" -V >/dev/null 2>&1; then
    rustup toolchain uninstall "$1" >/dev/null 2>&1
    rustup toolchain install "$1" --profile minimal >/dev/null 2>&1
  fi
  cargo "+$1" -V >/dev/null 2>&1
}

tool() {
  local toolchain=$1 crate=$2 probe=$3
  $probe >/dev/null 2>&1 && return 0
  cargo "+$toolchain" install "$crate" --locked --force --root "$TOOLS" >"$OUT/install-$crate.log" 2>&1
  $probe >/dev/null 2>&1
}

with_limit() {
  if command -v timeout >/dev/null 2>&1; then timeout "$@"; else shift; "$@"; fi
}

run_targets() {
  toolchain nightly || { report targets FAIL "no nightly toolchain"; return; }
  tool nightly cargo-fuzz "cargo +nightly fuzz --version" || { report targets FAIL "cargo-fuzz install failed, $OUT/install-cargo-fuzz.log"; return; }
  rm -rf fuzz/artifacts "$OUT/targets" "$OUT/crashes"
  mkdir -p "$OUT/targets"
  if ! (cd fuzz && cargo +nightly fuzz build -O) >"$OUT/targets/build.log" 2>&1; then
    grep -E "^(error|warning)|^ *--> " "$OUT/targets/build.log" | head -40 >"$OUT/targets.fail"
    report targets FAIL "build failed, $OUT/targets/build.log"
    return
  fi
  local names target log rc unit execs total=0 count=0 crashed=""
  names=$(cd fuzz && cargo +nightly fuzz list)
  for target in $names; do
    log=$OUT/targets/$target.log
    mkdir -p "$CORPUS/$target"
    (cd fuzz && cargo +nightly fuzz run "$target" -O "$CORPUS/$target" -- \
      -max_total_time="$SECONDS_PER_TARGET" -rss_limit_mb=2048 -timeout=10 -report_slow_units=0) >"$log" 2>&1
    rc=$?
    unit=$(grep -o "Test unit written to [^ ]*" "$log" | grep -v "/slow-unit-" | tail -1 | sed "s/Test unit written to //")
    execs=$(grep -E "^#[0-9]+" "$log" | tail -1 | sed -E "s/^#([0-9]+).*/\1/")
    count=$((count + 1))
    total=$((total + ${execs:-0}))
    if [ "$rc" -eq 0 ] && [ -z "$unit" ]; then
      echo "$target clean ${execs:-0} runs" >>"$OUT/targets/summary.txt"
      continue
    fi
    mkdir -p "$OUT/crashes/$target"
    [ -d "fuzz/artifacts/$target" ] && cp fuzz/artifacts/"$target"/* "$OUT/crashes/$target/" 2>/dev/null
    [ -n "$unit" ] && unit=$OUT/crashes/$target/$(basename "$unit")
    crashed="$crashed $target:${unit:-exit$rc}"
    {
      echo "== $target exit $rc input ${unit#"$ROOT"/}"
      grep -E "panicked|^thread '|ERROR: |SUMMARY: |deadly signal|ALARM: |out-of-memory|^ +#[0-9]+ 0x" "$log" | head -20
    } >>"$OUT/targets.fail"
  done
  rm -rf fuzz/artifacts
  if [ "$count" -eq 0 ]; then
    report targets FAIL "cargo fuzz list named no targets"
  elif [ -n "$crashed" ]; then
    report targets FAIL "crashed:$crashed"
  else
    report targets ok "$count targets clean, $total runs, $(find "$CORPUS" -type f | wc -l | tr -d ' ') corpus files"
  fi
}

audit_ids() {
  awk 'BEGIN { RS = "" } /(^|\n)ID:/ && !/(^|\n)Warning:/ { if (match($0, /ID: +[A-Z]+-[0-9]+-[0-9]+/)) { id = substr($0, RSTART, RLENGTH); sub(/ID: +/, "", id); printf "%s ", id } }' "$1"
}

run_audit() {
  toolchain stable || { report audit FAIL "no stable toolchain"; return; }
  tool stable cargo-audit "cargo +stable audit --version" || { report audit FAIL "cargo-audit install failed, $OUT/install-cargo-audit.log"; return; }
  local lock name rc ids failed="" summary=""
  for lock in Cargo.lock fuzz/Cargo.lock; do
    name=$(echo "$lock" | tr / -)
    cargo +stable audit --file "$lock" >"$OUT/audit-$name.log" 2>&1
    rc=$?
    ids=$(audit_ids "$OUT/audit-$name.log")
    if [ "$rc" -eq 0 ]; then
      summary="$summary $lock clean"
    else
      failed="$failed $lock: ${ids:-exit $rc}"
      {
        echo "== $lock exit $rc"
        grep -E "^(Crate|Version|Title|ID|Solution):|^error|panicked|^invalid" "$OUT/audit-$name.log" | head -30
      } >>"$OUT/audit.fail"
    fi
  done
  if [ -n "$failed" ]; then
    report audit FAIL "advisories in$failed"
  else
    report audit ok "$(cargo +stable audit --version | head -1),$summary"
  fi
}

test_counts() {
  local log=$1 pattern=$2 invert=$3 passed failed
  passed=$(grep -E "^test .* \.\.\. ok$" "$log" | grep -c $invert -E "$pattern")
  failed=$(grep -E "^test .* \.\.\. FAILED$" "$log" | grep -c $invert -E "$pattern")
  echo "$passed $failed"
}

unfinished_tests() {
  local log=$1 pattern=$2 invert=$3 name
  grep -E "^test .* has been running for over" "$log" | grep $invert -E "$pattern" | sed -E "s/^test ([^ ]+) has been.*/\1/" | sort -u |
    while read -r name; do
      grep -qE "^test $name \.\.\. (ok|FAILED)" "$log" || printf "%s " "$name"
    done
}

run_tests() {
  local want_state=$1 want_diff=$2 filters=() flags=() limit=1800
  toolchain stable || {
    [ "$want_state" = 1 ] && report state FAIL "no stable toolchain"
    [ "$want_diff" = 1 ] && report diff FAIL "no stable toolchain"
    return
  }
  [ "$want_state" = 1 ] && filters+=(state_fuzz) && flags+=(--nocapture) && limit=6300
  [ "$want_diff" = 1 ] && filters+=(arm7_ armv5_ timed_armv4_)
  export CARGO_PROFILE_DEV_OPT_LEVEL=2 CITRINE_STATE_FUZZ_FROM=$STATE_FROM CITRINE_STATE_FUZZ_RUNS=$STATE_TO CITRINE_DIFF_PROGRAMS=30000
  local cargo_test=(cargo +stable test -p citrine-gb --features jit,persistence,brotli --lib)
  local build=$OUT/tests-build.log log=$OUT/tests.log rc
  if ! "${cargo_test[@]}" --no-run >"$build" 2>&1; then
    grep -E "^(error|warning)|^ *--> " "$build" | head -40 >"$OUT/tests.fail"
    [ "$want_state" = 1 ] && report state FAIL "build failed, $build"
    [ "$want_diff" = 1 ] && report diff FAIL "build failed, $build"
    return
  fi
  local started took
  started=$(date +%s)
  with_limit "$limit" "${cargo_test[@]}" -- --test-threads=4 "${flags[@]}" "${filters[@]}" >"$log" 2>&1
  rc=$?
  took=$(($(date +%s) - started))
  grep -E "^---- |panicked at|seed|^  left|^ right|runs differ|^\[[0-9A-F, ]+\] after [0-9]+ instructions|^    [a-z0-9_:]+$" "$log" | grep -v ", 0 panic sites$" | cut -c1-400 | head -80 >"$OUT/tests.fail"
  if [ -s "$OUT/tests.fail" ]; then
    {
      echo "repeat on $(uname -s) $(uname -m): CARGO_PROFILE_DEV_OPT_LEVEL=2 CITRINE_STATE_FUZZ_FROM=$CITRINE_STATE_FUZZ_FROM CITRINE_STATE_FUZZ_RUNS=$CITRINE_STATE_FUZZ_RUNS CITRINE_DIFF_PROGRAMS=$CITRINE_DIFF_PROGRAMS ${cargo_test[*]} -- --test-threads=4 --nocapture <test name>"
      echo "the differential seeds are fixed per test in jit/differential.rs; a state fuzz failure prints its seed, give it back as CITRINE_STATE_FUZZ_SEED"
    } >>"$OUT/tests.fail"
  else
    rm -f "$OUT/tests.fail"
  fi
  local part label invert counts passed failed hung
  for part in state diff; do
    if [ "$part" = state ]; then
      [ "$want_state" = 1 ] || continue
      invert=""
      label="state runs $STATE_FROM-$STATE_TO"
    else
      [ "$want_diff" = 1 ] || continue
      invert="-v"
      label=diff
    fi
    counts=$(test_counts "$log" state_fuzz "$invert")
    passed=${counts% *}
    failed=${counts#* }
    hung=$(unfinished_tests "$log" state_fuzz "$invert")
    if [ "$rc" -eq 124 ] && [ -n "$hung" ]; then
      report "$label" FAIL "stopped after ${took} s, unfinished: $hung"
    elif [ "$failed" -gt 0 ]; then
      report "$label" FAIL "$failed of $((passed + failed)) tests failed, $log"
    elif [ "$passed" -eq 0 ]; then
      report "$label" FAIL "no tests matched (exit $rc), $log"
    elif [ "$rc" -ne 0 ]; then
      report "$label" FAIL "cargo test exit $rc, $log"
    else
      report "$label" ok "$passed tests passed in ${took} s on $(uname -s) $(uname -m)"
    fi
  done
}

WANT_STATE=0
WANT_DIFF=0
for part in "${PARTS[@]}"; do
  case $part in
    targets) run_targets ;;
    audit) run_audit ;;
    state) WANT_STATE=1 ;;
    diff) WANT_DIFF=1 ;;
  esac
done
if [ "$WANT_STATE" = 1 ] || [ "$WANT_DIFF" = 1 ]; then
  run_tests "$WANT_STATE" "$WANT_DIFF"
fi
exit "$FAILED"
