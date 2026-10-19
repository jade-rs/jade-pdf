//! Pdfiumだけへ渡すFont識別名。元入力・公開IRのFont名は変更しない。
use crate::{
    decode::DecodeBudget, metadata, metadata_workspace, resource_resolver::ResourceResolver,
};
use std::{collections::hash_map::RandomState, hash::BuildHasher, io::Write, ops::Range};

/// Pdfium専用の識別名に付ける固定prefix。
const MARKER_PREFIX: &[u8] = b"jade-pdf2docxType3_";
/// 二つの64bit seedと区切りの後に置くindexの開始位置。
const MARKER_INDEX_START: usize = MARKER_PREFIX.len() + 32 + 1;
/// prefix・二つのseed・区切り・indexを合わせたASCII名の長さ。
const MARKER_LEN: usize = MARKER_INDEX_START + 16;

/// 文字APIがSubtypeを公開しないType3を、解析ごとの識別名で判定する。
#[derive(Default)]
pub(crate) struct Tagged {
    /// 有効objectだけを再配置したPdfium専用入力。stream payloadはそのまま保存する。
    pub(crate) bytes: Vec<u8>,
    /// name末尾のindexで検索する、元のFont名と識別名。
    tags: Vec<Tag>,
}

/// Type3一辞書に付与する、予測困難な解析ローカルの名前。
struct Tag {
    /// 二つのrandom seedと辞書indexから作るASCII PDF name。
    marker: [u8; MARKER_LEN],
    /// 公開IRへ戻す元のBaseFont名。
    original: Option<String>,
}

/// 一つのobjectの借用本文と、Font名だけを置き換える範囲。
struct Object<'a> {
    /// 最新revisionのobject番号。
    number: u32,
    /// 有効参照に一致する世代。
    generation: u16,
    /// binary streamを含む元の本文。
    body: &'a [u8],
    /// 本文内のBaseFont valueまたは辞書終端への挿入位置。
    edits: Vec<(Range<usize>, usize)>,
}

impl Tagged {
    /// 識別名をindexで検索し、通常Fontの名前をType3へ誤対応させない。
    fn tag(&self, name: &str) -> Option<&Tag> {
        if name.len() != MARKER_LEN || !name.as_bytes().starts_with(MARKER_PREFIX) {
            return None;
        }
        let index = usize::from_str_radix(name.get(MARKER_INDEX_START..)?, 16).ok()?;
        let tag = self.tags.get(index)?;
        (name.as_bytes() == tag.marker).then_some(tag)
    }

    /// native文字のFontが元PDFでType3だったかを返す。
    pub(crate) fn is_type3(&self, name: &str) -> bool {
        self.tag(name).is_some()
    }

    /// 識別名を公開せず、元のFont名へ戻す。
    pub(crate) fn original_name<'a>(&'a self, name: &'a str) -> Option<&'a str> {
        match self.tag(name) {
            Some(tag) => tag.original.as_deref(),
            None => (!name.is_empty()).then_some(name),
        }
    }

    /// 元の検査済みstream境界と圧縮object cacheから、privateな入力を作る。
    /// 本文・編集位置・xref配列を全て共有metadata容量で制限する。
    pub(crate) fn prepare(budget: &DecodeBudget, bytes: &[u8]) -> Option<Self> {
        let source = crate::parser::parser_source_with_compressed_objects(budget, bytes);
        let objects = crate::object_index::build(budget, &source)?;
        let index = budget.stream_index(bytes)?;
        let compressed = budget.compressed_objects(bytes).unwrap_or_default();
        let mut resolver = ResourceResolver::new(&objects, budget);
        let random = RandomState::new();
        let seed = [random.hash_one(0u8), random.hash_one(1u8)];
        let mut result = Self::default();
        let mut raw = Vec::new();
        for object in metadata::objects(bytes, Some(&index)) {
            if !index.selects(&object) || objects.get(&object.number).is_none() {
                continue;
            }
            // Rootなし入力では同じ世代の古い本文もあり得る。合成索引の位置で後勝ちを守る。
            if !index.has_xref()
                && objects.get(&object.number).is_some_and(|(_, body)| {
                    body.as_ptr() as usize - source.as_ptr() as usize
                        != object.body.as_ptr() as usize - bytes.as_ptr() as usize
                })
            {
                continue;
            }
            let edits = edits(budget, object.body, &mut resolver, seed, &mut result.tags)?;
            metadata_workspace::try_push(
                budget,
                &mut raw,
                Object {
                    number: object.number,
                    generation: object.generation,
                    body: object.body,
                    edits,
                },
            )?;
        }
        for (number, body) in compressed.iter() {
            let edits = edits(budget, body, &mut resolver, seed, &mut result.tags)?;
            metadata_workspace::try_push(
                budget,
                &mut raw,
                Object {
                    number: *number,
                    generation: 0,
                    body,
                    edits,
                },
            )?;
        }
        if result.tags.is_empty() {
            return Some(result);
        }
        raw.sort_unstable_by_key(|object| object.number);
        let length = raw.iter().fold(256usize, |length, object| {
            length
                .saturating_add(object.body.len())
                .saturating_add(96)
                .saturating_add(
                    object
                        .edits
                        .len()
                        .saturating_mul(MARKER_LEN + b" /BaseFont / ".len()),
                )
        });
        result.bytes = budget.metadata_vector(length)?;
        result.bytes.extend_from_slice(b"%PDF-1.7\n");
        let mut offsets = budget.metadata_vector(raw.len())?;
        for object in &raw {
            offsets.push((object.number, object.generation, result.bytes.len()));
            writeln!(result.bytes, "{} {} obj", object.number, object.generation).ok()?;
            let mut cursor = 0;
            for (range, tag) in &object.edits {
                result
                    .bytes
                    .extend_from_slice(&object.body[cursor..range.start]);
                if range.is_empty() {
                    result.bytes.extend_from_slice(b" /BaseFont ");
                }
                result.bytes.push(b'/');
                result.bytes.extend_from_slice(&result.tags[*tag].marker);
                result.bytes.push(b' ');
                cursor = range.end;
            }
            result.bytes.extend_from_slice(&object.body[cursor..]);
            result.bytes.extend_from_slice(b"\nendobj\n");
        }
        let start = result.bytes.len();
        result
            .bytes
            .extend_from_slice(b"xref\n0 1\n0000000000 65535 f \n");
        // sparseな番号を連続配列へ展開しない。
        for (number, generation, offset) in offsets {
            write!(
                result.bytes,
                "{number} 1\n{offset:010} {generation:05} n \n"
            )
            .ok()?;
        }
        let size = raw.last()?.number as u64 + 1;
        write!(result.bytes, "trailer\n<< /Size {size}").ok()?;
        let root = index.root().or_else(|| {
            raw.iter()
                .find(|object| metadata::has_type(object.body, b"Catalog"))
                .map(|object| (object.number, object.generation))
        });
        if let Some((number, generation)) = root {
            write!(result.bytes, " /Root {number} {generation} R").ok()?;
        }
        write!(result.bytes, " >>\nstartxref\n{start}\n%%EOF\n").ok()?;
        Some(result)
    }
}

