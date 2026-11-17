#!/bin/sh
# The original QuickJS to a WebAssembly module with no imports — the code tool's engine (.sb/codemode-compute-plan.md
# §3, S0).
#
#   xtask/quickjs/build.sh fetch <dir>   the source archive into <dir>, its sha256 checked (network: bellard.org,
#                                        one host, no mirror — by curl, through tarae's proxy)
#   xtask/quickjs/build.sh build <dir>   the compile: the wasi-sdk image (pulled by colima's Docker if it isn't
#                                        there, its digest said), run with --network none and no mounts — the
#                                        sources go in on stdin, the modules come out on stdout into <dir>/out,
#                                        and the stripped one is copied to <dir>/quickjs.wasm
#
# What it builds from — the release, the archive's sha256, the image's digest — is `pin`, beside this script, alone.
# Any copy of the archive with that sha256 serves as the fetched one: put at <dir>/quickjs-TAG.tar.xz, build checks it.
# `cargo xtask quickjs` runs it on target/quickjs, where tarae's dev build and tests look.
set -eu
recipe=$(cd "$(dirname "$0")" && pwd)
. "$recipe/pin"

# the archive is the pinned one, or nothing goes on
check() {
    got=$( (shasum -a 256 "$1" 2>/dev/null || sha256sum "$1") | cut -d' ' -f1)
    if [ "$got" != "$SOURCE" ]; then
        echo "$1: sha256 $got, not the pinned $SOURCE (xtask/quickjs/pin)" >&2
        exit 1
    fi
    echo "QuickJS $TAG, sha256 $got (as pinned)"
}
what=${1:?usage: build.sh fetch|build <dir>}
dir=${2:?usage: build.sh fetch|build <dir>}
tarball="$dir/quickjs-$TAG.tar.xz"

case $what in
fetch)
    mkdir -p "$dir"
    curl -fsSL --proto '=https' -o "$tarball.part" "https://bellard.org/quickjs/quickjs-$TAG.tar.xz"
    check "$tarball.part"
    mv "$tarball.part" "$tarball"
    ;;
build)
    check "$tarball"
    docker image inspect "$IMAGE" >/dev/null 2>&1 || docker pull "$IMAGE"
    echo "image $IMAGE"
    rm -rf "$dir/in" "$dir/out" "$dir/quickjs.wasm"
    mkdir -p "$dir/in/src" "$dir/out"
    # (xz: macOS's bsdtar reads it itself, GNU tar through xz; the sha pins the bytes, so the listing needs no check
    # of its own — read once: plain files under one folder, no links)
    tar -xf "$tarball" -C "$dir/in/src" --strip-components=1 --no-same-owner
    cp "$recipe/glue.c" "$recipe/wasi.c" "$recipe/inside.sh" "$dir/in/"
    COPYFILE_DISABLE=1 tar --no-xattrs -C "$dir/in" -cf - . |
        docker run --rm -i --network none "$IMAGE" \
            sh -c 'mkdir /w && cd /w && tar -xf - && sh inside.sh >&2 && tar -C /w/out -cf - .' |
        tar -C "$dir/out" -xf -
    cp "$dir/out/quickjs.wasm" "$dir/quickjs.wasm"
    ls -l "$dir/out" "$dir/quickjs.wasm"
    ;;
*)
    echo "usage: build.sh fetch|build <dir>" >&2
    exit 2
    ;;
esac
