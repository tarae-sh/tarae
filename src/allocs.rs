//! The test binary's allocator (tests only): the system's, watched and counted. Watched — a run's child sets `NO_ALLOC`
//! after the keeper's fork (`sandbox::keeper`, Linux), and any call ends that process; counted — each thread's
//! allocations so far (`count`), so a budget can hold what a pass allocates (#214: an event that changes nothing
//! allocates the same with 17 sessions open and with 34).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

/// Set first thing in a run's child: from then on any allocation or free ends that process with `ALLOCATED`, before
/// it touches malloc's locks.
pub static NO_ALLOC: AtomicBool = AtomicBool::new(false);
/// The exit status of a run's child that allocated before its exec.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const ALLOCATED: i32 = 121;

thread_local! {
    /// Allocations (and reallocations) made on this thread so far — no destructor, so readable to a thread's end.
    static MADE: Cell<usize> = const { Cell::new(0) };
    /// Bytes allocated on this thread less those freed on it (a block freed on another thread is that one's).
    static LIVE: Cell<isize> = const { Cell::new(0) };
}

/// Bytes this thread has allocated and not freed so far, as it sees it (what it freed of another's taken off).
#[cfg_attr(tarae_optimized, allow(dead_code))]
pub fn live() -> isize {
    LIVE.try_with(Cell::get).unwrap_or(0)
}

/// Bytes allocated and not freed by every thread of the process (#214's fourth piece: what the scene keeps as it
/// lives, whichever thread made it).
static TOTAL: AtomicIsize = AtomicIsize::new(0);

/// Bytes the whole process has allocated and not freed so far.
#[cfg_attr(tarae_optimized, allow(dead_code))]
pub fn total() -> isize {
    TOTAL.load(Ordering::Relaxed)
}

fn grew(by: isize) {
    TOTAL.fetch_add(by, Ordering::Relaxed);
    let _ = LIVE.try_with(|l| l.set(l.get() + by));
}

/// Where what's held was allocated (#214's fourth piece, by hand): while `track` is on, each allocation of any thread
/// is kept with its stack until it's freed; `held` then says what of those is still held, by the tarae functions that
/// made it. Slow (a stack a block) — a stretch of a scene, never a budget.
///
/// The stack is walked inside the allocator, so it takes no lock another frame of the same thread may hold as it
/// allocates (review round 1: std's `Backtrace` takes std's backtrace lock, which a panic's hook holds as it writes —
/// into the test's captured output, allocating — so a panic while tracking waited on itself). execinfo's
/// `backtrace(3)` into a fixed array instead: macOS's walks the frame pointers (`thread_stack_pcs`: no lock, no
/// allocation); glibc's unwinds through libgcc (`_Unwind_Backtrace`, its loader lock taken recursively), which it loads
/// on its first call — made by `track` before anything is kept, never inside a malloc. Named after the run, by
/// `dladdr` (`held`): names are enough to count by site. musl has no execinfo: there nothing is kept.
#[cfg_attr(tarae_optimized, allow(dead_code))]
pub mod sites {
    use std::cell::Cell;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Frames kept of a stack, from the allocator's own.
    const DEPTH: usize = 48;

    /// A stack as walked: its return addresses, the first `n`.
    #[derive(Clone, Copy)]
    pub(super) struct Stack {
        at: [usize; DEPTH],
        n: usize,
    }

    /// New blocks kept with their stacks.
    static ON: AtomicBool = AtomicBool::new(false);
    /// Some kept: a free looks them up.
    static ANY: AtomicBool = AtomicBool::new(false);
    static KEPT: Mutex<Option<HashMap<usize, (usize, Stack)>>> = Mutex::new(None);

    thread_local! {
        /// Inside the keeping: its own allocations aren't kept.
        static IN: Cell<bool> = const { Cell::new(false) };
    }

    /// Keeping new blocks' stacks, from now (true) or no more (false; those kept stay until freed).
    pub fn track(on: bool) {
        if on {
            // (glibc's first walk loads libgcc — a dlopen that allocates: here, outside any malloc)
            walk();
            ANY.store(true, Ordering::Relaxed);
        }
        ON.store(on, Ordering::Relaxed);
    }

