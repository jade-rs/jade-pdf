//! SDK の ABI 検証に使う PDF をメモリ内で生成する。

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    /// 保存済み PDF の除去後も検証入力のバイト列を固定する。
    #[test]
    fn generated_probes_match_qualified_bytes() {
        for (bytes, expected) in [
            (
                character_box(),
                "07c19041f8e83854ea6062870caa20d7232ffba16faf5032c6c5e2c166a7cf0b",
            ),
            (
                actual_text(),
                "bd44a0712dffbc72ba12a58db641e65a9dec06ba07d548ae67adf89184322e91",
            ),
            (
                marked_scope(),
                "fccaa715e4b0da3a3106480778777d80da9aecda735bca0f78663a8dc1c4ec09",
            ),
        ] {
            assert_eq!(format!("{:x}", Sha256::digest(&bytes)), expected);
            assert!(bytes.len() <= 2048);
        }
    }
}

/// 外部フォント不要の Type3 文字で座標 ABI を検証する。
#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn character_box() -> Vec<u8> {
    document(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>",
        "<< /Type /Font /Subtype /Type3 /FontBBox [0 0 400 700] /FontMatrix [.001 0 0 .001 0 0] /CharProcs << /A 6 0 R >> /Encoding << /Type /Encoding /Differences [65 /A] >> /FirstChar 65 /LastChar 65 /Widths [500] /Resources << >> >>",
        "<< /Length 32 >>\nstream\nBT /F1 20 Tf 60 100 Td (A) Tj ET\nendstream",
        "<< /Length 37 >>\nstream\n500 0 0 0 400 700 d1 0 0 400 700 re f\nendstream",
    ])
}

/// 不完全な UTF-16 末尾を含む文字列で生バイト取得を検証する。
#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn actual_text() -> Vec<u8> {
    document(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>",
        "<< /Length 61 >>\nstream\n/Span << /ActualText <FEFF004100> >> BDC 10 10 20 20 re f EMC\nendstream",
    ])
}

/// 同じプロパティを持つ別スコープのマーク識別を検証する。
pub(crate) fn marked_scope() -> Vec<u8> {
    document(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /Font << /F1 5 0 R >> /Properties << /Same << /ActualText (XY) >> >> >> /Contents 4 0 R >>",
        "<< /Length 183 >>\nstream\n/Span /Same BDC BT /F1 12 Tf 10 20 Td (C) Tj ET /Nested BMC BT /F1 12 Tf 20 20 Td (D) Tj ET EMC EMC /Span /Same BDC BT /F1 12 Tf 30 20 Td (E) Tj ET BT /F1 12 Tf 40 20 Td (F) Tj ET EMC\nendstream",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ])
}

/// オブジェクト長から相互参照表を組み立てる。
fn document(objects: &[&str]) -> Vec<u8> {
    let mut source = String::from("%PDF-1.7\n");
    let mut offsets = Vec::with_capacity(objects.len());
    for (index, object) in objects.iter().enumerate() {
        offsets.push(source.len());
        source.push_str(&format!("{} 0 obj\n{object}\nendobj\n", index + 1));
    }
    let xref = source.len();
    let size = objects.len() + 1;
    source.push_str(&format!("xref\n0 {size}\n0000000000 65535 f \n"));
    for offset in offsets {
        source.push_str(&format!("{offset:010} 00000 n \n"));
    }
    source.push_str(&format!(
        "trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    ));
    source.into_bytes()
}
