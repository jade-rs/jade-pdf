//! Type1のcleartext辞書から静的な座標系・属性・元の文字列だけを読む。
use crate::syntax::Token;
use crate::type1_program::{
    lex::{Item, Lexer},
    preamble,
};
use crate::{decode::DecodeBudget, type1_program::Encoding};
#[path = "type1_outline_header_info.rs"]
mod info;
#[path = "type1_outline_header_values.rs"]
mod values;
use values::{definition, number, numbers, word};

/// 元のType1辞書を借用する情報。権利判断や派生名の生成は呼出元の責務。
pub(super) struct Header<'a> {
    /// 原本のPostScript名。派生fontのfamily名へそのまま使わない。
    pub(super) name: &'a [u8],
    /// 原本のFontMatrix六要素。
    pub(super) matrix: [f64; 6],
    /// 原本で宣言したFontBBox。
    pub(super) bbox: [f64; 4],
    /// 原本の静的なEncoding。
    pub(super) encoding: Encoding<'a>,
    /// FontInfoの情報。
    pub(super) info: Info<'a>,
}

/// 指定の有無を保つFontInfo。文字列は括弧を除いた未展開のPostScript byte列。
#[derive(Default)]
pub(super) struct Info<'a> {
    /// 元のNotice文字列。octal escape等は別途復号する。
    pub(super) notice: Option<&'a [u8]>,
    /// 元のCopyright文字列。
    pub(super) copyright: Option<&'a [u8]>,
    /// 元のversion文字列。
    pub(super) version: Option<&'a [u8]>,
    /// 元のFullName文字列。
    pub(super) full_name: Option<&'a [u8]>,
    /// 元のFamilyName文字列。
    pub(super) family: Option<&'a [u8]>,
    /// 元のWeight文字列。
    pub(super) weight: Option<&'a [u8]>,
    /// 元のItalicAngle。未指定はNone。
    pub(super) italic_angle: Option<f64>,
    /// 元の等幅指定。未指定はNone。
    pub(super) fixed: Option<bool>,
    /// 元の下線位置。
    pub(super) underline_position: Option<f64>,
    /// 元の下線の太さ。
    pub(super) underline_thickness: Option<f64>,
    /// 元のFSType。Noneを埋め込み許可の0へ置き換えない。
    pub(super) fs_type: Option<u16>,
    /// TeX系FontInfoのascent。PDF descriptorとの優先順位は呼出元で決める。
    pub(super) ascent: Option<f64>,
    /// TeX系FontInfoのdescent。
    pub(super) descent: Option<f64>,
}

/// PostScriptを実行せず、有限の静的定義と暗号化境界を確認する。
pub(super) fn parse<'a>(budget: &DecodeBudget, bytes: &'a [u8]) -> Option<Header<'a>> {
    if bytes.len() > crate::parser::MAX_EMBEDDED_FONT_BYTES || budget.is_stopped() {
        return None;
    }
    let mut lexer = Lexer { bytes, cursor: 0 };
    let mut first = lexer.next(budget)?;
    if matches!(first, Item::Pdf(Token::Word(b"FontDirectory"))) {
        preamble::skip(budget, &mut lexer)?;
        first = lexer.next(budget)?;
    }
    let Item::Pdf(Token::Word(size)) = first else {
        return None;
    };
    if crate::syntax::integer(size)? > 256 {
        return None;
    }
    word(budget, &mut lexer, b"dict")?;
    word(budget, &mut lexer, b"begin")?;
    let (mut name, mut matrix, mut bbox, mut encoding) = (None, None, None, None);
    let mut info = Info::default();
    let mut root_fs = None;
    let mut seen = 0u16;
    loop {
        let key = match lexer.next(budget)? {
            Item::Pdf(Token::Word(b"currentdict")) => break,
            Item::Pdf(Token::Name(key)) => key,
            _ => return None,
        };
        let bit = match key {
            b"FontType" => 0,
            b"FontName" => 1,
            b"FontMatrix" => 2,
            b"FontBBox" => 3,
            b"Encoding" => 4,
            b"FontInfo" => 5,
            b"PaintType" => 6,
            b"UniqueID" => 7,
            b"StrokeWidth" => 8,
            b"FSType" => 9,
            _ => return None,
        };
        if seen & (1 << bit) != 0 {
            return None;
        }
        seen |= 1 << bit;
        match key {
            b"FontType" => {
                if number(budget, &mut lexer)? != 1. {
                    return None;
                }
                definition(budget, &mut lexer)?;
            }
            b"FontName" => {
                let Item::Pdf(Token::Name(value)) = lexer.next(budget)? else {
                    return None;
                };
                if value.is_empty()
                    || value.len() > 127
                    || !value.iter().all(|b| (33..=126).contains(b))
                {
                    return None;
                }
                name = Some(value);
                definition(budget, &mut lexer)?;
            }
            b"FontMatrix" => {
                let values = numbers::<6>(budget, &mut lexer)?;
                let det = values[0] * values[3] - values[1] * values[2];
                if !det.is_finite() || det == 0. {
                    return None;
                }
                matrix = Some(values);
                definition(budget, &mut lexer)?;
            }
            b"FontBBox" => {
                let values = numbers::<4>(budget, &mut lexer)?;
                if values[0] > values[2] || values[1] > values[3] {
                    return None;
                }
                bbox = Some(values);
                definition(budget, &mut lexer)?;
            }
            b"Encoding" => encoding = Some(crate::type1_program::definition(budget, &mut lexer)?),
            b"FontInfo" => info = info::parse(budget, &mut lexer)?,
            b"FSType" => {
                root_fs = Some(values::permission(budget, &mut lexer)?);
                definition(budget, &mut lexer)?;
            }
            b"PaintType" => {
                if number(budget, &mut lexer)? != 0. {
                    return None;
                }
                definition(budget, &mut lexer)?;
            }
            b"UniqueID" | b"StrokeWidth" => {
                let value = number(budget, &mut lexer)?;
                if value < 0. || (key == b"UniqueID" && (value.fract() != 0. || value > 16777215.))
                {
                    return None;
                }
                definition(budget, &mut lexer)?;
            }
            _ => return None,
        }
    }
    for expected in [b"end".as_slice(), b"currentfile", b"eexec"] {
        word(budget, &mut lexer, expected)?;
    }
    if lexer.next(budget).is_some() || budget.is_stopped() || seen & 1 == 0 {
        return None;
    }
    if root_fs.is_some() && info.fs_type.is_some() {
        return None;
    }
    info.fs_type = info.fs_type.or(root_fs);
    Some(Header {
        name: name?,
        matrix: matrix?,
        bbox: bbox?,
        encoding: encoding?,
        info,
    })
}

#[cfg(test)]
#[path = "type1_outline_header_tests.rs"]
mod tests;
