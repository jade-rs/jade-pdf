//! 実Font辞書ごとの生成fontを解析内で共有し、未知codeには元font名のfallbackを残す。
use crate::{
    ParsedEmbeddedFont,
    decode::DecodeBudget,
    metadata,
    page_attributes::Objects,
    resource_resolver::ResourceResolver,
    text::ToUnicodeCMap,
    type1_outline::{self, Input, Prepared, resource_field},
};
use std::sync::Arc;

/// 同じFont辞書を使うページ・Formが共有する、確認済みのcode別選択。
#[derive(Debug)]
pub(crate) struct Routes {
    /// 原本resource内の各生成faceの派生family名。
    families: Vec<String>,
    /// 元byte codeのface index。未確認は元のfontへ戻す。
    pub(super) codes: [Option<u16>; 256],
    /// 実glyphに対応した単byte原文表。元の未知codeを補完しない。
    pub(super) cmap: ToUnicodeCMap,
}
impl Routes {
    /// 合字用faceを通常文字へ混用せず、検証済みindexだけを選ぶ。
    pub(super) fn family(&self, index: Option<u16>) -> Option<&str> {
        Some(self.families.get(usize::from(index?))?.as_str())
    }
}

/// 検証済み文書が変わると全破棄する、font生成と選択結果の所有cache。
#[derive(Default)]
pub(crate) struct Cache {
    /// 同じ文書のFont辞書全文の指紋。参照先は検証済みの不変object graphで解決する。
    entries: crate::metadata_map::Map<[u8; 32], Option<Arc<Routes>>>,
    /// 実際に文書へ渡す完成font。本体を複製せず最終結果へ移す。
    fonts: Vec<ParsedEmbeddedFont>,
}
impl Cache {
    /// 中止時はhitも公開せず、新規確保は完成結果を保存する直前まで検査する。
    fn get_or_build(
        &mut self,
        budget: &DecodeBudget,
        key: [u8; 32],
        build: impl FnOnce() -> Option<Prepared>,
    ) -> Option<Arc<Routes>> {
        if budget.is_stopped() {
            return None;
        }
        if let Some(found) = self.entries.get(&key) {
            return found.clone();
        }
        self.entries.prepare(budget, &key)?;
        let prepared = build();
        if budget.is_stopped() {
            return None;
        }
        let Some(prepared) = prepared else {
            self.entries.insert(budget, key, None)?;
            return None;
        };
        let extra = prepared
            .fonts
            .iter()
            .filter(|f| !self.fonts.iter().any(|old| old.font_name == f.font_name))
            .count();
        let count = self.fonts.len().checked_add(extra)?;
        budget.check_embedded_font_count(count)?;
        let mut families = budget.metadata_vector(prepared.fonts.len())?;
        for font in &prepared.fonts {
            families.push(crate::metadata_map::copy(budget, &font.font_name)?);
        }
        budget.charge_resource_workspace_bytes(
            (std::mem::size_of::<Routes>() + 2 * std::mem::size_of::<usize>()) as u64,
        )?;
        let routes = budget.budgeted_arc(Routes {
            families,
            codes: prepared.routes,
            cmap: prepared.cmap,
        })?;
        let mut fonts = budget.metadata_vector(count)?;
        fonts.append(&mut self.fonts);
        for font in prepared.fonts {
            if !fonts.iter().any(|old| old.font_name == font.font_name) {
                fonts.push(font);
            }
        }
        self.entries
            .insert(budget, key, Some(Arc::clone(&routes)))?;
        self.fonts = fonts;
        Some(routes)
    }

    /// 完成fontの所有権だけを出力へ移し、選択情報は残りの同文書走査に保つ。
    pub(crate) fn take_fonts(&mut self) -> Vec<ParsedEmbeddedFont> {
        std::mem::take(&mut self.fonts)
    }
}

/// 現行graphのType1/FontFileだけを生成し、FontFile2・未知権利・非対応programを混ぜない。
pub(super) fn for_dictionary<'bytes>(
    budget: &DecodeBudget,
    bytes: &[u8],
    dictionary: &'bytes [u8],
    objects: &Objects<'bytes>,
    offsets: &super::StreamOffsets,
) -> Option<Arc<Routes>> {
    let mut resolver = ResourceResolver::new(objects, budget);
    let subtype = resource_field(budget, dictionary, b"Subtype", &mut resolver)??;
    if !metadata::is_name(subtype, b"Type1") {
        return None;
    }
    let descriptor = resource_field(budget, dictionary, b"FontDescriptor", &mut resolver)??;
    let file = resource_field(budget, descriptor, b"FontFile", &mut resolver)??;
    let file = metadata::dictionary_iter(file).next()?;
    let (_, number) = resolver.resolve(metadata::value(descriptor, b"FontFile")?)?;
    let number = number?;
    let mut hash = hmac_sha256::Hash::new();
    for chunk in dictionary.chunks(4096) {
        if budget.is_stopped() {
            return None;
        }
        hash.update(chunk);
    }
    let key = hash.finalize();
    budget.with_type1_output(|cache| {
        cache.get_or_build(budget, key, || {
            let length1 =
                crate::syntax::integer(resource_field(budget, file, b"Length1", &mut resolver)??)?;
            let length2 =
                crate::syntax::integer(resource_field(budget, file, b"Length2", &mut resolver)??)?;
            let program = super::embedded_font_payload(budget, bytes, number, offsets)?;
            let cmap = match resource_field(budget, dictionary, b"ToUnicode", &mut resolver)? {
                None => ToUnicodeCMap::default(),
                Some(_) => {
                    let (_, number) =
                        resolver.resolve(metadata::value(dictionary, b"ToUnicode")?)?;
                    super::font_cmap_for_object_number(budget, bytes, number?, offsets)?
                }
            };
            type1_outline::prepare(
                budget,
                Input {
                    program: &program,
                    length1,
                    length2,
                    cmap: &cmap,
                },
                dictionary,
                descriptor,
                &mut resolver,
            )
        })
    })
}
