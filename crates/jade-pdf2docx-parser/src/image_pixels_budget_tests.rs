//! 色変換・palette・SMask・codec出力の画像保存容量回帰。
use super::*;
use fax::Color;
use flate2::{Compression, write::ZlibEncoder};
use proptest::prelude::*;

/// 画像保存容量だけを制限する。
fn budget(capacity: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_image_storage_bytes: capacity,
        ..Default::default()
    })
}

/// 画素fixtureをFlateで圧縮する。
fn compressed(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

/// 符号化出力だけの独立した必要容量を得る。
fn png_cost(rgb: &[u8]) -> u64 {
    64 + encode_rgb8_png(&budget(1024), (rgb.len() / 3) as u32, 1, rgb)
        .unwrap()
        .len() as u64
}

/// GrayのRGB展開はPNG出力と別に画素容量を必要とする。
#[test]
fn gray_pixels_require_storage_before_png_encoding() {
    let target = budget(png_cost(&[17; 6]));
    assert!(
        decode_image_stream_as_png(
            &target,
            b"",
            b"<< /BitsPerComponent 8 /ColorSpace /DeviceGray /Filter /FlateDecode >>",
            &compressed(&[17; 2]),
            2,
            1
        )
        .is_none()
    );
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "image storage bytes",
            ..
        })
    ));
}

/// Indexed展開もpaletteとRGB出力の容量を予約する。
#[test]
fn indexed_pixels_require_storage_before_png_encoding() {
    let target = budget(png_cost(&[17; 6]));
    assert!(decode_image_stream_as_png(&target, b"", b"<< /BitsPerComponent 8 /ColorSpace [/Indexed /DeviceRGB 0 <111111>] /Filter /FlateDecode >>", &compressed(&[0; 2]), 2, 1).is_none());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "image storage bytes",
            ..
        })
    ));
}

/// inline/間接paletteの文字列も画像容量不足で返さない。
#[test]
fn palette_strings_require_storage_capacity() {
    for source in [
        b"[/Indexed /DeviceRGB 0 <111111>]".as_slice(),
        b"[/Indexed /DeviceRGB 0 (abc)]",
    ] {
        let target = budget(2);
        assert!(parse_indexed_device_rgb_palette_from_pdf(&target, b"", source).is_none());
        assert!(matches!(
            target.check(),
            Err(ParseError::ResourceLimit {
                resource: "image storage bytes",
                limit: 2,
                actual: 3
            })
        ));
    }
}

/// 未圧縮paletteとJBIG2 globalsの保存も確保前に制限する。
#[test]
fn binary_image_metadata_copies_require_capacity() {
    let pdf = b"%PDF-1.7\n5 0 obj\n<< /Length 3 >>\nstream\nabc\nendstream\nendobj\n";
    let target = budget(2);
    assert!(read_palette_object_bytes(&target, pdf, 5).is_none());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "image storage bytes",
            limit: 2,
            actual: 3
        })
    ));
    let target = budget(2);
    assert!(jbig2_globals_stream(&target, pdf, "<< /JBIG2Globals 5 0 R >>").is_none());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "image storage bytes",
            limit: 2,
            actual: 3
        })
    ));
}

/// alphaのリサンプル保存は容量ちょうどまで許し、再作成を累積する。
#[test]
fn smask_alpha_storage_is_inclusive_and_cumulative() {
    let mask = compressed(&[10, 20]);
    let mut pdf = format!("%PDF-1.7\n5 0 obj\n<< /Width 2 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray /Filter /FlateDecode /Length {} >>\nstream\n", mask.len()).into_bytes();
    pdf.extend_from_slice(&mask);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    for capacity in [3, 4, 7, 8] {
        let target = budget(capacity);
        for pass in 1..=2 {
            let result = decode_smask_alpha(&target, &pdf, "<< /SMask 5 0 R >>", 4, 1);
            assert_eq!(result.is_some(), pass * 4 <= capacity);
            if let Some(alpha) = result {
                assert_eq!(alpha, [10, 10, 20, 20]);
            }
        }
        assert_eq!(target.check().is_ok(), capacity == 8);
        if capacity != 8 {
            assert!(decode_smask_alpha(&target, &pdf, "<< /SMask 5 0 R >>", 1, 1).is_none());
        }
    }
}

