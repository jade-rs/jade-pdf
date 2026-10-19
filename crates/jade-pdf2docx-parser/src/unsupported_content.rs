//! 実行された未対応描画を品質 report へ記録する。
use crate::{
    QualityIssue, QualityReport, UnsupportedXObjectReason,
    decode::DecodeBudget,
    metadata,
    metadata_workspace::{Set, try_push},
    page_attributes::Objects,
    parser,
    resource_resolver::ResourceResolver,
    syntax::{Lexer, Token},
};
#[path = "unsupported_references.rs"]
mod references;
use references::References;

/// Contents 配列の字句境界をまたぐページ単位の描画診断状態。
#[derive(Default)]
struct ContentState<'a> {
    /// 次の Do の operand になり得る直前の resource 名。
    previous_name: Option<&'a [u8]>,
    /// 偽 operator を除外する配列・辞書の入れ子深度。
    depth: usize,
    /// inline image 本文の誤認を避けるため、このページの走査を終了したか。
    stopped: bool,
}

#[cfg(test)]
#[path = "unsupported_content_budget_tests.rs"]
mod budget_tests;

#[cfg(test)]
#[path = "unsupported_content_scan_tests.rs"]
mod scan_tests;

/// 有効な所属ページの content と resource を照合し、未対応描画を一度ずつ報告する。
pub(crate) fn collect(budget: &DecodeBudget, bytes: &[u8], report: &mut QualityReport) {
    let source = parser::parser_source_with_compressed_objects(budget, bytes);
    let Some(references) = References::new(
        budget,
        parser::page_xobject_refs_from_source(budget, &source),
    ) else {
        return;
    };
    let Some(objects) = crate::object_index::build(budget, &source) else {
        return;
    };
    let contents = parser::extract_page_content_streams_with_page(budget, bytes);
    scan(budget, &objects, &references, &contents, report);
}

/// 準備済み本文を借用して走査し、状態・集合・出力の全拡張容量を予約する。
fn scan(
    budget: &DecodeBudget,
    objects: &Objects<'_>,
    references: &References,
    contents: &[(usize, Vec<u8>)],
    report: &mut QualityReport,
) {
    let mut resolver = ResourceResolver::new(objects, budget);
    let mut seen = Set::default();
    let mut states = Vec::new();
    for (page, content) in contents {
        let page = *page;
        let mut lexer = Lexer {
            bytes: content,
            cursor: 0,
        };
        let index = match states.binary_search_by_key(&page, |(page, _)| *page) {
            Ok(index) => index,
            Err(index) => {
                if try_push(budget, &mut states, (page, ContentState::default())).is_none() {
                    return;
                }
                states[index..].rotate_right(1);
                index
            }
        };
        let state = &mut states[index].1;
        if budget.is_stopped() {
            return;
        }
        if state.stopped {
            continue;
        }
        while let Some(token) = lexer.next() {
            match token {
                Token::Open(_) => {
                    state.depth = state.depth.saturating_add(1);
                    state.previous_name = None;
                }
                Token::Close(_) => {
                    state.depth = state.depth.saturating_sub(1);
                    state.previous_name = None;
                }
                _ if state.depth != 0 => {}
                Token::Word(b"BI") => {
                    state.stopped = true;
                    if try_push(
                        budget,
                        &mut report.issues,
                        QualityIssue::UnsupportedInlineImage { page },
                    )
                    .is_none()
                    {
                        return;
                    }
                    // inline image に正式な Length はない。binary を字句と誤認しないため止める。
                    break;
                }
                Token::Name(raw) => state.previous_name = Some(raw),
                Token::Word(b"Do") => {
                    if let Some(name) = state.previous_name.take()
                        && let Some(number) = references.get(page, name)
                        && let Some(&(_, dictionary)) = objects.get(&number)
                        && !parser::is_image_dictionary(dictionary)
                    {
                        let Some(new) = seen.try_insert(budget, (page, number)) else {
                            return;
                        };
                        if !new {
                            continue;
                        }
                        let reason = metadata::value(dictionary, b"Subtype")
                            .and_then(|value| resolver.resolve(value))
                            .filter(|(value, _)| metadata::is_name(value, b"Form"))
                            .map_or(UnsupportedXObjectReason::Subtype, |_| {
                                UnsupportedXObjectReason::Form
                            });
                        if try_push(
                            budget,
                            &mut report.issues,
                            QualityIssue::UnsupportedXObject {
                                page,
                                object_number: number,
                                reason,
                            },
                        )
                        .is_none()
                        {
                            return;
                        }
                    }
                }
                _ => state.previous_name = None,
            }
        }
    }
}
