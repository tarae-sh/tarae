//! tarae's WebAssembly runtime, for plugins.
//!
//! - **Scope**: WebAssembly 1.0 plus what rustc turns on by default for `wasm32-unknown-unknown` — bulk memory,
//!   multi-value, mutable globals, non-trapping float → int, reference types, sign extension. No SIMD,
//!   threads, exceptions, tail calls, 64-bit or multiple memories, and **no imports**: a plugin reads what
//!   tarae puts in its memory and answers there — no WASI, no host functions.
//! - **Shape**: `module.rs` decodes, `compile.rs` validates each function while compiling it to register ops
//!   (operands in fixed frame slots, branch targets and moves resolved). `jit/` turns those into machine code
//!   (AArch64, x86-64) where it can; `exec.rs` interprets them everywhere else — and is the reference the
//!   JIT is fuzzed against (below).
//! - **Safety**: `unsafe` only in `jit/mod.rs` (executable memory, and calling into it). Every malformed input
//!   is an `Error`, every fault a `Trap`.
//! - **Limits**: fuel is charged at every function entry and loop head for what the body costs (moves by
//!   their length, calls by the locals they zero), and per byte (or element) by the ops that copy, fill or
//!   grow — so every way to run long pays. Memory, tables, the value stack, and call depth have caps, and so
//!   does loading (types' arity, locals per module, tables, compiling steps).
//! - **Interrupt**: a flag only the host sets (`Interrupt`, from any thread), checked wherever fuel is charged —
//!   the call ends with `Trap::Interrupted`. It lives outside everything a module addresses (its memory,
//!   globals, tables, the value stack): no module can read or clear it.
//! - **Fuzzing**: two harnesses, each the JIT against the interpreter.
//!   - The repository's root `fuzz/` (a crate of its own, outside the workspace with its own lockfile, so
//!     wasmtime, wasmi and wasm-smith never touch tarae's build): wasm-smith's random modules, a seed each, run
//!     under the JIT and the interpreter, which must agree on every call exactly (results, traps, memory, the
//!     fuel left), and on wasmi, wasmtime deciding when wasmi disagrees; each module's byte mutations go through
//!     the decoder too. Its `spec` binary runs the official spec tests. In `fuzz/`: `cargo run --release --bin
//!     fuzz -- 300` (seconds; `found/` keeps a finding), `cargo run --release --bin spec -- testsuite/*.wast`.
//!   - `examples/qjs_fuzz.rs`, the code tool's hour: its engine (QuickJS, `cargo xtask quickjs`) on generated
//!     JavaScript, a seed a program, on a fresh instance under the JIT and one under the interpreter, comparing
//!     every reply whole, every trap, and after every step the fuel left, the memory's hash and length and
//!     whether a grow was refused. An
//!     architecture's JIT runs the code tool only after an hour of it came back clean (G36 in
//!     `docs/threat-model.md`, with the seeds and the engine's sha256 of the last one); a re-pin of the engine
//!     (`xtask/quickjs/pin`) or a change to `jit/` means the hour again, on that architecture, on a quiet machine:
//!
//!     ```text
//!     cargo build --release -p tarae-wasm --example qjs_fuzz
//!     target/release/examples/qjs_fuzz target/quickjs/quickjs.wasm --selftest   # sees a planted difference
//!     target/release/examples/qjs_fuzz target/quickjs/quickjs.wasm --out DIR --from 10000000 --secs 3600 --threads 8
//!     target/release/examples/qjs_fuzz target/quickjs/quickjs.wasm --replay SEED   # one case: program, both traces
//!     ```
//!
//!     Clean is `DIR/summary.txt` saying 0 findings, 0 not compared and 0 controls failed; a finding is
//!     `findings/<seed>.js` (the program) and `findings/<seed>.txt` (its limits, both traces, the first difference
//!     marked), and `--replay` with its seed runs it again.
//! - **Plugins** (`plugin`): tarae's plugin API on it — its types, loading and calling one plugin, and the
//!   thread the editor talks to by messages. The editor's side (commands, the plugins folder, permission cards,
//!   panels, `tarae plugin`) is tarae's `src/plugin/`; this crate knows nothing of the editor.

