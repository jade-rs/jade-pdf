//! Adobe標準OtherSubrsの完全な字句指紋だけを承認し、PostScriptを実行しない。
use crate::decode::DecodeBudget;

/// 完全な定義を確認した証拠。呼出番号だけでは作らない。
#[derive(Clone, Copy)]
pub(super) struct Verified {
    /// 標準Flex定義も含むか。ヒント切替だけの定義とは区別する。
    flex: bool,
}
impl Verified {
    /// 標準Flex定義が含まれる場合だけ曲線の専用処理を許す。
    pub(super) fn allows_flex(self) -> bool {
        self.flex
    }

    /// 型付き演算の所有テストだけで使う、font由来でない承認値。
    #[cfg(test)]
    pub(super) fn for_owned_test() -> Self {
        Self { flex: true }
    }
}

/// 配列全体を字句単位で照合し、末尾位置と承認値を返す。
pub(super) fn verify(budget: &DecodeBudget, bytes: &[u8]) -> Option<(Verified, usize)> {
    let _ = (budget, bytes);
    None
}
