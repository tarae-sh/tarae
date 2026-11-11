#!/bin/sh
# QuickJS-ng to a WebAssembly module with no imports — the code tool's engine (.sb/codemode-compute-plan.md §3, S0).
#
#   xtask/quickjs/build.sh fetch <dir>   the source tarball into <dir>, its sha256 said (network:
#                                        codeload.github.com, where github.com's archive link redirects — by
#                                        curl, through tarae's proxy)
#   xtask/quickjs/build.sh build <dir>   the compile: the wasi-sdk image (pulled by colima's Docker if it isn't
#                                        there, its digest said), run with --network none and no mounts — the
#                                        sources go in on stdin, the modules come out on stdout into <dir>/out
set -eu
TAG=v0.10.1
IMAGE=ghcr.io/webassembly/wasi-sdk:wasi-sdk-25
recipe=$(cd "$(dirname "$0")" && pwd)
what=${1:?usage: build.sh fetch|build <dir>}
dir=${2:?usage: build.sh fetch|build <dir>}
tarball="$dir/quickjs-ng-$TAG.tar.gz"

case $what in
fetch)
    mkdir -p "$dir"
    curl -fsSL --proto '=https' -o "$tarball" "https://codeload.github.com/quickjs-ng/quickjs/tar.gz/refs/tags/$TAG"
    shasum -a 256 "$tarball"
    ;;
build)
    shasum -a 256 "$tarball"
    docker image inspect "$IMAGE" >/dev/null 2>&1 || docker pull "$IMAGE"
    docker image inspect --format '{{index .RepoDigests 0}}' "$IMAGE"
    rm -rf "$dir/in" "$dir/out"
    mkdir -p "$dir/in/src" "$dir/out"
    tar -xzf "$tarball" -C "$dir/in/src" --strip-components=1
    cp "$recipe/glue.c" "$recipe/wasi.c" "$recipe/inside.sh" "$dir/in/"
    COPYFILE_DISABLE=1 tar --no-xattrs -C "$dir/in" -cf - . |
        docker run --rm -i --network none "$IMAGE" \
            sh -c 'mkdir /w && cd /w && tar -xf - && sh inside.sh >&2 && tar -C /w/out -cf - .' |
        tar -C "$dir/out" -xf -
    ls -l "$dir/out"
    ;;
*)
    echo "usage: build.sh fetch|build <dir>" >&2
    exit 2
    ;;
esac
