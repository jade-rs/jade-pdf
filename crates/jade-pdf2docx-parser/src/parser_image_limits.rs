//! 画像の内部寸法と描画先 allocation の上限回帰。
use super::{
    decode_ccitt_fax_as_png, decode_filter0_png_to_rgba, encode_rgb8_png, warp_png_to_ctm_bounds,
};
use crate::{ParseError, ParseLimits, ParsedBounds, decode::DecodeBudget};

/// 有効な CCITT が辞書より大きい寸法で decode されても上限ちょうどだけを許可する。
#[test]
fn valid_ccitt_decode_parameters_are_bounded_inclusively() {
    let mut encoder = fax::encoder::Encoder::new(fax::VecWriter::new());
    for _ in 0..2 {
        encoder
            .encode_line([fax::Color::Black; 3].into_iter(), 3)
            .unwrap();
    }
    let mut writer = encoder.finish().unwrap();
    writer.pad();
    let data = writer.finish();
    let parameters = "<< /Columns 3 /Rows 2 /K -1 >>";
    let budget = DecodeBudget::new(&ParseLimits {
        max_image_pixels: 6,
        ..Default::default()
    });
    let png = decode_ccitt_fax_as_png(&budget, b"", &data, 1, 2, parameters.as_bytes()).unwrap();
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 1);
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 2);
    assert!(budget.check().is_ok());
    let budget = DecodeBudget::new(&ParseLimits {
        max_image_pixels: 5,
        ..Default::default()
    });
    assert!(decode_ccitt_fax_as_png(&budget, b"", &data, 1, 2, parameters.as_bytes()).is_none());
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "image pixels",
            actual: 6,
            limit: 5
        })
    ));
}

/// 未知行数でも終端優先でも、実際の画像高さを確保前に検査する。
#[test]
fn ccitt_unknown_and_overridden_rows_use_image_pixel_budget() {
    let mut encoder = fax::encoder::Encoder::new(fax::VecWriter::new());
    for _ in 0..2 {
        encoder
            .encode_line([fax::Color::Black; 3].into_iter(), 3)
            .unwrap();
    }
    let data = encoder.finish().unwrap().finish();
    for rows in [0, 1, 65_536] {
        let parameters = format!("<< /K -1 /Columns 3 /Rows {rows} >>");
        let budget = DecodeBudget::new(&ParseLimits {
            max_image_pixels: 6,
            ..Default::default()
        });
        assert!(
            decode_ccitt_fax_as_png(&budget, b"", &data, 3, 2, parameters.as_bytes()).is_some()
        );
        assert!(budget.check().is_ok());
        let budget = DecodeBudget::new(&ParseLimits {
            max_image_pixels: 5,
            ..Default::default()
        });
        assert!(
            decode_ccitt_fax_as_png(&budget, b"", &data, 3, 2, parameters.as_bytes()).is_none()
        );
        assert!(matches!(
            budget.check(),
            Err(ParseError::ResourceLimit {
                resource: "image pixels",
                actual: 6,
                limit: 5
            })
        ));
    }
}

/// Columns省略時の1728画素も、画像辞書の幅とは独立に予算へ含める。
#[test]
fn ccitt_default_columns_require_the_standard_width_budget() {
    let mut encoder = fax::encoder::Encoder::new(fax::VecWriter::new());
    encoder
        .encode_line(std::iter::repeat_n(fax::Color::White, 1728), 1728)
        .unwrap();
    let data = encoder.finish().unwrap().finish();
    let budget = DecodeBudget::new(&ParseLimits {
        max_image_pixels: 1728,
        ..Default::default()
    });
    let png = decode_ccitt_fax_as_png(&budget, b"", &data, 3, 1, b"<< /K -1 >>").unwrap();
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 3);
    assert!(budget.check().is_ok());
    let budget = DecodeBudget::new(&ParseLimits {
        max_image_pixels: 1727,
        ..Default::default()
    });
    assert!(decode_ccitt_fax_as_png(&budget, b"", &data, 3, 1, b"<< /K -1 >>").is_none());
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "image pixels",
            actual: 1728,
            limit: 1727
        })
    ));
}

/// byte strideから算出するfilter行数を、画像高さと独立に制限する。
#[test]
fn ccitt_filter_stride_uses_decoded_pixel_budget() {
    let mut encoder = fax::encoder::Encoder::new(fax::VecWriter::new());
    for _ in 0..4 {
        encoder
            .encode_line([fax::Color::White; 8].into_iter(), 8)
            .unwrap();
    }
    let data = encoder.finish().unwrap().finish();
    for limit in [32, 31] {
        let budget = DecodeBudget::new(&ParseLimits {
            max_image_pixels: limit,
            ..Default::default()
        });
        let result = decode_ccitt_fax_as_png(&budget, b"", &data, 9, 2, b"<< /K -1 /Columns 8 >>");
        if limit == 32 {
            let png = result.unwrap();
            assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 9);
            assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 2);
            assert!(budget.check().is_ok());
        } else {
            assert!(result.is_none());
            assert!(matches!(
                budget.check(),
                Err(ParseError::ResourceLimit {
                    resource: "image pixels",
                    actual: 32,
                    limit: 31,
                })
            ));
        }
    }
}

