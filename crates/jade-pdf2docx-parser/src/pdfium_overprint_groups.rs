//! 同じForm内で完全に重なる、確認済みfontの描画object候補を保存する。
use crate::{ParseError, decode::DecodeBudget, metadata_workspace, pdfium_font_tags::Tagged};
use pdfium_render::prelude::*;

/// 個別に読み直す元object。fontのfamily名を識別子にしない。
pub(crate) struct Target {
    /// ページ内を深さ優先で数えたtext objectの順番。
    pub(crate) ordinal: usize,
    /// 解析ローカルの確認済みfont tag index。
    pub(crate) source_key: usize,
    /// 元FontDescriptorが宣言した整数weight。
    pub(crate) weight: u16,
}

/// 同じ局所矩形とfont sizeを持つ、まだ回復採用前のobject候補群。
pub(crate) struct Group {
    /// 元のpaint順に保持する描画object。
    pub(crate) targets: Vec<Target>,
}

/// 候補の収集だけを行い、Unicodeや幾何の回復成功とは扱わない。
pub(crate) fn collect<'a>(
    budget: &DecodeBudget,
    objects: &'a impl PdfPageObjectsCommon<'a>,
    tags: &Tagged,
) -> Result<Vec<Group>, ParseError> {
    metadata_workspace::ensure_active(budget)?;
    if !tags.has_weights() {
        return Ok(Vec::new());
    }
    let mut scan = Scan {
        entries: Vec::new(),
        next_ordinal: 0,
        next_context: 1,
        valid: true,
    };
    scan.visit(budget, objects, tags, 0, 0)?;
    if !scan.valid {
        return Ok(Vec::new());
    }
    scan.entries
        .sort_unstable_by(|a, b| a.key.cmp(&b.key).then(a.ordinal.cmp(&b.ordinal)));
    metadata_workspace::ensure_active(budget)?;
    let mut groups = Vec::new();
    let mut start = 0;
    while start < scan.entries.len() {
        metadata_workspace::ensure_active(budget)?;
        let mut end = start + 1;
        while end < scan.entries.len() && scan.entries[end].key == scan.entries[start].key {
            end += 1;
        }
        let entries = &scan.entries[start..end];
        if entries.len() > 1 && entries.iter().all(|entry| entry.source.is_some()) {
            let mut targets = metadata_workspace::vector(budget, entries.len())?;
            for entry in entries {
                let (source_key, weight) = entry.source.unwrap();
                targets.push(Target {
                    ordinal: entry.ordinal,
                    source_key,
                    weight,
                });
            }
            metadata_workspace::push(budget, &mut groups, Group { targets })?;
        }
        start = end;
    }
    Ok(groups)
}

/// 異なるForm contextを、同じ局所矩形だけで混同しない完全一致key。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    /// ページを深さ優先で走査した、Form containerの解析ローカル番号。
    context: usize,
    /// source text objectの局所包絡矩形のfloat bit列。
    bounds: [u32; 4],
    /// SDKが重複判定に使う未変換font sizeのbit列。
    size: u32,
}

/// 未確認のobjectも残し、同じ矩形の既知fontだけを誤って回復しない。
struct Entry {
    /// 完全一致した局所contextと幾何。
    key: Key,
    /// 元のpaint順。
    ordinal: usize,
    /// SDK埋め込み確認済みのtag indexと宣言weight。未確認・不可視はNone。
    source: Option<(usize, u16)>,
}

/// 元の順番とcontextを共有し、追加配列を全てmetadata予算へ予約する。
struct Scan {
    /// 未確認objectも含む、コピーしない識別用の観察値。
    entries: Vec<Entry>,
    /// 次のtext objectのpaint順番号。
    next_ordinal: usize,
    /// 次のForm context番号。
    next_context: usize,
    /// 幾何を確認できないobjectがあれば候補の採用を止める。
    valid: bool,
}

