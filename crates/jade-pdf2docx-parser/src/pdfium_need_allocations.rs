//! parser unit testの指定範囲だけ、現在threadのheap要求を測定するallocator。
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

thread_local! {
    /// 測定範囲外は記録せず、thread間で測定値を共有しない。
    static REQUESTS: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
}

/// 全操作をSystemへ委譲し、測定範囲内の要求容量だけを記録する。
struct TrackedSystem;

/// parser unit testの実行形式だけで使用するallocator。
#[global_allocator]
static ALLOCATOR: TrackedSystem = TrackedSystem;

/// 初回確保と再確保の要求回数・全要求byte数を記録する。
fn record(bytes: usize) {
    let _ = REQUESTS.try_with(|requests| {
        if let Some((count, total)) = requests.get() {
            requests.set(Some((count.saturating_add(1), total.saturating_add(bytes))));
        }
    });
}

// SAFETY: layout/ptrを変更せず同じSystem allocatorの対応する操作へ委譲する。
unsafe impl GlobalAlloc for TrackedSystem {
    /// 要求を記録し、Systemの未初期化領域を返す。
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: 呼び出し元の有効なlayoutをそのまま委譲する。
        unsafe { System.alloc(layout) }
    }

    /// 要求を記録し、Systemのゼロ初期化領域を返す。
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        record(layout.size());
        // SAFETY: 呼び出し元の有効なlayoutをそのまま委譲する。
        unsafe { System.alloc_zeroed(layout) }
    }

    /// 要求する新容量を記録し、既存領域をSystemで再確保する。
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        record(size);
        // SAFETY: 同じSystemから得たptr/layoutと呼び出し元のsizeを委譲する。
        unsafe { System.realloc(ptr, layout, size) }
    }

    /// Systemから得た領域を、対応するlayoutで解放する。
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: 同じSystemから得たptr/layoutをそのまま委譲する。
        unsafe { System.dealloc(ptr, layout) }
    }
}

/// panicでも測定開始前の状態を復元し、後続テストを汚染しない。
struct Guard {
    /// 測定開始前のthread固有状態。
    previous: Option<(usize, usize)>,
}

impl Drop for Guard {
    /// thread固有の測定状態を復元する。
    fn drop(&mut self) {
        REQUESTS.with(|requests| requests.set(self.previous));
    }
}

/// closure内部のheap要求だけを回数/byte数で返す。native内部や他threadは対象外。
pub(crate) fn measure<T>(operation: impl FnOnce() -> T) -> (T, (usize, usize)) {
    let guard = Guard {
        previous: REQUESTS.with(|requests| requests.replace(Some((0, 0)))),
    };
    let result = operation();
    let requests = REQUESTS.with(|requests| requests.get().unwrap());
    drop(guard);
    (result, requests)
}
