#!/bin/sh
set -eu

blocksds=skylyrac/blocksds@sha256:779f6193d1030c62e60d1f0f39c3e32cf4045c0bd224a45c0ceadf9d9ab01a51
here=$(cd "$(dirname "$0")" && pwd)

docker run --rm -v "$here:/src:ro" -v "$here/..:/out" "$blocksds" sh -c '
cp -r /src /w && cd /w
make >/tmp/log 2>&1 || { tail -25 /tmp/log; exit 1; }
cp nickname.nds /out/nickname.nds
chown '"$(id -u):$(id -g)"' /out/nickname.nds
'
sha256sum "$here/../nickname.nds"
