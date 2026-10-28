#!/bin/sh
# Builds what the targets compile of tarae — src/lib.rs, the sandbox's readers' modules under `--cfg fuzzing`, the
# web reader's crate with its `check` — on the pinned
# stable, offline, without cargo-fuzz or libFuzzer: a copy of this folder with libfuzzer-sys and the [[bin]]s left out,
# in a temp folder (README.md "Checking without the nightly"). Run from anywhere: sh fuzz/parsers/check.sh
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
d=$(mktemp -d "${TMPDIR:-/tmp}/fuzz-check.XXXX")
trap 'rm -rf "$d"' EXIT
mkdir -p "$d/fuzz/parsers"
cp -r "$here/src" "$d/fuzz/parsers/"
ln -s "$root/src" "$d/src"
# (the lib alone: no libFuzzer, no targets; tarae-web where it is, so it finds the workspace it inherits from)
sed -e '/^libfuzzer-sys/d' -e '/^\[\[bin\]\]/,$d' -e "s|\"\.\./\.\./crates/|\"$root/crates/|" \
  "$here/Cargo.toml" >"$d/fuzz/parsers/Cargo.toml"
grep -q "\"$root/crates/tarae-web\"" "$d/fuzz/parsers/Cargo.toml" || { echo "tarae-web's path not set"; exit 1; }
# (tarae's lock: the readers' crates at tarae's versions)
cp "$root/Cargo.lock" "$d/fuzz/parsers/Cargo.lock"
cd "$d/fuzz/parsers"
RUSTFLAGS="--cfg fuzzing -D warnings" CARGO_TARGET_DIR="$d/t" CARGO_BUILD_BUILD_DIR="$d/b" \
  cargo +"$(sed -n 's/^channel = "\(.*\)"$/\1/p' "$root/rust-toolchain.toml")" build --offline --lib
echo "fuzz/parsers: the targets' library builds under --cfg fuzzing"
