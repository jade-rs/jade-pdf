#[cfg(test)]
#[path = "xref_budget_tests.rs"]
mod budget_tests;

#[path = "xref_storage.rs"]
mod storage;
use std::path::Path;
use storage::{History, RevisionEntries};

use crate::{
    ParseError, ParseLimits,
    limits::check_limit,
    syntax::{Lexer, Token, integer, name_is},
};

/// 最新 revision を優先した、xref table/stream の参照先。
pub(crate) struct XrefIndex {
    /// 共有metadata容量を予約した全履歴と、旧offsetの参照時点。
    history: History,
    /// 最新 trailer に指定された catalog 参照。省略時は過去 revision から継承する。
    root: Option<(u32, u16)>,
}

impl XrefIndex {
    /// 最新 revision の catalog 参照を返す。
    pub(crate) fn root(&self) -> Option<(u32, u16)> {
        self.root
    }

    /// 通常 object の header 位置と世代が最新の有効 entry に一致するか確認する。
    pub(crate) fn selects(&self, number: u32, generation: u16, offset: usize) -> bool {
        matches!(self.entry_at(number as usize, 0), Some(Entry::Uncompressed { generation: selected_generation, offset: selected_offset }) if *selected_generation == usize::from(generation) && *selected_offset == offset)
    }

    /// 間接参照の世代が最新の有効 entry に一致するか確認する。
    pub(crate) fn references(&self, number: u32, generation: u16) -> bool {
        match self.entry_at(number as usize, 0) {
            Some(Entry::Uncompressed {
                generation: selected,
                ..
            }) => *selected == usize::from(generation),
            Some(Entry::Compressed { .. }) => generation == 0,
            _ => false,
        }
    }

