//! 正式な trailer Root から Page の所属と Kids 順を解決する。
use crate::{ParseError, ParseLimits, decode::DecodeBudget, limits::check_limit, metadata};
use std::path::Path;

/// DFS で閉じる Pages の宣言数と開始位置を保持する。
struct SubtreeCount {
    /// Pages が tree 内にある深さ。
    depth: usize,
    /// Count が宣言する子孫 Page 数。
    declared: usize,
    /// subtree を開く直前の抽出済み Page 数。
    start: usize,
}

/// 有効 object の合成本文を一度索引化し、再帰せず page tree を辿る。
/// Root のない旧形式入力は従来の最外 Page 列挙へ委ねる。
pub(crate) fn resolve(
    path: &Path,
    bytes: &[u8],
    source: &[u8],
    budget: &DecodeBudget,
    limits: &ParseLimits,
    version: &mut crate::PdfVersion,
) -> Result<Option<Vec<crate::page_attributes::Page>>, ParseError> {
    let Some(root) = budget.stream_index(bytes).and_then(|index| index.root()) else {
        return Ok(None);
    };
    let Some(objects) = crate::object_index::build(budget, source) else {
        budget.check()?;
        return Err(invalid(path, "metadata index unavailable"));
    };
    let catalog = dictionary(path, &objects, root)?;
    if !metadata::has_type(catalog, b"Catalog") {
        return Err(invalid(path, "Root is not a Catalog"));
    }
    *version = crate::catalog_version::read(path, &objects, catalog, version, limits, budget)?;
    let pages = metadata::value(catalog, b"Pages")
        .and_then(metadata::reference_with_generation)
        .ok_or_else(|| invalid(path, "missing Catalog Pages reference"))?;
    if !metadata::has_type(dictionary(path, &objects, pages)?, b"Pages") {
        return Err(invalid(path, "Catalog Pages is not a Pages node"));
    }
    let mut pending = Vec::new();
    crate::metadata_workspace::push(
        budget,
        &mut pending,
        (
            pages,
            None,
            1usize,
            crate::page_attributes::Inherited::default(),
        ),
    )?;
    let mut counts: Vec<SubtreeCount> = Vec::new();
    let mut visited = crate::metadata_workspace::Set::default();
    let mut order = Vec::new();
    while let Some((reference, parent, depth, inherited)) = pending.pop() {
        while counts.last().is_some_and(|count| count.depth >= depth) {
            validate_subtree_count(path, counts.pop().unwrap(), order.len())?;
        }
        check_limit(
            "page tree depth",
            depth as u64,
            limits.max_container_nesting as u64,
        )?;
        if !visited.insert(budget, reference)? {
            return Err(invalid(path, "cyclic or repeated page tree node"));
        }
        check_limit(
            "page tree nodes",
            visited.len() as u64,
            limits.max_xref_entries as u64,
        )?;
        let node = dictionary(path, &objects, reference)?;
        let declared_parent = metadata::value(node, b"Parent");
        match parent {
            Some(parent)
                if declared_parent.and_then(metadata::reference_with_generation)
                    == Some(parent) => {}
            None if declared_parent.is_none() => {}
            _ => return Err(invalid(path, "Parent does not match page tree edge")),
        }
        let inherited = inherited.at_node(path, &objects, reference.0, node, limits, budget)?;
        if metadata::has_type(node, b"Page") {
            let page = inherited.page(path, &objects, reference.0, limits, budget)?;
            crate::metadata_workspace::push(budget, &mut order, page)?;
            check_limit("page count", order.len() as u64, limits.max_pages as u64)?;
        } else if metadata::has_type(node, b"Pages") {
            let count = metadata::value(node, b"Count")
                .ok_or_else(|| invalid(path, "missing Pages Count"))?;
            let count = resolve_count(path, &objects, count, limits, budget)?;
            crate::metadata_workspace::push(
                budget,
                &mut counts,
                SubtreeCount {
                    depth,
                    declared: count,
                    start: order.len(),
                },
            )?;
            let kids = metadata::value(node, b"Kids")
                .ok_or_else(|| invalid(path, "invalid Pages Kids array"))?;
            let (kids, _) =
                crate::page_attributes::resolve_value(path, &objects, kids, limits, budget)?;
            let kids =
                crate::metadata_workspace::array_values(budget, kids, limits.max_xref_entries)?
                    .ok_or_else(|| invalid(path, "invalid Pages Kids array"))?;
            check_limit(
                "page tree nodes",
                (visited.len() + pending.len() + kids.len()) as u64,
                limits.max_xref_entries as u64,
            )?;
            let reference_parent = reference;
            for kid in kids.into_iter().rev() {
                let reference = metadata::reference_with_generation(kid)
                    .ok_or_else(|| invalid(path, "invalid page tree child reference"))?;
                crate::metadata_workspace::push(
                    budget,
                    &mut pending,
                    (reference, Some(reference_parent), depth + 1, inherited),
                )?;
            }
        } else {
            return Err(invalid(path, "page tree child is neither Page nor Pages"));
        }
    }
    for count in counts {
        validate_subtree_count(path, count, order.len())?;
    }
    Ok(Some(order))
}

