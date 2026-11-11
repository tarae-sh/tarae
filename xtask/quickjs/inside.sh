# Inside the wasi-sdk container (build.sh build): QuickJS-ng's core, the glue and WASI's stubs, compiled once and
# linked twice — with the name section (for reading a trap) and stripped (the size that would ship).
set -eu
cd /w
S=/opt/wasi-sdk
CC="$S/bin/clang --target=wasm32-wasip1 --sysroot=$S/share/wasi-sysroot"
mkdir -p out obj
"$S/bin/clang" --version | head -1 >out/clang.txt
srcs=""
for f in libregexp.c libunicode.c cutils.c dtoa.c xsum.c; do
    if [ -f "src/$f" ]; then srcs="$srcs src/$f"; fi
done
echo "compiling glue.c (quickjs.c included) wasi.c$srcs" | tee out/sources.txt
for f in glue.c wasi.c $srcs; do
    $CC -O2 -mbulk-memory -mnontrapping-fptoint -D_GNU_SOURCE -Isrc \
        -c "$f" -o "obj/$(basename "$f" .c).o"
done
link() {
    $CC -mexec-model=reactor obj/*.o -lm \
        -Wl,--stack-first -Wl,-z,stack-size=1048576 -Wl,--max-memory=67108864 -Wl,--fatal-warnings "$@"
}
link -Wl,--strip-debug -o out/quickjs-names.wasm
link -Wl,--strip-all -o out/quickjs.wasm
