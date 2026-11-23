//! Type1自身の埋め込み指定と明示OFL宣言を区別し、原本noticeを省かず出力へ渡す。
use super::header::Info;
use crate::decode::DecodeBudget;

/// 元のFSType欠落を、暗黙のinstallable指定へ置き換えない判定結果。
pub(super) enum Decision {
    /// 確認できた指定と元の表示を保持する。
    Allowed(License),
    /// sourceに利用可能な指定も、認識できる明示OFL宣言もない。
    Unknown,
    /// editable DOCXへ使えない指定、またはsubset禁止との矛盾がある。
    Prohibited,
}

/// 判断の根拠を、元の権利情報の有無とともに残す。
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Basis {
    /// 元FSTypeのinstallableまたはeditable指定。
    FsType,
    /// FontInfoか先頭commentによる、明示されたOFL1.1宣言。
    OpenFontLicense,
}

/// 名前を決める処理とfont writerへ渡す、変更していない原本指定と表示全文。
pub(super) struct License {
    /// 元のFSTypeの有無。Noneと明示0を区別する。
    pub(super) declared_flags: Option<u16>,
    /// 生成fontに保持する埋め込みbit。OFLによる許可時だけ欠落を0へ解決する。
    pub(super) flags: u16,
    /// 判断根拠。一般的なライセンス条件の免除を意味しない。
    pub(super) basis: Basis,
    /// 先頭commentと復号済みNotice/Copyrightの元byte値。
    pub(super) notice: Vec<u8>,
    /// 人間が読める原本表示と、明示OFLの場合の正規ライセンス全文。
    pub(super) text: String,
    /// 明示OFLの場合の公式URL。それ以外は推測せず空文字列。
    pub(super) url: &'static str,
}

/// 検査済みcleartextの先頭commentだけを参照し、実行式や文字列中の偽commentを読まない。
pub(super) fn read(
    _budget: &DecodeBudget,
    _clear: &[u8],
    _info: &Info<'_>,
    _subset: bool,
) -> Option<Decision> {
    None
}

#[cfg(test)]
#[path = "type1_outline_permission_tests.rs"]
mod tests;
