#!/bin/sh
set -eu
if [ $# -ne 2 ]; then
  echo "usage: $0 <unstripped libcitrine_gb_app.so> <crash report>" >&2
  exit 2
fi
so=$1
report=$2
base=$(grep -oE 'libcitrine_gb_app\.so loaded at 0x[0-9a-f]+' "$report" | head -1 | sed 's/.* //')
if [ -z "$base" ]; then
  echo "no load address in $report" >&2
  exit 1
fi
grep -oE '^ *[0-9]+: +0x[0-9a-f]+' "$report" | while read -r n ip; do
  if [ $((ip)) -le $((base)) ]; then
    echo "$n $ip outside libcitrine_gb_app.so"
    continue
  fi
  offset=$(printf '0x%x' $((ip - base - 1)))
  echo "$n $offset $("${ADDR2LINE:-llvm-addr2line}" -C -f -p -e "$so" "$offset")"
done
