//! 生SDK文字数を、固定原本から独立に確認した論理置換と照合するテスト参照。
//! 製品の復元器を呼ばず、ActualTextの所有MCIDと字体の固定資料を検証する。
use anyhow::{ensure, Context, Result};
use lopdf::{Document, Object};
use pdfium_render::prelude::Pdfium;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// 特定の固定原本にだけ適用する、独立に採取した参照資料。
#[derive(Clone, Deserialize)]
pub struct Gold {
    /// 元PDF全体のSHA-256。
    pub source_sha256: String,
    /// 原文構造要素が明示する論理文字列と所有MCID。
    #[serde(default)]
    pub actual_text: Vec<Actual>,
    /// 埋め込み字体の固定資料と、一意な標準文字対応。
    #[serde(default)]
    pub fonts: Vec<Font>,
}

/// SDK生文字から独立に確定した原文ActualText置換。
#[derive(Clone, Deserialize)]
pub struct Actual {
    /// 一始まりの対象ページ。
    pub page: usize,
    /// 原文の構造要素オブジェクト番号と世代。
    pub object: (u32, u16),
    /// ページ内容ストリームで使用されたMCID。
    pub mcid: i32,
    /// 原文文字列の未解釈バイトを表す16進数。
    pub source_hex: String,
    /// 独立に読んだ論理文字列。
    pub text: String,
    /// 同じMCIDに実際に所属するSDK生文字。ハイフンAPIの正規化を含む。
    pub raw_sdk_text: String,
}

/// 一つの固定字体に対する独立した符号対応資料。
#[derive(Clone, Deserialize)]
pub struct Font {
    /// PDFのフォント辞書オブジェクト番号と世代。
    pub object: (u32, u16),
    /// 原文のBaseFont名。
    pub base_font: String,
    /// SDKから取得するBaseFont名。
    pub sdk_name: String,
    /// 原文ToUnicodeの展開済みバイトのSHA-256。
    pub to_unicode_sha256: String,
    /// フォントプログラムの展開済みバイトのSHA-256。プログラム自体は保存しない。
    pub program_sha256: String,
    /// FontToolsと原文ToUnicodeから独立に照合した、私用符号と標準文字。
    pub aliases: BTreeMap<char, char>,
}

/// 論理置換後の参照文字列と、文字保存量だけに使用する一意な等価対応。
pub struct Page {
    /// 元の順序を保持した参照文字列。順序の正解を主張するものではない。
    pub text: String,
    /// 全使用が検証済み字体に所属するときだけ許す、私用符号の等価対応。
    pub aliases: BTreeMap<char, char>,
}

/// 参照資料の元バイト列へ固定する。
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// 深さを制限して元文書の間接参照を読む。
fn resolve<'a>(doc: &'a Document, mut object: &'a Object) -> Result<&'a Object> {
    for _ in 0..32 {
        match object {
            Object::Reference(id) => object = doc.get_object(*id)?,
            _ => return Ok(object),
        }
    }
    anyhow::bail!("Cyclic source-inventory reference")
}

