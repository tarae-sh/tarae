# Inside the wasi-sdk container (build.sh build): the original QuickJS's core, the glue and WASI's stubs, compiled once
# and linked twice — with the name section (for reading a trap) and stripped (the size that would ship) — and the glue
# compiled again with GLUE_PROBE into out/quickjs-probe.wasm, the tests' alone, never shipped.
set -eu
cd /w
S=/opt/wasi-sdk
CC="$S/bin/clang --target=wasm32-wasip1 --sysroot=$S/share/wasi-sysroot"
mkdir -p out obj probe shim
"$S/bin/clang" --version | head -1 >out/clang.txt
# dtoa.c includes <setjmp.h> and uses nothing of it; wasi-sdk 25's refuses to be included without wasm exceptions
: >shim/setjmp.h
# the original's four besides quickjs.c (which glue.c includes); a missing one fails the build
srcs="src/libregexp.c src/libunicode.c src/cutils.c src/dtoa.c"
for f in $srcs; do
    [ -f "$f" ] || { echo "$f: not in the archive" >&2; exit 1; }
done
# the version quickjs.c prints is the archive's own, as its Makefile has it
version=$(cat src/VERSION)
echo "QuickJS $version: compiling glue.c (quickjs.c included) wasi.c $srcs" | tee out/sources.txt
# -fwrapv from the original's Makefile ("ensure that signed overflows behave as expected")
cc() {
    $CC -O2 -fwrapv -mbulk-memory -mnontrapping-fptoint -D_GNU_SOURCE "-DCONFIG_VERSION=\"$version\"" \
        -Ishim -Isrc "$@"
}
for f in glue.c wasi.c $srcs; do
    cc -c "$f" -o "obj/$(basename "$f" .c).o"
done
cc -DGLUE_PROBE -c glue.c -o probe/glue.o
link() {
    $CC -mexec-model=reactor "$@" -lm \
        -Wl,--stack-first -Wl,-z,stack-size=1048576 -Wl,--max-memory=67108864 -Wl,--fatal-warnings
}
link obj/*.o -Wl,--strip-debug -o out/quickjs-names.wasm
link obj/*.o -Wl,--strip-all -o out/quickjs.wasm
link probe/glue.o obj/wasi.o obj/libregexp.o obj/libunicode.o obj/cutils.o obj/dtoa.o -Wl,--strip-all \
    -o out/quickjs-probe.wasm
