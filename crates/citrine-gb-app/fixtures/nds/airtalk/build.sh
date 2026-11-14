#!/bin/sh
set -eu

blocksds=skylyrac/blocksds@sha256:94daaba8a26fd1a4e0875a419fb317f20e6f0587d9e132dd1e91e7b46bfcd664
here=$(cd "$(dirname "$0")" && pwd)

docker run --rm -v "$here:/src:ro" -v "$here/..:/out" "$blocksds" sh -c '
cp -r /src /w && cd /w
make BLOCKSDS=/opt/wonderful/thirdparty/blocksds/core >/tmp/log 2>&1 || { tail -40 /tmp/log; exit 1; }
cp airtalk.nds /out/airtalk.nds
chown '"$(id -u):$(id -g)"' /out/airtalk.nds
'
sha256sum "$here/../airtalk.nds"
