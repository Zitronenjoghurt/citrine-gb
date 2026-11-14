#!/bin/sh
set -eu

blocksds=skylyrac/blocksds@sha256:779f6193d1030c62e60d1f0f39c3e32cf4045c0bd224a45c0ceadf9d9ab01a51
devkitarm=devkitpro/devkitarm@sha256:116afba8df8453961de2936ffab20dd441edf4d682856c1ec8b0e53d7ed0bbf5
devkitarm_libnds1=devkitpro/devkitarm@sha256:a998edf6b06416b5c053edbcd879abfa22b1b88e9cd3f267f5c4ee9fec71a93a
manifest=$(dirname "$0")/manifest.toml
cache=${LEDGER_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/citrine-gb/ledger}

if ! docker info >/dev/null 2>&1; then
    echo "no docker, the DS ledger ROMs built from source are skipped"
    exit 0
fi

arm7wrestler='
cd /w
sed -i "1i .arch armv5te" arm7/source/armwrestler-ds.s
make >/tmp/log 2>&1 || { tail -15 /tmp/log; exit 1; }
cp ./*.nds /out/arm7wrestler.nds
'

nds_interp='
apt-get update -qq >/dev/null && apt-get install -y -qq p7zip-full >/dev/null
cd /w/nds-interp/data
7z x -y -odata data.7z >/dev/null
sed -i "s/constexpr bool generateData = true;/constexpr bool generateData = false;/" src/source/main.cpp
cd src
for corner in TL TR BL BR; do
    rm -rf nitrofiles build ./*.nds
    mkdir nitrofiles
    cp ../data/$corner.bin nitrofiles/data.bin
    make >/tmp/log 2>&1 || { tail -15 /tmp/log; exit 1; }
    cp src.nds /out/nds-interp-$(echo $corner | tr A-Z a-z).nds
done
'

blocksds_examples='
export PATH=/opt/wonderful/toolchain/gcc-arm-none-eabi/bin:/opt/wonderful/thirdparty/blocksds/core/tools/ndstool:$PATH
for e in console/both_screens graphics_2d/video_mode_vram input/touch_input graphics_3d/basic_cube; do
    cd /w/examples/$e
    make -j4 >/tmp/log 2>&1 || { tail -15 /tmp/log; exit 1; }
    mkdir -p /out/examples/$e
    cp ./*.nds /out/examples/$e/
done
'

python3 -c '
import hashlib, os, sys, tomllib, zipfile
with open(sys.argv[1], "rb") as f:
    manifest = tomllib.load(f)

def current(name, built):
    if not os.path.isfile(built):
        return False
    with zipfile.ZipFile(built) as z:
        names = set(z.namelist())
        for suite in manifest["suites"].values():
            if suite["source"] != name:
                continue
            for rom in suite["roms"]:
                path = suite["root"] + "/" + rom["path"]
                if path not in names or hashlib.sha256(z.read(path)).hexdigest() != rom["sha256"]:
                    return False
    return True

for name, source in manifest["sources"].items():
    built = os.path.join(sys.argv[2], "built-" + source["sha256"] + ".zip")
    if source.get("built") and not current(name, built):
        print(name, source["sha256"], source["url"])
' "$manifest" "$cache" | while read -r name sha url; do
    built=$cache/built-$sha.zip
    rm -f "$built"
    mkdir -p "$cache"
    archive=$cache/$sha.zip
    if [ ! -f "$archive" ]; then
        echo "fetching $name from $url"
        curl -fsSL --retry 3 -o "$archive.part" "$url"
        if [ "$(sha256sum "$archive.part" | cut -d' ' -f1)" != "$sha" ]; then
            rm -f "$archive.part"
            echo "$name does not match the SHA-256 the manifest pins"
            exit 1
        fi
        mv "$archive.part" "$archive"
    fi
    case $name in
        arm7wrestler) image=$devkitarm_libnds1 script=$arm7wrestler ;;
        nds-interp) image=$devkitarm script=$nds_interp ;;
        blocksds) image=$blocksds script=$blocksds_examples ;;
        *) echo "no build for $name"; exit 1 ;;
    esac
    echo "building $name"
    work=$(mktemp -d)
    unzip -q "$archive" -d "$work/src"
    root=$(ls "$work/src")
    mkdir -p "$work/out/$root"
    status=0
    docker run --rm -v "$work/src/$root:/src:ro" "$image" \
        sh -c "mkdir -p /out && cp -r /src /w && ($script) >&2 && tar -C /out -cf - ." \
        >"$work/out.tar" || status=$?
    if [ "$status" -eq 0 ]; then
        tar -xf "$work/out.tar" -C "$work/out/$root"
        (cd "$work/out" && python3 -m zipfile -c "$built.part" "$root" && mv "$built.part" "$built")
    fi
    rm -rf "$work"
    [ "$status" -eq 0 ] || { echo "building $name failed"; exit 1; }
done
