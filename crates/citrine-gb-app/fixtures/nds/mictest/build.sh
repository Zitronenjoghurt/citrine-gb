#!/bin/sh
set -eu

blocksds=skylyrac/blocksds@sha256:c5f19b95d9c2f312e6a68ea244d2ed5120203b4b82af9785937a349096f8e5e4
here=$(cd "$(dirname "$0")" && pwd)

docker run --rm -v "$here:/src:ro" -v "$here/..:/out" "$blocksds" sh -c '
cp -r /src /w && cd /w
make BLOCKSDS=/opt/wonderful/thirdparty/blocksds/core >/tmp/log 2>&1 || { tail -40 /tmp/log; exit 1; }
cp mictest.nds /out/mictest.nds
chown '"$(id -u):$(id -g)"' /out/mictest.nds
'
sha256sum "$here/../mictest.nds"
