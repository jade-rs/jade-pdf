//! 他objectをSDK内だけで不可視化し、選択元objectの一文字だけを独立に確認する。
use super::{Tagged, Target};
use crate::{ParseError, ParsedTextElement, decode::DecodeBudget, metadata_workspace};
use pdfium_render::prelude::*;

/// 元入力を変更せず、sourceの識別子・weight・文字を確認した場合だけ一文字を返す。
pub(crate) fn extract(
    budget: &DecodeBudget,
    sdk: &Pdfium,
    bytes: &[u8],
    page: usize,
    target: &Target,
    tags: &Tagged,
    path: &std::path::Path,
) -> Result<Option<ParsedTextElement>, ParseError> {
    metadata_workspace::ensure_active(budget)?;
    // opaqueなSDK再解析の処理量を、入力長の累積要求量で保守的に制限する。
    let _ = budget.charge_metadata_workspace_bytes(bytes.len() as u64);
    budget.check()?;
    let document = sdk
        .load_pdf_from_byte_slice(bytes, None)
        .map_err(|error| sdk_error(path, error))?;
    let index = page
        .try_into()
        .map_err(|_| sdk_error(path, "overprint page index overflow"))?;
    let native_page = document
        .pages()
        .get(index)
        .map_err(|error| sdk_error(path, error))?;
    crate::pdfium_clip_overlap::check(budget, path, native_page.objects(), tags)?;
    crate::pdfium_visible_text::suppress_hidden(budget, path, native_page.objects(), tags, 0)?;
    let mut state = Suppression {
        target,
        tags,
        ordinal: 0,
        found: false,
        valid: true,
    };
    state.visit(budget, path, native_page.objects(), 0)?;
    if !state.found || !state.valid {
        return Ok(None);
    }
    let text = native_page.text().map_err(|error| sdk_error(path, error))?;
    let characters = crate::pdfium_text::characters(budget, &text)?;
    let excluded = crate::pdfium_clip::excluded(budget, native_page.objects(), &text)?;
    let mut observed = 0;
    for character in characters.iter() {
        metadata_workspace::ensure_active(budget)?;
        if excluded.get(character.index()).copied().unwrap_or(false)
            || matches!(
                character.render_mode().ok(),
                Some(PdfPageTextRenderMode::Invisible)
            )
        {
            continue;
        }
        let Some(ch) = char::from_u32(character.unicode_value()) else {
            return Ok(None);
        };
        if ch.is_whitespace() {
            if character.text_object().is_ok() {
                return Ok(None);
            }
            continue;
        }
        if tags.source_key(&character.font_name()) != Some(target.source_key) {
            return Ok(None);
        }
        observed += 1;
    }
    if observed != 1 {
        return Ok(None);
    }
    let (_, mut elements) =
        crate::pdfium_visible_text::extract(budget, path, page, &characters, tags, &excluded)?;
    if elements.len() != 1 {
        return Ok(None);
    }
    let mut element = elements.pop().unwrap();
    let b = element.bounds;
    if !element.font_size.is_finite()
        || ![b.x0, b.y0, b.x1, b.y1]
            .iter()
            .all(|value| value.is_finite())
        || b.x0 >= b.x1
        || b.y0 >= b.y1
    {
        return Ok(None);
    }
    let trimmed = element.text.trim_end();
    if trimmed.chars().count() != 1
        || trimmed
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
        || element.font_weight != Some(target.weight)
    {
        return Ok(None);
    }
    element.text.truncate(trimmed.len());
    Ok(Some(element))
}

/// 選択元objectをそのまま残し、他objectのUnicodeを回復結果へ混ぜない。
struct Suppression<'a> {
    /// 元のpaint順と確認済みresourceを持つ対象。
    target: &'a Target,
    /// 元の名前を復元する解析ローカルtag。
    tags: &'a Tagged,
    /// 次のtext objectの元の順番。
    ordinal: usize,
    /// 指定された元objectが実在したか。
    found: bool,
    /// 対象の元resource・weight・描画・埋め込みを再確認できたか。
    valid: bool,
}