    /// This thread's stack, here — no lock, nothing allocated (empty where there's no execinfo: musl).
    pub(super) fn walk() -> Stack {
        let mut s = Stack { at: [0; DEPTH], n: 0 };
        #[cfg(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))]
        {
            let mut buf = [std::ptr::null_mut::<libc::c_void>(); DEPTH];
            // SAFETY: a buffer of ours, `DEPTH` entries, its size given — filled up to what it returns.
            let n = unsafe { libc::backtrace(buf.as_mut_ptr(), DEPTH as libc::c_int) };
            s.n = usize::try_from(n).unwrap_or(0).min(DEPTH);
            for (a, p) in s.at.iter_mut().zip(&buf[..s.n]) {
                *a = *p as usize;
            }
        }
        s
    }

    fn inside<R>(f: impl FnOnce() -> R) -> Option<R> {
        let entered = IN.try_with(|i| !i.replace(true)).unwrap_or(false);
        if !entered {
            return None;
        }
        let r = f();
        let _ = IN.try_with(|i| i.set(false));
        Some(r)
    }

    pub(super) fn made(ptr: *mut u8, size: usize) {
        if ptr.is_null() || !ON.load(Ordering::Relaxed) {
            return;
        }
        inside(|| {
            let s = walk();
            if s.n > 0 {
                let mut k = KEPT.lock().unwrap_or_else(|e| e.into_inner());
                k.get_or_insert_with(HashMap::new).insert(ptr as usize, (size, s));
            }
        });
    }

    pub(super) fn freed(ptr: *mut u8) {
        if !ANY.load(Ordering::Relaxed) {
            return;
        }
        inside(|| {
            if let Some(k) = KEPT.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                k.remove(&(ptr as usize));
            }
        });
    }

    /// What's still held of the blocks kept, by the first `depth` tarae frames that made it (but this allocator's),
    /// most bytes first: (bytes, blocks, the frames).
    pub fn held(depth: usize) -> Vec<(usize, usize, String)> {
        track(false);
        let kept = KEPT.lock().unwrap_or_else(|e| e.into_inner()).take().unwrap_or_default();
        ANY.store(false, Ordering::Relaxed);
        let mut known: HashMap<usize, Option<String>> = HashMap::new();
        let mut by: HashMap<String, (usize, usize)> = HashMap::new();
        for (size, s) in kept.values() {
            let frames: Vec<String> = s.at[..s.n]
                .iter()
                .filter_map(|&a| known.entry(a).or_insert_with(|| tarae_frame(a)).clone())
                .take(depth)
                .collect();
            let e = by.entry(frames.join(" < ")).or_default();
            e.0 += size;
            e.1 += 1;
        }
        let mut all: Vec<_> = by.into_iter().map(|(k, (b, n))| (b, n, k)).collect();
        all.sort_by_key(|h| std::cmp::Reverse(h.0));
        all
    }

    /// A stack's frames by name (`names`, tests).
    pub(super) fn names(s: &Stack) -> Vec<Option<String>> {
        s.at[..s.n].iter().map(|&a| named(a)).collect()
    }

    /// The frame at return address `a` as a site: its function's last two parts (`checkpoint::save`) when it's
    /// tarae's and not this module's (the allocator's, its tests'); one with no name `binary+0x…` (a glibc test binary exports few: `addr2line
    /// -e` names it by hand); None: another's (std's, a crate's) or this allocator's.
    fn tarae_frame(a: usize) -> Option<String> {
        let name = named(a)?;
        if name.contains('+') {
            return Some(name);
        }
        let parts: Vec<&str> = name.split("::").collect();
        let tarae = parts.first().is_some_and(|p| p.contains("tarae"));
        let ours = name.contains("allocs::") || name.contains("6allocs");
        (tarae && !ours).then(|| parts[parts.len().saturating_sub(2)..].join("::"))
    }

    /// What `dladdr` says of address `a` (the instruction before it: a return address is the call's next): its
    /// symbol, demangled where it's Rust's legacy form (the hash left off); no symbol, its object's name and the
    /// offset in it. None: in no object.
    fn named(a: usize) -> Option<String> {
        // SAFETY: an out-struct dladdr fills, zeroed (null pointers) first; any address may be asked.
        let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
        // SAFETY: as above.
        if unsafe { libc::dladdr(a.saturating_sub(1) as *const libc::c_void, &mut info) } == 0 {
            return None;
        }
        let text = |p: *const libc::c_char| {
            // SAFETY: dladdr's strings, NUL-terminated, the loaded object's own (it stays loaded).
            (!p.is_null()).then(|| unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned())
        };
        match text(info.dli_sname) {
            Some(s) => Some(demangled(&s)),
            None => {
                let object = text(info.dli_fname).unwrap_or_default();
                let object = object.rsplit('/').next().unwrap_or_default().to_string();
                Some(format!("{object}+{:#x}", a.saturating_sub(info.dli_fbase as usize)))
            }
        }
    }

    /// A Rust symbol in the v0 form (the toolchain's) as its path when it's a plain one — crate, modules, the
    /// function, a closure `{closure}` (`_RNvNtCs…_5tarae10checkpoint4save` → `tarae::checkpoint::save`); anything
    /// else (an impl's, generics) as it is — a name all the same, to count by.
    fn demangled(s: &str) -> String {
        let b = s.as_bytes();
        let mut i = 2;
        let path = s.starts_with("_R").then(|| v0_path(b, &mut i)).flatten();
        path.filter(|_| i == b.len()).map_or_else(|| s.to_string(), |p| p.join("::"))
    }

    /// A v0 path of crate roots and namespaces from `b[*i..]` — None at any other kind.
    fn v0_path(b: &[u8], i: &mut usize) -> Option<Vec<String>> {
        let tag = *b.get(*i)?;
        *i += 1;
        match tag {
            b'C' => {
                v0_disambiguator(b, i);
                Some(vec![v0_ident(b, i)?])
            }
            b'N' => {
                let ns = *b.get(*i)?;
                *i += 1;
                let mut p = v0_path(b, i)?;
                v0_disambiguator(b, i);
                let id = v0_ident(b, i)?;
                p.push(match ns {
                    b'C' => "{closure}".to_string(),
                    _ if id.is_empty() => format!("{{{}}}", ns as char),
                    _ => id,
                });
                Some(p)
            }
            _ => None,
        }
    }

    /// `s<base-62>_`, where there is one.
    fn v0_disambiguator(b: &[u8], i: &mut usize) {
        if b.get(*i) == Some(&b's') {
            let end = b[*i..].iter().position(|&c| c == b'_').map_or(b.len(), |e| *i + e + 1);
            *i = end;
        }
    }

    /// A length-prefixed identifier (`_` after the length where the name starts with one or a digit).
    fn v0_ident(b: &[u8], i: &mut usize) -> Option<String> {
        if b.get(*i) == Some(&b'u') {
            *i += 1;
        }
        let digits = b[*i..].iter().take_while(|c| c.is_ascii_digit()).count();
        let n: usize = std::str::from_utf8(&b[*i..*i + digits]).ok()?.parse().ok()?;
        *i += digits;
        if b.get(*i) == Some(&b'_') {
            *i += 1;
        }
        let id = b.get(*i..*i + n)?;
        *i += n;
        Some(String::from_utf8_lossy(id).into_owned())
    }
}