impl Scan {
    /// Formを深さ予算内で辿り、sourceのfont tagと未変換の局所幾何を保存する。
    fn visit<'a>(
        &mut self,
        budget: &DecodeBudget,
        objects: &'a impl PdfPageObjectsCommon<'a>,
        tags: &Tagged,
        context: usize,
        depth: usize,
    ) -> Result<(), ParseError> {
        let _ = budget.check_form_depth(depth);
        budget.check()?;
        metadata_workspace::ensure_active(budget)?;
        for object in objects.iter() {
            metadata_workspace::ensure_active(budget)?;
            if !self.valid {
                break;
            }
            if let Some(text) = object.as_text_object() {
                let ordinal = self.next_ordinal;
                self.next_ordinal += 1;
                let Ok(quad) = text.bounds() else {
                    self.valid = false;
                    break;
                };
                let rect = quad.to_rect();
                let bounds = [
                    rect.left().value,
                    rect.bottom().value,
                    rect.right().value,
                    rect.top().value,
                ];
                let size = text.unscaled_font_size().value;
                if !size.is_finite() || !bounds.iter().all(|value| value.is_finite()) {
                    self.valid = false;
                    break;
                }
                let name = text.font().name();
                let source = if crate::pdfium_visible_text::hidden_object(text, tags) {
                    None
                } else {
                    tags.source_key(&name).zip(tags.weight(&name))
                };
                metadata_workspace::push(
                    budget,
                    &mut self.entries,
                    Entry {
                        key: Key {
                            context,
                            bounds: bounds.map(f32::to_bits),
                            size: size.to_bits(),
                        },
                        ordinal,
                        source,
                    },
                )?;
            } else if let Some(form) = object.as_x_object_form_object() {
                let child = self.next_context;
                self.next_context += 1;
                self.visit(budget, form, tags, child, depth + 1)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParseLimits;

    /// 所有CDF対照を同じSDK文書へ渡し、確認済みtagと借用ページを組み合わせる。
    fn with_page(check: impl FnOnce(&PdfPage<'_>, &Tagged)) {
        let bytes = crate::pdfium_subset_native_tests::identical_overprint_source();
        let budget = DecodeBudget::default();
        crate::parser::parse_pdf_bytes_pure_with_budget(
            "overprint.pdf".into(),
            &bytes,
            &ParseLimits::default(),
            &budget,
        )
        .unwrap();
        let mut tags = Tagged::prepare_weights(&budget, &bytes, false).unwrap();
        let outcome = {
            let sdk = pdfium_auto::bind_pdfium_silent().unwrap();
            let confirmed = tags.confirm_embedded(&budget, &sdk, 1000).unwrap();
            assert!(confirmed);
            let doc = sdk.load_pdf_from_byte_slice(&tags.bytes, None).unwrap();
            let page = doc.pages().get(0).unwrap();
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check(&page, &tags)))
        };
        if let Err(error) = outcome {
            std::panic::resume_unwind(error);
        }
    }

    /// 同じfamily・位置でも別source resourceであることを保ち、元paint順を固定する。
    #[test]
    fn exact_overprint_group_preserves_distinct_source_resources_and_order() {
        with_page(|page, tags| {
            let groups = collect(&DecodeBudget::default(), page.objects(), tags).unwrap();
            assert_eq!(groups.len(), 1);
            assert_eq!(
                groups[0]
                    .targets
                    .iter()
                    .map(|t| (t.ordinal, t.weight))
                    .collect::<Vec<_>>(),
                vec![(0, 400), (1, 700)]
            );
            assert_ne!(
                groups[0].targets[0].source_key,
                groups[0].targets[1].source_key
            );
        });
    }

    /// 新しい候補配列も容量を確保前に予約し、元の上限エラーを返す。
    #[test]
    fn overprint_group_storage_preserves_original_budget_failure() {
        with_page(|page, tags| {
            let budget = DecodeBudget::new(&ParseLimits {
                max_total_metadata_workspace_bytes: 0,
                ..Default::default()
            });
            assert!(matches!(
                collect(&budget, page.objects(), tags),
                Err(ParseError::ResourceLimit {
                    resource: "metadata workspace bytes",
                    limit: 0,
                    ..
                })
            ));
        });
    }
}