/// 原本・構造要素・内容ストリーム・字体の同一性を、SDKを開く前に確認する。
fn verify(bytes: &[u8], gold: &Gold) -> Result<()> {
    ensure!(
        digest(bytes) == gold.source_sha256,
        "Source-inventory PDF hash differs"
    );
    let doc = Document::load_mem(bytes)?;
    let pages = doc.get_pages();
    let mut groups = BTreeSet::new();
    for g in &gold.actual_text {
        ensure!(
            g.page > 0 && groups.insert((g.page, g.mcid)),
            "Duplicate source ActualText scope"
        );
        let element = doc.get_dictionary(g.object)?;
        let raw = element.get(b"ActualText")?.as_str()?;
        let hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        ensure!(hex == g.source_hex, "Source ActualText bytes differ");
        ensure!(
            raw.starts_with(&[0xfe, 0xff]) && raw.len() % 2 == 0,
            "Expected independently reviewed UTF-16 source"
        );
        let words: Vec<_> = raw[2..]
            .chunks_exact(2)
            .map(|p| u16::from_be_bytes([p[0], p[1]]))
            .collect();
        ensure!(
            String::from_utf16(&words)? == g.text,
            "Source ActualText decoding differs"
        );
        ensure!(
            element.get(b"K")?.as_i64()? == i64::from(g.mcid),
            "Source ActualText MCID differs"
        );
        let page_id = *pages
            .get(&(g.page as u32))
            .context("Missing source inventory page")?;
        let mut ancestor = element;
        let mut owner = None;
        for _ in 0..32 {
            if let Ok(pg) = ancestor.get(b"Pg") {
                owner = Some(pg.as_reference()?);
                break;
            }
            ancestor = resolve(&doc, ancestor.get(b"P")?)?.as_dict()?;
        }
        ensure!(
            owner == Some(page_id),
            "Source ActualText page owner differs"
        );
        let content = lopdf::content::Content::decode(
            &doc.get_page_content_with_limit(page_id, 16 * 1024 * 1024)?,
        )?;
        let scopes = content
            .operations
            .iter()
            .filter(|op| {
                op.operator == "BDC"
                    && op
                        .operands
                        .get(1)
                        .and_then(|p| p.as_dict().ok())
                        .and_then(|d| d.get(b"MCID").ok())
                        .and_then(|id| id.as_i64().ok())
                        == Some(i64::from(g.mcid))
            })
            .count();
        ensure!(
            scopes == 1,
            "Source ActualText MCID is not unique page content"
        );
    }
    for g in &gold.fonts {
        ensure!(g.sdk_name == g.base_font, "Unverified SDK font name alias");
        let matching: BTreeSet<_> = doc
            .objects
            .iter()
            .filter_map(|(&id, object)| {
                let d = object.as_dict().ok()?;
                (d.get(b"Subtype").ok()?.as_name().ok()? == b"Type0"
                    && d.get(b"BaseFont").ok()?.as_name().ok()? == g.base_font.as_bytes())
                .then_some(id)
            })
            .collect();
        ensure!(
            matching == BTreeSet::from([g.object]),
            "Ambiguous SDK font name ownership"
        );
        let font = doc.get_dictionary(g.object)?;
        ensure!(
            font.get(b"BaseFont")?.as_name()? == g.base_font.as_bytes(),
            "Source font name differs"
        );
        ensure!(
            font.get(b"Encoding")?.as_name()? == b"Identity-H",
            "Source font encoding differs"
        );
        let cmap = resolve(&doc, font.get(b"ToUnicode")?)?
            .as_stream()?
            .decompressed_content_with_limit(2 * 1024 * 1024)?;
        ensure!(
            digest(&cmap) == g.to_unicode_sha256,
            "Source ToUnicode differs"
        );
        let descendants = resolve(&doc, font.get(b"DescendantFonts")?)?.as_array()?;
        ensure!(descendants.len() == 1, "Ambiguous source font descendant");
        let child = resolve(&doc, &descendants[0])?.as_dict()?;
        ensure!(
            child.get(b"CIDToGIDMap")?.as_name()? == b"Identity",
            "Source glyph ownership differs"
        );
        let descriptor = resolve(&doc, child.get(b"FontDescriptor")?)?.as_dict()?;
        let program = resolve(&doc, descriptor.get(b"FontFile2")?)?
            .as_stream()?
            .decompressed_content_with_limit(2 * 1024 * 1024)?;
        ensure!(
            digest(&program) == g.program_sha256,
            "Source font program differs"
        );
        for (&raw, &target) in &g.aliases {
            ensure!(
                matches!(raw, '\u{f000}'..='\u{f0ff}'),
                "Not a reviewed legacy Symbol position"
            );
            let code = raw as u32 - 0xf000;
            let candidates: BTreeSet<String> = include_str!("../../resources/adobe-symbol.txt")
                .lines()
                .filter(|s| !s.starts_with('#'))
                .filter_map(|line| {
                    let mut columns = line.split_whitespace();
                    let unicode = u32::from_str_radix(columns.next()?, 16).ok()?;
                    let byte = u32::from_str_radix(columns.next()?, 16).ok()?;
                    use unicode_normalization::UnicodeNormalization;
                    (byte == code)
                        .then(|| char::from_u32(unicode).unwrap().to_string().nfc().collect())
                })
                .collect();
            ensure!(
                candidates.len() == 1
                    && candidates.contains(&target.to_string())
                    && !matches!(target, '\u{e000}'..='\u{f8ff}'),
                "Declared Symbol equivalence differs from official source"
            );
        }
    }
    Ok(())
}

/// 検証済み字体のコードがページ内の全出現を所有するときだけ、等価化を許す。
fn owned_aliases(
    text: &str,
    used: &BTreeMap<char, usize>,
    fonts: &[Font],
) -> Result<BTreeMap<char, char>> {
    let mut output = BTreeMap::new();
    for f in fonts {
        for (&raw, &target) in &f.aliases {
            let count = text.chars().filter(|&c| c == raw).count();
            if count > 0 && used.get(&raw) == Some(&count) {
                ensure!(
                    output.insert(raw, target).is_none_or(|old| old == target),
                    "Ambiguous source equivalence"
                );
            }
        }
    }
    Ok(output)
}

