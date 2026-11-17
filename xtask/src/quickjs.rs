//! `cargo xtask quickjs [fetch|build]`: the code tool's engine (the original QuickJS as a WebAssembly module with no
//! imports, `.sb/codemode-compute-plan.md` §3) built into `target/quickjs/quickjs.wasm`, where tarae's dev build and
//! its tests look (`codetool::ENGINE_DEV`). The recipe is `xtask/quickjs/build.sh`, what it builds from
//! `xtask/quickjs/pin` alone; this runs it there, `build` in its turn on the machine's Docker (`dockerlock`).
//!
//! - `fetch`: the pinned archive into `target/quickjs/` (the network: bellard.org, one host, no mirror), its sha256
//!   checked; any copy with that sha256 serves as well, put there by hand.
//! - `build` (the default): the compile in wasi-sdk's image by digest, `--network none`, no mounts.

use std::path::Path;
use std::process::Command;

use crate::Result;

const USAGE: &str = "usage: cargo xtask quickjs [fetch|build]";

pub fn run(args: &[String]) -> Result<()> {
    let what = match args {
        [] => "build",
        [w] if w == "fetch" || w == "build" => w.as_str(),
        _ => return Err(USAGE.into()),
    };
    let root = crate::root();
    let dir = root.join("target/quickjs");
    let script = root.join("xtask/quickjs/build.sh");
    let _turn = match what {
        "build" => {
            crate::linux::reachable(&mut Command::new("docker"))
                .map_err(|why| crate::linux::skipped(&why))?;
            crate::dockerlock::take(&crate::dockerlock::endpoint(&Command::new("docker")), &root)
        }
        _ => None,
    };
    sh(&script, what, &dir)?;
    if what == "build" {
        eprintln!("the engine: {}", dir.join("quickjs.wasm").display());
    }
    Ok(())
}

fn sh(script: &Path, what: &str, dir: &Path) -> Result<()> {
    let status = Command::new("sh")
        .arg(script)
        .arg(what)
        .arg(dir)
        .status()
        .map_err(|e| format!("sh {}: {e}", script.display()))?;
    match status.success() {
        true => Ok(()),
        false => Err(format!("{} {what}: {status}", script.display())),
    }
}
