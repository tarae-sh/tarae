# demo

The plugin tarae's tests load ([`../demo.wasm`](../demo.wasm)): a command for every action, and some that misbehave
on purpose. tarae's tests patch bytes of the build (`abi::demo_asking`, `abi::demo_reading`) to ask for other
permissions.

It's a test fixture, not an example: it runs `wit_bindgen::generate!` itself instead of using the SDK's bindings
(`tarae-plugin`'s `Guest` and `export!`, as every other plugin here does), so that the committed build stays the
bytes those patches expect. Start a plugin with `tarae plugin new` instead.

After changing it (the remap keeps your home folder's path out of the committed file):

```sh
RUSTFLAGS="--remap-path-prefix=$HOME=~" cargo build --release
cp target/wasm32-unknown-unknown/release/demo.wasm ..
```
