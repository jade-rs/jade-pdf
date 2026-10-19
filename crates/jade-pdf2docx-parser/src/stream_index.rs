//! 構造検査済み stream の範囲を、解析内の extractor へ共有する。
use std::{cmp::Reverse, ops::Range};

use crate::{decode::DecodeBudget, structure::StreamSpan};

/// 元入力を複製せず、辞書・payload の位置だけ保持する解析ローカルの索引。
pub(crate) struct StreamIndex {
    /// 元入力の識別子。解析中は呼び出し元が入力を保持する。
    source_ptr: usize,
    /// 合成入力や部分 slice を識別する元入力の長さ。
    source_len: usize,
    /// 入力順の stream 範囲。
    ranges: Vec<StreamRange>,
    /// 最新xrefが選択したstream。xrefのない入力では最後の出現位置。
    objects: Vec<(u32, usize)>,
    /// xref が指定した object header の位置。
    offsets: Vec<(usize, usize)>,
    /// 最新 revision の object 選択。xref のない互換入力では空。
    xref: Option<crate::xref::XrefIndex>,
    /// 最新 xref に選択されなかった通常 object の範囲。
    inactive: Vec<Range<usize>>,
}

/// 1 stream の元入力内位置。
struct StreamRange {
    /// 最外 dictionary の範囲。
    dictionary: Range<usize>,
    /// Length 検査済み payload の範囲。
    payload: Range<usize>,
    /// 所有 object 番号。
    object_number: Option<u32>,
    /// 所有 object header の位置。
    object_offset: Option<usize>,
}

impl StreamIndex {
    /// 同じ入力を借用した検査結果から、容量予算内で範囲索引を作る。
    pub(crate) fn new(
        budget: &DecodeBudget,
        bytes: &[u8],
        streams: &[StreamSpan<'_>],
    ) -> Option<Self> {
        let source_ptr = bytes.as_ptr() as usize;
        let mut index = Self {
            source_ptr,
            source_len: bytes.len(),
            ranges: budget.metadata_vector(streams.len())?,
            objects: budget.metadata_vector(
                streams
                    .iter()
                    .filter(|stream| stream.object_number.is_some())
                    .count(),
            )?,
            offsets: budget.metadata_vector(
                streams
                    .iter()
                    .filter(|stream| stream.object_offset.is_some())
                    .count(),
            )?,
            xref: None,
            inactive: Vec::new(),
        };
        for (position, stream) in streams.iter().enumerate() {
            let dictionary = stream.dictionary.as_ptr() as usize - source_ptr;
            let payload = stream.payload.as_ptr() as usize - source_ptr;
            index.ranges.push(StreamRange {
                dictionary: dictionary..dictionary + stream.dictionary.len(),
                payload: payload..payload + stream.payload.len(),
                object_number: stream.object_number,
                object_offset: stream.object_offset,
            });
            if let Some(number) = stream.object_number {
                index.objects.push((number, position));
            }
            if let Some(offset) = stream.object_offset {
                index.offsets.push((offset, position));
            }
        }
        index
            .objects
            .sort_unstable_by_key(|&(number, position)| (number, Reverse(position)));
        index.objects.dedup_by_key(|entry| entry.0);
        index
            .offsets
            .sort_unstable_by_key(|&(offset, position)| (offset, Reverse(position)));
        index.offsets.dedup_by_key(|entry| entry.0);
        index.select_latest_legacy_streams(bytes);
        Some(index)
    }

    /// 全objectの最終出現を走査し、非streamへの更新も予約済み番号配列へ反映する。
    fn select_latest_legacy_streams(&mut self, bytes: &[u8]) {
        if self.objects.is_empty() {
            return;
        }
        let mut objects = std::mem::take(&mut self.objects);
        for object in crate::metadata::objects(bytes, Some(self)) {
            if let Ok(entry) = objects.binary_search_by_key(&object.number, |entry| entry.0) {
                objects[entry].1 = self.offset_position(object.offset).unwrap_or(usize::MAX);
            }
        }
        objects.retain(|entry| entry.1 != usize::MAX);
        self.objects = objects;
    }

    /// 有効な通常 object の header だけを参照対象にし、除外範囲も容量予算内で保存する。
    pub(crate) fn activate(
        &mut self,
        budget: &DecodeBudget,
        bytes: &[u8],
        xref: Option<crate::xref::XrefIndex>,
    ) -> Option<()> {
        self.xref = xref;
        self.inactive.clear();
        if let Some(xref) = &self.xref {
            let count = crate::metadata::objects(bytes, Some(self))
                .filter(|object| !xref.selects(object.number, object.generation, object.offset))
                .count();
            let mut inactive = budget.metadata_vector(count)?;
            // 番号索引の予約済み容量を再利用し、選択時の追加確保を避ける。
            let mut objects = std::mem::take(&mut self.objects);
            objects.clear();
            for object in crate::metadata::objects(bytes, Some(self)) {
                if !xref.selects(object.number, object.generation, object.offset) {
                    inactive.push(object.offset..object.end);
                } else if let Some(position) = self.offset_position(object.offset)
                    && self.ranges[position].object_number == Some(object.number)
                {
                    objects.push((object.number, position));
                }
            }
            objects.sort_unstable_by_key(|&(number, position)| (number, Reverse(position)));
            objects.dedup_by_key(|entry| entry.0);
            self.objects = objects;
            self.inactive = inactive;
        }
        Some(())
    }

    /// 番号順に整列した索引から、選択済みstreamの位置を検索する。
    fn object_position(&self, number: u32) -> Option<usize> {
        let position = self
            .objects
            .binary_search_by_key(&number, |entry| entry.0)
            .ok()?;
        Some(self.objects[position].1)
    }

    /// header offset順に整列した索引からstreamの位置を検索する。
    fn offset_position(&self, offset: usize) -> Option<usize> {
        let position = self
            .offsets
            .binary_search_by_key(&offset, |entry| entry.0)
            .ok()?;
        Some(self.offsets[position].1)
    }

    /// 字句解析が確認した header が最新の通常 object か判定する。
    pub(crate) fn selects(&self, object: &crate::metadata::Object<'_>) -> bool {
        self.xref
            .as_ref()
            .is_none_or(|xref| xref.selects(object.number, object.generation, object.offset))
    }

    /// 参照に指定された世代を照合し、削除済み entry と不一致を除外する。
    pub(crate) fn reference(&self, value: &[u8]) -> Option<u32> {
        let (number, generation) = crate::metadata::reference_with_generation(value)?;
        self.xref
            .as_ref()
            .is_none_or(|xref| xref.references(number, generation))
            .then_some(number)
    }

    /// xref のある入力では、最新 revision の圧縮 object だけを返す。
    pub(crate) fn compressed(&self) -> Option<impl Iterator<Item = (u32, u32, u32)> + '_> {
        Some(self.xref.as_ref()?.compressed())
    }