/// The allocations this thread has made so far (a reallocation counts as one).
pub fn count() -> usize {
    MADE.try_with(Cell::get).unwrap_or(0)
}

/// `f`, its allocations left out of `count` (a test's own check inside what a budget counts).
#[cfg_attr(tarae_optimized, allow(dead_code))]
pub fn uncounted<R>(f: impl FnOnce() -> R) -> R {
    let was = count();
    let r = f();
    let _ = MADE.try_with(|m| m.set(was));
    r
}

struct Watched;

impl Watched {
    fn check() {
        if NO_ALLOC.load(Ordering::Relaxed) {
            // SAFETY: the end of this process (a run's child), nothing allocated.
            unsafe { libc::_exit(ALLOCATED) }
        }
    }

    fn made() {
        Watched::check();
        let _ = MADE.try_with(|m| m.set(m.get() + 1));
    }
}

// SAFETY: the system's allocator, a check (and a count) before each call.
unsafe impl GlobalAlloc for Watched {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        Watched::made();
        grew(layout.size() as isize);
        // SAFETY: as the caller's contract.
        let p = unsafe { System.alloc(layout) };
        sites::made(p, layout.size());
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        Watched::made();
        grew(layout.size() as isize);
        // SAFETY: as the caller's contract.
        let p = unsafe { System.alloc_zeroed(layout) };
        sites::made(p, layout.size());
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        Watched::check();
        grew(-(layout.size() as isize));
        sites::freed(ptr);
        // SAFETY: as the caller's contract.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        Watched::made();
        grew(new as isize - layout.size() as isize);
        // SAFETY: as the caller's contract.
        let p = unsafe { System.realloc(ptr, layout, new) };
        if !p.is_null() {
            sites::freed(ptr);
            sites::made(p, new);
        }
        p
    }
}