/// CCITTのRGB出力はPNG符号化容量の他に予約する。
#[test]
fn ccitt_pixels_require_storage_before_png_encoding() {
    let mut encoder = fax::encoder::Encoder::new(fax::VecWriter::new());
    encoder
        .encode_line([Color::Black; 2].into_iter(), 2)
        .unwrap();
    let mut writer = encoder.finish().unwrap();
    writer.pad();
    let target = budget(png_cost(&[0; 6]));
    assert!(
        decode_ccitt_fax_as_png(
            &target,
            b"",
            &writer.finish(),
            2,
            1,
            b"<< /K -1 /Columns 2 >>"
        )
        .is_none()
    );
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "image storage bytes",
            ..
        })
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// paletteの解放後の再作成を、独立した要求byte数モデルと比較する。
    #[test]
    fn palette_storage_matches_decoded_length_model(count in 1u64..16) {
        for capacity in [count * 3 - 1, count * 3] {
            let target = budget(capacity);
            for pass in 1..=count {
                let result = parse_indexed_device_rgb_palette_from_pdf(&target, b"", b"[/Indexed /DeviceRGB 0 <112233>]");
                prop_assert_eq!(result.is_some(), pass * 3 <= capacity);
                if let Some(value) = result { prop_assert_eq!(value, [0x11, 0x22, 0x33]); }
            }
            prop_assert_eq!(target.check().is_ok(), capacity == count * 3);
        }
    }
}

/// 作業配列の拡張は旧容量も累積し、境界不足で処理を再開しない。
#[test]
fn jpeg_scratch_growth_is_charged_to_storage_and_shared_allocation() {
    for shared in [false, true] {
        for limit in [8, 9] {
            let target = DecodeBudget::new(&ParseLimits {
                max_total_image_storage_bytes: if shared { 1024 } else { limit },
                max_total_allocation_bytes: if shared { limit } else { 1024 },
                ..Default::default()
            });
            target
                .with_jpeg_pixels(4, |pixels| {
                    pixels.fill(9);
                    Some(())
                })
                .unwrap();
            target
                .with_jpeg_pixels(3, |pixels| {
                    assert_eq!(pixels, [0; 3]);
                    Some(())
                })
                .unwrap();
            assert_eq!(
                target.with_jpeg_pixels(5, |_| Some(())).is_some(),
                limit == 9
            );
            if limit == 8 {
                assert!(target.with_jpeg_pixels(1, |_| Some(())).is_none());
                assert!(matches!(
                    target.check(),
                    Err(ParseError::ResourceLimit { actual: 9, .. })
                ));
            } else {
                assert!(target.check().is_ok());
            }
        }
    }
}

/// JBIG2 callbackのRGB保存は上限ちょうどまで許し、作り直しも累積する。
#[test]
fn jbig2_callback_storage_is_inclusive_and_cumulative() {
    for capacity in [5, 6, 11, 12] {
        let target = budget(capacity);
        for pass in 1..=2 {
            let decoder = RgbJbig2Decoder::new(&target, 2, 1);
            assert_eq!(decoder.is_some(), pass * 6 <= capacity);
            if let Some(mut decoder) = decoder {
                decoder.push_pixel(true);
                decoder.push_pixel(false);
                assert_eq!(decoder.rgb, [0, 0, 0, 255, 255, 255]);
                decoder.push_pixel(true);
                assert_eq!(decoder.rgb.len(), 6);
            }
        }
        assert_eq!(target.check().is_ok(), capacity == 12);
    }
}

/// 画像末尾を越えた行通知・巨大run長でもcallbackを終了し、画素を増やさない。
#[test]
fn jbig2_callbacks_stop_at_last_row() {
    let target = budget(6);
    let mut decoder = RgbJbig2Decoder::new(&target, 2, 1).unwrap();
    decoder.push_pixel_chunk(true, 1);
    decoder.next_line();
    decoder.next_line();
    decoder.push_pixel_chunk(false, u32::MAX);
    decoder.push_pixel(false);
    assert_eq!(decoder.rgb, [0; 6]);
    assert!(target.check().is_ok());
}

/// 巨大なchunk数でも実際の残り画素分だけ処理する。
#[test]
fn jbig2_chunk_work_is_bounded_by_image_width() {
    let target = budget(6);
    let mut decoder = RgbJbig2Decoder::new(&target, 2, 1).unwrap();
    decoder.push_pixel_chunk(true, u32::MAX);
    assert_eq!(decoder.rgb, [0; 6]);
    assert!(target.check().is_ok());
}

