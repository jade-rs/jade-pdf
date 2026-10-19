//! 共通fallible確保の要求容量を用途横断で累積予約する。
use super::DecodeBudget;
use crate::ParseError;
use std::{
    alloc::Layout,
    sync::{Arc, atomic::AtomicUsize},
};

impl DecodeBudget {
    /// 確保より先に型の容量を検査し、新配列全体を共通予算へ予約する。
    pub(crate) fn reserve_allocation<T>(&self, capacity: usize) -> Option<()> {
        if self.is_stopped() {
            return None;
        }
        let bytes = capacity
            .checked_mul(std::mem::size_of::<T>())
            .filter(|bytes| *bytes <= isize::MAX as usize);
        let Some(bytes) = bytes else {
            self.allocation_overflow();
            return None;
        };
        let actual = self.allocation_total.get().checked_add(bytes as u64);
        if actual.is_none_or(|actual| actual > self.allocation_limit) {
            self.exceeded(
                "total allocation bytes",
                self.allocation_limit,
                actual.unwrap_or(u64::MAX),
            );
            return None;
        }
        self.allocation_total.set(actual?);
        Some(())
    }

    /// 個別予算で検査した固定本体も共通容量へ予約してからBoxを作る。
    /// allocator自身のOOMは扱わず、要求容量の超過を確保前に拒否する。
    pub(crate) fn budgeted_box<T>(&self, value: T) -> Option<Box<T>> {
        self.reserve_allocation::<T>(1)?;
        Some(Box::new(value))
    }

    /// Arcの本体と二つのcounterをalignment込みで共通容量へ予約する。
    /// 内部のVec等のpayloadは既存予約を使い、共有cloneでは再予約しない。
    /// allocatorの管理領域やOOMは対象外で、実RSSの保証ではない。
    pub(crate) fn budgeted_arc<T>(&self, value: T) -> Option<Arc<T>> {
        if self.is_stopped() {
            return None;
        }
        let Ok((layout, _)) = Layout::new::<[AtomicUsize; 2]>().extend(Layout::new::<T>()) else {
            self.allocation_overflow();
            return None;
        };
        self.reserve_allocation::<u8>(layout.pad_to_align().size())?;
        Some(Arc::new(value))
    }

    /// addressable容量を超える要求を共通のOOMエラーへ変換し停止状態を保持する。
    fn allocation_overflow(&self) {
        self.stopped.set(true);
        *self.failure.borrow_mut() = Some(ParseError::Io(std::io::Error::new(
            std::io::ErrorKind::OutOfMemory,
            "requested allocation exceeds addressable capacity",
        )));
    }
}
