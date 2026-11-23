//! 実PDF descriptorとFontInfoから読む属性を、別解析器が先に保存した期待値へ照合する。
use crate::{
    decode::DecodeBudget,
    resource_resolver::ResourceResolver,
    type1_outline::{attributes, file::Font},
};
use serde::Deserialize;

/// 先に凍結された元PDF単位の期待値。
#[derive(Deserialize)]
pub(super) struct Reference {
    /// 別PDFの期待値を混ぜないための元入力hash。
    source_pdf_sha256: String,
    /// 元font object順の独立な属性。
    fonts: Vec<Expected>,
}

/// 未変更の元programに対応する、descriptorとFontInfoの期待値。
#[derive(Deserialize)]
struct Expected {
    /// 元font辞書の番号。
    object_number: u32,
    /// 元font辞書の有効世代。
    generation: u16,
    /// FontFileの未変更payloadのhash。
    program_sha256: String,
    /// pypdfとfontToolsで独立に宣言した属性。
    attributes: serde_json::Value,
}

impl Reference {
    /// 原本hashを照合し、product出力より前に固定された期待値だけを読む。
    pub(super) fn load(source_sha256: &str) -> Self {
        let raw = std::fs::read(std::env::var("JADE_TYPE1_ATTRIBUTES_GOLD").unwrap()).unwrap();
        let reference: Self = serde_json::from_slice(&raw).unwrap();
        assert_eq!(reference.source_pdf_sha256, source_sha256);
        assert_eq!(reference.fonts.len(), 11);
        reference
    }

    /// 実readerに期待値を供給せず、得られた全属性とprogramの対応を比較する。
    pub(super) fn compare<'bytes>(
        &self,
        budget: &DecodeBudget,
        object: (u32, u16),
        payload: &[u8],
        font: &Font<'_>,
        descriptor: &'bytes [u8],
        resolver: &mut ResourceResolver<'_, 'bytes>,
    ) -> serde_json::Value {
        let (number, generation) = object;
        let expected = self
            .fonts
            .iter()
            .find(|f| f.object_number == number && f.generation == generation)
            .unwrap();
        let hash = hmac_sha256::Hash::hash(payload)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(hash, expected.program_sha256);
        let a = attributes::read(budget, font, descriptor, resolver).unwrap();
        let actual = serde_json::json!({"ascent":a.ascent,"descent":a.descent,"weight":a.weight,"bold":a.bold,"italic":a.italic,"italic_angle":a.italic_angle,"fixed":a.fixed,"underline_position":a.underline_position,"underline_thickness":a.underline_thickness,"x_height":a.x_height,"cap_height":a.cap_height});
        assert_eq!(actual, expected.attributes, "font object {number}");
        println!(
            "font object {number}: all original intrinsic attributes and descriptor metrics match"
        );
        serde_json::json!({"object_number":number,"generation":generation,"attributes":actual})
    }
}
