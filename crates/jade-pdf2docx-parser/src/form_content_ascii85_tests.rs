//! 実際のReportLab入力が使うASCII85→FlateのContents/Form本文を復元する回帰。
use super::*;

/// 固定4byteを85進数へ展開する、テスト側の独立したencoder。
fn encode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(4) {
        let mut bytes = [0; 4];
        bytes[..chunk.len()].copy_from_slice(chunk);
        let mut value = u32::from_be_bytes(bytes);
        let mut digits = [0; 5];
        for digit in digits.iter_mut().rev() {
            *digit = (value % 85) as u8 + b'!';
            value /= 85;
        }
        out.extend_from_slice(&digits[..chunk.len() + 1]);
    }
    out.extend_from_slice(b"~>");
    out
}

/// 通常のzlib出力と、そのASCII85表現を同時に返す。
fn compressed(data: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    let bytes = encoder.finish().unwrap();
    let encoded = encode(&bytes);
    (bytes, encoded)
}

/// ページとFormの双方を順に復号し、無内容の成功で画像を落とさない。
#[test]
fn ascii85_flate_page_and_form_keep_called_images() {
    let mut objects = objects("/Fm Do", "/Im Do", "");
    let (_, page) = compressed(b"/Fm Do");
    let (_, form) = compressed(b"8 0 0 6 1 2 cm /Im Do");
    objects[3] = stream("/Filter [/ASCII85Decode /FlateDecode]", &page);
    objects[4] = stream(
        "/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << /XObject << /Im 6 0 R >> >> /Filter [/ASCII85Decode /FlateDecode]",
        &form,
    );
    let parsed = parse_pdf_bytes("owned-ascii85-form.pdf", &document(&objects)).unwrap();
    assert_eq!(
        parsed.image_elements.len(),
        1,
        "ASCII85 Flate content chain must retain called image"
    );
}

/// 二段の展開量をどちらも累積し、一段分の容量だけで成功しない。
#[test]
fn ascii85_flate_counts_both_decoded_stages() {
    let text = b"BT (owned content) Tj ET";
    let (compressed, encoded) = compressed(text);
    let total = (compressed.len() + text.len()) as u64;
    for capacity in [total - 1, total] {
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_decoded_bytes: capacity,
            ..Default::default()
        });
        let result = decode_content_stream_payload(
            &budget,
            b"<< /Filter [/ASCII85Decode /FlateDecode] >>",
            &encoded,
        );
        if capacity == total {
            assert_eq!(
                result.as_deref(),
                Some(text.as_slice()),
                "ASCII85 Flate stages must decode in declared order"
            );
            budget.check().unwrap();
        } else {
            assert!(result.is_none());
            assert!(matches!(
                budget.check(),
                Err(ParseError::ResourceLimit {
                    resource: "total decoded bytes",
                    ..
                })
            ));
        }
    }
}