#![deny(unsafe_code)]
// allowed only in `jit`
// (std's `sort_by` family goes through tarae's `sorting` in the binary — clippy.toml; tests sort as they like)
#![cfg_attr(test, allow(clippy::disallowed_methods, reason = "tests: not in the release binary"))]

mod compile;
mod exec;
mod jit;
mod module;
pub mod plugin;
mod read;

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

pub use module::Module;

use module::{Const, ExportKind, Mode};

/// The null reference in a slot. A reference to function `f` is `f + 1` — so zeroed locals, tables and
/// globals start out null.
pub(crate) const NULL: u64 = 0;
const PAGE: usize = 65536;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ty {
    I32,
    I64,
    F32,
    F64,
    FuncRef,
    ExternRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FuncType {
    pub params: Box<[Ty]>,
    pub results: Box<[Ty]>,
}

/// A value going in or out of a call. Floats travel as their bits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Val {
    I32(i32),
    I64(i64),
    F32(u32),
    F64(u64),
    /// A function index, or null.
    FuncRef(Option<u32>),
    ExternRef(Option<u32>),
}

impl Val {
    fn ty(self) -> Ty {
        match self {
            Val::I32(_) => Ty::I32,
            Val::I64(_) => Ty::I64,
            Val::F32(_) => Ty::F32,
            Val::F64(_) => Ty::F64,
            Val::FuncRef(_) => Ty::FuncRef,
            Val::ExternRef(_) => Ty::ExternRef,
        }
    }

    fn to_slot(self) -> u64 {
        match self {
            Val::I32(v) => v as u32 as u64,
            Val::I64(v) => v as u64,
            Val::F32(v) => v as u64,
            Val::F64(v) => v,
            Val::FuncRef(r) | Val::ExternRef(r) => r.map_or(NULL, |i| i as u64 + 1),
        }
    }

    fn from_slot(ty: Ty, s: u64) -> Val {
        let r = (s != NULL).then(|| (s - 1) as u32);
        match ty {
            Ty::I32 => Val::I32(s as u32 as i32),
            Ty::I64 => Val::I64(s as i64),
            Ty::F32 => Val::F32(s as u32),
            Ty::F64 => Val::F64(s),
            Ty::FuncRef => Val::FuncRef(r),
            Ty::ExternRef => Val::ExternRef(r),
        }
    }
}

/// A module that can't be used: malformed, invalid, or using something unsupported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(String);

impl Error {
    fn new(s: impl Into<String>) -> Error {
        Error(s.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trap {
    Unreachable,
    MemoryOutOfBounds,
    TableOutOfBounds,
    /// `call_indirect` past the table's end.
    UndefinedElement,
    UninitializedElement,
    IndirectCallType,
    DivideByZero,
    IntegerOverflow,
    InvalidConversion,
    StackOverflow,
    OutOfFuel,
    /// The host called with the wrong arguments, or something that isn't a function.
    BadCall,
    /// The host set the instance's `Interrupt`.
    Interrupted,
}

impl fmt::Display for Trap {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            Trap::Unreachable => "unreachable",
            Trap::MemoryOutOfBounds => "out of bounds memory access",
            Trap::TableOutOfBounds => "out of bounds table access",
            Trap::UndefinedElement => "undefined element",
            Trap::UninitializedElement => "uninitialized element",
            Trap::IndirectCallType => "indirect call type mismatch",
            Trap::DivideByZero => "integer divide by zero",
            Trap::IntegerOverflow => "integer overflow",
            Trap::InvalidConversion => "invalid conversion to integer",
            Trap::StackOverflow => "call stack exhausted",
            Trap::OutOfFuel => "out of fuel",
            Trap::BadCall => "wrong arguments",
            Trap::Interrupted => "interrupted",
        })
    }
}

impl std::error::Error for Trap {}