/// object の存在・世代・最外 dictionary を検証する。
fn dictionary<'a>(
    path: &Path,
    objects: &crate::page_attributes::Objects<'a>,
    reference: (u32, u16),
) -> Result<&'a [u8], ParseError> {
    let (_, body) = objects
        .get(&reference.0)
        .filter(|(generation, _)| *generation == reference.1)
        .ok_or_else(|| invalid(path, "missing or wrong-generation page tree object"))?;
    metadata::dictionary_iter(body)
        .next()
        .ok_or_else(|| invalid(path, "page tree object is not a dictionary"))
}

/// page tree の破損を入力パス付き構造エラーにする。
fn invalid(path: &Path, reason: &'static str) -> ParseError {
    ParseError::InvalidPdfStructure {
        path: path.into(),
        reason,
    }
}

/// DFS の開始位置との差で、その Pages の子孫 Page 数を照合する。
fn validate_subtree_count(path: &Path, count: SubtreeCount, end: usize) -> Result<(), ParseError> {
    if end - count.start != count.declared {
        return Err(invalid(path, "Pages Count does not match descendants"));
    }
    Ok(())
}

/// Count の直接・間接整数を、世代と循環と参照深度を検査して読む。
fn resolve_count<'a>(
    path: &Path,
    objects: &crate::page_attributes::Objects<'a>,
    mut value: &'a [u8],
    limits: &ParseLimits,
    budget: &DecodeBudget,
) -> Result<usize, ParseError> {
    let mut visited = crate::metadata_workspace::Set::default();
    while let Some(reference) = metadata::reference_with_generation(value) {
        if !visited.insert(budget, reference)? {
            return Err(invalid(path, "cyclic Pages Count reference"));
        }
        check_limit(
            "page tree scalar depth",
            visited.len() as u64,
            limits.max_container_nesting as u64,
        )?;
        value = objects
            .get(&reference.0)
            .filter(|(generation, _)| *generation == reference.1)
            .map(|(_, body)| *body)
            .ok_or_else(|| invalid(path, "missing or wrong-generation Pages Count"))?;
    }
    let mut lexer = crate::syntax::Lexer {
        bytes: value,
        cursor: 0,
    };
    let Some(crate::syntax::Token::Word(word)) = lexer.next() else {
        return Err(invalid(path, "Pages Count is not a nonnegative integer"));
    };
    let count = std::str::from_utf8(word)
        .ok()
        .and_then(|word| word.parse::<i128>().ok())
        .and_then(|count| usize::try_from(count).ok())
        .filter(|_| lexer.next().is_none())
        .ok_or_else(|| invalid(path, "Pages Count is not a nonnegative integer"))?;
    Ok(count)
}
