//! Rootなし互換入力の継承矩形・回転を、共有索引と容量制限付き訪問集合で読む。
use super::{DecodeBudget, ParsedBounds, page_bounds_are_usable, parse_integer_after_name};
use crate::page_attributes::Objects;

/// page objectと親Pages chainから最も近い指定page boxを読む。
pub(super) fn parse_inherited_page_box(
    budget: &DecodeBudget,
    objects: &Objects<'_>,
    page_object: &str,
    box_name: &str,
) -> Option<ParsedBounds> {
    inherit(budget, objects, page_object, |object| {
        parse_page_box_from_object(object, box_name)
    })
}

/// page objectと親Pages chainから最も近い`/Rotate`を読む。
pub(super) fn parse_inherited_page_rotation(
    budget: &DecodeBudget,
    objects: &Objects<'_>,
    page_object: &str,
) -> Option<i32> {
    inherit(budget, objects, page_object, |object| {
        parse_integer_after_name(object, "/Rotate")
    })
}

/// 借用索引の最新世代だけを辿り、訪問容量不足・循環・欠落では継承を停止する。
fn inherit<T>(
    budget: &DecodeBudget,
    objects: &Objects<'_>,
    page_object: &str,
    read: impl Fn(&str) -> Option<T>,
) -> Option<T> {
    let mut object = page_object;
    let mut visited = crate::metadata_workspace::Set::default();
    let mut depth = 0usize;
    loop {
        if budget.is_stopped() {
            return None;
        }
        if let Some(value) = read(object) {
            return Some(value);
        }
        let (number, generation) = crate::metadata::value(object.trim().as_bytes(), b"Parent")
            .and_then(crate::metadata::reference_with_generation)?;
        depth = depth.saturating_add(1);
        budget.check_page_inheritance_depth(depth)?;
        if !visited.try_insert(budget, number)? {
            return None;
        }
        let &(actual, parent) = objects.get(&number)?;
        if actual != generation {
            return None;
        }
        object = std::str::from_utf8(parent).ok()?;
    }
}

/// object本文から指定page boxを読む。
pub(super) fn parse_page_box_from_object(object: &str, box_name: &str) -> Option<ParsedBounds> {
    let value = crate::metadata::value(
        object.trim().as_bytes(),
        box_name.strip_prefix('/')?.as_bytes(),
    )?;
    use crate::syntax::{Lexer, Token};
    let mut lexer = Lexer {
        bytes: value,
        cursor: 0,
    };
    if !matches!(lexer.next(), Some(Token::Open(false))) {
        return None;
    }
    let mut numbers = [0.0; 4];
    for number in &mut numbers {
        let Token::Word(word) = lexer.next()? else {
            return None;
        };
        *number = crate::page_attributes::number_value(word)?;
    }
    if !matches!(lexer.next(), Some(Token::Close(false))) || lexer.next().is_some() {
        return None;
    }
    let bounds = ParsedBounds {
        x0: numbers[0].min(numbers[2]),
        y0: numbers[1].min(numbers[3]),
        x1: numbers[0].max(numbers[2]),
        y1: numbers[1].max(numbers[3]),
    };
    page_bounds_are_usable(bounds).then_some(bounds)
}
