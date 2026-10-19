//! CCITTのscalar属性を字句と間接参照に従って検証する。
use super::super::{DecodeBudget, ObjectBytes, image_attribute};
use crate::syntax::{Lexer, Token};

/// 復号前の属性辞書と参照解決に必要な入力を借用する。
pub(super) struct Reader<'a> {
    /// 参照探索とmetadata保存を制限する予算。
    pub budget: &'a DecodeBudget,
    /// 間接scalarのobjectを含む元PDF。
    pub pdf: &'a [u8],
    /// 完全性検査済みのDecodeParms辞書。
    pub dictionary: &'a [u8],
}

impl Reader<'_> {
    /// 属性省略とnullに既定値を適用し、指定値は参照先から読む。
    fn scalar<'a>(&'a self, key: &[u8], default: &'a [u8]) -> Option<ObjectBytes<'a>> {
        if crate::metadata::value(self.dictionary, key).is_none() {
            return Some(ObjectBytes::Borrowed(default));
        }
        let value = image_attribute(self.budget, self.pdf, self.dictionary, key)?;
        if single_word(&value)? == b"null" {
            Some(ObjectBytes::Borrowed(default))
        } else {
            Some(value)
        }
    }

    /// 指定された整数を正確に読み、実数・飽和変換・余剰tokenを拒否する。
    pub fn integer(&self, key: &[u8], default: &[u8]) -> Option<i64> {
        let value = self.scalar(key, default)?;
        std::str::from_utf8(single_word(&value)?).ok()?.parse().ok()
    }

    /// trueとfalseだけを認め、他の型へ既定値を適用しない。
    pub fn boolean(&self, key: &[u8], default: &[u8]) -> Option<bool> {
        let value = self.scalar(key, default)?;
        match single_word(&value)? {
            b"true" => Some(true),
            b"false" => Some(false),
            _ => None,
        }
    }
}

/// 空白とコメントを許し、単一wordであることを確認する。
fn single_word(bytes: &[u8]) -> Option<&[u8]> {
    let mut lexer = Lexer { bytes, cursor: 0 };
    let Token::Word(word) = lexer.next()? else {
        return None;
    };
    lexer.next().is_none().then_some(word)
}
