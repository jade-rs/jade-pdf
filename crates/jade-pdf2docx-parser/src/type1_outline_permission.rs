//! Type1自身の埋め込み指定と明示OFL宣言を区別し、原本noticeを省かず出力へ渡す。
use super::header::Info;
use crate::decode::DecodeBudget;
#[path = "type1_outline_permission_notice.rs"]
mod notice;

/// 公式標準本文だけを含め、個々のfontのcopyright/RFNは原本から別に保つ。
const OFL_BODY: &str = include_str!("../licenses/open-font-license-1.1.txt");

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
    budget: &DecodeBudget,
    clear: &[u8],
    info: &Info<'_>,
    subset: bool,
) -> Option<Decision> {
    if budget.is_stopped() || clear.len() > crate::parser::MAX_EMBEDDED_FONT_BYTES {
        return None;
    }
    if info
        .fs_type
        .is_some_and(|flags| flags & !0x0108 != 0 || (subset && flags & 0x100 != 0))
    {
        return Some(Decision::Prohibited);
    }
    let comments = leading_comments(budget, clear)?;
    let original_notice = match info.notice {
        Some(raw) => super::ps_string::decode(budget, raw)?,
        None => Vec::new(),
    };
    let copyright = match info.copyright {
        Some(raw) => super::ps_string::decode(budget, raw)?,
        None => Vec::new(),
    };
    let ofl = declares_ofl(comments) || declares_ofl(&original_notice) || declares_ofl(&copyright);
    if info.fs_type.is_none() && !ofl {
        return Some(Decision::Unknown);
    }
    let (notice, text) = notice::collect(budget, comments, &original_notice, &copyright, ofl)?;
    if budget.is_stopped() {
        return None;
    }
    Some(Decision::Allowed(License {
        declared_flags: info.fs_type,
        flags: info.fs_type.unwrap_or(0),
        basis: if ofl {
            Basis::OpenFontLicense
        } else {
            Basis::FsType
        },
        notice,
        text,
        url: if ofl {
            "https://openfontlicense.org/open-font-license-official-text/"
        } else {
            ""
        },
    }))
}

/// 最初のPostScript tokenで止まり、CR/LF/CRLFのいずれでもcomment境界を保つ。
fn leading_comments<'a>(budget: &DecodeBudget, clear: &'a [u8]) -> Option<&'a [u8]> {
    let mut cursor = 0;
    while cursor < clear.len() {
        if budget.is_stopped() {
            return None;
        }
        if crate::syntax::whitespace(clear[cursor]) {
            cursor += 1;
            continue;
        }
        if clear[cursor] != b'%' {
            break;
        }
        while cursor < clear.len() && !matches!(clear[cursor], b'\r' | b'\n') {
            if budget.is_stopped() {
                return None;
            }
            cursor += 1;
        }
    }
    Some(&clear[..cursor])
}

/// 明示された標準宣言の一行だけを認識し、font名や似た語句から推定しない。
fn declares_ofl(bytes: &[u8]) -> bool {
    bytes
        .split(|byte| matches!(byte, b'\r' | b'\n'))
        .any(|line| {
            let line = line.trim_ascii();
            let line = line.strip_prefix(b"%").unwrap_or(line).trim_ascii();
            line == b"This Font Software is licensed under the SIL Open Font License, Version 1.1."
        })
}

#[cfg(test)]
#[path = "type1_outline_permission_tests.rs"]
mod tests;