/// 固定容量のASCII名を作り、敵対的な元Font名との衝突をrandom seedで避ける。
fn marker(seed: [u64; 2], index: usize) -> [u8; MARKER_LEN] {
    let mut name = [b'_'; MARKER_LEN];
    name[..MARKER_PREFIX.len()].copy_from_slice(MARKER_PREFIX);
    for (value, start) in [
        (seed[0], MARKER_PREFIX.len()),
        (seed[1], MARKER_PREFIX.len() + 16),
        (index as u64, MARKER_INDEX_START),
    ] {
        for digit in 0..16 {
            name[start + digit] =
                b"0123456789abcdef"[((value >> (4 * (15 - digit))) & 15) as usize];
        }
    }
    name
}

/// 最外辞書と入れ子の直接Fontだけを走査し、streamのbinary本文は読まない。
fn edits<'a>(
    budget: &DecodeBudget,
    body: &'a [u8],
    resolver: &mut ResourceResolver<'_, 'a>,
    seed: [u64; 2],
    tags: &mut Vec<Tag>,
) -> Option<Vec<(Range<usize>, usize)>> {
    let Some(dictionary) = metadata::dictionary_iter(body).next() else {
        return Some(Vec::new());
    };
    let base = dictionary.as_ptr() as usize - body.as_ptr() as usize;
    let mut lexer = crate::syntax::Lexer {
        bytes: dictionary,
        cursor: 0,
    };
    let mut starts = Vec::new();
    let mut edits = Vec::new();
    while let Some((token, span)) = lexer.next_spanned() {
        match token {
            crate::syntax::Token::Open(true) => {
                budget.check_form_depth(starts.len() + 1)?;
                metadata_workspace::try_push(budget, &mut starts, span.start)?;
            }
            crate::syntax::Token::Close(true) => {
                let start = starts.pop()?;
                let current = &dictionary[start..span.end];
                // 間接Subtypeも、最新object世代を検査したresolverで解決する。
                let type3 = metadata::value(current, b"Subtype")
                    .and_then(|value| resolver.resolve(value))
                    .is_some_and(|(value, _)| metadata::is_name(value, b"Type3"));
                if !type3 {
                    continue;
                }
                let range = metadata::value(current, b"BaseFont").map_or(
                    base + span.start..base + span.start,
                    |value| {
                        let start = value.as_ptr() as usize - body.as_ptr() as usize;
                        start..start + value.len()
                    },
                );
                let tag = tags.len();
                let original = metadata::value(current, b"BaseFont")
                    .and_then(|value| resolver.resolve(value))
                    .and_then(|(value, _)| crate::metadata_names::value(budget, value));
                if budget.is_stopped() {
                    return None;
                }
                metadata_workspace::try_push(
                    budget,
                    tags,
                    Tag {
                        marker: marker(seed, tag),
                        original,
                    },
                )?;
                metadata_workspace::try_push(budget, &mut edits, (range, tag))?;
            }
            _ => {}
        }
    }
    edits.sort_unstable_by_key(|(range, _)| range.start);
    // 外側BaseFontの不正な辞書値を置き換える場合、その内部の編集は不要となる。
    let mut end = 0;
    edits.retain(|(range, _)| {
        if range.start < end {
            return false;
        }
        end = range.end;
        true
    });
    Some(edits)
}
