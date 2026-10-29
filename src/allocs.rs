//! The test binary's allocator (tests only): the system's, watched and counted. Watched — a run's child sets `NO_ALLOC`
//! after the keeper's fork (`sandbox::keeper`, Linux), and any call ends that process; counted — each thread's
//! allocations so far (`count`), so a budget can hold what a pass allocates (#214: an event that changes nothing
//! allocates the same with 17 sessions open and with 34).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

/// Set first thing in a run's child: from then on any allocation or free ends that process with `ALLOCATED`, before
/// it touches malloc's locks.
pub static NO_ALLOC: AtomicBool = AtomicBool::new(false);
/// The exit status of a run's child that allocated before its exec.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const ALLOCATED: i32 = 121;

thread_local! {
    /// Allocations (and reallocations) made on this thread so far — no destructor, so readable to a thread's end.
    static MADE: Cell<usize> = const { Cell::new(0) };
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
        // SAFETY: as the caller's contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        Watched::made();
        // SAFETY: as the caller's contract.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        Watched::check();
        // SAFETY: as the caller's contract.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new: usize) -> *mut u8 {
        Watched::made();
        // SAFETY: as the caller's contract.
        unsafe { System.realloc(ptr, layout, new) }
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
}