    /// 最新 revision に残る圧縮 object と所属 stream・member index を列挙する。
    pub(crate) fn compressed(&self) -> impl Iterator<Item = (u32, u32, u32)> + '_ {
        self.history.compressed()
    }

    /// 最後の startxref から table/stream、hybrid XRefStm、Prev chain を上限付きで読む。
    /// xref のない旧形式はここでは対象外とする。
    pub(crate) fn read(
        path: &Path,
        bytes: &[u8],
        limits: &ParseLimits,
        budget: &crate::decode::DecodeBudget,
    ) -> Result<Option<Self>, ParseError> {
        crate::metadata_workspace::ensure_active(budget)?;
        let Some(start) = bytes.windows(9).rposition(|word| word == b"startxref") else {
            return Ok(None);
        };
        let mut tail = Lexer {
            bytes,
            cursor: start + 9,
        };
        let Some(mut offset) = number(&mut tail) else {
            return Err(invalid(path, "invalid startxref offset"));
        };
        // 一部 exporter は先頭の余分な文字を含めず、PDF header 起点の offset を書く。
        let header = bytes
            .windows(5)
            .position(|word| word == b"%PDF-")
            .unwrap_or(0);
        let bias = if !xref_header_at(bytes, offset)
            && offset
                .checked_add(header)
                .is_some_and(|position| xref_header_at(bytes, position))
        {
            header
        } else {
            0
        };
        offset = offset
            .checked_add(bias)
            .ok_or_else(|| invalid(path, "xref offset overflow"))?;
        let mut history = History::default();
        let mut root = None;
        let mut revision_number = 0;
        let mut visited = crate::metadata_workspace::Set::default();
        let mut total = 0u64;
        loop {
            if !visited.insert(budget, offset)? {
                return Err(invalid(path, "cyclic xref Prev chain"));
            }
            let mut lexer = Lexer {
                bytes,
                cursor: offset,
            };
            let mut revision = RevisionEntries::default();
            let prev = if !matches!(lexer.next(), Some(Token::Word(b"xref"))) {
                let stream = crate::indexed_stream::read(path, bytes, offset, limits, budget)?;
                if !stream.dictionary.xref {
                    return Err(invalid(path, "xref offset does not point to XRef stream"));
                }
                read_stream_entries(
                    path,
                    &stream,
                    &mut revision,
                    &mut total,
                    limits,
                    budget,
                    false,
                )?;
                root = root.or(stream.dictionary.root);
                stream.dictionary.prev
            } else {
                loop {
                    let token = lexer.next();
                    if matches!(token, Some(Token::Word(b"trailer"))) {
                        break;
                    }
                    let Some(first) = token.and_then(token_number) else {
                        return Err(invalid(path, "invalid xref subsection"));
                    };
                    let Some(count) = number(&mut lexer) else {
                        return Err(invalid(path, "invalid xref entry count"));
                    };
                    total = total.saturating_add(count as u64);
                    check_limit("xref entries", total, limits.max_xref_entries as u64)?;
                    for index in 0..count {
                        let Some(object) = first.checked_add(index) else {
                            return Err(invalid(path, "xref object number overflow"));
                        };
                        let Some(position) = number(&mut lexer) else {
                            return Err(invalid(path, "invalid xref entry offset"));
                        };
                        let Some(generation) = number(&mut lexer) else {
                            return Err(invalid(path, "invalid xref entry generation"));
                        };
                        let position = match lexer.next() {
                            Some(Token::Word(b"n")) => Entry::Uncompressed {
                                generation,
                                offset: position,
                            },
                            Some(Token::Word(b"f")) => Entry::Free,
                            _ => return Err(invalid(path, "invalid xref entry state")),
                        };
                        revision.insert(budget, object, position, false)?;
                    }
                }
                let trailer = trailer_offsets(path, &mut lexer, limits)?;
                if let Some(supplement) = trailer.hybrid {
                    let supplement = supplement
                        .checked_add(bias)
                        .ok_or_else(|| invalid(path, "xref offset overflow"))?;
                    let stream =
                        crate::indexed_stream::read(path, bytes, supplement, limits, budget)?;
                    if !stream.dictionary.xref {
                        return Err(invalid(
                            path,
                            "XRefStm offset does not point to XRef stream",
                        ));
                    }
                    // 同じ revision の table は補助 stream で上書きする（qpdf 互換）。
                    // 補助 stream の Prev は追わず、table trailer の Prev だけを辿る。
                    read_stream_entries(
                        path,
                        &stream,
                        &mut revision,
                        &mut total,
                        limits,
                        budget,
                        true,
                    )?;
                }
                root = root.or(trailer.root);
                trailer.prev
            };
            for (object, mut entry) in revision.into_entries() {
                if let Entry::Uncompressed { offset, .. } = &mut entry {
                    *offset = offset
                        .checked_add(bias)
                        .ok_or_else(|| invalid(path, "object offset overflow"))?;
                    let mut lexer = Lexer {
                        bytes,
                        cursor: *offset,
                    };
                    if let Some((Token::Word(word), span)) = lexer.next_spanned()
                        && integer(word).is_some()
                    {
                        *offset = span.start;
                    }
                }
                history.push(budget, object, revision_number, entry)?;
            }
            let Some(prev) = prev else { break };
            offset = prev
                .checked_add(bias)
                .ok_or_else(|| invalid(path, "xref offset overflow"))?;
            revision_number += 1;
        }
        Ok(Some(Self {
            history: history.finish(budget)?,
            root,
        }))
    }

    /// 指定 revision に有効だった entry を、二分探索で取得する。
    fn entry_at(&self, number: usize, revision: usize) -> Option<&Entry> {
        self.history.entry_at(number, revision)
    }

    /// 有効な stream は最新 revision、置換済み stream は最後に有効だった revision を使う。
    pub(crate) fn stream_revision(&self, number: u32, generation: usize, offset: usize) -> usize {
        let matches = |entry: &Entry| {
            matches!(entry,
            Entry::Uncompressed { generation: selected_generation, offset: selected_offset }
                if *selected_generation == generation && *selected_offset == offset)
        };
        if self.entry_at(number as usize, 0).is_some_and(matches) {
            return 0;
        }
        self.history.stream_revision(number, generation, offset)
    }

    /// xref が指定する通常 object または object stream の整数を検証して長さを得る。
    pub(crate) fn length(
        &self,
        path: &Path,
        bytes: &[u8],
        reference: (usize, usize),
        revision: usize,
        limits: &ParseLimits,
        budget: &crate::decode::DecodeBudget,
    ) -> Result<usize, ParseError> {
        let (object, generation) = reference;
        match self.entry_at(object, revision) {
            Some(Entry::Uncompressed {
                generation: entry_generation,
                offset,
            }) => {
                if *entry_generation != generation {
                    return Err(invalid(path, "stream Length generation mismatch"));
                }
                let mut lexer = Lexer {
                    bytes,
                    cursor: *offset,
                };
                if number(&mut lexer) != Some(object)
                    || number(&mut lexer) != Some(generation)
                    || !matches!(lexer.next(), Some(Token::Word(b"obj")))
                {
                    return Err(invalid(path, "stream Length xref header mismatch"));
                }
                let Some(length) = number(&mut lexer) else {
                    return Err(invalid(path, "stream Length is not an integer"));
                };
                if !matches!(lexer.next(), Some(Token::Word(b"endobj"))) {
                    return Err(invalid(path, "invalid stream Length object"));
                }
                Ok(length)
            }
            Some(Entry::Compressed {
                object_stream,
                index,
            }) => {
                if generation != 0 {
                    return Err(invalid(path, "compressed Length generation must be zero"));
                }
                let Some(Entry::Uncompressed {
                    generation: 0,
                    offset,
                }) = self.entry_at(*object_stream, revision)
                else {
                    return Err(invalid(path, "missing Length object stream"));
                };
                let stream = crate::indexed_stream::read(path, bytes, *offset, limits, budget)?;
                if stream.number != *object_stream
                    || stream.generation != 0
                    || !stream.dictionary.object_stream
                {
                    return Err(invalid(path, "invalid Length object stream header"));
                }
                compressed_length(path, &stream, object, *index, limits)
            }
            _ => Err(invalid(path, "missing stream Length object")),
        }
    }
}

