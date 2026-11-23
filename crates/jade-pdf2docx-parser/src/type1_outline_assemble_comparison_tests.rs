//! 期待metadataを渡さず、原本PDF resourceから生成した全fontを外部検査へ保存する。
use crate::{
    decode::DecodeBudget,
    resource_resolver::ResourceResolver,
    type1_outline::{self, Input},
};

/// 原本一resourceから生成したfontと元codeの出力先を、一度だけ保存する。
pub(super) fn export<'bytes>(
    budget: &DecodeBudget,
    number: u32,
    input: Input<'_>,
    dictionary: &'bytes [u8],
    descriptor: &'bytes [u8],
    resolver: &mut ResourceResolver<'_, 'bytes>,
) -> serde_json::Value {
    let program_sha256 = sha256(input.program);
    let prepared = type1_outline::prepare(budget, input, dictionary, descriptor, resolver).unwrap();
    let directory =
        std::path::PathBuf::from(std::env::var_os("JADE_TYPE1_ASSEMBLED_EXPORT_DIR").unwrap());
    let mut fonts = Vec::new();
    for (index, font) in prepared.fonts.iter().enumerate() {
        let name = format!("font{number}-face{index}.otf");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join(&name))
            .unwrap();
        std::io::Write::write_all(&mut file, &font.bytes).unwrap();
        fonts.push(
            serde_json::json!({"file":name,"family":font.font_name,"sha256":sha256(&font.bytes)}),
        );
    }
    let routes = prepared.routes.to_vec();
    println!(
        "font object {number}: {} actual source-derived faces",
        fonts.len()
    );
    serde_json::json!({"object_number":number,"program_sha256":program_sha256,"fonts":fonts,"code_routes":routes})
}

/// 保存した実byte列を、外部readerへ渡す前に固定する。
fn sha256(bytes: &[u8]) -> String {
    hmac_sha256::Hash::hash(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