/// Caps on what an instance may use.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub memory_bytes: usize,
    pub table_elems: u32,
    /// Value stack slots (8 bytes each), shared by every frame.
    pub stack_slots: usize,
    /// Calls deep. Compiled code nests on the calling thread's native stack, up to 96 bytes a call
    /// (AArch64; 32 on x86-64): the thread needs `call_depth` × 96 bytes to spare, 1.9 MB for the default.
    pub call_depth: usize,
    /// Run compiled machine code when there is some (else the interpreter).
    pub jit: bool,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            memory_bytes: 256 << 20,
            table_elems: 100_000,
            stack_slots: 1 << 19,
            call_depth: 20_000,
            jit: true,
        }
    }
}

/// A stop for a running instance that only the host sets — from any thread, at any moment (a clone is the same
/// flag). The code checks it wherever it charges fuel (every function entry and loop head, the bulk ops), so a
/// call it's set during ends at the next of those with `Trap::Interrupted`, and every call after it at once. It
/// stays set: an instance stopped is done (`Instance::set_interrupt` gives it a new one).
#[derive(Clone, Debug, Default)]
pub struct Interrupt(Arc<AtomicU32>);

impl Interrupt {
    pub fn new() -> Interrupt {
        Interrupt::default()
    }

    pub fn set(&self) {
        self.0.store(1, Ordering::Relaxed);
    }

    pub fn is_set(&self) -> bool {
        self.0.load(Ordering::Relaxed) != 0
    }

    /// The word the code reads (the JIT's context holds its address).
    fn word(&self) -> &AtomicU32 {
        &self.0
    }
}

#[derive(Clone, Copy)]
struct Frame {
    ret: u32,
    base: u32,
}

/// A running module: its memory, tables, and globals. The module itself is shared: it's never written once
/// compiled (its machine code neither), so any number of instances on any threads may run one `Arc<Module>`,
/// each with nothing of the others — what the code tool does, a fresh instance a program.
pub struct Instance {
    module: Arc<Module>,
    limits: Limits,
    memory: Vec<u8>,
    memory_max: usize,
    /// A `memory.grow` past `memory_max` was refused (it said -1) — the host's cap, or the module's own maximum.
    grow_refused: bool,
    globals: Vec<u64>,
    tables: Vec<Vec<u64>>,
    table_max: Vec<u32>,
    /// Element segments as evaluated references — emptied when dropped.
    elems: Vec<Vec<u64>>,
    data_live: Vec<bool>,
    stack: Vec<u64>,
    frames: Vec<Frame>,
    fuel: u64,
    interrupt: Interrupt,
}

fn eval(c: Const, globals: &[u64]) -> u64 {
    match c {
        Const::Bits(b) => b,
        Const::Global(g) => globals[g as usize],
    }
}

impl Instance {
    /// Sets up memory, tables, and globals, applies the active segments, and runs the start function
    /// (with `fuel`). `module` is a `Module` (this instance's alone) or an `Arc<Module>` shared with others.
    pub fn new(module: impl Into<Arc<Module>>, limits: Limits, fuel: u64) -> Result<Instance, Trap> {
        let module = module.into();
        let mut globals = Vec::with_capacity(module.globals.len());
        for g in &module.globals {
            globals.push(eval(g.init, &globals));
        }
        let pages_cap = limits.memory_bytes / PAGE;
        let (memory, memory_max) = match module.memory {
            None => (Vec::new(), 0),
            Some(l) => {
                if l.min as usize > pages_cap {
                    return Err(Trap::MemoryOutOfBounds);
                }
                let max = l.max.map_or(pages_cap, |m| (m as usize).min(pages_cap));
                (vec![0; l.min as usize * PAGE], max * PAGE)
            }
        };
        let mut tables = Vec::new();
        let mut table_max = Vec::new();
        for t in &module.tables {
            if t.limits.min > limits.table_elems {
                return Err(Trap::TableOutOfBounds);
            }
            tables.push(vec![NULL; t.limits.min as usize]);
            table_max.push(t.limits.max.unwrap_or(u32::MAX).min(limits.table_elems));
        }
        let elems =
            module.elems.iter().map(|e| e.items.iter().map(|&c| eval(c, &globals)).collect()).collect();
        let data_live = vec![true; module.datas.len()];
        let mut inst = Instance {
            stack: vec![0; limits.stack_slots],
            module,
            limits,
            memory,
            memory_max,
            grow_refused: false,
            globals,
            tables,
            table_max,
            elems,
            data_live,
            frames: Vec::new(),
            fuel,
            interrupt: Interrupt::new(),
        };
        for i in 0..inst.module.elems.len() {
            let e = &inst.module.elems[i];
            if let Mode::Active { index, offset } = e.mode {
                let at = eval(offset, &inst.globals) as u32 as usize;
                let items = std::mem::take(&mut inst.elems[i]);
                let table = &mut inst.tables[index as usize];
                let dst = table.get_mut(at..at + items.len()).ok_or(Trap::TableOutOfBounds)?;
                dst.copy_from_slice(&items);
            } else if let Mode::Declarative = e.mode {
                inst.elems[i].clear();
            }
        }
        for i in 0..inst.module.datas.len() {
            let d = &inst.module.datas[i];
            if let Mode::Active { offset, .. } = d.mode {
                let at = eval(offset, &inst.globals) as u32 as usize;
                let dst = inst.memory.get_mut(at..at + d.bytes.len()).ok_or(Trap::MemoryOutOfBounds)?;
                dst.copy_from_slice(&d.bytes);
                inst.data_live[i] = false;
            }
        }
        if let Some(f) = inst.module.start {
            inst.call(f, &[])?;
        }
        Ok(inst)
    }

