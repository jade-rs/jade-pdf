//! 外部フォントを保存せず、旧数式用Symbol符号の所有関係だけを有界に読む。
use anyhow::{ensure, Context, Result};
use lopdf::{Dictionary, Document, Object};
use std::{collections::BTreeMap, sync::OnceLock};
use ttf_parser::{Face, PlatformId, Tag};
use unicode_normalization::UnicodeNormalization;

/// 同じ私用コードでも意味が異なる、独立した二つの旧字体符号化。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// Adobe Symbolの独立した符号表。
    Symbol,
    /// MathType Extraの独立した符号表。
    MtExtra,
}

/// 正式な字体名だけを受け入れ、有効な六文字のsubset接頭辞を除く。
fn kind(name: &[u8]) -> Option<Kind> {
    let name = if name.len() > 7 && name[6] == b'+' && name[..6].iter().all(u8::is_ascii_uppercase)
    {
        &name[7..]
    } else {
        name
    };
    match name {
        b"Symbol" | b"SymbolMT" => Some(Kind::Symbol),
        b"MT Extra" | b"MT-Extra" | b"MTExtra" => Some(Kind::MtExtra),
        _ => None,
    }
}

/// 独立した対応表を持つ字体だけを、旧私用符号の診断対象にする。
pub(super) fn family(name: &[u8]) -> bool {
    kind(name).is_some()
}

/// 参照の深さを32段に制限し、循環した辞書を受け入れない。
fn resolve<'a>(doc: &'a Document, mut value: &'a Object) -> Result<&'a Object> {
    for _ in 0..32 {
        match value {
            Object::Reference(id) => {
                value = doc.objects.get(id).context("Missing Symbol resource")?
            }
            _ => return Ok(value),
        }
    }
    anyhow::bail!("Symbol resource exceeds reference budget")
}

/// 元の通知付きAdobe表から、正準合成後も一意な非私用文字だけを採用する。
fn mapping(encoding: Kind) -> &'static BTreeMap<u8, char> {
    if encoding == Kind::MtExtra {
        // Mozilla MathMLの一次資料に記載された、単独文字である二つの対応事実。
        // フォント・字形・表全体を複製せず、組み立て片を一文字へ誤変換しない。
        // 出典と範囲はresources/mt-extra.jsonに記録する。
        static MT: OnceLock<BTreeMap<u8, char>> = OnceLock::new();
        return MT.get_or_init(|| BTreeMap::from([(0x4b, '…'), (0x6c, 'ℓ')]));
    }
    static MAP: OnceLock<BTreeMap<u8, char>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut entries = BTreeMap::<u8, Option<char>>::new();
        for line in include_str!("../../resources/adobe-symbol.txt").lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let scalar = u32::from_str_radix(parts.next().unwrap(), 16).unwrap();
            let code = u8::from_str_radix(parts.next().unwrap(), 16).unwrap();
            let text: String = char::from_u32(scalar).unwrap().to_string().nfc().collect();
            let mut chars = text.chars();
            let value = chars
                .next()
                .filter(|c| !private(*c) && !c.is_whitespace() && !c.is_control());
            let value = value.filter(|_| chars.next().is_none());
            entries
                .entry(code)
                .and_modify(|old| {
                    if *old != value {
                        *old = None;
                    }
                })
                .or_insert(value);
        }
        entries
            .into_iter()
            .filter_map(|(c, v)| v.map(|v| (c, v)))
            .collect()
    })
}

/// すべてのUnicode私用領域を、標準的な意味の証明から除外する。
pub(super) fn private(c: char) -> bool {
    matches!(c, '\u{e000}'..='\u{f8ff}' | '\u{f0000}'..='\u{ffffd}' | '\u{100000}'..='\u{10fffd}')
}