/// 寸法変換用RGBも累積保存予算へ予約してから確保する。
#[test]
fn ccitt_remapped_rgb_is_reserved_before_allocation() {
    let mut encoder = fax::encoder::Encoder::new(fax::VecWriter::new());
    for _ in 0..4 {
        encoder
            .encode_line([fax::Color::White; 8].into_iter(), 8)
            .unwrap();
    }
    let data = encoder.finish().unwrap().finish();
    let budget = DecodeBudget::new(&ParseLimits {
        max_total_image_storage_bytes: 149,
        ..Default::default()
    });
    assert!(
        decode_ccitt_fax_as_png(&budget, b"", &data, 9, 2, b"<< /K -1 /Columns 8 >>").is_none()
    );
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "image storage bytes",
            actual: 150,
            limit: 149,
        })
    ));
}

/// 画像寸法による画素予算もfilter出力確保前に検査する。
#[test]
fn ccitt_image_dimensions_are_checked_before_decoding() {
    let budget = DecodeBudget::new(&ParseLimits {
        max_image_pixels: 63,
        ..Default::default()
    });
    assert!(
        decode_ccitt_fax_as_png(
            &budget,
            b"",
            b"",
            16,
            4,
            b"<< /K -1 /Columns 3 /Rows 8 /EndOfBlock false >>"
        )
        .is_none()
    );
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "image pixels",
            actual: 64,
            limit: 63,
        })
    ));
}

/// PNG の寸法も PDF 辞書とは独立に上限を検査する。
#[test]
fn png_pixels_are_bounded_inclusively() {
    let png = encode_rgb8_png(&DecodeBudget::default(), 3, 2, &[0; 18]).unwrap();
    let budget = DecodeBudget::new(&ParseLimits {
        max_image_pixels: 6,
        ..Default::default()
    });
    assert!(decode_filter0_png_to_rgba(&budget, &png).is_some());
    assert!(budget.check().is_ok());
    let budget = DecodeBudget::new(&ParseLimits {
        max_image_pixels: 5,
        ..Default::default()
    });
    assert!(decode_filter0_png_to_rgba(&budget, &png).is_none());
    assert!(matches!(
        budget.check(),
        Err(ParseError::ResourceLimit {
            resource: "image pixels",
            limit: 5,
            actual: 6
        })
    ));
}

/// 小さい原画像でも CTM による描画先拡大を allocation 前に拒否する。
#[test]
fn warped_output_pixels_are_bounded_inclusively() {
    let png = encode_rgb8_png(&DecodeBudget::default(), 1, 1, &[0; 3]).unwrap();
    for limit in [6, 5] {
        let budget = DecodeBudget::new(&ParseLimits {
            max_image_pixels: limit,
            ..Default::default()
        });
        let result = warp_png_to_ctm_bounds(
            &budget,
            &png,
            [2.0, 0.0, 1.0, 2.0, 0.0, 0.0],
            ParsedBounds {
                x0: 0.0,
                y0: 0.0,
                x1: 3.0,
                y1: 2.0,
            },
            3,
            2,
        );
        if limit == 6 {
            assert!(result.is_some());
            assert!(budget.check().is_ok());
        } else {
            assert!(result.is_none());
            assert!(matches!(
                budget.check(),
                Err(ParseError::ResourceLimit {
                    resource: "image pixels",
                    limit: 5,
                    actual: 6
                })
            ));
        }
    }
}

/// 追加行のRGB容量を拡張する前に、再確保分の累積予算を予約する。
#[test]
fn ccitt_extra_rows_reserve_image_storage_before_growth() {
    let mut encoder = fax::encoder::Encoder::new(fax::VecWriter::new());
    for _ in 0..2 {
        encoder
            .encode_line([fax::Color::White; 3].into_iter(), 3)
            .unwrap();
    }
    let data = encoder.finish().unwrap().finish();
    for dictionary in [
        b"<< /K -1 /Columns 3 >>".as_slice(),
        b"<< /K -1 /Columns 3 /Rows 0 /EndOfBlock false >>".as_slice(),
    ] {
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_image_storage_bytes: 26,
            ..Default::default()
        });
        assert!(decode_ccitt_fax_as_png(&budget, b"", &data, 3, 1, dictionary).is_none());
        assert!(matches!(
            budget.check(),
            Err(ParseError::ResourceLimit {
                resource: "image storage bytes",
                limit: 26,
                actual: 27
            })
        ));
    }
}
