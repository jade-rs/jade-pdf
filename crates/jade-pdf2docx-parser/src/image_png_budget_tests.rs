//! PNG保存・配置コピー・変形停止の容量回帰。
use super::*;
use proptest::prelude::*;
use std::io::Read;

/// 圧縮bufferの初期64 byteと完成PNGの予約容量を境界ちょうどまで許す。
#[test]
fn encoder_capacity_includes_compressed_output_and_final_png() {
    let png = encode_rgb8_png(&budget(1024), 1, 1, &[255; 3]).unwrap();
    let cost = 64 + png.len() as u64;
    for capacity in [cost - 1, cost, 2 * cost - 1, 2 * cost] {
        let target = budget(capacity);
        for pass in 1..=2 {
            let result = encode_rgb8_png(&target, 1, 1, &[255; 3]);
            assert_eq!(result.is_some(), pass * cost <= capacity);
            if let Some(result) = result {
                assert_eq!(result, png);
            }
        }
        assert_eq!(target.check().is_ok(), capacity == 2 * cost);
    }
    assert_eq!(&png[29..33], &0x907753deu32.to_be_bytes());
    let length = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
    let mut decoded = Vec::new();
    flate2::read::ZlibDecoder::new(&png[41..41 + length])
        .read_to_end(&mut decoded)
        .unwrap();
    assert_eq!(decoded, [0, 255, 255, 255]);
    assert_eq!(&png[png.len() - 4..], &0xae426082u32.to_be_bytes());
}

/// 拡張では旧容量との差分でなく全容量を加算し、失敗時のprefixを変更しない。
#[test]
fn compressed_buffer_growth_is_atomic_and_cumulative() {
    for capacity in [191, 192] {
        let target = budget(capacity);
        let mut bytes = Vec::new();
        image_png::append(&target, &mut bytes, &[7; 64]).unwrap();
        let result = image_png::append(&target, &mut bytes, &[8]);
        assert_eq!(result.is_some(), capacity == 192);
        if capacity == 191 {
            assert_eq!(bytes, [7; 64]);
            assert!(matches!(
                target.check(),
                Err(ParseError::ResourceLimit {
                    resource: "image storage bytes",
                    limit: 191,
                    actual: 192
                })
            ));
            assert!(image_png::append(&target, &mut bytes, &[]).is_none());
        } else {
            assert_eq!(bytes.len(), 65);
            assert!(target.check().is_ok());
        }
    }
}

/// PNG読み戻しのIDAT保存とRGBA配列も別の展開容量とは独立に数える。
#[test]
fn png_decode_storage_is_bounded_inclusively() {
    let png = encode_rgb8_png(&budget(1024), 1, 1, &[255; 3]).unwrap();
    for capacity in [67, 68] {
        let target = budget(capacity);
        assert_eq!(
            decode_filter0_png_to_rgba(&target, &png).is_some(),
            capacity == 68
        );
        assert_eq!(target.check().is_ok(), capacity == 68);
    }
}

/// 容量確保そのものの失敗でも停止を保持する。
#[test]
fn image_allocation_failure_remains_stopped() {
    let target = budget(u64::MAX);
    assert!(target.image_vector::<u8>(usize::MAX).is_none());
    assert!(
        matches!(target.check(), Err(ParseError::Io(error)) if error.kind() == std::io::ErrorKind::OutOfMemory)
    );
    assert!(image_png::copy(&target, &[]).is_none());
}

/// 先行する配置が完成していても後続PNGコピーの不足では全画像を破棄する。
#[test]
fn extraction_discards_completed_images_when_later_copy_exceeds_capacity() {
    let stream = crate::structure::StreamSpan {
        dictionary: b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode >>",
        payload: b"x",
        object_offset: None,
        object_number: Some(100),
    };
    let png = fallback_image_png_bytes(&budget(1024), 1, 1).unwrap();
    let generation = 3 + 64 + png.len() as u64;
    for capacity in [
        generation + png.len() as u64 - 1,
        generation + png.len() as u64,
    ] {
        let target = budget(capacity);
        let mut quality = QualityReport::default();
        let images = extract_image_elements_from_pdf_bytes(
            &target,
            b"%PDF-1.7\n%%EOF",
            std::slice::from_ref(&stream),
            &mut quality,
        );
        assert_eq!(
            images.len(),
            usize::from(capacity == generation + png.len() as u64)
        );
        assert_eq!(target.check().is_ok(), !images.is_empty());
    }
    let streams = (0..21)
        .map(|number| crate::structure::StreamSpan {
            dictionary:
                b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode >>",
            payload: b"x",
            object_offset: None,
            object_number: Some(number),
        })
        .collect::<Vec<_>>();
    let target = budget(22 * one_pixel_png_bytes().len() as u64);
    let mut quality = QualityReport::default();
    assert!(
        extract_image_elements_from_pdf_bytes(&target, b"%PDF-1.7\n%%EOF", &streams, &mut quality)
            .is_empty()
    );
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "image storage bytes",
            ..
        })
    ));
}

/// 画像保存だけを制限する共有予算を作る。
fn budget(capacity: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_image_storage_bytes: capacity,
        ..Default::default()
    })
}

/// 画像コピーに使う既存PNGを持つstream情報を作る。
fn entry() -> ImageStreamEntry {
    ImageStreamEntry {
        stencil: false,
        object_number: Some(100),
        png_bytes: one_pixel_png_bytes().to_vec(),
        width: 1,
        height: 1,
    }
}

