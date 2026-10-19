//! clone可能で不可逆な解析キャンセル要求を、解析の停止点で共有する。
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// 他のthreadから解析停止を要求するhandle。cloneは同じ停止要求を共有する。
///
/// 既定は未停止。cancel後の再利用は直ちに停止し、新しい解析には新しいhandleを使う。
/// 協調キャンセルのため、実行中のnative/codec呼び出し・blocking I/Oを強制終了しない。
#[derive(Debug, Clone, Default)]
pub struct ParseCancellation {
    /// clone間で共有する不可逆の停止要求。
    requested: Arc<AtomicBool>,
}

impl ParseCancellation {
    /// 未停止のhandleを作成する。
    pub fn new() -> Self {
        Self::default()
    }

    /// 全cloneに停止を要求する。繰り返し呼んでも停止状態を解除しない。
    pub fn cancel(&self) {
        self.requested.store(true, Ordering::Relaxed);
    }

    /// 停止要求が発行済みかを返す。
    pub fn is_cancelled(&self) -> bool {
        self.requested.load(Ordering::Relaxed)
    }
}