/// offset が xref table または間接 object の header を指すか字句検査する。
fn xref_header_at(bytes: &[u8], offset: usize) -> bool {
    let mut lexer = Lexer {
        bytes,
        cursor: offset,
    };
    match lexer.next() {
        Some(Token::Word(b"xref")) => true,
        Some(Token::Word(word)) if integer(word).is_some() => {
            number(&mut lexer).is_some() && matches!(lexer.next(), Some(Token::Word(b"obj")))
        }
        _ => false,
    }
}

/// 通常 table trailer が指定する参照 section。
#[derive(Default)]
struct TrailerOffsets {
    /// この revision の catalog 参照。
    root: Option<(u32, u16)>,
    /// 前の revision。
    prev: Option<usize>,
    /// 同じ revision を補完する xref stream。
    hybrid: Option<usize>,
}

/// trailer の最上位 Root・Prev・XRefStm を読む。
fn trailer_offsets(
    path: &Path,
    lexer: &mut Lexer<'_>,
    limits: &ParseLimits,
) -> Result<TrailerOffsets, ParseError> {
    if !matches!(lexer.next(), Some(Token::Open(true))) {
        return Err(invalid(path, "missing xref trailer dictionary"));
    }
    let mut depth = 1usize;
    check_limit(
        "container nesting",
        depth as u64,
        limits.max_container_nesting as u64,
    )?;
    let mut offsets = TrailerOffsets::default();
    let mut value_pending = false;
    while let Some(token) = lexer.next() {
        if depth == 1 && !matches!(token, Token::Close(_)) {
            if value_pending {
                value_pending = false;
            } else if let Token::Name(name) = token {
                if name_is(name, b"Root") {
                    offsets.root = Some(read_root(path, lexer)?);
                } else if name_is(name, b"Prev") || name_is(name, b"XRefStm") {
                    let offset = number(lexer)
                        .ok_or_else(|| invalid(path, "invalid xref trailer offset"))?;
                    let mut lookahead = lexer.clone();
                    if number(&mut lookahead).is_some()
                        && matches!(lookahead.next(), Some(Token::Word(b"R")))
                    {
                        return Err(invalid(path, "xref trailer offset must be direct"));
                    }
                    if name_is(name, b"Prev") {
                        offsets.prev = Some(offset);
                    } else {
                        offsets.hybrid = Some(offset);
                    }
                } else {
                    value_pending = true;
                }
            }
        }
        match token {
            Token::Open(_) => {
                depth += 1;
                check_limit(
                    "container nesting",
                    depth as u64,
                    limits.max_container_nesting as u64,
                )?;
            }
            Token::Close(_) => {
                depth -= 1;
                if depth == 0 {
                    return Ok(offsets);
                }
            }
            _ => {}
        }
    }
    Err(invalid(path, "incomplete xref trailer dictionary"))
}

