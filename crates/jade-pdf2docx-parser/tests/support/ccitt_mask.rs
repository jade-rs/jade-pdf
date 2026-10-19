//! CCITT maskの白黒sampleを生成する共通test helper。
use fax::{Color, VecWriter};

/// trueを黒としてGroup 4へ符号化し、終端まで保持する。
pub fn encoded_mask(rows: &[Vec<bool>]) -> Vec<u8> {
    let width = u16::try_from(rows[0].len()).unwrap();
    let mut encoder = fax::encoder::Encoder::new(VecWriter::new());
    for row in rows {
        assert_eq!(row.len(), usize::from(width));
        encoder
            .encode_line(
                row.iter()
                    .map(|&black| if black { Color::Black } else { Color::White }),
                width,
            )
            .unwrap();
    }
    encoder.finish().unwrap().finish()
}
