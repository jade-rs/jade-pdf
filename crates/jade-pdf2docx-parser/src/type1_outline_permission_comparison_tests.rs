//! 原本の埋め込み指定と表示を、fontToolsとpypdfによる独立byte列へ照合する。
use crate::{
    decode::DecodeBudget,
    type1_outline::{
        header::Header,
        permission::{self, Basis, Decision},
    },
};
use serde::Deserialize;

/// 実行前に固定した、同じ原本PDFの権利情報と表示の独立期待値。
#[derive(Deserialize)]
pub(super) struct Reference {
    /// 原本PDFのSHA256。
    source_pdf_sha256: String,
    /// 公式OFL標準本文のSHA256。
    license_body_sha256: String,
    /// 原本の全11font。
    fonts: Vec<Expected>,
}

/// 原本programと結び付いた、元の指定と表示全文の期待値。
#[derive(Deserialize)]
struct Expected {
    /// 原本Font object番号。
    object_number: u32,
    /// 対応する原本FontFileのSHA256。
    source_font_sha256: String,
    /// 原本のsubset名に基づく指定。
    subset: bool,
    /// 元FSTypeの有無。
    declared_flags: Option<u16>,
    /// 生成fontへ保持するbit。
    expected_flags: u16,
    /// sourceの明示宣言に基づく根拠。
    expected_basis: String,
    /// pypdfのliteral readerで独立に復号した元byte値。
    notice_bytes: Vec<u8>,
    /// 元表示と公式ライセンス全文を含むUTF8のSHA256。
    text_sha256: String,
    /// 明示OFLの公式参照先。
    url: String,
}

impl Reference {
    /// sourceと標準本文の取り違えを除き、独立goldを一度だけ読む。
    pub(super) fn load(source_sha256: &str) -> Self {
        let bytes = std::fs::read(std::env::var_os("JADE_TYPE1_PERMISSION_GOLD").unwrap()).unwrap();
        assert!(bytes.len() <= 128 * 1024);
        let result: Self = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(result.source_pdf_sha256, source_sha256);
        assert_eq!(
            result.license_body_sha256,
            sha256(include_bytes!("../licenses/open-font-license-1.1.txt"))
        );
        assert_eq!(result.fonts.len(), 11);
        result
    }

    /// 入力は原本PDFから取り出したpayloadとproduct headerだけであり、goldを供給しない。
    pub(super) fn compare(
        &self,
        budget: &DecodeBudget,
        number: u32,
        payload: &[u8],
        length1: usize,
        header: &Header<'_>,
    ) {
        let expected = self
            .fonts
            .iter()
            .find(|f| f.object_number == number)
            .unwrap();
        assert_eq!(sha256(payload), expected.source_font_sha256);
        let subset = header.name.len() > 7
            && header.name[6] == b'+'
            && header.name[..6].iter().all(u8::is_ascii_uppercase);
        assert_eq!(subset, expected.subset);
        let Some(Decision::Allowed(actual)) =
            permission::read(budget, &payload[..length1], &header.info, subset)
        else {
            panic!("original source permission for font{number}");
        };
        assert_eq!(actual.declared_flags, expected.declared_flags);
        assert_eq!(actual.flags, expected.expected_flags);
        assert_eq!(expected.expected_basis, "OFL-1.1");
        assert_eq!(actual.basis, Basis::OpenFontLicense);
        assert_eq!(actual.notice, expected.notice_bytes);
        assert_eq!(sha256(actual.text.as_bytes()), expected.text_sha256);
        assert_eq!(actual.url, expected.url);
        println!(
            "font{number}: missing source FSType stays distinct; explicit OFL, original notice bytes and complete license text match independent gold"
        );
    }
}

/// 固定データのSHA256を、比較用の小文字hexへ変える。
fn sha256(bytes: &[u8]) -> String {
    hmac_sha256::Hash::hash(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