impl Suppression<'_> {
    /// 元のForm階層とmatrixを保ったまま、対象外だけをSDK内で不可視化する。
    fn visit<'a>(
        &mut self,
        budget: &DecodeBudget,
        path: &std::path::Path,
        objects: &'a impl PdfPageObjectsCommon<'a>,
        depth: usize,
    ) -> Result<(), ParseError> {
        let _ = budget.check_form_depth(depth);
        budget.check()?;
        metadata_workspace::ensure_active(budget)?;
        for mut object in objects.iter() {
            metadata_workspace::ensure_active(budget)?;
            if let Some(text) = object.as_text_object_mut() {
                let current = self.ordinal;
                self.ordinal += 1;
                let name = text.font().name();
                if current == self.target.ordinal {
                    self.found = true;
                    self.valid = self.tags.source_key(&name) == Some(self.target.source_key)
                        && self.tags.weight(&name) == Some(self.target.weight)
                        && text.font().is_embedded().is_ok_and(|value| value)
                        && !crate::pdfium_visible_text::hidden_object(text, self.tags);
                } else {
                    // 確認済みType1Cだけ文字列を変更し、未知fontやType3を再符号化しない。
                    if self.tags.weight(&name).is_some() {
                        text.set_text("").map_err(|error| sdk_error(path, error))?;
                    }
                    text.set_render_mode(PdfPageTextRenderMode::Invisible)
                        .map_err(|error| sdk_error(path, error))?;
                }
            } else if let Some(form) = object.as_x_object_form_object() {
                self.visit(budget, path, form, depth + 1)?;
            }
        }
        Ok(())
    }
}

/// probe失敗を元文書のpath付きで返し、SDK専用入力や識別名を公開しない。
fn sdk_error(path: &std::path::Path, error: impl std::fmt::Display) -> ParseError {
    ParseError::Pdfium {
        path: path.to_path_buf(),
        message: format!("overprint object read failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParseLimits;

    /// 同じfamily・位置のobjectを別々に読み、確認済みsource weightとAAを保持する。
    #[test]
    fn isolated_source_objects_recover_both_owned_unicode_and_weights() {
        super::super::tests::with_page(|sdk, page, tags| {
            let groups =
                super::super::collect(&DecodeBudget::default(), page.objects(), tags).unwrap();
            let mut elements = Vec::new();
            for target in &groups[0].targets {
                elements.push(
                    extract(
                        &DecodeBudget::default(),
                        sdk,
                        &tags.bytes,
                        0,
                        target,
                        tags,
                        std::path::Path::new("overprint.pdf"),
                    )
                    .unwrap()
                    .unwrap(),
                );
            }
            assert_eq!(
                elements.iter().map(|e| e.text.as_str()).collect::<String>(),
                "AA"
            );
            assert_eq!(
                elements.iter().map(|e| e.font_weight).collect::<Vec<_>>(),
                vec![Some(400), Some(700)]
            );
            assert!(
                elements
                    .iter()
                    .all(|e| e.font_name.as_deref() == Some("OwnedSubset"))
            );
            assert_eq!(elements[0].bounds, elements[1].bounds);
        });
    }

    /// SDKの再読込みも入力長を処理量の代理値として予約し、上限超過のまま進めない。
    #[test]
    fn isolated_source_reload_obeys_original_workspace_failure() {
        super::super::tests::with_page(|sdk, page, tags| {
            let groups =
                super::super::collect(&DecodeBudget::default(), page.objects(), tags).unwrap();
            let budget = DecodeBudget::new(&ParseLimits {
                max_total_metadata_workspace_bytes: 0,
                ..Default::default()
            });
            assert!(matches!(
                extract(
                    &budget,
                    sdk,
                    &tags.bytes,
                    0,
                    &groups[0].targets[0],
                    tags,
                    std::path::Path::new("overprint.pdf")
                ),
                Err(ParseError::ResourceLimit {
                    resource: "metadata workspace bytes",
                    limit: 0,
                    ..
                })
            ));
        });
    }

    /// 同名familyの別resourceや別weightを渡しても、選択objectの確認値へ取り違えない。
    #[test]
    fn isolated_glyph_rejects_another_resource_or_weight() {
        super::super::tests::with_page(|sdk, page, tags| {
            let groups =
                super::super::collect(&DecodeBudget::default(), page.objects(), tags).unwrap();
            let first = &groups[0].targets[0];
            let other = &groups[0].targets[1];
            for (source_key, weight) in [
                (other.source_key, first.weight),
                (first.source_key, other.weight),
            ] {
                let target = Target {
                    ordinal: first.ordinal,
                    source_key,
                    weight,
                };
                assert!(
                    extract(
                        &DecodeBudget::default(),
                        sdk,
                        &tags.bytes,
                        0,
                        &target,
                        tags,
                        std::path::Path::new("overprint.pdf")
                    )
                    .unwrap()
                    .is_none()
                );
            }
        });
    }

    /// family名と埋め込みが同じでも、解析ローカルの識別名がない入力から値を推測しない。
    #[test]
    fn isolated_glyph_never_matches_an_untagged_family_name() {
        super::super::tests::with_page(|sdk, page, tags| {
            let groups =
                super::super::collect(&DecodeBudget::default(), page.objects(), tags).unwrap();
            let original = crate::pdfium_subset_native_tests::identical_overprint_source();
            assert!(
                extract(
                    &DecodeBudget::default(),
                    sdk,
                    &original,
                    0,
                    &groups[0].targets[0],
                    tags,
                    std::path::Path::new("overprint.pdf")
                )
                .unwrap()
                .is_none()
            );
        });
    }
}