#[global_allocator]
static ALLOC: Watched = Watched;

#[cfg(test)]
mod tests {
    /// The count is this thread's, and an allocation is one: another thread's are counted on it, not here.
    #[test]
    fn an_allocation_counts_on_its_own_thread() {
        let before = super::count();
        let v = std::hint::black_box(vec![1u8; 10]);
        assert_eq!(super::count() - before, 1);
        // (the thread's start allocates on this one too — its handle: counted before its vectors are made)
        let started = std::sync::Barrier::new(2);
        let here = std::thread::scope(|s| {
            let other = s.spawn(|| {
                started.wait();
                let at = super::count();
                let w: Vec<Vec<u8>> = (0..100).map(|i| std::hint::black_box(vec![0u8; 64 + i])).collect();
                drop(std::hint::black_box(w));
                super::count() - at
            });
            let at = super::count();
            started.wait();
            let there = other.join().unwrap();
            assert!(there >= 101, "the other thread's own count: {there}");
            super::count() - at
        });
        drop(v);
        assert!(here < 10, "this thread took the other's in: {here}");
    }

    /// A stack walked as the allocator walks it is this thread's, its frames named after (review round 1).
    #[test]
    fn a_stack_is_walked_here_and_named_after() {
        #[inline(never)]
        fn walked_here() -> super::sites::Stack {
            std::hint::black_box(super::sites::walk())
        }
        let names = super::sites::names(&walked_here());
        if cfg!(not(any(target_os = "macos", all(target_os = "linux", target_env = "gnu")))) {
            return assert!(names.is_empty(), "no execinfo: nothing walked");
        }
        assert!(names.len() > 2, "{names:?}");
        // (a glibc test binary exports few of its names: there the frame is the binary's and an offset)
        if cfg!(target_os = "macos") {
            let at = "tests::a_stack_is_walked_here_and_named_after::walked_here";
            let here = names.iter().flatten().any(|n| n.ends_with(at));
            assert!(here, "{names:?}");
        }
    }

    /// Review round 1: a panic while stacks are kept never waits on itself — its hook allocates holding std's
    /// backtrace lock (writing into the test's captured output, the backtrace resolved), and the walk takes none.
    /// Tracking is the whole process's: run in a process of its own (`panic_while_tracking`), waited on here — a
    /// process that waits on itself can't end itself (every thread's allocation waits on the lock too), so past the
    /// patience it's stopped from here.
    #[test]
    fn a_panic_while_tracking_never_waits_on_itself() {
        use std::io::Read as _;
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "allocs::tests::panic_while_tracking", "--ignored", "--test-threads=1"])
            .env("RUST_BACKTRACE", "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let mut out = child.stdout.take().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let reader = std::thread::Builder::new().name("tarae-child-out".into());
        reader
            .spawn(move || {
                let mut said = String::new();
                let _ = out.read_to_string(&mut said);
                let _ = tx.send(said);
            })
            .unwrap();
        let said = rx.recv_timeout(crate::test_results::PATIENCE);
        if said.is_err() {
            let _ = child.kill();
        }
        let status = child.wait().unwrap();
        let said = said.unwrap_or_else(|_| "(stopped past the patience: it waited on itself)".into());
        assert!(status.success() && said.contains("1 passed"), "{status}\n{said}");
    }

    #[test]
    #[ignore = "a process of its own: a_panic_while_tracking_never_waits_on_itself runs it"]
    fn panic_while_tracking() {
        super::sites::track(true);
        let kept = std::hint::black_box(vec![0u8; 4096]);
        let (tx, rx) = std::sync::mpsc::channel();
        let panics = std::thread::Builder::new().name("tarae-panics".into());
        panics
            .spawn(move || {
                let r = std::panic::catch_unwind(|| panic!("on purpose"));
                tx.send(r.is_err()).unwrap();
            })
            .unwrap();
        let panicked = rx.recv_timeout(crate::test_results::PATIENCE);
        super::sites::track(false);
        assert_eq!(panicked, Ok(true), "the panic came back");
        let held = super::sites::held(4);
        if cfg!(any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))) {
            assert!(held.iter().any(|h| h.0 >= 4096), "the block kept meanwhile: {held:?}");
        }
        drop(kept);
    }
}