/// PDF辞書、フォント名、Microsoft Symbol cmapの字形IDを照合する。
/// 字形輪郭は解析せず、フォントのバイト列は戻り値へ保持しない。
pub(super) fn read(
    doc: &Document,
    font: &Dictionary,
    bytes_used: &mut usize,
) -> Result<BTreeMap<u32, (char, char)>> {
    ensure!(
        font.get(b"Subtype")?.as_name()? == b"Type0",
        "Not a Type0 Symbol font"
    );
    let encoding = kind(font.get(b"BaseFont")?.as_name()?).context("Unknown legacy math font")?;
    ensure!(
        font.get(b"Encoding")?.as_name()? == b"Identity-H",
        "Unverified Symbol encoding"
    );
    let descendants = resolve(doc, font.get(b"DescendantFonts")?)?.as_array()?;
    ensure!(descendants.len() == 1, "Ambiguous Symbol descendants");
    let descendant = resolve(doc, &descendants[0])?.as_dict()?;
    ensure!(
        descendant.get(b"Subtype")?.as_name()? == b"CIDFontType2",
        "Not TrueType CID"
    );
    ensure!(
        kind(descendant.get(b"BaseFont")?.as_name()?) == Some(encoding),
        "Symbol family disagreement"
    );
    ensure!(
        descendant.get(b"CIDToGIDMap")?.as_name()? == b"Identity",
        "Unverified Symbol glyph identity"
    );
    let descriptor = resolve(doc, descendant.get(b"FontDescriptor")?)?.as_dict()?;
    ensure!(
        kind(descriptor.get(b"FontName")?.as_name()?) == Some(encoding),
        "Symbol descriptor disagreement"
    );
    let stream = resolve(doc, descriptor.get(b"FontFile2")?)?.as_stream()?;
    if stream.dict.has(b"Filter") {
        stream.filters()?;
    }
    let limit = (2 * 1024 * 1024).min((16usize * 1024 * 1024).saturating_sub(*bytes_used));
    ensure!(limit > 0, "Symbol font metadata exceeds byte budget");
    // 展開の失敗も処理済みの上限量を消費し、悪いフォントを繰り返して予算を回避しない。
    *bytes_used = bytes_used.saturating_add(limit);
    let bytes = stream.decompressed_content_with_limit(limit)?;
    *bytes_used -= limit - bytes.len();
    ensure!(
        bytes.starts_with(&[0, 1, 0, 0]),
        "Not a single TrueType SFNT"
    );
    let face = Face::parse(&bytes, 0)?;
    ensure!(
        face.raw_face().table_records.len() <= 64,
        "Too many Symbol font tables"
    );
    ensure!(
        face.raw_face().table(Tag::from_bytes(b"fvar")).is_none(),
        "Variable Symbol font is not verified"
    );
    ensure!(face.names().len() <= 128, "Too many Symbol names");
    let (mut family_seen, mut ps_seen) = (false, false);
    for name in face.names() {
        if !matches!(name.name_id, 1 | 6) {
            continue;
        }
        ensure!(name.name.len() <= 64, "Symbol name exceeds budget");
        let text = if name.is_unicode() {
            ensure!(name.name.len() % 2 == 0, "Odd-length Symbol name");
            name.to_string().context("Invalid Symbol name")?
        } else if name.platform_id == PlatformId::Macintosh && name.encoding_id == 0 {
            std::str::from_utf8(name.name)?.to_owned()
        } else {
            anyhow::bail!("Unverified Symbol name encoding");
        };
        ensure!(
            kind(text.as_bytes()) == Some(encoding),
            "Embedded font disagrees with legacy math encoding"
        );
        family_seen |= name.name_id == 1;
        ps_seen |= name.name_id == 6;
    }
    ensure!(
        family_seen && ps_seen,
        "Symbol program lacks independent names"
    );
    let cmap = face.tables().cmap.context("Missing Symbol cmap")?;
    ensure!(cmap.subtables.len() <= 16, "Too many Symbol cmap tables");
    let symbol: Vec<_> = cmap
        .subtables
        .into_iter()
        .filter(|s| s.platform_id == PlatformId::Windows && s.encoding_id == 0)
        .collect();
    ensure!(!symbol.is_empty(), "Missing Microsoft Symbol cmap");
    let mut result = BTreeMap::new();
    for (&code, &target) in mapping(encoding) {
        let raw = char::from_u32(0xf000 + u32::from(code)).unwrap();
        let Some(gid) = symbol[0].glyph_index(raw as u32) else {
            continue;
        };
        if gid.0 >= face.number_of_glyphs()
            || symbol
                .iter()
                .any(|s| s.glyph_index(raw as u32) != Some(gid))
        {
            continue;
        }
        if cmap
            .subtables
            .into_iter()
            .filter(|s| s.is_unicode())
            .any(|s| {
                [raw, target]
                    .into_iter()
                    .any(|c| s.glyph_index(c as u32).is_some_and(|g| g != gid))
            })
        {
            continue;
        }
        // 複数の旧符号が同じ字形IDを共有するときは、ToUnicodeで選ばず曖昧として拒否する。
        if result.insert(u32::from(gid.0), (raw, target)).is_some() {
            anyhow::bail!("Ambiguous Symbol glyph alias");
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 数学的に異なるDelta・muと、空白・私用の組み立て字形を推測しない。
    #[test]
    fn symbol_table_preserves_ambiguous_semantics() {
        assert_eq!(mapping(Kind::Symbol).get(&0xa3), Some(&'≤'));
        assert_eq!(mapping(Kind::Symbol).get(&0x73), Some(&'σ'));
        for code in [0x20, 0x44, 0x6d, 0x60] {
            assert!(!mapping(Kind::Symbol).contains_key(&code));
        }
        assert!(family(b"ABCDEF+Symbol"));
        assert!(!family(b"abcdef+Symbol"));
        assert!(!family(b"MySymbol"));
        assert_eq!(kind(b"ABCDEF+MT Extra"), Some(Kind::MtExtra));
        assert_eq!(kind(b"ABCDEF+MT-Extra"), Some(Kind::MtExtra));
        assert_eq!(mapping(Kind::MtExtra).get(&0x4b), Some(&'…'));
        assert_eq!(mapping(Kind::Symbol).get(&0x4b), Some(&'Κ'));
        assert_eq!(mapping(Kind::MtExtra).get(&0x6c), Some(&'ℓ'));
        assert_eq!(mapping(Kind::Symbol).get(&0x6c), Some(&'λ'));
        assert!(!mapping(Kind::MtExtra).contains_key(&0x31));
    }

    /// 自作PDFのフォント辞書だけを取得し、外部フォントをテストへ持ち込まない。
    fn owned_font() -> (Document, Dictionary) {
        let bytes = crate::appearance_fixture::symbol_glyph_cells("valid", 1., 0, false);
        let doc = Document::load_mem(&bytes).unwrap();
        let font = doc.get_object((4, 0)).unwrap().as_dict().unwrap().clone();
        (doc, font)
    }

    /// 名前のないpost-3でも正確なCIDだけを対応付け、独立したメモリ予算を守る。
    #[test]
    fn profile_uses_symbol_cmap_glyph_ids_and_independent_byte_budget() {
        let (doc, font) = owned_font();
        let mut bytes = 0;
        let aliases = read(&doc, &font, &mut bytes).unwrap();
        assert_eq!(aliases.get(&1), Some(&('\u{f0a3}', '≤')));
        assert_eq!(aliases.get(&2), Some(&('\u{f03c}', '<')));
        assert_eq!(aliases.get(&3), Some(&('\u{f073}', 'σ')));
        assert!(!aliases.contains_key(&4) && !aliases.contains_key(&5));
        assert!(bytes > 0 && bytes < 2048);
        let mut exhausted = 16 * 1024 * 1024;
        assert!(read(&doc, &font, &mut exhausted).is_err());
        let mut too_small = 16 * 1024 * 1024 - 1;
        assert!(read(&doc, &font, &mut too_small).is_err());
        assert_eq!(too_small, 16 * 1024 * 1024);
        let mut truncated = font.clone();
        truncated.set("DescendantFonts", Object::Reference((4, 0)));
        assert!(read(&doc, &truncated, &mut 0).is_err());
    }

    /// 配布する通知付き原本を、Windowsの改行変換も含めて固定SHAへ照合する。
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn pinned_symbol_table_matches_provenance() {
        use sha2::{Digest, Sha256};
        let pin: serde_json::Value =
            serde_json::from_str(include_str!("../../resources/adobe-symbol.json")).unwrap();
        assert_eq!(
            format!(
                "{:x}",
                Sha256::digest(include_bytes!("../../resources/adobe-symbol.txt"))
            ),
            pin["sha256"].as_str().unwrap()
        );
        assert!(mapping(Kind::Symbol).len() > 150);
    }
}