/// 次の非負整数 token を読む。
fn number(lexer: &mut Lexer<'_>) -> Option<usize> {
    lexer.next().and_then(token_number)
}

/// word token を非負整数へ変換する。
fn token_number(token: Token<'_>) -> Option<usize> {
    match token {
        Token::Word(word) => integer(word),
        _ => None,
    }
}

/// 構造不正を入力 path とともに返す。
fn invalid(path: &Path, reason: &'static str) -> ParseError {
    ParseError::InvalidPdfStructure {
        path: path.into(),
        reason,
    }
}

/// 各 xref entry の参照形式。
enum Entry {
    /// 削除済みで、過去 revision の entry を復活させない。
    Free,
    /// 通常の indirect object。
    Uncompressed {
        /// generation 番号。
        generation: usize,
        /// 入力全体の byte offset。
        offset: usize,
    },
    /// object stream に格納された generation 0 の object。
    Compressed {
        /// 格納先 object stream の番号。
        object_stream: usize,
        /// header における member index。
        index: usize,
    },
}

/// 幅と Index を検証し、追加確保の前に entry 予算を消費する。
fn read_stream_entries(
    path: &Path,
    stream: &crate::indexed_stream::IndexedStream<'_>,
    entries: &mut RevisionEntries,
    total: &mut u64,
    limits: &ParseLimits,
    budget: &crate::decode::DecodeBudget,
    supplemental: bool,
) -> Result<(), ParseError> {
    let widths = stream
        .dictionary
        .widths
        .as_deref()
        .ok_or_else(|| invalid(path, "missing xref W"))?;
    if widths.len() != 3 || widths.iter().any(|width| *width > 8) {
        return Err(invalid(path, "invalid xref field widths"));
    }
    let row = widths.iter().sum::<usize>();
    if row == 0 {
        return Err(invalid(path, "empty xref entry"));
    }
    let size = stream
        .dictionary
        .size
        .ok_or_else(|| invalid(path, "missing xref Size"))?;
    let default_index = [0, size];
    let index = stream.dictionary.index.as_deref().unwrap_or(&default_index);
    if !index.len().is_multiple_of(2) {
        return Err(invalid(path, "invalid xref Index"));
    }
    let mut count = 0u64;
    for pair in index.chunks_exact(2) {
        let end = pair[0]
            .checked_add(pair[1])
            .ok_or_else(|| invalid(path, "xref Index overflow"))?;
        if end > size {
            return Err(invalid(path, "xref Index exceeds Size"));
        }
        count = count.saturating_add(pair[1] as u64);
    }
    *total = total.saturating_add(count);
    check_limit("xref entries", *total, limits.max_xref_entries as u64)?;
    let expected = usize::try_from(count)
        .ok()
        .and_then(|n| n.checked_mul(row))
        .ok_or_else(|| invalid(path, "xref payload size overflow"))?;
    if stream.payload.len() != expected {
        return Err(invalid(path, "xref payload size mismatch"));
    }
    let mut cursor = 0;
    for pair in index.chunks_exact(2) {
        for object in pair[0]..pair[0] + pair[1] {
            let payload = &stream.payload[cursor..cursor + row];
            cursor += row;
            let kind = if widths[0] == 0 {
                1
            } else {
                field(path, &payload[..widths[0]])?
            };
            let field1 = field(path, &payload[widths[0]..widths[0] + widths[1]])?;
            let field2 = field(path, &payload[widths[0] + widths[1]..])?;
            let entry = match kind {
                0 => Entry::Free,
                1 => Entry::Uncompressed {
                    generation: field2,
                    offset: field1,
                },
                2 => Entry::Compressed {
                    object_stream: field1,
                    index: field2,
                },
                _ => Entry::Free,
            };
            entries.insert(budget, object, entry, supplemental)?;
        }
    }
    Ok(())
}