    /// 正式 trailer の最新 catalog 参照を返す。
    pub(crate) fn root(&self) -> Option<(u32, u16)> {
        self.xref.as_ref()?.root()
    }

    /// 正式な xref がある場合は全 stream の本文抽出 fallback を止める。
    pub(crate) fn has_xref(&self) -> bool {
        self.xref.is_some()
    }

    /// 別世代・free・未登録の参照を PDF の null として合成 metadata に反映する。
    pub(crate) fn mask_references(&self, source: &mut [u8]) {
        let Some(xref) = &self.xref else {
            return;
        };
        let mut lexer = crate::syntax::Lexer {
            bytes: source,
            cursor: 0,
        };
        let mut missing = Vec::new();
        while let Some((token, span)) = lexer.next_spanned() {
            let crate::syntax::Token::Word(word) = token else {
                continue;
            };
            if crate::syntax::integer(word).is_none() {
                continue;
            }
            let mut reference = lexer.clone();
            if let Some(crate::syntax::Token::Word(generation)) = reference.next()
                && crate::syntax::integer(generation).is_some()
                && matches!(reference.next(), Some(crate::syntax::Token::Word(b"R")))
                && let Some((number, generation)) = crate::metadata::reference_with_generation(
                    &source[span.start..reference.cursor],
                )
            {
                if !xref.references(number, generation) {
                    missing.push(span.start..reference.cursor);
                }
                lexer = reference;
            }
        }
        for range in missing {
            source[range.clone()].fill(b' ');
            source[range.start..range.start + 4].copy_from_slice(b"null");
        }
    }