    pub fn module(&self) -> &Module {
        &self.module
    }

    #[cfg_attr(not(test), allow(dead_code))] // for tarae's code tool, fuzz/, the examples (qjs_fuzz, qjs_spike), the tests
    pub fn fuel(&self) -> u64 {
        self.fuel
    }

    pub fn set_fuel(&mut self, fuel: u64) {
        self.fuel = fuel;
    }

    /// Its interrupt: a handle the host keeps, to stop a call from another thread.
    pub fn interrupt(&self) -> Interrupt {
        self.interrupt.clone()
    }

    /// Checks `interrupt` from now on — one the host made before the instance (a watchdog armed first).
    pub fn set_interrupt(&mut self, interrupt: Interrupt) {
        self.interrupt = interrupt;
    }

    pub fn memory(&self) -> &[u8] {
        &self.memory
    }

    /// Whether a `memory.grow` was ever refused for going past the instance's maximum (`Limits::memory_bytes`, or
    /// the module's own when that's less): the module saw -1, so whatever it did next came of running out.
    pub fn grow_refused(&self) -> bool {
        self.grow_refused
    }

    pub fn memory_mut(&mut self) -> &mut [u8] {
        &mut self.memory
    }

    /// An exported global's value.
    #[allow(dead_code)] // for fuzz/ (its spec tests read globals, `fuzz/src/bin/spec.rs`) and the tests
    pub fn global(&self, name: &str) -> Option<Val> {
        let g = self.module.export(name, ExportKind::Global)? as usize;
        Some(Val::from_slot(self.module.globals[g].ty, self.globals[g]))
    }

    /// Calls function `f` (an index — `Module::func` finds exported ones by name).
    pub fn call(&mut self, f: u32, args: &[Val]) -> Result<Vec<Val>, Trap> {
        let Some(&ty) = self.module.func_ty.get(f as usize) else { return Err(Trap::BadCall) };
        let ty = &self.module.types[ty as usize];
        if ty.params.len() != args.len() || ty.params.iter().zip(args).any(|(&t, a)| a.ty() != t) {
            return Err(Trap::BadCall);
        }
        // A function reference must name a function of this module: code trusts the ones in tables
        let funcs = self.module.func_ty.len();
        if args.iter().any(|a| matches!(a, Val::FuncRef(Some(f)) if *f as usize >= funcs)) {
            return Err(Trap::BadCall);
        }
        if args.len() > self.stack.len() {
            return Err(Trap::StackOverflow);
        }
        for (slot, a) in self.stack.iter_mut().zip(args) {
            *slot = a.to_slot();
        }
        let results = ty.results.clone();
        if self.limits.jit && self.module.jit.is_some() {
            jit::run(self, f)?;
        } else {
            self.run(f)?;
        }
        Ok(results.iter().zip(&self.stack).map(|(&t, &s)| Val::from_slot(t, s)).collect())
    }
}

#[cfg(test)]
mod tests;