/// 合成途中の不足ではopaque PNGへfallbackせず、最初の容量エラーを保持する。
#[test]
fn smask_rgba_failure_does_not_publish_opaque_png() {
    let alpha = compressed(&[0, 128]);
    let mut pdf = format!("%PDF-1.7\n5 0 obj\n<< /Width 2 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceGray /Filter /FlateDecode /Length {} >>\nstream\n", alpha.len()).into_bytes();
    pdf.extend_from_slice(&alpha);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    let pixels = [10, 20, 30, 40, 50, 60];
    let rgba = [10, 20, 30, 0, 40, 50, 60, 128];
    let png = encode_rgba8_png(&budget(1024), 2, 1, &rgba).unwrap();
    let cost = 2 + 8 + 64 + png.len() as u64;
    let dict =
        b"<< /BitsPerComponent 8 /ColorSpace /DeviceRGB /Filter /FlateDecode /SMask 5 0 R >>";
    for capacity in [1, 2, cost - 1, cost, 2 * cost - 1, 2 * cost] {
        let target = budget(capacity);
        for pass in 1..=2 {
            let result =
                decode_image_stream_as_png(&target, &pdf, dict, &compressed(&pixels), 2, 1);
            assert_eq!(result.is_some(), pass * cost <= capacity);
            if let Some(result) = result {
                assert_eq!(result, png);
            }
        }
        if capacity == 1 || capacity == 2 {
            assert!(decode_dct_jpeg_as_png(&target, b"invalid").is_none());
        }
        let error = target.check();
        assert_eq!(error.is_ok(), capacity == 2 * cost);
        if capacity == 1 || capacity == 2 {
            assert!(
                matches!(error, Err(ParseError::ResourceLimit { resource: "image storage bytes", actual, .. }) if actual == if capacity == 1 { 2 } else { 10 })
            );
            assert!(decode_dct_jpeg_as_png(&target, b"invalid").is_none());
            assert!(target.is_stopped());
        }
    }
    let decoded = decode_filter0_png_to_rgba(&budget(1024), &png).unwrap();
    assert_eq!((decoded.width, decoded.height), (2, 1));
    assert_eq!(decoded.rgba, rgba);
}

/// PDF文字列の容量はescape・改行・奇数nibbleを復号した長さに等しい。
#[test]
fn binary_strings_reserve_decoded_length_and_preserve_pdf_semantics() {
    for (source, expected) in [
        (b"<1 2 3>".as_slice(), b"\x12\x30".as_slice()),
        (
            b"(a\\053(b)\\\r\n\\n\\r\\t\\b\\f\\400\\q)",
            b"a+(b)\n\r\t\x08\x0c\0q",
        ),
        (b"(a\r\nb\rc\nd)", b"a\nb\nc\nd"),
    ] {
        let cost = expected.len() as u64;
        for capacity in [cost - 1, cost, 2 * cost - 1, 2 * cost] {
            let target = budget(capacity);
            for pass in 1..=2 {
                let result = crate::image_metadata::string_bytes(&target, source);
                assert_eq!(result.is_some(), pass * cost <= capacity);
                if let Some(result) = result {
                    assert_eq!(result, expected);
                }
            }
            assert_eq!(target.check().is_ok(), capacity == 2 * cost);
        }
    }
    for invalid in [b"<fg>".as_slice(), b"(unterminated", b"(ok) (extra)"] {
        let target = budget(1);
        assert!(crate::image_metadata::string_bytes(&target, invalid).is_none());
        assert_eq!(
            crate::image_metadata::string_bytes(&target, b"(x)"),
            Some(vec![b'x'])
        );
        assert!(target.check().is_ok());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// RGB借用・Gray展開・Indexed展開を、独立した画素容量とPNG内容のモデルで比較する。
    #[test]
    fn color_conversion_storage_preserves_pixels_and_matches_model(
        width in 1usize..9, value in any::<u8>(), count in 1u64..5,
    ) {
        let rgb = vec![value; width * 3];
        let png = encode_rgb8_png(&budget(1024), width as u32, 1, &rgb).unwrap();
        let png_cost = 64 + png.len() as u64;
        for mode in 0..3 {
            let (color_space, input, pixels_cost) = match mode {
                0 => ("/DeviceRGB".to_string(), rgb.clone(), 0),
                1 => ("/DeviceGray".to_string(), vec![value; width], (width * 3) as u64),
                _ => (format!("[/Indexed /DeviceRGB 0 <{value:02x}{value:02x}{value:02x}>]"), vec![0; width], 3 + (width * 3) as u64),
            };
            let dict = format!("<< /BitsPerComponent 8 /ColorSpace {color_space} /Filter /FlateDecode >>");
            let cost = pixels_cost + png_cost;
            for capacity in [count * cost - 1, count * cost] {
                let target = budget(capacity);
                for pass in 1..=count {
                    let result = decode_image_stream_as_png(&target, b"", dict.as_bytes(), &compressed(&input), width as u32, 1);
                    prop_assert_eq!(result.is_some(), pass * cost <= capacity);
                    if let Some(result) = result { prop_assert_eq!(result.as_slice(), png.as_slice()); }
                }
                prop_assert_eq!(target.check().is_ok(), capacity == count * cost);
            }
        }
    }
}
