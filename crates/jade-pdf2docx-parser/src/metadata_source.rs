//! 完成した合成metadata本文を解析内で共有し、未検査入力は所有本文に留める。
use std::rc::Rc;

/// 元入力に対応する共有本文、または未検査入力から作った所有本文。
pub(crate) enum Source {
    /// 元入力以外の一度だけ作った本文。追加の共有管理領域は確保しない。
    Owned(Vec<u8>),
    /// 完成後だけcacheへ保存する不変本文。
    Shared(Rc<Vec<u8>>),
}

impl std::ops::Deref for Source {
    type Target = [u8];

    /// 共有・所有の本文を同じ借用byte列として読む。
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Owned(bytes) => bytes,
            Self::Shared(bytes) => bytes,
        }
    }
}

/// 完成本文の共有管理領域を予算へ予約し、検査済み入力に限り保存する。
pub(crate) fn store(budget: &crate::decode::DecodeBudget, bytes: &[u8], source: Vec<u8>) -> Source {
    if budget.is_stopped() {
        return Source::Owned(Vec::new());
    }
    if budget.stream_index(bytes).is_none() || source.is_empty() {
        return Source::Owned(source);
    }
    let cost = std::mem::size_of::<Vec<u8>>() + 2 * std::mem::size_of::<usize>();
    if budget
        .charge_metadata_workspace_bytes(cost as u64)
        .is_none()
    {
        return Source::Owned(Vec::new());
    }
    let shared = Rc::new(source);
    budget.cache_parser_source(bytes, shared.clone());
    Source::Shared(shared)
}