/// 確認した保存量の等価対応だけを適用し、未確認の私用文字をそのまま残す。
pub fn normalize(text: &str, aliases: &BTreeMap<char, char>) -> String {
    text.chars()
        .map(|c| aliases.get(&c).copied().unwrap_or(c))
        .collect()
}

/// 原本の所有資料でSDK参照を補正する。製品の文字復元器は参照しない。
pub fn pages(
    pdfium: &Pdfium,
    bytes: &[u8],
    raw: &[(String, usize)],
    gold: &Gold,
) -> Result<Vec<Page>> {
    verify(bytes, gold)?;
    let b = pdfium.bindings();
    let document = b.FPDF_LoadMemDocument64(bytes, None);
    ensure!(!document.is_null(), "Cannot load inventory reference");
    let result = (|| {
        ensure!(
            b.FPDF_GetPageCount(document) as usize == raw.len(),
            "Inventory page count differs"
        );
        let mut output = Vec::new();
        for (number, (raw_text, _)) in raw.iter().enumerate() {
            let page = b.FPDF_LoadPage(document, number as i32);
            ensure!(!page.is_null(), "Cannot load inventory page");
            let page_result = (|| {
                let text = b.FPDFText_LoadPage(page);
                ensure!(!text.is_null(), "Cannot load inventory text");
                let text_result = (|| {
                    let mut original = String::new();
                    let mut projected = String::new();
                    let mut raw_groups = BTreeMap::<i32, String>::new();
                    let groups: BTreeMap<_, _> = gold
                        .actual_text
                        .iter()
                        .filter(|g| g.page == number + 1)
                        .map(|g| (g.mcid, g))
                        .collect();
                    let mut owned = BTreeSet::new();
                    for i in 0..b.FPDFPage_CountObjects(page) {
                        owned.insert(
                            b.FPDFPageObj_GetMarkedContentID(b.FPDFPage_GetObject(page, i)),
                        );
                    }
                    ensure!(
                        groups.keys().all(|g| owned.contains(g)),
                        "Declared MCID has no SDK page object"
                    );
                    let mut used = BTreeMap::<char, usize>::new();
                    let mut names = HashMap::new();
                    for i in 0..b.FPDFText_CountChars(text) {
                        let value = b.FPDFText_GetUnicode(text, i);
                        ensure!(
                            !(0xd800..=0xdfff).contains(&value),
                            "This reference cohort requires a supplementary decoder"
                        );
                        let scalar = char::from_u32(value);
                        let error = b.FPDFText_HasUnicodeMapError(text, i);
                        ensure!(matches!(error, 0 | 1), "Invalid SDK mapping flag");
                        let scalar = if error == 1
                            && scalar.is_none_or(|c| {
                                c.is_control() || c.is_whitespace() || c == '\u{ad}'
                            }) {
                            Some('\u{fffd}')
                        } else if b.FPDFText_IsHyphen(text, i) == 1 {
                            Some('-')
                        } else {
                            scalar
                        };
                        let Some(c) = scalar else {
                            anyhow::bail!("Unexpected invalid scalar in reviewed reference");
                        };
                        original.push(c);
                        let object = b.FPDFText_GetTextObject(text, i);
                        let mcid = if object.is_null() {
                            -1
                        } else {
                            b.FPDFPageObj_GetMarkedContentID(object)
                        };
                        if let Some(group) = groups.get(&mcid) {
                            let seen = raw_groups.entry(mcid).or_default();
                            if seen.is_empty() {
                                projected.push_str(&group.text);
                            }
                            seen.push(c);
                        } else {
                            projected.push(c);
                            if !object.is_null() && matches!(c, '\u{f000}'..='\u{f0ff}') {
                                let font = b.FPDFTextObj_GetFont(object);
                                let name = names.entry(font as usize).or_insert_with(|| {
                                    let n =
                                        b.FPDFFont_GetBaseFontName(font, std::ptr::null_mut(), 0);
                                    if n == 0 || n > 256 {
                                        return String::new();
                                    }
                                    let mut bytes = vec![0u8; n];
                                    if b.FPDFFont_GetBaseFontName(
                                        font,
                                        bytes.as_mut_ptr().cast(),
                                        n,
                                    ) != n
                                    {
                                        return String::new();
                                    }
                                    bytes.pop();
                                    String::from_utf8(bytes).unwrap_or_default()
                                });
                                if gold
                                    .fonts
                                    .iter()
                                    .any(|f| f.sdk_name == *name && f.aliases.contains_key(&c))
                                {
                                    *used.entry(c).or_default() += 1;
                                }
                            }
                        }
                    }
                    ensure!(
                        original == *raw_text,
                        "Independent SDK inventory passes disagree"
                    );
                    for (&mcid, g) in &groups {
                        ensure!(
                            raw_groups.get(&mcid).map(String::as_str).unwrap_or("")
                                == g.raw_sdk_text,
                            "SDK MCID {mcid} raw text differs on page {}: {:?}",
                            number + 1,
                            raw_groups.get(&mcid)
                        );
                    }
                    for (&mcid, g) in &groups {
                        if !raw_groups.contains_key(&mcid) {
                            projected.push_str(&g.text);
                        }
                    }
                    let aliases = owned_aliases(&projected, &used, &gold.fonts)?;
                    Ok(Page {
                        text: projected,
                        aliases,
                    })
                })();
                b.FPDFText_ClosePage(text);
                text_result
            })();
            b.FPDF_ClosePage(page);
            output.push(page_result?);
        }
        Ok(output)
    })();
    b.FPDF_CloseDocument(document);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 所有数だけを検証するための、原本検証とは分離した字体資料。
    fn font() -> Font {
        Font {
            object: (1, 0),
            base_font: "Symbol".into(),
            sdk_name: "Symbol".into(),
            to_unicode_sha256: String::new(),
            program_sha256: String::new(),
            aliases: BTreeMap::from([('\u{f0a3}', '≤')]),
        }
    }
    /// 同じ私用符号の一部だけが別字体やActualTextに属する場合は等価化しない。
    #[test]
    fn partial_ownership_retains_every_private_scalar() {
        let text = "\u{f0a3}\u{f0a3}";
        let aliases = owned_aliases(text, &BTreeMap::from([('\u{f0a3}', 1)]), &[font()]).unwrap();
        assert!(aliases.is_empty());
        assert_eq!(normalize(text, &aliases), text);
        let aliases = owned_aliases(text, &BTreeMap::from([('\u{f0a3}', 2)]), &[font()]).unwrap();
        assert_eq!(normalize(text, &aliases), "≤≤");
        assert_eq!(normalize("\u{f04b}≤x", &aliases), "\u{f04b}≤x");
    }
    proptest::proptest! {
        /// 所有数が一致するときだけ対応を許し、文字の個数を変えない。
        #[test]
        fn ownership_is_exact_and_count_preserving(total in 1usize..128, owned in 0usize..128) {
            let text="\u{f0a3}".repeat(total);
            let aliases=owned_aliases(&text,&BTreeMap::from([('\u{f0a3}',owned)]),&[font()]).unwrap();
            proptest::prop_assert_eq!(!aliases.is_empty(),total==owned);
            proptest::prop_assert_eq!(normalize(&text,&aliases).chars().count(),total);
        }
    }
    /// 固定原本のハッシュ・タグ・MCID・字体資料の改変を拒否する。
    #[test]
    #[ignore = "requires separately fetched SHA-256-pinned NIST corpus"]
    fn original_evidence_is_required() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for year in [2012, 2015] {
            let bytes = std::fs::read(root.join(format!("corpus/cache/nist-fips180-4-{year}.pdf")))
                .unwrap();
            let gold: Gold = serde_json::from_slice(
                &std::fs::read(root.join(format!("corpus/source-inventory-nist-{year}.json")))
                    .unwrap(),
            )
            .unwrap();
            verify(&bytes, &gold).unwrap();
            let mut bad = gold.clone();
            bad.source_sha256.push('0');
            assert!(verify(&bytes, &bad).is_err());
            if year == 2012 {
                let mut bad = gold.clone();
                bad.actual_text[0].mcid += 1;
                assert!(verify(&bytes, &bad).is_err());
                let mut bad = gold.clone();
                bad.actual_text[0].text = "-".into();
                assert!(verify(&bytes, &bad).is_err());
                let mut bad = gold.clone();
                bad.actual_text[0].source_hex = "feff002d".into();
                assert!(verify(&bytes, &bad).is_err());
                let mut bad = gold.clone();
                bad.actual_text[0].page += 1;
                assert!(verify(&bytes, &bad).is_err());
                let mut bad = gold.clone();
                bad.actual_text.push(bad.actual_text[0].clone());
                assert!(verify(&bytes, &bad).is_err());
            } else {
                let mut bad = gold.clone();
                bad.fonts[0].program_sha256.push('0');
                assert!(verify(&bytes, &bad).is_err());
                let mut bad = gold.clone();
                bad.fonts[0].to_unicode_sha256.push('0');
                assert!(verify(&bytes, &bad).is_err());
                let mut bad = gold.clone();
                bad.fonts[0].sdk_name = "MT-Extra".into();
                assert!(verify(&bytes, &bad).is_err());
                let mut bad = gold.clone();
                bad.fonts[0].aliases.insert('\u{f0a3}', '≥');
                assert!(verify(&bytes, &bad).is_err());
            }
        }
    }
}