/// 単純配置を作り、停止時には部分結果を返さない。
fn place(target: &DecodeBudget, stream: &ImageStreamEntry) -> Option<ParsedImageElement> {
    stream.to_element(
        target,
        &ImagePlacement {
            object_number: 100,
            page: 0,
            bounds: ParsedBounds {
                x0: 0.,
                y0: 0.,
                x1: 1.,
                y1: 1.,
            },
            floating: false,
            rotation_degrees: 0,
            matrix: [1., 0., 0., 1., 0., 0.],
            paint: image_draws::ImagePaint::default(),
        },
    )
}

/// コピー容量はinclusiveで、解放後の別配置も累積へ数える。
#[test]
fn png_placement_copies_are_inclusive_and_cumulative() {
    let stream = entry();
    let length = stream.png_bytes.len() as u64;
    for capacity in [length - 1, length, 2 * length - 1, 2 * length] {
        let target = budget(capacity);
        for pass in 1..=3 {
            let image = place(&target, &stream);
            assert_eq!(image.is_some(), pass * length <= capacity);
            if let Some(image) = image {
                assert_eq!(image.png_bytes, stream.png_bytes);
                assert_eq!((image.width_px, image.height_px), (1, 1));
            }
        }
        assert!(
            matches!(target.check(), Err(ParseError::ResourceLimit { resource: "image storage bytes", limit, .. }) if limit == capacity)
        );
        assert!(place(&target, &stream).is_none());
    }
}

/// 最初の画素超過後はPNGコピーへfallbackせず、元の資源エラーを保持する。
#[test]
fn warp_limit_does_not_fall_back_to_png_copy() {
    let stream = entry();
    let target = DecodeBudget::new(&ParseLimits {
        max_image_pixels: 1,
        ..Default::default()
    });
    let image: Option<ParsedImageElement> = stream.to_element(
        &target,
        &ImagePlacement {
            object_number: 100,
            page: 0,
            bounds: ParsedBounds {
                x0: 0.,
                y0: 0.,
                x1: 2.,
                y1: 2.,
            },
            floating: true,
            rotation_degrees: 0,
            matrix: [1., 0., 0.2, 1., 0., 0.],
            paint: image_draws::ImagePaint::default(),
        },
    );
    assert!(image.is_none());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "image pixels",
            ..
        })
    ));
    assert!(place(&target, &stream).is_none());
}

/// 未対応filterのPNG生成も確保前に画像保存容量を要求する。
#[test]
fn fallback_png_requires_storage_capacity() {
    let target = budget(0);
    let mut quality = QualityReport::default();
    let images = extract_image_elements_from_pdf_bytes(
        &target,
        b"%PDF-1.7\n%%EOF",
        &[crate::structure::StreamSpan {
            dictionary:
                b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode >>",
            payload: b"x",
            object_offset: None,
            object_number: Some(100),
        }],
        &mut quality,
    );
    assert!(images.is_empty());
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "image storage bytes",
            limit: 0,
            ..
        })
    ));
}

/// 未対応filterを持つ多数画像でも、代替PNGの保存には容量が必要。
#[test]
fn unsupported_images_require_placeholder_storage_capacity() {
    let streams = (0..21)
        .map(|number| crate::structure::StreamSpan {
            dictionary:
                b"<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode >>",
            payload: b"x",
            object_offset: None,
            object_number: Some(number),
        })
        .collect::<Vec<_>>();
    let target = budget(0);
    let mut quality = QualityReport::default();
    assert!(
        extract_image_elements_from_pdf_bytes(&target, b"%PDF-1.7\n%%EOF", &streams, &mut quality)
            .is_empty()
    );
    assert!(matches!(
        target.check(),
        Err(ParseError::ResourceLimit {
            resource: "image storage bytes",
            limit: 0,
            ..
        })
    ));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    /// RGB/RGBAの行境界・filter byteと画素列を独立した展開結果で照合する。
    #[test]
    fn encoded_rows_preserve_pixel_bytes(width in 1usize..12, height in 1usize..12, alpha in any::<bool>(), input in prop::collection::vec(any::<u8>(), 600)) {
        let channels = if alpha { 4 } else { 3 };
        let count = width * height * channels;
        let png = image_png::encode(&budget(16384), width as u32, height as u32, &input[..count], channels, if alpha { 6 } else { 2 }).unwrap();
        let length = u32::from_be_bytes(png[33..37].try_into().unwrap()) as usize;
        let mut decoded = Vec::new();
        flate2::read::ZlibDecoder::new(&png[41..41 + length]).read_to_end(&mut decoded).unwrap();
        prop_assert_eq!(decoded.len(), (width * channels + 1) * height);
        for (row, expected) in decoded.chunks_exact(width * channels + 1).zip(input[..count].chunks_exact(width * channels)) {
            prop_assert_eq!(row[0], 0);
            prop_assert_eq!(&row[1..], expected);
        }
    }
    /// 独立したbyte数モデルと配置コピーの予算境界を比較する。
    #[test]
    fn png_copy_bytes_match_independent_model(count in 1u64..24) {
        let stream = entry();
        let cost = count * stream.png_bytes.len() as u64;
        for maximum in [cost - 1, cost] {
            let target = budget(maximum);
            for pass in 1..=count {
                let image = place(&target, &stream);
                prop_assert_eq!(image.is_some(), pass * stream.png_bytes.len() as u64 <= maximum);
            }
            prop_assert_eq!(target.check().is_ok(), maximum == cost);
        }
    }
}