/// big endian 整数を native usize の overflow なしで読む。
fn field(path: &Path, bytes: &[u8]) -> Result<usize, ParseError> {
    bytes
        .iter()
        .try_fold(0usize, |value, byte| {
            value.checked_mul(256)?.checked_add(usize::from(*byte))
        })
        .ok_or_else(|| invalid(path, "xref field value overflow"))
}

/// object stream header の index・番号・隣接 offset を検証して整数 body を読む。
fn compressed_length(
    path: &Path,
    stream: &crate::indexed_stream::IndexedStream<'_>,
    object: usize,
    index: usize,
    limits: &ParseLimits,
) -> Result<usize, ParseError> {
    let count = stream
        .dictionary
        .count
        .ok_or_else(|| invalid(path, "missing object stream N"))?;
    check_limit("xref entries", count as u64, limits.max_xref_entries as u64)?;
    if index >= count {
        return Err(invalid(path, "Length object stream index outside N"));
    }
    let first = stream
        .dictionary
        .first
        .filter(|first| *first <= stream.payload.len())
        .ok_or_else(|| invalid(path, "invalid object stream First"))?;
    let mut header = Lexer {
        bytes: &stream.payload[..first],
        cursor: 0,
    };
    let mut selected = None;
    let mut end = stream.payload.len() - first;
    let mut previous = None;
    for member in 0..count {
        let member_number = number(&mut header)
            .ok_or_else(|| invalid(path, "invalid object stream member number"))?;
        let offset = number(&mut header)
            .ok_or_else(|| invalid(path, "invalid object stream member offset"))?;
        if offset > stream.payload.len() - first
            || previous.is_some_and(|previous| offset <= previous)
        {
            return Err(invalid(path, "invalid object stream member boundary"));
        }
        previous = Some(offset);
        if member == index {
            if member_number != object {
                return Err(invalid(path, "Length object stream index number mismatch"));
            }
            selected = Some(offset);
        } else if member == index + 1 {
            end = offset;
        }
    }
    if header.next().is_some() {
        return Err(invalid(path, "extra object stream header tokens"));
    }
    let start = selected.ok_or_else(|| invalid(path, "missing Length object stream member"))?;
    let mut body = Lexer {
        bytes: &stream.payload[first + start..first + end],
        cursor: 0,
    };
    let length =
        number(&mut body).ok_or_else(|| invalid(path, "compressed Length is not an integer"))?;
    if body.next().is_some() {
        return Err(invalid(path, "invalid compressed Length body"));
    }
    Ok(length)
}

/// catalog の間接参照を整数範囲と末尾 R まで検証する。
pub(crate) fn read_root(path: &Path, lexer: &mut Lexer<'_>) -> Result<(u32, u16), ParseError> {
    let object = number(lexer).and_then(|number| u32::try_from(number).ok());
    let generation = number(lexer).and_then(|number| u16::try_from(number).ok());
    match (object, generation, lexer.next()) {
        (Some(object), Some(generation), Some(Token::Word(b"R"))) => Ok((object, generation)),
        _ => Err(invalid(path, "invalid trailer Root reference")),
    }
}