    /// 所有objectの最新本文だけを使い、xrefなしの無所属streamは保持する。
    pub(crate) fn active_stream(&self, stream: &StreamSpan<'_>) -> bool {
        match stream.object_number {
            Some(number) => self.object_position(number).is_some_and(|position| {
                self.ranges[position].object_offset == stream.object_offset
            }),
            None => self.xref.is_none(),
        }
    }

    /// 索引を構築した元入力か確認する。別 allocation には範囲を適用しない。
    pub(crate) fn matches(&self, bytes: &[u8]) -> bool {
        self.source_ptr == bytes.as_ptr() as usize && self.source_len == bytes.len()
    }

    /// object 番号の有効な stream を返す。
    pub(crate) fn for_object<'a>(
        &self,
        bytes: &'a [u8],
        number: u32,
    ) -> Option<(&'a [u8], &'a [u8])> {
        self.pair(bytes, self.object_position(number)?)
    }

    /// xref の object header 位置に一致する stream を返す。
    pub(crate) fn at_offset<'a>(
        &self,
        bytes: &'a [u8],
        offset: usize,
    ) -> Option<(&'a [u8], &'a [u8])> {
        self.pair(bytes, self.offset_position(offset)?)
    }

    /// 範囲に対応する辞書・payload を元入力から借用する。
    fn pair<'a>(&self, bytes: &'a [u8], position: usize) -> Option<(&'a [u8], &'a [u8])> {
        let range = self.ranges.get(position)?;
        Some((
            bytes.get(range.dictionary.clone())?,
            bytes.get(range.payload.clone())?,
        ))
    }

    /// 検査済み stream を入力順に走査する。
    pub(crate) fn streams<'a>(&'a self, bytes: &'a [u8]) -> impl Iterator<Item = StreamSpan<'a>> {
        self.ranges
            .iter()
            .map(move |range| StreamSpan {
                dictionary: &bytes[range.dictionary.clone()],
                payload: &bytes[range.payload.clone()],
                object_number: range.object_number,
                object_offset: range.object_offset,
            })
            .filter(|stream| self.active_stream(stream))
    }

    /// 指定 offset が本文内なら、その本文の終了位置を返す。
    pub(crate) fn payload_end_at(&self, offset: usize) -> Option<usize> {
        let index = self
            .ranges
            .partition_point(|range| range.payload.start <= offset)
            .checked_sub(1)?;
        let payload = &self.ranges[index].payload;
        (offset < payload.end).then_some(payload.end)
    }

    /// extractor 用の本文除外と無効参照の null 置換を行う。
    pub(crate) fn without_payloads(&self, budget: &DecodeBudget, bytes: &[u8]) -> Vec<u8> {
        let mut out = self.without_payloads_for_validation(budget, bytes);
        self.mask_references(&mut out);
        out
    }

    /// 構造検証用に本文と非採用 object を除き、不正参照は診断のため保持する。
    pub(crate) fn without_payloads_for_validation(
        &self,
        budget: &DecodeBudget,
        bytes: &[u8],
    ) -> Vec<u8> {
        let length = self
            .ranges
            .iter()
            .fold(bytes.len(), |length, range| length - range.payload.len());
        let Some(mut out) = budget.metadata_vector(length) else {
            return Vec::new();
        };
        let mut cursor = 0;
        let mut hidden = 0;
        for range in &self.ranges {
            append_active(
                bytes,
                cursor..range.payload.start,
                &self.inactive,
                &mut hidden,
                &mut out,
            );
            cursor = range.payload.end;
        }
        append_active(
            bytes,
            cursor..bytes.len(),
            &self.inactive,
            &mut hidden,
            &mut out,
        );
        out
    }
}

/// 除外 object を空白に置換し、stream 本文以外の有効部分を合成する。
fn append_active(
    bytes: &[u8],
    span: Range<usize>,
    inactive: &[Range<usize>],
    hidden: &mut usize,
    out: &mut Vec<u8>,
) {
    let start = out.len();
    out.extend_from_slice(&bytes[span.clone()]);
    while let Some(range) = inactive.get(*hidden) {
        if range.start >= span.end {
            break;
        }
        let overlap = range.start.max(span.start)..range.end.min(span.end);
        if overlap.start < overlap.end {
            out[start + overlap.start - span.start..start + overlap.end - span.start].fill(b' ');
        }
        if range.end > span.end {
            break;
        }
        *hidden += 1;
    }
}

#[cfg(test)]
#[path = "stream_index_budget_tests.rs"]
mod budget_tests;
