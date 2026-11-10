//! A global allocator that counts the heap bytes each thread holds, so a test can bound the
//! memory of one scan. Deterministic: the same input allocates the same sizes every run.
//! A test binary opts in with `#[global_allocator] static HEAP: heap::Counting = heap::Counting;`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

pub struct Counting;

thread_local! {
    static LIVE: Cell<usize> = const { Cell::new(0) };
    static PEAK: Cell<usize> = const { Cell::new(0) };
}

fn grow(n: usize) {
    LIVE.with(|live| {
        let now = live.get() + n;
        live.set(now);
        PEAK.with(|peak| peak.set(peak.get().max(now)));
    });
}

fn shrink(n: usize) {
    LIVE.with(|live| live.set(live.get().saturating_sub(n)));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            grow(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            grow(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        shrink(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            shrink(layout.size());
            grow(new_size);
        }
        p
    }
}

/// Runs `f` and returns its result and the most heap bytes this thread held during the run,
/// above what it held before.
pub fn peak<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let base = LIVE.with(Cell::get);
    PEAK.with(|p| p.set(base));
    let out = f();
    let top = PEAK.with(Cell::get);
    (out, top.saturating_sub(base))
}
