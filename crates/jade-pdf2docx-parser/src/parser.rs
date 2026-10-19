use std::{
    borrow::Cow,
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    rc::Rc,
};

use hayro_jbig2::{Decoder as Jbig2Decoder, Image as Jbig2Image};
use zune_jpeg::{
    JpegDecoder,
    zune_core::{colorspace::ColorSpace, options::DecoderOptions},
};

use crate::{
    ImageReplacementReason, ParseOptions, QualityIssue, QualityReport,
    decode::{DecodeBudget, decode_zlib, decode_zlib_font},
    font_repair::repair_sfnt_unicode_cmap,
    image_filter::ImageFilter,
    limits::{ParseLimits, check_limit},
    model::{
        ParseError, ParsedBounds, ParsedDocument, ParsedEmbeddedFont, ParsedGraphicElement,
        ParsedImageElement, ParsedPage, ParsedTextElement, PdfVersion,
    },
    object_bytes::ObjectBytes,
    text::{
        ToUnicodeCMap, decode_text_show_operand, extract_text_preview_from_pdf_bytes,
        parse_font_resource_encodings, parse_tounicode_body, parse_tounicode_cmap,
    },
};

#[cfg(test)]
#[path = "graphics_budget_tests.rs"]
mod graphics_budget_tests;

#[path = "graphics_draws.rs"]
mod graphics_draws;
#[path = "graphics_merge.rs"]
mod graphics_merge;

/// 罫線本文を共有予算付きで抽出し、失敗時は部分結果を破棄する。
#[cfg(test)]
fn graphics_with_budget(
    budget: &DecodeBudget,
    page: usize,
    content: &[u8],
) -> Vec<ParsedGraphicElement> {
    let mut output = Vec::new();
    if graphics_draws::append(budget, page, content, &mut output).is_none() {
        return Vec::new();
    }
    output
}

#[cfg(test)]
#[path = "content_bytes_tests.rs"]
mod content_bytes_tests;

#[cfg(test)]
#[path = "text_result_budget_tests.rs"]
mod text_result_budget_tests;

#[cfg(test)]
#[path = "parser_stream_offsets_budget_tests.rs"]
mod stream_offsets_budget_tests;

#[cfg(test)]
#[path = "compressed_storage_tests.rs"]
mod compressed_storage_tests;

#[cfg(test)]
#[path = "font_global_budget_tests.rs"]
mod font_global_budget_tests;

#[cfg(test)]
#[path = "embedded_font_metadata_tests.rs"]
mod embedded_font_metadata_tests;

#[cfg(test)]
#[path = "font_resource_budget_tests.rs"]
mod font_resource_budget_tests;

#[cfg(test)]
#[path = "image_diagnostic_budget_tests.rs"]
mod image_diagnostic_budget_tests;
#[cfg(test)]
#[path = "image_draw_budget_tests.rs"]
mod image_draw_budget_tests;
#[cfg(test)]
#[path = "image_storage_budget_tests.rs"]
mod image_storage_budget_tests;

#[path = "image_ccitt.rs"]
mod image_ccitt;
#[cfg(test)]
#[path = "image_pixels_budget_tests.rs"]
mod image_pixels_budget_tests;
#[cfg(test)]
use image_ccitt::decode as decode_ccitt_fax_as_png;
#[path = "image_ascii85.rs"]
mod image_ascii85;
#[path = "image_codec_decode.rs"]
mod image_codec_decode;
#[path = "image_filter_chain.rs"]
mod image_filter_chain;
#[path = "image_icc.rs"]
mod image_icc;
#[path = "image_paint_pixels.rs"]
mod image_paint_pixels;
#[path = "image_separation.rs"]
mod image_separation;
#[path = "image_smask.rs"]
mod image_smask;
#[path = "image_stencil.rs"]
mod image_stencil;
#[cfg(test)]
use image_smask::decode_alpha as decode_smask_alpha;
#[cfg(test)]
#[path = "image_ccitt_smask_tests.rs"]
mod image_ccitt_smask_tests;
#[cfg(test)]
#[path = "image_matte_tests.rs"]
mod image_matte_tests;
#[cfg(test)]
#[path = "image_png_budget_tests.rs"]
mod image_png_budget_tests;
#[cfg(test)]
#[path = "image_predictor_tests.rs"]
mod image_predictor_tests;
#[cfg(test)]
#[path = "image_sample_decode_tests.rs"]
mod image_sample_decode_tests;
#[cfg(test)]
#[path = "image_smask_tests.rs"]
mod image_smask_tests;

#[path = "image_draws.rs"]
mod image_draws;
#[path = "image_pixels.rs"]
mod image_pixels;
#[path = "image_png.rs"]
mod image_png;
#[path = "image_predictor.rs"]
mod image_predictor;
#[path = "image_sample_decode.rs"]
mod image_sample_decode;

#[path = "compressed_objects.rs"]
mod compressed_objects;
#[path = "embedded_fonts.rs"]
mod embedded_fonts;
#[path = "font_descriptor_cmaps.rs"]
mod font_descriptor_cmaps;

#[path = "font_cmaps.rs"]
mod font_cmaps;
#[path = "font_definitions.rs"]
mod font_definitions;
#[path = "font_local.rs"]
mod font_local;

#[path = "font_local_cache.rs"]
mod font_local_cache;
use font_local::{Resources as PageFontResources, build as font_resources_from_definitions};

#[path = "font_names.rs"]
pub(crate) mod font_names;
#[path = "stream_offsets.rs"]
mod stream_offsets;
#[path = "xref_offset_entries.rs"]
mod xref_offset_entries;
use compressed_objects::extract as extract_compressed_object_bodies;
use font_definitions::{
    Definition as PageFontResourceDefinition, extract as font_resource_definitions_from_dictionary,
};
use font_names::read as font_object_names;
use stream_offsets::StreamOffsets;

#[path = "legacy_font_resources.rs"]
mod legacy_font_resources;
use legacy_font_resources::read as inherited_page_font_resource_definitions;

#[cfg(test)]
#[path = "legacy_font_resources_tests.rs"]
mod legacy_font_resources_tests;

#[cfg(test)]
#[path = "legacy_font_null_tests.rs"]
mod legacy_font_null_tests;

/// PDF の最小パース（ヘッダとバージョン抽出）を行う。
pub fn parse_pdf(path: &Path) -> Result<ParsedDocument, ParseError> {
    parse_pdf_with_limits(path, &ParseLimits::default())
}

/// file解析に資源上限・品質方針・任意の協調キャンセルを適用する。
/// strictでは欠落report、停止検出ではCancelledを返し、部分結果を公開しない。
pub fn parse_pdf_with_options(
    path: &Path,
    options: &ParseOptions,
) -> Result<ParsedDocument, ParseError> {
    let budget = DecodeBudget::with_cancellation(&options.limits, options.cancellation.as_ref());
    budget.check()?;
    let bytes = crate::input::read(fs::File::open(path)?, &options.limits, &budget)?;
    #[cfg(not(target_arch = "wasm32"))]
    let parsed = crate::pdfium_backend::parse_pdf(path, &bytes, &options.limits, &budget)?;
    #[cfg(target_arch = "wasm32")]
    let parsed =
        parse_pdf_bytes_pure_with_budget(path.to_path_buf(), &bytes, &options.limits, &budget)?;
    budget.check()?;
    check_quality(parsed, options.strict)
}

/// bytes解析に資源上限・品質方針・任意の協調キャンセルを適用する。
pub fn parse_pdf_bytes_with_options(
    path: impl Into<PathBuf>,
    bytes: &[u8],
    options: &ParseOptions,
) -> Result<ParsedDocument, ParseError> {
    let budget = DecodeBudget::with_cancellation(&options.limits, options.cancellation.as_ref());
    let parsed = parse_pdf_bytes_pure_with_budget(path.into(), bytes, &options.limits, &budget)?;
    budget.check()?;
    check_quality(parsed, options.strict)
}

/// 資源検査が成功した結果だけを品質方針で判定する。
fn check_quality(parsed: ParsedDocument, strict: bool) -> Result<ParsedDocument, ParseError> {
    if strict && !parsed.quality.issues.is_empty() {
        return Err(ParseError::QualityRejected {
            path: parsed.path,
            report: parsed.quality,
        });
    }
    Ok(parsed)
}

/// 入力を上限付きで読み込み、指定されたページ数・画像寸法上限で解析する。
pub fn parse_pdf_with_limits(
    path: &Path,
    limits: &ParseLimits,
) -> Result<ParsedDocument, ParseError> {
    parse_pdf_with_options(
        path,
        &ParseOptions {
            limits: *limits,
            ..Default::default()
        },
    )
}

/// PDF バイト列の最小パース（ヘッダとバージョン抽出）を行う。
pub fn parse_pdf_bytes(
    path: impl Into<std::path::PathBuf>,
    bytes: &[u8],
) -> Result<ParsedDocument, ParseError> {
    parse_pdf_bytes_pure(path.into(), bytes)
}

/// bytes 入力に指定されたサイズ・ページ数・画像寸法上限を適用する。
pub fn parse_pdf_bytes_with_limits(
    path: impl Into<PathBuf>,
    bytes: &[u8],
    limits: &ParseLimits,
) -> Result<ParsedDocument, ParseError> {
    parse_pdf_bytes_pure_with_limits(path.into(), bytes, limits)
}

/// pure Rust 実装だけで PDF バイト列を解析する。
///
/// native の `parse_pdf()` では Pdfium を必須バックエンドにするため、この関数は
/// wasm 入力と Pdfium IR へ未移植の graphics/image 補助情報抽出に使う。
pub(crate) fn parse_pdf_bytes_pure(
    path: PathBuf,
    bytes: &[u8],
) -> Result<ParsedDocument, ParseError> {
    parse_pdf_bytes_pure_with_limits(path, bytes, &ParseLimits::default())
}

/// pure Rust 解析を指定された上限で行う。
pub(crate) fn parse_pdf_bytes_pure_with_limits(
    path: PathBuf,
    bytes: &[u8],
    limits: &ParseLimits,
) -> Result<ParsedDocument, ParseError> {
    let decode_budget = DecodeBudget::new(limits);
    parse_pdf_bytes_pure_with_budget(path, bytes, limits, &decode_budget)
}

/// native の追加判定でも同じ展開予算を共有できる pure Rust 解析入口。
pub(crate) fn parse_pdf_bytes_pure_with_budget(
    path: PathBuf,
    bytes: &[u8],
    limits: &ParseLimits,
    decode_budget: &DecodeBudget,
) -> Result<ParsedDocument, ParseError> {
    decode_budget.check()?;
    check_limit("input bytes", bytes.len() as u64, limits.max_input_bytes)?;
    let path = path.into();
    let Some(header_offset) = pdf_header_offset(&bytes) else {
        return Err(ParseError::InvalidPdfHeader { path });
    };

    let header_end = (header_offset + 8).min(bytes.len());
    let header = String::from_utf8_lossy(&bytes[header_offset..header_end]).to_string();
    let mut version =
        parse_pdf_version_from_header(&header).ok_or(ParseError::InvalidPdfVersion {
            path: path.clone(),
            header: header.clone(),
        })?;
    let mut streams =
        crate::structure::validate_structure(&path, &bytes, header_end, limits, decode_budget)?;
    decode_budget.cache_streams(bytes, &streams);
    decode_budget.check()?;
    if let Some(index) = decode_budget.stream_index(bytes) {
        streams.retain(|stream| index.active_stream(stream));
    }
    validate_image_pixel_limits(&streams, limits, decode_budget)?;
    let mut tree_source = decode_budget
        .stream_index(bytes)
        .map(|index| index.without_payloads_for_validation(decode_budget, bytes))
        .unwrap_or_else(|| crate::text::pdf_bytes_without_stream_payloads(decode_budget, bytes));
    append_compressed_objects_as_indirect_objects(decode_budget, bytes, &mut tree_source);
    decode_budget.check()?;
    let page_order = crate::page_tree::resolve(
        &path,
        bytes,
        &tree_source,
        decode_budget,
        limits,
        &mut version,
    )?;
    if page_order.is_none() {
        let geometry =
            crate::legacy_page_resources::validate(&path, &tree_source, limits, decode_budget)?;
        decode_budget.cache_legacy_geometry(geometry);
    }
    decode_budget.cache_pages(page_order);
    decode_budget.set_pdf_version(&version);
    let page_count = count_page_objects(decode_budget, &bytes);
    decode_budget.check()?;
    check_limit("page count", page_count as u64, limits.max_pages as u64)?;
    let page_viewports = parse_page_viewports(decode_budget, &bytes);
    decode_budget.check()?;
    let pages = publish_pages(decode_budget, &page_viewports);
    decode_budget.check()?;
    let mut quality = QualityReport::default();
    crate::page_geometry_diagnostics::collect(decode_budget, page_count, &pages, &mut quality)?;
    let mut graphics = extract_graphics_from_pdf_bytes(decode_budget, &bytes);
    decode_budget.check()?;
    let mut text_elements = extract_text_elements_from_pdf_bytes(decode_budget, &bytes);
    decode_budget.check()?;
    let form_text = extract_form_text_elements(decode_budget, bytes, &mut quality);
    decode_budget.check()?;
    crate::text_result_storage::append(decode_budget, &mut text_elements, form_text);
    decode_budget.check()?;
    crate::unsupported_content::collect(decode_budget, bytes, &mut quality);
    decode_budget.check()?;
    let mut image_elements =
        extract_image_elements_from_pdf_bytes(decode_budget, &bytes, &streams, &mut quality);
    decode_budget.check()?;
    normalize_elements_to_page_viewports(
        &page_viewports,
        &mut graphics,
        &mut text_elements,
        &mut image_elements,
    );
    let embedded_fonts = extract_embedded_truetype_fonts(decode_budget, &bytes);
    decode_budget.check()?;
    let text_preview = if page_count > 0 {
        crate::text::preview_from_text_elements(decode_budget, &text_elements)
    } else {
        extract_text_preview_from_pdf_bytes(decode_budget, &bytes)
    };
    decode_budget.check()?;
    let mut parsed = ParsedDocument {
        quality,
        path,
        byte_len: bytes.len(),
        header,
        version,
        page_count,
        pages,
        graphics,
        text_elements,
        image_elements,
        embedded_fonts,
        text_preview,
    };
    crate::quality::refresh_ocr_diagnostics(decode_budget, &mut parsed)?;
    Ok(parsed)
}

/// 画像の decode/白画像生成より前に、各 Image XObject の寸法を検査する。
fn validate_image_pixel_limits(
    streams: &[crate::structure::StreamSpan<'_>],
    limits: &ParseLimits,
    decode_budget: &DecodeBudget,
) -> Result<(), ParseError> {
    crate::metadata_workspace::ensure_active(decode_budget)?;
    for stream in streams {
        if !is_image_dictionary(stream.dictionary) {
            continue;
        }
        let Some(dict) = crate::metadata_input::text(decode_budget, stream.dictionary) else {
            crate::metadata_workspace::ensure_active(decode_budget)?;
            return Ok(());
        };
        // decoder と同じ変換を行い、u64 積で u32 寸法の overflow を避ける。
        let width = parse_number_after_name(&dict, "/Width")
            .unwrap_or(1.0)
            .max(1.0) as u32;
        let height = parse_number_after_name(&dict, "/Height")
            .unwrap_or(1.0)
            .max(1.0) as u32;
        check_limit(
            "image pixels",
            u64::from(width) * u64::from(height),
            limits.max_image_pixels,
        )?;
    }
    Ok(())
}

/// PDF header marker の開始位置を先頭付近から探す。
fn pdf_header_offset(bytes: &[u8]) -> Option<usize> {
    const MAX_HEADER_OFFSET: usize = 1024;
    bytes
        .windows(b"%PDF-".len())
        .take(MAX_HEADER_OFFSET.min(bytes.len()) + 1)
        .position(|window| window == b"%PDF-")
}

fn parse_pdf_version_from_header(header: &str) -> Option<PdfVersion> {
    let bytes = header.as_bytes();
    if bytes.len() < 8 || &bytes[0..5] != b"%PDF-" {
        return None;
    }
    if !bytes[5].is_ascii_digit() || bytes[6] != b'.' || !bytes[7].is_ascii_digit() {
        return None;
    }
    Some(PdfVersion {
        major: bytes[5] - b'0',
        minor: bytes[7] - b'0',
    })
}

/// metadata の最外辞書から Page を数え、入れ子・文字列・コメントを除外する。
/// 正式な page tree がある場合は所属する Page だけを数える。
fn count_page_objects(decode_budget: &DecodeBudget, bytes: &[u8]) -> usize {
    let parser_source = parser_source_with_compressed_objects(decode_budget, bytes);
    decode_budget.page_dictionaries(&parser_source).len()
}

/// PDF page の表示寸法と、content座標を表示原点へ移すoffsetを表す。
#[derive(Debug, Clone, PartialEq)]
struct ParsedPageViewport {
    /// 公開IRへ渡す表示ページ情報。
    page: ParsedPage,
    /// content座標上の表示矩形左端。
    origin_x: f32,
    /// content座標上の表示矩形下端。
    origin_y: f32,
    /// 明示的なCropBoxで表示領域外をクリップするか。
    clip_to_page: bool,
}

#[path = "page_output.rs"]
mod page_output;
use page_output::publish_pages;

#[cfg(test)]
#[path = "page_output_budget_tests.rs"]
mod page_output_budget_tests;

/// PDF バイト列から `/Type /Page` 辞書の最小ページ情報を抽出する。
#[cfg(test)]
fn parse_page_metadata(decode_budget: &DecodeBudget, bytes: &[u8]) -> Vec<ParsedPage> {
    publish_pages(decode_budget, &parse_page_viewports(decode_budget, bytes))
}

/// ページごとの表示矩形を抽出し、結果配列の全容量を確保前に予約する。
fn parse_page_viewports(decode_budget: &DecodeBudget, bytes: &[u8]) -> Vec<ParsedPageViewport> {
    let parser_source = parser_source_with_compressed_objects(decode_budget, bytes);
    let dictionaries = decode_budget.page_dictionaries(&parser_source);
    let Some(mut viewports) = decode_budget.metadata_vector(dictionaries.len()) else {
        return Vec::new();
    };
    let objects = if decode_budget.has_page_geometry() || dictionaries.is_empty() {
        crate::page_attributes::Objects::default()
    } else {
        let Some(objects) = crate::object_index::build(decode_budget, &parser_source) else {
            return Vec::new();
        };
        objects
    };
    for (page_index, page_bytes) in dictionaries.into_iter().enumerate() {
        let Some(page_object) = crate::metadata_input::text(decode_budget, page_bytes) else {
            break;
        };
        let page_object = page_object.as_ref();
        let media_box = decode_budget
            .page_media_box(page_index)
            .map(|bounds| ParsedBounds {
                x0: bounds[0],
                y0: bounds[1],
                x1: bounds[2],
                y1: bounds[3],
            })
            .or_else(|| {
                parse_inherited_page_box(decode_budget, &objects, page_object, "/MediaBox")
            });
        let Some(media_box) = media_box else {
            continue;
        };
        let crop_box = decode_budget
            .page_crop_box(page_index)
            .map(|crop| {
                crop.map(|bounds| ParsedBounds {
                    x0: bounds[0],
                    y0: bounds[1],
                    x1: bounds[2],
                    y1: bounds[3],
                })
            })
            .unwrap_or_else(|| {
                parse_inherited_page_box(decode_budget, &objects, page_object, "/CropBox")
                    .and_then(|crop_box| intersect_page_bounds(media_box, crop_box))
            });
        let display_box = crop_box.unwrap_or(media_box);
        let rotation = decode_budget
            .page_rotation(page_index)
            .or_else(|| parse_inherited_page_rotation(decode_budget, &objects, page_object))
            .unwrap_or(0);
        viewports.push(ParsedPageViewport {
            page: ParsedPage {
                page_number: page_index,
                width: display_box.x1 - display_box.x0,
                height: display_box.y1 - display_box.y0,
                rotation,
            },
            origin_x: display_box.x0,
            origin_y: display_box.y0,
            clip_to_page: crop_box.is_some(),
        });
    }
    if decode_budget.is_stopped() {
        return Vec::new();
    }
    viewports
}

#[cfg(test)]
#[path = "legacy_page_geometry_tests.rs"]
mod legacy_page_geometry_tests;

#[path = "legacy_page_geometry.rs"]
mod legacy_page_geometry;
#[cfg(test)]
use legacy_page_geometry::parse_page_box_from_object;
use legacy_page_geometry::{parse_inherited_page_box, parse_inherited_page_rotation};

/// 2つのpage boxが重なる有効表示矩形を返す。
fn intersect_page_bounds(first: ParsedBounds, second: ParsedBounds) -> Option<ParsedBounds> {
    let intersection = ParsedBounds {
        x0: first.x0.max(second.x0),
        y0: first.y0.max(second.y0),
        x1: first.x1.min(second.x1),
        y1: first.y1.min(second.y1),
    };
    page_bounds_are_usable(intersection).then_some(intersection)
}

/// page boxが有限で正の面積を持つか判定する。
fn page_bounds_are_usable(bounds: ParsedBounds) -> bool {
    [bounds.x0, bounds.y0, bounds.x1, bounds.y1]
        .into_iter()
        .all(f32::is_finite)
        && bounds.x1 > bounds.x0
        && bounds.y1 > bounds.y0
}

/// 抽出要素をpageの有効表示原点へ移し、表示矩形外を除外する。
fn normalize_elements_to_page_viewports(
    viewports: &[ParsedPageViewport],
    graphics: &mut Vec<ParsedGraphicElement>,
    text_elements: &mut Vec<ParsedTextElement>,
    image_elements: &mut Vec<ParsedImageElement>,
) {
    graphics.retain_mut(|graphic| {
        normalize_bounds_for_page(graphic.page, &mut graphic.bounds, viewports)
    });
    text_elements
        .retain_mut(|text| normalize_bounds_for_page(text.page, &mut text.bounds, viewports));
    image_elements
        .retain_mut(|image| normalize_bounds_for_page(image.page, &mut image.bounds, viewports));
}

/// 1要素のboundsを表示原点へ移し、page内に残る場合だけ`true`を返す。
fn normalize_bounds_for_page(
    page: usize,
    bounds: &mut ParsedBounds,
    viewports: &[ParsedPageViewport],
) -> bool {
    let Some(viewport) = viewports
        .iter()
        .find(|viewport| viewport.page.page_number == page)
    else {
        return true;
    };
    let min_x = bounds.x0.min(bounds.x1) - viewport.origin_x;
    let max_x = bounds.x0.max(bounds.x1) - viewport.origin_x;
    let min_y = bounds.y0.min(bounds.y1) - viewport.origin_y;
    let max_y = bounds.y0.max(bounds.y1) - viewport.origin_y;
    if viewport.clip_to_page
        && (max_x < 0.0
            || min_x > viewport.page.width
            || max_y < 0.0
            || min_y > viewport.page.height)
    {
        return false;
    }
    let relative = if viewport.clip_to_page {
        ParsedBounds {
            x0: (bounds.x0 - viewport.origin_x).clamp(0.0, viewport.page.width),
            y0: (bounds.y0 - viewport.origin_y).clamp(0.0, viewport.page.height),
            x1: (bounds.x1 - viewport.origin_x).clamp(0.0, viewport.page.width),
            y1: (bounds.y1 - viewport.origin_y).clamp(0.0, viewport.page.height),
        }
    } else {
        ParsedBounds {
            x0: bounds.x0 - viewport.origin_x,
            y0: bounds.y0 - viewport.origin_y,
            x1: bounds.x1 - viewport.origin_x,
            y1: bounds.y1 - viewport.origin_y,
        }
    };
    *bounds = rotate_page_relative_bounds(
        relative,
        viewport.page.width,
        viewport.page.height,
        viewport.page.rotation,
    );
    true
}

/// CropBox相対boundsをPDF page rotationに従って時計回りへ回転する。
fn rotate_page_relative_bounds(
    bounds: ParsedBounds,
    page_width: f32,
    page_height: f32,
    rotation: i32,
) -> ParsedBounds {
    let min_x = bounds.x0.min(bounds.x1);
    let max_x = bounds.x0.max(bounds.x1);
    let min_y = bounds.y0.min(bounds.y1);
    let max_y = bounds.y0.max(bounds.y1);
    match rotation.rem_euclid(360) {
        90 => ParsedBounds {
            x0: min_y,
            y0: page_width - max_x,
            x1: max_y,
            y1: page_width - min_x,
        },
        180 => ParsedBounds {
            x0: page_width - max_x,
            y0: page_height - max_y,
            x1: page_width - min_x,
            y1: page_height - min_y,
        },
        270 => ParsedBounds {
            x0: page_height - max_y,
            y0: min_x,
            x1: page_height - min_y,
            y1: max_x,
        },
        _ => bounds,
    }
}

/// PDF辞書文字列から`/Name integer`を読む。
fn parse_integer_after_name(object: &str, name: &str) -> Option<i32> {
    let value =
        crate::metadata::value(object.trim().as_bytes(), name.strip_prefix('/')?.as_bytes())?;
    std::str::from_utf8(value).ok()?.parse::<i32>().ok()
}

/// stream 本文を除いた構造と object stream 展開済み object を連結する。
pub(crate) fn parser_source_with_compressed_objects(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
) -> crate::metadata_source::Source {
    if let Some(source) = decode_budget.parser_source(bytes) {
        return crate::metadata_source::Source::Shared(source);
    }
    let mut source = crate::text::pdf_bytes_without_stream_payloads(decode_budget, bytes);
    append_compressed_objects_as_indirect_objects(decode_budget, bytes, &mut source);
    if decode_budget.is_stopped() {
        return crate::metadata_source::Source::Owned(Vec::new());
    }
    if let Some(index) = decode_budget.stream_index(bytes) {
        index.mask_references(&mut source);
    }
    crate::metadata::mask_ignored(&mut source);
    crate::metadata_source::store(decode_budget, bytes, source)
}

/// object stream 展開済み body を `n 0 obj ... endobj` として追記する。
fn append_compressed_objects_as_indirect_objects(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
    target: &mut Vec<u8>,
) {
    if decode_budget.is_stopped() {
        return;
    }
    let bodies = extract_compressed_object_bodies(decode_budget, bytes);
    let additional = bodies.iter().fold(0usize, |length, (number, body)| {
        let digits = number.checked_ilog10().unwrap_or(0) as usize + 1;
        length
            .saturating_add(digits)
            .saturating_add(16)
            .saturating_add(body.len())
    });
    if crate::metadata_input::extend_capacity(decode_budget, target, additional).is_none() {
        return;
    }
    for (object_number, body) in bodies.iter() {
        // 全出力容量は予約済みであり、formatによる一時Stringを作らない。
        let _ = write!(target, "\n{object_number} 0 obj ");
        target.extend_from_slice(body);
        target.extend_from_slice(b"\nendobj\n");
    }
}

/// PDF のページ本文を filter に応じて展開し、線分 graphics を抽出する。
fn extract_graphics_from_pdf_bytes(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
) -> Vec<ParsedGraphicElement> {
    let mut segments = Vec::new();
    let streams = extract_page_content_streams_with_page(decode_budget, bytes);
    let streams = if streams.is_empty() && !decode_budget.has_page_tree() {
        if has_more_than_image_xobject_streams(decode_budget, bytes, 20) {
            return Vec::new();
        }
        extract_flate_streams(decode_budget, bytes)
    } else {
        streams
    };
    for (page, stream) in streams {
        if graphics_draws::append(decode_budget, page, &stream, &mut segments).is_none() {
            return Vec::new();
        }
    }
    if decode_budget.is_stopped() {
        return Vec::new();
    }
    merge_axis_aligned_segments(segments)
}

/// PDFの座標付き本文を抽出し、空のページFont対応も全体fallbackより優先する。
fn extract_text_elements_from_pdf_bytes(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
) -> Vec<ParsedTextElement> {
    let font_names = parse_font_resource_base_fonts(decode_budget, bytes);
    let font_encodings = parse_font_resource_encodings(decode_budget, bytes);
    let font_cmaps = parse_font_resource_tounicode_cmaps(decode_budget, bytes);
    let page_font_resources = parse_page_font_resources(decode_budget, bytes);
    let cmap = parse_tounicode_cmap(decode_budget, bytes);
    let streams = extract_page_content_streams_with_page(decode_budget, bytes);
    let streams = if streams.is_empty() && !decode_budget.has_page_tree() {
        extract_flate_streams(decode_budget, bytes)
    } else {
        streams
    };
    let mut elements = Vec::new();
    let empty_metrics = BTreeMap::new();
    let empty_cmap = ToUnicodeCMap::default();
    // Contents配列は一つの命令列であり、Tr・Font・文字行列をstream境界で初期化しない。
    let Some(streams) = crate::form_storage::pages(decode_budget, streams) else {
        return Vec::new();
    };
    // gsがある場合だけ索引を用意し、通常本文のmetadata予約量を増やさない。
    let alpha_source = streams
        .iter()
        .any(|(_, content)| crate::text_alpha::is_used(content))
        .then(|| parser_source_with_compressed_objects(decode_budget, bytes));
    let alpha_objects = alpha_source
        .as_ref()
        .and_then(|source| crate::object_index::build(decode_budget, source));
    let alpha_resources = alpha_source
        .as_ref()
        .and_then(|source| decode_budget.page_resources(source));
    let mut alpha_resolver = alpha_objects
        .as_ref()
        .map(|objects| crate::resource_resolver::ResourceResolver::new(objects, decode_budget));
    for (page, stream) in streams {
        let page_resources = page_font_resources.get(page);
        let page_font_names = page_resources
            .map(|resources| &resources.base_fonts as &dyn crate::metadata_map::Lookup<String>)
            .unwrap_or(&font_names);
        let page_font_encodings = page_resources
            .map(|resources| &resources.encodings as &dyn crate::metadata_map::Lookup<String>)
            .unwrap_or(&font_encodings);
        let page_font_cmaps = page_resources
            .map(|resources| &resources.cmaps as &dyn crate::metadata_map::Lookup<ToUnicodeCMap>)
            .unwrap_or(&font_cmaps);
        let texts = extract_text_elements_from_content_stream(
            decode_budget,
            page,
            &stream,
            page_font_names,
            page_font_encodings,
            page_font_cmaps,
            if page_resources.is_some() {
                &empty_cmap
            } else {
                &cmap
            },
            page_resources
                .map(|r| {
                    &r.metrics
                        as &dyn crate::metadata_map::Lookup<
                            std::sync::Arc<crate::font_widths::Metrics>,
                        >
                })
                .unwrap_or(&empty_metrics),
            |name| match (
                alpha_resources
                    .as_ref()
                    .and_then(|resources| resources.get(page))
                    .copied()
                    .flatten(),
                alpha_resolver.as_mut(),
            ) {
                (Some(resources), Some(resolver)) => {
                    crate::text_alpha::resolve(resources, name, resolver)
                }
                _ => Default::default(),
            },
        );
        if crate::text_result_storage::append(decode_budget, &mut elements, texts).is_none() {
            return Vec::new();
        }
    }
    if decode_budget.is_stopped() {
        Vec::new()
    } else {
        elements
    }
}

/// 各ページの Font 情報を返し、ページ順の結果配列を確保前に予約する。
fn parse_page_font_resources(decode_budget: &DecodeBudget, bytes: &[u8]) -> Vec<PageFontResources> {
    let parser_source = parser_source_with_compressed_objects(decode_budget, bytes);
    let dictionaries = decode_budget.page_dictionaries(&parser_source);
    let Some(mut resources) = decode_budget.metadata_vector(dictionaries.len()) else {
        return Vec::new();
    };
    let Some(source) = crate::metadata_input::text(decode_budget, &parser_source) else {
        return Default::default();
    };
    let stream_offsets = stream_object_offsets(decode_budget, bytes);
    let resolved_resources = decode_budget.page_resources(&parser_source);
    let Some(objects) = crate::object_index::build(decode_budget, source.as_bytes()) else {
        return Default::default();
    };
    let mut metrics_cache = crate::font_metrics_cache::Cache::new(&objects, decode_budget, bytes);
    let base_fonts = font_object_names(decode_budget, &objects, b"BaseFont");
    let encodings = font_object_names(decode_budget, &objects, b"Encoding");
    let cmaps = font_object_tounicode_cmaps_in(decode_budget, bytes, &objects, &stream_offsets);
    for (page, page_bytes) in dictionaries.into_iter().enumerate() {
        if decode_budget.is_stopped() {
            break;
        }
        let Some(page_object) = crate::metadata_input::text(decode_budget, page_bytes) else {
            break;
        };
        let page_object = page_object.as_ref();
        let definitions = match &resolved_resources {
            Some(resources) => resources[page]
                .and_then(|dictionary| std::str::from_utf8(dictionary).ok())
                .map(|dictionary| {
                    font_resource_definitions_from_dictionary(decode_budget, &objects, dictionary)
                })
                .unwrap_or_default(),
            None => inherited_page_font_resource_definitions(decode_budget, &objects, page_object),
        };
        let page_resources = font_resources_from_definitions(
            decode_budget,
            bytes,
            definitions,
            &base_fonts,
            &encodings,
            &cmaps,
            &stream_offsets,
            &mut metrics_cache,
        );
        resources.push(page_resources);
    }
    if decode_budget.is_stopped() {
        return Vec::new();
    }
    resources
}

/// Form の内部 Font を優先し、呼び出し時の合成 CTM を本文の位置・寸法へ適用する。
fn extract_form_text_elements(
    budget: &DecodeBudget,
    bytes: &[u8],
    quality: &mut QualityReport,
) -> Vec<ParsedTextElement> {
    let source = parser_source_with_compressed_objects(budget, bytes);
    let Some(objects) = crate::object_index::build(budget, &source) else {
        return Default::default();
    };
    let mut font_maps = None;
    let mut resource_fonts = font_local_cache::Cache::default();
    let mut metrics_cache = crate::font_metrics_cache::Cache::new(&objects, budget, bytes);
    let mut elements = Vec::new();
    crate::form_content::visit(
        budget,
        bytes,
        quality,
        |page, content, resources, matrix, clip, inherited| {
            let mut alpha_resolver =
                crate::resource_resolver::ResourceResolver::new(&objects, budget);
            let (offsets, names, encodings, cmaps) = font_maps.get_or_insert_with(|| {
                let offsets = stream_object_offsets(budget, bytes);
                let names = font_object_names(budget, &objects, b"BaseFont");
                let encodings = font_object_names(budget, &objects, b"Encoding");
                let cmaps = font_object_tounicode_cmaps_in(budget, bytes, &objects, &offsets);
                (offsets, names, encodings, cmaps)
            });
            // visitor の不変 source が生きている間だけ、同じ局所辞書の Font map を共有する。
            let selected_resources = if inherited.state.font.is_some() {
                inherited.resources
            } else {
                resources
            };
            for dictionary in [resources, selected_resources] {
                if resource_fonts
                    .get_or_build(budget, dictionary, || {
                        let definitions = std::str::from_utf8(dictionary)
                            .ok()
                            .map(|r| font_resource_definitions_from_dictionary(budget, &objects, r))
                            .unwrap_or_default();
                        font_resources_from_definitions(
                            budget,
                            bytes,
                            definitions,
                            names,
                            encodings,
                            cmaps,
                            offsets,
                            &mut metrics_cache,
                        )
                    })
                    .is_none()
                {
                    return;
                }
            }
            let Some(fonts) = resource_fonts.get(resources) else {
                return;
            };
            let Some(inherited_fonts) = resource_fonts.get(selected_resources) else {
                return;
            };
            let Some(mut state) = inherited.state.saved(budget) else {
                return;
            };
            state.inherited_font = true;
            let matrices = crate::form_content::text_matrices(budget, content, matrix);
            let clips = crate::content_clip::scan(budget, content, matrix);
            if budget.is_stopped() {
                return;
            }
            let texts = extract_text_elements_with_matrices(
                budget,
                page,
                content,
                &fonts.base_fonts,
                &fonts.encodings,
                &fonts.cmaps,
                &ToUnicodeCMap::default(),
                &fonts.metrics,
                Some(inherited_fonts),
                state,
                |name| crate::text_alpha::resolve(resources, name, &mut alpha_resolver),
                |prefix| {
                    matrices.get(&prefix.len()).copied().map(|matrix| {
                        crate::text_geometry::TextGeometry::new(matrix, Some(clip))
                            .clipped(clips.get(prefix.len()))
                    })
                },
            );
            if crate::text_result_storage::append(budget, &mut elements, texts).is_none() {
                elements.clear();
            }
        },
    );
    if budget.is_stopped() {
        Vec::new()
    } else {
        elements
    }
}

/// PDF page content stream に text showing operator が含まれるか判定する。
pub(crate) fn pdf_bytes_have_text_showing_operators(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
) -> bool {
    let streams = extract_page_content_streams_with_page(decode_budget, bytes);
    let streams = if streams.is_empty() && !decode_budget.has_page_tree() {
        extract_flate_streams(decode_budget, bytes)
    } else {
        streams
    };
    !decode_budget.is_stopped()
        && streams
            .iter()
            .any(|(_, stream)| content_stream_has_text_showing_operator(stream))
}

/// コメント・文字列・containerを除外し、借用byteから文字描画命令を判定する。
fn content_stream_has_text_showing_operator(content: &[u8]) -> bool {
    use crate::syntax::{Lexer, Token};
    let mut lexer = Lexer {
        bytes: content,
        cursor: 0,
    };
    let mut depth = 0usize;
    while let Some(token) = lexer.next() {
        match token {
            Token::Open(_) => depth = depth.saturating_add(1),
            Token::Close(_) => depth = depth.saturating_sub(1),
            Token::Word(b"BI") if depth == 0 => return false,
            Token::Word(b"Tj" | b"TJ" | b"'" | b"\"") if depth == 0 => return true,
            _ => {}
        }
    }
    false
}

/// PDF バイト列から最小 Image XObject 情報を抽出する。
fn extract_image_elements_from_pdf_bytes(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
    streams: &[crate::structure::StreamSpan<'_>],
    quality: &mut QualityReport,
) -> Vec<ParsedImageElement> {
    let placements = extract_image_placements_from_pdf_bytes(decode_budget, bytes);
    let mut placed_object_numbers = crate::metadata_workspace::Set::default();
    for placement in &placements {
        if placed_object_numbers
            .try_insert(decode_budget, placement.object_number)
            .is_none()
        {
            return Vec::new();
        }
    }
    let unpainted_object_numbers = unpainted_xobject_image_object_numbers(decode_budget, bytes);
    let require_placement = decode_budget.has_page_tree() || !placed_object_numbers.is_empty();
    let selected_images = || {
        streams.iter().filter(|stream| {
            is_extracted_image_stream(
                stream,
                require_placement,
                &placed_object_numbers,
                &unpainted_object_numbers,
            )
        })
    };
    let mut image_streams = Vec::new();
    for stream in selected_images() {
        if decode_budget.is_stopped() {
            return Vec::new();
        }
        let dict = stream.dictionary;
        let Some(object) = crate::metadata_input::text(decode_budget, dict) else {
            break;
        };
        let width = parse_number_after_name(&object, "/Width")
            .unwrap_or(1.0)
            .max(1.0) as u32;
        let height = parse_number_after_name(&object, "/Height")
            .unwrap_or(1.0)
            .max(1.0) as u32;
        let object_number = stream.object_number;
        let filter = ImageFilter::read(dict).or_else(|| image_filter_chain::terminal_filter(dict));
        let mut stencil = false;
        // 枚数や旧400万画素閾値では代替せず、decoderの明示的な資源予算で制限する。
        let png_bytes = {
            let repaired_before = decode_budget.ccitt_repaired_rows();
            let decoded = decode_image_stream_as_png(
                decode_budget,
                bytes,
                dict,
                stream.payload,
                width,
                height,
            );
            if decode_budget.is_stopped() {
                return Vec::new();
            }
            match decoded {
                Some(png_bytes) => {
                    stencil = image_stencil::is_mask(decode_budget, bytes, dict) == Some(true);
                    let damaged_rows = decode_budget.ccitt_repaired_rows() - repaired_before;
                    if damaged_rows > 0
                        && crate::metadata_workspace::try_push(
                            decode_budget,
                            &mut quality.issues,
                            QualityIssue::ImageRepaired {
                                object_number,
                                damaged_rows,
                            },
                        )
                        .is_none()
                    {
                        return Vec::new();
                    }
                    png_bytes
                }
                None => {
                    if crate::metadata_workspace::try_push(
                        decode_budget,
                        &mut quality.issues,
                        QualityIssue::ImageReplaced {
                            object_number,
                            reason: if filter.is_some() {
                                ImageReplacementReason::DecodeFailed
                            } else {
                                ImageReplacementReason::UnsupportedFilter
                            },
                        },
                    )
                    .is_none()
                    {
                        return Vec::new();
                    }
                    let Some(png) = fallback_image_png_bytes(decode_budget, width, height) else {
                        return Vec::new();
                    };
                    png
                }
            }
        };
        if crate::metadata_workspace::try_push(
            decode_budget,
            &mut image_streams,
            ImageStreamEntry {
                object_number,
                png_bytes,
                width,
                height,
                stencil,
            },
        )
        .is_none()
        {
            return Vec::new();
        }
    }
    let mut images = Vec::new();
    let mut painted_pages = Vec::new();
    for stream in image_streams {
        if decode_budget.is_stopped() {
            return Vec::new();
        }
        let mut matching_placements = placements
            .iter()
            .filter(|placement| Some(placement.object_number) == stream.object_number)
            .peekable();
        if matching_placements.peek().is_none() {
            if placements.is_empty() {
                let placement = ImagePlacement {
                    object_number: stream.object_number.unwrap_or(0),
                    page: 0,
                    bounds: ParsedBounds {
                        x0: 0.0,
                        y0: 0.0,
                        x1: stream.width as f32,
                        y1: stream.height as f32,
                    },
                    floating: false,
                    rotation_degrees: 0,
                    matrix: [1., 0., 0., 1., 0., 0.],
                    paint: image_draws::ImagePaint::default(),
                };
                let Some(image) = stream.to_element(decode_budget, &placement) else {
                    return Vec::new();
                };
                if crate::metadata_workspace::try_push(decode_budget, &mut images, image).is_none()
                {
                    return Vec::new();
                }
            }
            continue;
        }
        for placement in matching_placements {
            if decode_budget.is_stopped() {
                return Vec::new();
            }
            if !placement.paint.supported(stream.stencil)
                && !quality.issues.iter().any(|issue| matches!(issue, QualityIssue::ImageReplaced { object_number, .. } if *object_number == stream.object_number))
                && crate::metadata_workspace::try_push(decode_budget, &mut quality.issues, QualityIssue::ImageReplaced { object_number: stream.object_number, reason: ImageReplacementReason::DecodeFailed }).is_none()
            {
                return Vec::new();
            }
            let Some(image) = stream.to_element(decode_budget, placement) else {
                return Vec::new();
            };
            if crate::metadata_workspace::try_push(decode_budget, &mut images, image).is_none() {
                return Vec::new();
            }
            if let Err(index) = painted_pages.binary_search(&placement.page) {
                if crate::metadata_workspace::try_push(
                    decode_budget,
                    &mut painted_pages,
                    placement.page,
                )
                .is_none()
                {
                    return Vec::new();
                }
                painted_pages[index..].rotate_right(1);
            }
        }
    }
    for page in painted_pages {
        if crate::metadata_workspace::try_push(
            decode_budget,
            &mut quality.issues,
            QualityIssue::OcrRequired { page },
        )
        .is_none()
        {
            return Vec::new();
        }
    }
    images
}

/// 現行 extractor が Image XObject とみなす dictionary を判定する。
pub(crate) fn is_image_dictionary(dict: &[u8]) -> bool {
    crate::metadata::value(dict, b"Subtype")
        .is_some_and(|value| crate::metadata::is_name(value, b"Image"))
}

/// 枚数判定と復号で共通の条件を使い、未描画の補助maskやresourceを抽出しない。
fn is_extracted_image_stream(
    stream: &crate::structure::StreamSpan<'_>,
    require_placement: bool,
    placed_object_numbers: &crate::metadata_workspace::Set<u32>,
    unpainted_object_numbers: &crate::metadata_workspace::Set<u32>,
) -> bool {
    is_image_dictionary(stream.dictionary)
        && (!require_placement
            || stream
                .object_number
                .is_some_and(|number| placed_object_numbers.contains(&number)))
        && !stream
            .object_number
            .is_some_and(|number| unpainted_object_numbers.contains(&number))
}

/// PDF 内の Image XObject stream 数が閾値を超えるか判定する。
fn has_more_than_image_xobject_streams(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
    threshold: usize,
) -> bool {
    if let Some(index) = decode_budget.stream_index(bytes) {
        return index
            .streams(bytes)
            .filter(|stream| is_image_dictionary(stream.dictionary))
            .count()
            > threshold;
    }
    let mut count = 0usize;
    let mut cursor = 0usize;
    while let Some(stream_rel) = find_bytes(&bytes[cursor..], b"stream") {
        let stream_pos = cursor + stream_rel;
        let dict = stream_dictionary_before_stream(bytes, stream_pos).unwrap_or_default();
        let Some(end_rel) = find_bytes(&bytes[stream_pos..], b"endstream") else {
            break;
        };
        if is_image_dictionary(dict) {
            count += 1;
            if count > threshold {
                return true;
            }
        }
        cursor = stream_pos + end_rel + b"endstream".len();
    }
    false
}

/// 描画された共有objectを保持し、未描画候補の集合を累積容量で制限する。
fn unpainted_xobject_image_object_numbers(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
) -> crate::metadata_workspace::Set<u32> {
    use crate::metadata_workspace::Set;
    let refs = parse_page_xobject_refs_with_page(decode_budget, bytes);
    let contents = extract_page_content_streams_with_page(decode_budget, bytes);
    let mut painted_pages = Set::default();
    let mut painted_objects = Set::default();
    if image_draws::visit(decode_budget, &contents, |page, name, _| {
        painted_pages.try_insert(decode_budget, page)?;
        if let Some((_, _, number)) = refs.iter().rev().find(|(ref_page, ref_name, _)| {
            *ref_page == page && crate::metadata_names::resource_name_order(name, ref_name).is_eq()
        }) {
            painted_objects.try_insert(decode_budget, *number)?;
        }
        Some(())
    })
    .is_none()
    {
        return Set::default();
    }
    let mut unpainted = Set::default();
    for (page, _, number) in refs {
        if painted_pages.contains(&page)
            && !painted_objects.contains(&number)
            && unpainted.try_insert(decode_budget, number).is_none()
        {
            return Set::default();
        }
    }
    unpainted
}

/// PDF 内の Image XObject stream 情報。
#[derive(Debug)]
struct ImageStreamEntry {
    /// 成功復号したstencilは配置ごとの塗り色が必要。
    stencil: bool,
    /// image object number。
    object_number: Option<u32>,
    /// DOCX に埋め込む PNG バイト列。
    png_bytes: Vec<u8>,
    /// 元画像の幅 px。
    width: u32,
    /// 元画像の高さ px。
    height: u32,
}

impl ImageStreamEntry {
    /// 配置情報を付与した `ParsedImageElement` へ変換する。
    fn to_element(
        &self,
        decode_budget: &DecodeBudget,
        placement: &ImagePlacement,
    ) -> Option<ParsedImageElement> {
        let ImagePlacement {
            page,
            bounds,
            floating,
            rotation_degrees,
            paint,
            matrix,
            ..
        } = *placement;
        let matrix = Some(matrix);
        if decode_budget.is_stopped() {
            return None;
        }
        let display_width = pdf_points_to_display_px((bounds.x1 - bounds.x0).abs())
            .unwrap_or(self.width)
            .max(1);
        let display_height = pdf_points_to_display_px((bounds.y1 - bounds.y0).abs())
            .unwrap_or(self.height)
            .max(1);
        let painted;
        let source = if !paint.supported(self.stencil) {
            painted = fallback_image_png_bytes(decode_budget, self.width, self.height)?;
            painted.as_slice()
        } else if self.stencil || paint.opacity != Some(1.0) {
            painted = image_paint_pixels::apply(
                decode_budget,
                &self.png_bytes,
                self.stencil.then_some(paint.color).flatten(),
                paint.opacity?,
            )?;
            painted.as_slice()
        } else {
            self.png_bytes.as_slice()
        };
        let has_shear = matrix.is_some_and(matrix_has_shear);
        let png_bytes = matrix
            .filter(|matrix| matrix_has_shear(*matrix))
            .and_then(|matrix| {
                warp_png_to_ctm_bounds(
                    decode_budget,
                    source,
                    matrix,
                    bounds,
                    display_width,
                    display_height,
                )
            })
            .or_else(|| image_png::copy(decode_budget, source))?;
        Some(ParsedImageElement {
            page,
            bounds,
            png_bytes,
            width_px: display_width,
            height_px: display_height,
            floating,
            rotation_degrees: if has_shear { 0 } else { rotation_degrees },
        })
    }
}

/// PDF point 寸法を 96dpi 相当の表示 px に変換する。
fn pdf_points_to_display_px(points: f32) -> Option<u32> {
    if !points.is_finite() || points <= 0.0 {
        return None;
    }
    Some((points * 96.0 / 72.0).round() as u32)
}

/// RGBA8 の PNG pixel buffer。
struct PngRgbaImage {
    /// PNG の幅 px。
    width: u32,
    /// PNG の高さ px。
    height: u32,
    /// RGBA8 画素列。
    rgba: Vec<u8>,
}

/// 自前 PNG encoder が出した filter 0 の RGB/RGBA PNG を RGBA8 に戻す。
fn decode_filter0_png_to_rgba(decode_budget: &DecodeBudget, png: &[u8]) -> Option<PngRgbaImage> {
    if png.len() < 33 || &png[0..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let mut cursor = 8usize;
    let mut width = 0u32;
    let mut height = 0u32;
    let mut color_type = 0u8;
    let mut idat = Vec::new();
    while cursor.checked_add(8)? <= png.len() {
        let length = u32::from_be_bytes(png[cursor..cursor + 4].try_into().ok()?) as usize;
        let kind = &png[cursor + 4..cursor + 8];
        let data_start = cursor + 8;
        let data_end = data_start.checked_add(length)?;
        if data_end.checked_add(4)? > png.len() {
            return None;
        }
        match kind {
            b"IHDR" => {
                if length < 13 {
                    return None;
                }
                width = u32::from_be_bytes(png[data_start..data_start + 4].try_into().ok()?);
                height = u32::from_be_bytes(png[data_start + 4..data_start + 8].try_into().ok()?);
                if png[data_start + 8] != 8 {
                    return None;
                }
                color_type = png[data_start + 9];
            }
            b"IDAT" => image_png::append(decode_budget, &mut idat, &png[data_start..data_end])?,
            b"IEND" => break,
            _ => {}
        }
        cursor = data_end + 4;
    }
    let channels = match color_type {
        2 => 3usize,
        6 => 4usize,
        _ => return None,
    };
    decode_budget.check_image_pixels(width, height)?;
    let row_len = (width as usize).checked_mul(channels)?;
    let scanlines = decode_zlib(idat.as_slice(), decode_budget)?;
    let expected = (row_len + 1).checked_mul(height as usize)?;
    if scanlines.len() < expected {
        return None;
    }
    let mut rgba =
        decode_budget.image_vector(width.checked_mul(height)?.checked_mul(4)? as usize)?;
    for row in 0..height as usize {
        let start = row.checked_mul(row_len + 1)?;
        if scanlines[start] != 0 {
            return None;
        }
        for pixel in scanlines[start + 1..start + 1 + row_len].chunks_exact(channels) {
            rgba.extend_from_slice(&pixel[0..3]);
            rgba.push(if channels == 4 { pixel[3] } else { 255 });
        }
    }
    Some(PngRgbaImage {
        width,
        height,
        rgba,
    })
}

/// CTM の shear/skew を画像 pixel に焼き込んだ RGBA PNG を作る。
fn warp_png_to_ctm_bounds(
    decode_budget: &DecodeBudget,
    png: &[u8],
    matrix: [f32; 6],
    bounds: ParsedBounds,
    dst_width: u32,
    dst_height: u32,
) -> Option<Vec<u8>> {
    decode_budget.check_image_pixels(dst_width, dst_height)?;
    let source = decode_filter0_png_to_rgba(decode_budget, png)?;
    let [a, b, c, d, e, f] = matrix;
    let determinant = a * d - b * c;
    if determinant.abs() < 0.0001 || dst_width == 0 || dst_height == 0 {
        return None;
    }
    let length = dst_width.checked_mul(dst_height)?.checked_mul(4)? as usize;
    let mut out = decode_budget.image_vector(length)?;
    out.resize(length, 0);
    let bounds_width = bounds.x1 - bounds.x0;
    let bounds_height = bounds.y1 - bounds.y0;
    for y in 0..dst_height {
        let pdf_y = bounds.y1 - ((y as f32 + 0.5) / dst_height as f32) * bounds_height;
        for x in 0..dst_width {
            let pdf_x = bounds.x0 + ((x as f32 + 0.5) / dst_width as f32) * bounds_width;
            let rel_x = pdf_x - e;
            let rel_y = pdf_y - f;
            let u = (d * rel_x - c * rel_y) / determinant;
            let v = (-b * rel_x + a * rel_y) / determinant;
            if !(-0.001..=1.001).contains(&u) || !(-0.001..=1.001).contains(&v) {
                continue;
            }
            let src_x =
                ((u.clamp(0.0, 0.999_999) * source.width as f32) as u32).min(source.width - 1);
            let src_y = (((1.0 - v).clamp(0.0, 0.999_999) * source.height as f32) as u32)
                .min(source.height - 1);
            let src_index = src_y
                .checked_mul(source.width)?
                .checked_add(src_x)?
                .checked_mul(4)? as usize;
            let dst_index = y.checked_mul(dst_width)?.checked_add(x)?.checked_mul(4)? as usize;
            out[dst_index..dst_index + 4].copy_from_slice(&source.rgba[src_index..src_index + 4]);
        }
    }
    encode_rgba8_png(decode_budget, dst_width, dst_height, &out)
}

/// content stream 内の Image XObject 配置。
#[derive(Debug, Clone, Copy)]
struct ImagePlacement {
    /// Do時の非stroke色と透明度。
    paint: image_draws::ImagePaint,
    /// 対象ページ番号。
    page: usize,
    /// image object number。
    object_number: u32,
    /// PDF user unit 上の配置 bounds。
    bounds: ParsedBounds,
    /// 回転または傾き CTM から floating 配置が必要か。
    floating: bool,
    /// CTM から推定した時計回り回転角度。
    rotation_degrees: u16,
    /// PDF の画像描画 CTM。
    matrix: [f32; 6],
}

/// page resources と content stream の `cm /Name Do` から画像配置を抽出する。
fn extract_image_placements_from_pdf_bytes(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
) -> Vec<ImagePlacement> {
    let structural_source = parser_source_with_compressed_objects(decode_budget, bytes);
    let xobject_refs = page_xobject_refs_from_source(decode_budget, &structural_source);
    let contents = extract_page_content_streams_with_page(decode_budget, bytes);
    if decode_budget.is_stopped() {
        return Vec::new();
    }
    let mut placements = Vec::new();
    if image_draws::visit_in_source(
        decode_budget,
        &structural_source,
        &contents,
        |page, name, matrix, paint| {
            if !matrix.iter().all(|value| value.is_finite()) {
                return Some(());
            }
            let Some((_, _, object_number)) =
                xobject_refs.iter().rev().find(|(ref_page, ref_name, _)| {
                    *ref_page == page
                        && crate::metadata_names::resource_name_order(name, ref_name).is_eq()
                })
            else {
                return Some(());
            };
            crate::metadata_workspace::try_push(
                decode_budget,
                &mut placements,
                ImagePlacement {
                    paint,
                    page,
                    object_number: *object_number,
                    bounds: bounds_from_unit_square_matrix(matrix),
                    floating: matrix_has_rotation_or_skew(matrix),
                    rotation_degrees: rotation_degrees_from_matrix(matrix),
                    matrix,
                },
            )
        },
    )
    .is_none()
    {
        return Vec::new();
    }
    placements
}

/// PDF の page object から page 番号付き `/XObject /Name n 0 R` 参照を抽出する。
fn parse_page_xobject_refs_with_page(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
) -> Vec<(usize, String, u32)> {
    let structural_bytes = parser_source_with_compressed_objects(decode_budget, bytes);
    page_xobject_refs_from_source(decode_budget, &structural_bytes)
}

/// 有効な合成 object 本文から所属ページの XObject map を読む。
pub(crate) fn page_xobject_refs_from_source(
    decode_budget: &DecodeBudget,
    structural_bytes: &[u8],
) -> Vec<(usize, String, u32)> {
    crate::xobject_refs::page_references(decode_budget, structural_bytes)
}

/// CTM が回転だけでなく shear/skew を含むか判定する。
fn matrix_has_shear(matrix: [f32; 6]) -> bool {
    let [a, b, c, d, ..] = matrix;
    let dot = a * c + b * d;
    dot.abs() > 0.01
}

/// CTM が軸平行 scale/translate 以外の画像変形を含むか判定する。
fn matrix_has_rotation_or_skew(matrix: [f32; 6]) -> bool {
    let [_, b, c, ..] = matrix;
    b.abs() > 0.001 || c.abs() > 0.001
}

/// CTM の a/b 成分から時計回り回転角度を推定する。
fn rotation_degrees_from_matrix(matrix: [f32; 6]) -> u16 {
    let [a, b, ..] = matrix;
    let mut degrees = b.atan2(a).to_degrees().round() as i32;
    degrees %= 360;
    if degrees < 0 {
        degrees += 360;
    }
    degrees as u16
}

/// PDF の単位正方形へ CTM を適用した外接 bounds を返す。
fn bounds_from_unit_square_matrix(matrix: [f32; 6]) -> ParsedBounds {
    let [a, b, c, d, e, f] = matrix;
    let points = [
        (e, f),
        (a + e, b + f),
        (c + e, d + f),
        (a + c + e, b + d + f),
    ];
    let (mut min_x, mut min_y) = points[0];
    let (mut max_x, mut max_y) = points[0];
    for (x, y) in points.into_iter().skip(1) {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    ParsedBounds {
        x0: min_x,
        y0: min_y,
        x1: max_x,
        y1: max_y,
    }
}

/// PDF Image XObject stream を DOCX 埋め込み用 PNG に変換する。
fn decode_image_stream_as_png(
    decode_budget: &DecodeBudget,
    pdf_bytes: &[u8],
    dict: &[u8],
    data: &[u8],
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    decode_budget.check_image_pixels(width, height)?;
    if image_filter_chain::terminal_filter(dict).is_some() {
        let (dictionary, decoded) =
            image_filter_chain::unwrap(decode_budget, pdf_bytes, dict, data)?;
        return decode_single_filter_image(
            decode_budget,
            pdf_bytes,
            &dictionary,
            &decoded,
            width,
            height,
        );
    }
    decode_single_filter_image(decode_budget, pdf_bytes, dict, data, width, height)
}

/// 前段filterの展開後も、ICC・Decode・Matte・alphaの既存処理を共有する。
fn decode_single_filter_image(
    decode_budget: &DecodeBudget,
    pdf_bytes: &[u8],
    dict: &[u8],
    data: &[u8],
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    if image_stencil::is_mask(decode_budget, pdf_bytes, dict)? {
        return image_stencil::decode(decode_budget, pdf_bytes, dict, data, width, height);
    }
    if image_icc::is_icc(decode_budget, pdf_bytes, dict)? {
        return image_icc::decode(decode_budget, pdf_bytes, dict, data, width, height);
    }
    if image_separation::is_separation(decode_budget, pdf_bytes, dict)? {
        return image_separation::decode(decode_budget, pdf_bytes, dict, data, width, height);
    }
    let dict_text = crate::metadata_input::text(decode_budget, dict)?;
    let pixel_count = width.checked_mul(height)?;
    let filter = ImageFilter::read(dict)?;
    if matches!(filter, ImageFilter::Dct | ImageFilter::Jbig2)
        && (image_sample_decode::require_codec_identity(decode_budget, pdf_bytes, dict).is_none()
            || image_smask::requires_source_color(decode_budget, pdf_bytes, dict)?)
    {
        return image_codec_decode::decode(
            decode_budget,
            pdf_bytes,
            dict,
            data,
            width,
            height,
            filter,
        );
    }
    if filter == ImageFilter::Dct {
        let png = decode_dct_jpeg_as_png(decode_budget, data)?;
        return image_smask::apply_to_png(decode_budget, pdf_bytes, dict, png);
    }
    if filter == ImageFilter::Jbig2 {
        let png = decode_jbig2_as_png(decode_budget, pdf_bytes, &dict_text, data, width, height)?;
        return image_smask::apply_to_png(decode_budget, pdf_bytes, dict, png);
    }
    if filter == ImageFilter::Ccitt {
        let png = image_ccitt::decode_source(decode_budget, pdf_bytes, data, width, height, dict)?;
        return image_smask::apply_to_png(decode_budget, pdf_bytes, dict, png);
    }
    if width == 0 || height == 0 {
        return None;
    }
    if parse_number_after_name(&dict_text, "/BitsPerComponent")? as u32 != 8 {
        return None;
    }
    let sample_decode = image_sample_decode::SampleDecode::read(decode_budget, pdf_bytes, dict)?;
    let mut decoded = decode_zlib(data, decode_budget)?;
    let pixel_count = pixel_count as usize;
    let color_space = image_attribute(decode_budget, pdf_bytes, dict, b"ColorSpace")?;
    let components = if crate::metadata::is_name(&color_space, b"DeviceRGB") {
        3
    } else {
        1
    };
    image_predictor::apply(
        decode_budget,
        pdf_bytes,
        dict,
        &mut decoded,
        pixel_count.checked_mul(components)?,
    )?;
    let source_mask = if image_smask::is_present(dict) {
        Some(image_smask::decode(
            decode_budget,
            pdf_bytes,
            &dict_text,
            width,
            height,
        )?)
    } else {
        None
    };
    // Device色とMatteは浮動小数点で合成し、途中の8 bit丸めを避ける。
    let defer_decode = source_mask.is_some() && !sample_decode.is_indexed();
    if !defer_decode {
        sample_decode.apply(
            decode_budget,
            decoded.get_mut(..pixel_count.checked_mul(components)?)?,
        )?;
    }
    let rgb = if crate::metadata::array_values(&color_space, 4).is_some() {
        let palette =
            parse_indexed_device_rgb_palette_from_pdf(decode_budget, pdf_bytes, &color_space)?;
        if decoded.len() < pixel_count {
            return None;
        }
        Cow::Owned(image_pixels::indexed(
            decode_budget,
            &decoded[..pixel_count],
            &palette,
        )?)
    } else if crate::metadata::is_name(&color_space, b"DeviceRGB") {
        let expected_len = width.checked_mul(height)?.checked_mul(3)? as usize;
        Cow::Borrowed(decoded.get(..expected_len)?)
    } else if crate::metadata::is_name(&color_space, b"DeviceGray") {
        Cow::Owned(image_pixels::gray(
            decode_budget,
            decoded.get(..pixel_count)?,
        )?)
    } else {
        return None;
    };
    if let Some(mask) = source_mask {
        let mut rgba = image_pixels::with_alpha(decode_budget, &rgb, &mask.alpha)?;
        if defer_decode {
            mask.apply_source_decode(decode_budget, &mut rgba, &sample_decode)?;
        } else {
            mask.apply(decode_budget, &mut rgba)?;
        }
        return encode_rgba8_png(decode_budget, width, height, &rgba);
    }
    encode_rgb8_png(decode_budget, width, height, &rgb)
}

/// JBIG2Decode の 1bpp 画像 stream を RGB PNG に変換する。
fn decode_jbig2_as_png(
    decode_budget: &DecodeBudget,
    pdf_bytes: &[u8],
    dict_text: &str,
    data: &[u8],
    fallback_width: u32,
    fallback_height: u32,
) -> Option<Vec<u8>> {
    decode_jbig2_rgb(
        decode_budget,
        pdf_bytes,
        dict_text,
        data,
        fallback_width,
        fallback_height,
    )?
    .finish_png(decode_budget)
}

/// 既存の予約済みRGBバッファでJBIG2を復号し、PNGとstencilで共有する。
fn decode_jbig2_rgb(
    decode_budget: &DecodeBudget,
    pdf_bytes: &[u8],
    dict_text: &str,
    data: &[u8],
    fallback_width: u32,
    fallback_height: u32,
) -> Option<RgbJbig2Decoder> {
    if decode_budget.is_stopped() {
        return None;
    }
    let globals = jbig2_globals_stream(decode_budget, pdf_bytes, dict_text);
    if decode_budget.is_stopped() {
        return None;
    }
    let image = Jbig2Image::new_embedded(data, globals.as_deref())
        .or_else(|_| Jbig2Image::new(data))
        .ok()?;
    let width = image.width().max(fallback_width.max(1));
    let height = image.height().max(fallback_height.max(1));
    decode_budget.check_image_pixels(width, height)?;
    let mut decoder = RgbJbig2Decoder::new(decode_budget, width, height)?;
    image.decode(&mut decoder).ok()?;
    Some(decoder)
}

/// stencilの寸法を厳密に照合し、RGBを同じ容量内でGray sampleへ詰める。
fn decode_jbig2_gray(
    budget: &DecodeBudget,
    pdf: &[u8],
    dictionary: &[u8],
    data: &[u8],
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    let dictionary = crate::metadata_input::text(budget, dictionary)?;
    let mut decoded = decode_jbig2_rgb(budget, pdf, &dictionary, data, 0, 0)?;
    if (decoded.width, decoded.height) != (width, height) {
        return None;
    }
    let count = usize::try_from(width.checked_mul(height)?).ok()?;
    if decoded.rgb.len() != count.checked_mul(3)? {
        return None;
    }
    for index in 0..count {
        if index % 4096 == 0 && budget.is_stopped() {
            return None;
        }
        decoded.rgb[index] = decoded.rgb[index * 3];
    }
    decoded.rgb.truncate(count);
    Some(decoded.rgb)
}

/// JBIG2Decode の DecodeParms から globals stream を取得する。
fn jbig2_globals_stream(
    decode_budget: &DecodeBudget,
    pdf_bytes: &[u8],
    dict_text: &str,
) -> Option<Vec<u8>> {
    let dict = dict_text.as_bytes();
    let direct = crate::metadata::value(dict, b"JBIG2Globals")
        .and_then(|value| decode_budget.reference(pdf_bytes, value));
    let from_params = crate::metadata::value(dict, b"DecodeParms").and_then(|params| {
        let params = if let Some(number) = decode_budget.reference(pdf_bytes, params) {
            object_body_for_number(decode_budget, pdf_bytes, number)?
        } else {
            ObjectBytes::Borrowed(params)
        };
        crate::metadata::value(&params, b"JBIG2Globals")
            .and_then(|value| decode_budget.reference(pdf_bytes, value))
    });
    let object_number = direct.or(from_params)?;
    let (_, data) = extract_stream_for_object(decode_budget, pdf_bytes, object_number)?;
    image_png::copy(decode_budget, data)
}

/// hayro-jbig2 の pixel callback を RGB8 バッファへ受ける decoder。
struct RgbJbig2Decoder {
    /// 出力する RGB8 画素列。
    rgb: Vec<u8>,
    /// 出力画像の幅 px。
    width: u32,
    /// 出力画像の高さ px。
    height: u32,
    /// 現在の x 座標。
    x: u32,
    /// 現在の y 座標。
    y: u32,
}

impl RgbJbig2Decoder {
    /// 指定寸法の RGB decoder を作成する。
    fn new(decode_budget: &DecodeBudget, width: u32, height: u32) -> Option<Self> {
        decode_budget.check_image_pixels(width, height)?;
        let capacity = width.checked_mul(height)?.checked_mul(3)? as usize;
        Some(Self {
            rgb: decode_budget.image_vector(capacity)?,
            width,
            height,
            x: 0,
            y: 0,
        })
    }

    /// 1 pixel を RGB8 として追記する。
    fn push_rgb_pixel(&mut self, black: bool) {
        if self.y >= self.height || self.x >= self.width {
            return;
        }
        let value = if black { 0 } else { 255 };
        self.rgb.extend_from_slice(&[value, value, value]);
        self.x += 1;
    }

    /// 不足分を白で埋めて PNG に変換する。
    fn finish_png(mut self, decode_budget: &DecodeBudget) -> Option<Vec<u8>> {
        if decode_budget.is_stopped() {
            return None;
        }
        let expected_len = self.width.checked_mul(self.height)?.checked_mul(3)? as usize;
        self.rgb.resize(expected_len, 255);
        encode_rgb8_png(decode_budget, self.width, self.height, &self.rgb)
    }
}

impl Jbig2Decoder for RgbJbig2Decoder {
    /// 画像範囲内の1画素だけを受け取る。
    fn push_pixel(&mut self, black: bool) {
        self.push_rgb_pixel(black);
    }

    /// 巨大なrun長でも残りの行幅だけ処理する。
    fn push_pixel_chunk(&mut self, black: bool, chunk_count: u32) {
        if self.y >= self.height {
            return;
        }
        let remaining = self.width.saturating_sub(self.x);
        for _ in 0..chunk_count.saturating_mul(8).min(remaining) {
            self.push_rgb_pixel(black);
        }
    }

    /// 画像末尾を越えた行通知では補完を行わない。
    fn next_line(&mut self) {
        while self.y < self.height && self.x < self.width {
            self.push_rgb_pixel(false);
        }
        self.x = 0;
        self.y = self.y.saturating_add(1);
    }
}

/// DCTDecode JPEG stream を RGB PNG に変換する。
fn decode_dct_jpeg_as_png(decode_budget: &DecodeBudget, data: &[u8]) -> Option<Vec<u8>> {
    with_dct_jpeg_rgb(decode_budget, data, |width, height, _, pixels| {
        image_png::encode_jpeg(decode_budget, width, height, pixels)
    })
}

/// 予約済みJPEG作業配列を借用し、色変換後も所有配列へ複製せず処理する。
fn with_dct_jpeg_rgb<T>(
    decode_budget: &DecodeBudget,
    data: &[u8],
    process: impl FnOnce(u32, u32, usize, &mut [u8]) -> Option<T>,
) -> Option<T> {
    let mut decoder = jpeg_rgb_decoder(decode_budget, data)?;
    let info = decoder.info()?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    let components = usize::from(info.components);
    decode_budget.with_jpeg_pixels(decoder.output_buffer_size()?, |pixels| {
        decoder.decode_into(pixels).ok()?;
        match decoder.get_output_colorspace()? {
            ColorSpace::RGB => process(width, height, components, pixels),
            ColorSpace::Luma => {
                let mut rgb = image_pixels::gray(decode_budget, pixels)?;
                process(width, height, components, &mut rgb)
            }
            _ => None,
        }
    })
}

/// JPEGヘッダと実画素上限を検証し、RGB出力用のdecoderを返す。
fn jpeg_rgb_decoder<'a>(
    decode_budget: &DecodeBudget,
    data: &'a [u8],
) -> Option<JpegDecoder<&'a [u8]>> {
    if decode_budget.is_stopped() {
        return None;
    }
    let options = DecoderOptions::default()
        .set_max_width(u16::MAX as usize)
        .set_max_height(u16::MAX as usize)
        .jpeg_set_out_colorspace(ColorSpace::RGB);
    let mut decoder = JpegDecoder::new_with_options(data, options);
    decoder.decode_headers().ok()?;
    let info = decoder.info()?;
    let width = u32::from(info.width);
    let height = u32::from(info.height);
    decode_budget.check_image_pixels(width, height)?;
    Some(decoder)
}

/// JPEGのsource成分数と、予算内で復号したRGBをPNG化せず引き渡す。
fn decode_dct_jpeg_rgb(
    decode_budget: &DecodeBudget,
    data: &[u8],
) -> Option<image_codec_decode::Image> {
    let mut decoder = jpeg_rgb_decoder(decode_budget, data)?;
    let info = decoder.info()?;
    let width = u32::from(info.width);
    let height = u32::from(info.height);
    let output_length = decoder.output_buffer_size()?;
    let mut decoded = decode_budget.image_vector(output_length)?;
    decoded.resize(output_length, 0);
    decoder.decode_into(&mut decoded).ok()?;
    let rgb = match decoder.get_output_colorspace()? {
        ColorSpace::RGB => decoded,
        ColorSpace::Luma => image_pixels::gray(decode_budget, &decoded)?,
        _ => return None,
    };
    Some(image_codec_decode::Image {
        width,
        height,
        components: usize::from(info.components),
        rgb,
    })
}

/// indirect object number に対応する object 本文を返す。
fn object_body_for_number<'a>(
    decode_budget: &DecodeBudget,
    bytes: &'a [u8],
    object_number: u32,
) -> Option<ObjectBytes<'a>> {
    let index = decode_budget.stream_index(bytes);
    crate::metadata::objects(bytes, index.as_deref())
        .find(|object| {
            object.number == object_number
                && index
                    .as_ref()
                    .map_or(object.generation == 0, |index| index.selects(object))
        })
        .map(|object| ObjectBytes::Borrowed(object.body))
        .or_else(|| {
            extract_compressed_object_bodies(decode_budget, bytes)
                .get(&object_number)
                .map(|body| ObjectBytes::shared(Rc::clone(body)))
        })
}

/// object number に対応する stream の辞書と payload を返す。
fn extract_stream_for_object<'a>(
    decode_budget: &DecodeBudget,
    bytes: &'a [u8],
    object_number: u32,
) -> Option<(&'a [u8], &'a [u8])> {
    if let Some(index) = decode_budget.stream_index(bytes) {
        return index.for_object(bytes, object_number);
    }
    let marker = format!("{object_number} 0 obj");
    let object_pos = find_bytes(bytes, marker.as_bytes())?;
    let stream_rel = find_bytes(&bytes[object_pos..], b"stream")?;
    let stream_pos = object_pos + stream_rel;
    let dict_start = bytes[object_pos..stream_pos]
        .windows(2)
        .position(|window| window == b"<<")
        .map(|pos| object_pos + pos)?;
    let dict = &bytes[dict_start..stream_pos];
    let end_rel = find_bytes(&bytes[stream_pos..], b"endstream")?;
    let mut data_start = stream_pos + b"stream".len();
    if bytes.get(data_start) == Some(&b'\r') {
        data_start += 1;
    }
    if bytes.get(data_start) == Some(&b'\n') {
        data_start += 1;
    }
    let mut data_end = stream_pos + end_rel;
    while data_end > data_start && matches!(bytes.get(data_end - 1), Some(b'\n') | Some(b'\r')) {
        data_end -= 1;
    }
    Some((dict, &bytes[data_start..data_end]))
}

/// `stream` 直前の object 辞書全体を返す。
fn stream_dictionary_before_stream(bytes: &[u8], stream_pos: usize) -> Option<&[u8]> {
    let prefix = object_header_prefix_before_stream(bytes, stream_pos)?;
    let obj_marker_pos = rfind_bytes(prefix, b" obj")?;
    let dict_start = prefix[obj_marker_pos..]
        .windows(2)
        .position(|window| window == b"<<")
        .map(|pos| obj_marker_pos + pos)?;
    Some(&prefix[dict_start..])
}

/// 画像の辞書直下の属性を、直接値または間接 object から読む。
fn image_attribute<'a>(
    decode_budget: &DecodeBudget,
    pdf_bytes: &'a [u8],
    dictionary: &'a [u8],
    key: &[u8],
) -> Option<ObjectBytes<'a>> {
    let value = crate::metadata::value(dictionary, key)?;
    if let Some(number) = decode_budget.reference(pdf_bytes, value) {
        object_body_for_number(decode_budget, pdf_bytes, number)
    } else {
        Some(ObjectBytes::Borrowed(value))
    }
}

/// 単一 filter の DecodeParms を直接・間接・単一配列から解決する。
fn image_decode_parameters<'a>(
    decode_budget: &DecodeBudget,
    pdf_bytes: &'a [u8],
    dictionary: &'a [u8],
) -> Option<ObjectBytes<'a>> {
    if crate::metadata::value(dictionary, b"DecodeParms").is_none() {
        return Some(ObjectBytes::Borrowed(b"<< >>"));
    }
    let value = image_attribute(decode_budget, pdf_bytes, dictionary, b"DecodeParms")?;
    let value = if let Some(values) = crate::metadata::array_values(&value, 1) {
        let selected = *values.first()?;
        if let Some(number) = decode_budget.reference(pdf_bytes, selected) {
            object_body_for_number(decode_budget, pdf_bytes, number)?
        } else {
            let start = (selected.as_ptr() as usize).checked_sub(value.as_ptr() as usize)?;
            let end = start.checked_add(selected.len())?;
            value.slice(start..end)?
        }
    } else {
        value
    };
    let mut lexer = crate::syntax::Lexer {
        bytes: &value,
        cursor: 0,
    };
    match lexer.next()? {
        crate::syntax::Token::Open(true) => Some(value),
        crate::syntax::Token::Word(b"null") if lexer.next().is_none() => {
            Some(ObjectBytes::Borrowed(b"<< >>"))
        }
        _ => None,
    }
}

/// Indexed DeviceRGB の四要素を読み、lookup の必要長も検査する。
fn parse_indexed_device_rgb_palette_from_pdf(
    decode_budget: &DecodeBudget,
    pdf_bytes: &[u8],
    color_space: &[u8],
) -> Option<Vec<u8>> {
    let values = crate::metadata::array_values(color_space, 4)?;
    let [kind, base, high, lookup] = values.as_slice() else {
        return None;
    };
    let base = if let Some(number) = decode_budget.reference(pdf_bytes, base) {
        object_body_for_number(decode_budget, pdf_bytes, number)?
    } else {
        ObjectBytes::Borrowed(base)
    };
    if !crate::metadata::is_name(kind, b"Indexed") || !crate::metadata::is_name(&base, b"DeviceRGB")
    {
        return None;
    }
    let high = std::str::from_utf8(high).ok()?.trim().parse::<u8>().ok()?;
    let palette = if let Some(number) = decode_budget.reference(pdf_bytes, lookup) {
        read_palette_object_bytes(decode_budget, pdf_bytes, number)?
    } else {
        crate::image_metadata::string_bytes(decode_budget, lookup)?
    };
    (palette.len() >= (usize::from(high) + 1) * 3).then_some(palette)
}

/// palette object の stream または単一 PDF string 本体を読む。
fn read_palette_object_bytes(
    decode_budget: &DecodeBudget,
    pdf_bytes: &[u8],
    object_number: u32,
) -> Option<Vec<u8>> {
    if let Some((dict, data)) = extract_stream_for_object(decode_budget, pdf_bytes, object_number) {
        if crate::metadata::value(dict, b"Filter").is_none() {
            return image_png::copy(decode_budget, data);
        }
        return (ImageFilter::read(dict)? == ImageFilter::Flate)
            .then(|| decode_zlib(data, decode_budget))?;
    }
    crate::image_metadata::string_bytes(
        decode_budget,
        &object_body_for_number(decode_budget, pdf_bytes, object_number)?,
    )
}

/// RGB8 の画素列を予算内で PNG バイト列へ符号化する。
fn encode_rgb8_png(
    decode_budget: &DecodeBudget,
    width: u32,
    height: u32,
    rgb: &[u8],
) -> Option<Vec<u8>> {
    image_png::encode(decode_budget, width, height, rgb, 3, 2)
}

/// RGBA8 の画素列を予算内で PNG バイト列へ符号化する。
fn encode_rgba8_png(
    decode_budget: &DecodeBudget,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Option<Vec<u8>> {
    image_png::encode(decode_budget, width, height, rgba, 4, 6)
}

/// PDF 辞書文字列から `/Name number` を読む。
fn parse_number_after_name(object: &str, name: &str) -> Option<f32> {
    let value = crate::metadata::value(object.as_bytes(), name.strip_prefix('/')?.as_bytes())?;
    let number = std::str::from_utf8(value).ok()?.parse::<f32>().ok()?;
    number.is_finite().then_some(number)
}

/// 未対応画像 filter 用の白背景 fallback PNG を作る。
fn fallback_image_png_bytes(
    decode_budget: &DecodeBudget,
    width: u32,
    height: u32,
) -> Option<Vec<u8>> {
    let width = width.max(1);
    let height = height.max(1);
    let Some(byte_len) = width
        .checked_mul(height)
        .and_then(|count| count.checked_mul(3))
    else {
        return image_png::copy(decode_budget, one_pixel_png_bytes());
    };
    let mut pixels = decode_budget.image_vector(byte_len as usize)?;
    pixels.resize(byte_len as usize, 255);
    encode_rgb8_png(decode_budget, width, height, &pixels)
        .or_else(|| image_png::copy(decode_budget, one_pixel_png_bytes()))
}

/// DOCX 埋め込み用の最小 PNG。
fn one_pixel_png_bytes() -> &'static [u8] {
    &[
        0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, b'I', b'H', b'D',
        b'R', 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f,
        0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, b'I', b'D', b'A', b'T', 0x78, 0x9c, 0x63, 0xf8,
        0xff, 0xff, 0xff, 0x7f, 0x00, 0x09, 0xfb, 0x03, 0xfd, 0x05, 0x43, 0x45, 0xca, 0x00, 0x00,
        0x00, 0x00, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82,
    ]
}

/// content stream から座標付き text element を抽出する。
#[allow(clippy::too_many_arguments)]
fn extract_text_elements_from_content_stream(
    budget: &DecodeBudget,
    page: usize,
    content: &[u8],
    font_names: &dyn crate::metadata_map::Lookup<String>,
    font_encodings: &dyn crate::metadata_map::Lookup<String>,
    font_cmaps: &dyn crate::metadata_map::Lookup<ToUnicodeCMap>,
    cmap: &ToUnicodeCMap,
    metrics: &dyn crate::metadata_map::Lookup<std::sync::Arc<crate::font_widths::Metrics>>,
    alpha: impl FnMut(&[u8]) -> crate::text_alpha::Parameters,
) -> Vec<ParsedTextElement> {
    let matrices = crate::form_content::text_matrices(budget, content, [1., 0., 0., 1., 0., 0.]);
    let clips = crate::content_clip::scan(budget, content, [1., 0., 0., 1., 0., 0.]);
    extract_text_elements_with_matrices(
        budget,
        page,
        content,
        font_names,
        font_encodings,
        font_cmaps,
        cmap,
        metrics,
        None,
        crate::text_draws::State::initial(Default::default()),
        alpha,
        |prefix| {
            matrices.get(&prefix.len()).copied().map(|matrix| {
                crate::text_geometry::TextGeometry::new(matrix, None)
                    .clipped(clips.get(prefix.len()))
            })
        },
    )
}

/// 呼び出し元が決めた CTM と予算で、命令ごとの文字列・位置・Font を抽出する。
#[allow(clippy::too_many_arguments)]
fn extract_text_elements_with_matrices<'clip>(
    budget: &DecodeBudget,
    page: usize,
    content: &[u8],
    font_names: &dyn crate::metadata_map::Lookup<String>,
    font_encodings: &dyn crate::metadata_map::Lookup<String>,
    font_cmaps: &dyn crate::metadata_map::Lookup<ToUnicodeCMap>,
    cmap: &ToUnicodeCMap,
    metrics: &dyn crate::metadata_map::Lookup<std::sync::Arc<crate::font_widths::Metrics>>,
    inherited_fonts: Option<&PageFontResources>,
    state: crate::text_draws::State,
    alpha: impl FnMut(&[u8]) -> crate::text_alpha::Parameters,
    geometry: impl Fn(&[u8]) -> Option<crate::text_geometry::TextGeometry<'clip>>,
) -> Vec<ParsedTextElement> {
    let bytes = content;
    let mut elements = Vec::<ParsedTextElement>::new();
    let mut last_sequence = None;
    let mut last_draw = None;
    let mut current_sequence = None;
    let mut current_geometry = None;
    crate::text_draws::visit_with_state(
        budget,
        bytes,
        state,
        alpha,
        |begin, sequence, raw, state, matrix| {
            if budget.is_stopped() {
                return 0.;
            }
            let (font_names, font_encodings, font_cmaps, metrics) =
                if let Some(fonts) = inherited_fonts.filter(|_| state.inherited_font) {
                    (
                        &fonts.base_fonts as &dyn crate::metadata_map::Lookup<String>,
                        &fonts.encodings as &dyn crate::metadata_map::Lookup<String>,
                        &fonts.cmaps as &dyn crate::metadata_map::Lookup<ToUnicodeCMap>,
                        &fonts.metrics
                            as &dyn crate::metadata_map::Lookup<
                                std::sync::Arc<crate::font_widths::Metrics>,
                            >,
                    )
                } else {
                    (font_names, font_encodings, font_cmaps, metrics)
                };
            let mut text = decode_text_show_operand(
                budget,
                raw,
                cmap,
                font_encodings,
                font_cmaps,
                state.font.as_deref(),
            );
            if budget.is_stopped() {
                return 0.;
            }
            let font_name = state.font.as_deref().map(|resource| {
                font_names
                    .get(resource)
                    .map(String::as_str)
                    .unwrap_or(resource)
            });
            if current_sequence != Some(sequence) {
                current_sequence = Some(sequence);
                current_geometry = geometry(&bytes[..sequence]);
            }
            let rendering_matrix =
                concat_matrix(matrix, [state.horizontal_scale, 0., 0., 1., 0., state.rise]);
            let geometry = current_geometry.map(|geometry| geometry.transformed(rendering_matrix));
            let page_matrix = geometry.map(|geometry| geometry.matrix);
            let layout = state
                .font
                .as_ref()
                .and_then(|font| metrics.get(font))
                .and_then(|metrics| {
                    metrics.layout(
                        budget,
                        raw,
                        state.size,
                        state.character_spacing,
                        state.word_spacing,
                    )
                })
                .unwrap_or_else(|| {
                    let width = estimate_text_width_for_matrix(
                        &text,
                        state.size,
                        font_name,
                        geometry.as_ref().map_or(matrix, |geometry| geometry.matrix),
                    );
                    state
                        .font
                        .as_ref()
                        .and_then(|font| metrics.get(font))
                        .and_then(|metrics| {
                            metrics.estimated_layout(
                                budget,
                                raw,
                                width,
                                state.character_spacing,
                                state.word_spacing,
                            )
                        })
                        .unwrap_or_else(|| {
                            crate::font_widths::Layout::estimated(
                                &text,
                                width,
                                state.character_spacing,
                                state.word_spacing,
                            )
                        })
                });
            if budget.is_stopped() {
                return 0.;
            }
            let font_size =
                state.size * geometry.map_or(1., |g| g.vertical_scale().max(f32::EPSILON));
            let clipped = geometry
                .filter(|g| g.needs_glyph_clip(layout.left, layout.right, state.size))
                .and_then(|geometry| {
                    crate::text_clipping::filter(
                        budget,
                        raw,
                        state.font.as_deref().and_then(|font| metrics.get(font))?,
                        geometry,
                        state.size,
                        state.character_spacing,
                        state.word_spacing,
                        state
                            .font
                            .as_deref()
                            .and_then(|font| font_cmaps.get(font))
                            .unwrap_or(cmap),
                        state
                            .font
                            .as_deref()
                            .and_then(|font| font_encodings.get(font))
                            .map(String::as_str),
                    )
                });
            let bounds = if let Some((visible, bounds)) = clipped {
                text = visible;
                bounds
            } else {
                geometry.and_then(|geometry| {
                    geometry
                        .transformed([1., 0., 0., 1., layout.left, 0.])
                        .bounds(budget, layout.right - layout.left, state.size)
                })
            };
            let adjacent = page_matrix.is_some_and(|current| {
                last_draw.is_some_and(|(last_begin, previous, width)| {
                    last_begin == begin
                        && crate::text_draws::adjacent(previous, width, current, state.size)
                })
            });
            let merge = (last_sequence == Some(sequence) || adjacent)
                && elements.last().is_some_and(|previous| {
                    previous.font_size == font_size
                        && previous.font_name.as_deref() == font_name
                        && previous.fill_color == state.color
                });
            if state.rendering.paints_with_alpha(
                state
                    .font
                    .as_deref()
                    .and_then(|name| metrics.get(name))
                    .is_some_and(|font| font.is_type3()),
                budget.is_pdf20(),
                state.alpha,
            ) && (!text.trim().is_empty()
                || (merge && !text.is_empty())
                || (!text.is_empty()
                    && text
                        .chars()
                        .all(|ch| ch.is_whitespace() && !ch.is_control())))
                && let Some(bounds) = bounds
            {
                if merge && let Some(previous) = elements.last_mut() {
                    if crate::text_result_storage::extend(budget, &mut previous.text, &text)
                        .is_none()
                    {
                        return 0.;
                    }
                    previous.bounds.x0 = previous.bounds.x0.min(bounds.x0);
                    previous.bounds.y0 = previous.bounds.y0.min(bounds.y0);
                    previous.bounds.x1 = previous.bounds.x1.max(bounds.x1);
                    previous.bounds.y1 = previous.bounds.y1.max(bounds.y1);
                } else {
                    let Some(font_name) = crate::text_result_storage::copy(budget, font_name)
                    else {
                        return 0.;
                    };
                    let Some(fill_color) =
                        crate::text_result_storage::copy(budget, state.color.as_deref())
                    else {
                        return 0.;
                    };
                    if crate::metadata_workspace::try_push(
                        budget,
                        &mut elements,
                        ParsedTextElement {
                            page,
                            bounds,
                            font_size,
                            font_name,
                            fill_color,
                            text,
                        },
                    )
                    .is_none()
                    {
                        return 0.;
                    }
                }
                last_sequence = Some(sequence);
                last_draw = page_matrix.map(|matrix| (begin, matrix, layout.advance));
            } else {
                last_sequence = None;
                last_draw = None;
            }
            layout.advance * state.horizontal_scale
        },
    );
    if budget.is_stopped() {
        Vec::new()
    } else {
        elements
    }
}

/// PDF 全体から font resource 名と BaseFont 名の対応を読む。
fn parse_font_resource_base_fonts(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
) -> crate::metadata_map::Map<String, String> {
    let parser_source = parser_source_with_compressed_objects(decode_budget, bytes);
    let Some(source) = crate::metadata_input::text(decode_budget, &parser_source) else {
        return Default::default();
    };
    let base_font_by_object = font_object_base_fonts(decode_budget, &source);
    crate::font_fallback::build(decode_budget, &source, base_font_by_object)
}

/// 間接 Type を解決し、Font 辞書かを判定する。
fn is_font_dictionary<'bytes>(
    resolver: &mut crate::resource_resolver::ResourceResolver<'_, 'bytes>,
    dictionary: &'bytes [u8],
) -> bool {
    crate::metadata::value(dictionary, b"Type")
        .and_then(|value| resolver.resolve(value))
        .is_some_and(|(value, _)| crate::metadata::is_name(value, b"Font"))
}

/// Font object numberとBaseFont名の対応を読む。
fn font_object_base_fonts(decode_budget: &DecodeBudget, source: &str) -> font_names::Names {
    let Some(objects) = crate::object_index::build(decode_budget, source.as_bytes()) else {
        return Default::default();
    };
    font_object_names(decode_budget, &objects, b"BaseFont")
}

/// Font object numberとexternal CMap encoding名の対応を読む。
pub(crate) fn font_object_encodings(
    decode_budget: &DecodeBudget,
    source: &str,
) -> font_names::Names {
    let Some(objects) = crate::object_index::build(decode_budget, source.as_bytes()) else {
        return Default::default();
    };
    font_object_names(decode_budget, &objects, b"Encoding")
}

/// parser が保持する埋め込み font 1 件の最大展開サイズ。
const MAX_EMBEDDED_FONT_BYTES: usize = 8 * 1024 * 1024;

/// 1 PDF から保持する埋め込み font の最大数。
const MAX_EMBEDDED_FONT_COUNT: usize = 32;

/// PDF の FontDescriptor から再利用可能な `FontFile2` stream を抽出する。
fn extract_embedded_truetype_fonts(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
) -> Vec<ParsedEmbeddedFont> {
    let parser_source = parser_source_with_compressed_objects(decode_budget, bytes);
    let Some(source) = crate::metadata_input::text(decode_budget, &parser_source) else {
        return Default::default();
    };
    let stream_offsets = stream_object_offsets(decode_budget, bytes);
    let descriptor_mappings =
        font_descriptor_tounicode_mappings(decode_budget, bytes, &source, &stream_offsets);
    embedded_fonts::build(
        decode_budget,
        source.as_bytes(),
        |number| embedded_font_payload(decode_budget, bytes, number, &stream_offsets),
        |number| {
            descriptor_mappings
                .get(&number)
                .cloned()
                .unwrap_or_default()
        },
    )
}

/// Font objectのDescriptor参照と個別ToUnicode CMapを対応付ける。
fn font_descriptor_tounicode_mappings(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
    source: &str,
    stream_offsets: &StreamOffsets,
) -> crate::metadata_map::Map<u32, ToUnicodeCMap> {
    let cmap_by_font_object =
        font_object_tounicode_cmaps(decode_budget, bytes, source, stream_offsets);
    font_descriptor_cmaps::build(decode_budget, source.as_bytes(), |number| {
        cmap_by_font_object.get(&number)
    })
}

/// font resource名と各Font object固有のToUnicode CMapを対応付ける。
fn parse_font_resource_tounicode_cmaps(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
) -> crate::metadata_map::Map<String, ToUnicodeCMap> {
    let parser_source = parser_source_with_compressed_objects(decode_budget, bytes);
    let Some(source) = crate::metadata_input::text(decode_budget, &parser_source) else {
        return Default::default();
    };
    let stream_offsets = stream_object_offsets(decode_budget, bytes);
    let cmap_by_font_object =
        font_object_tounicode_cmaps(decode_budget, bytes, &source, &stream_offsets);
    crate::font_fallback::build(decode_budget, &source, cmap_by_font_object)
}

/// Font object numberとそのToUnicode streamを個別CMapへ展開する。
fn font_object_tounicode_cmaps(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
    source: &str,
    stream_offsets: &StreamOffsets,
) -> font_cmaps::Index {
    let Some(objects) = crate::object_index::build(decode_budget, source.as_bytes()) else {
        return Default::default();
    };
    font_object_tounicode_cmaps_in(decode_budget, bytes, &objects, stream_offsets)
}

/// 共有 object index の Font Type を解決して ToUnicode CMap を抽出する。
fn font_object_tounicode_cmaps_in(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
    objects: &crate::page_attributes::Objects<'_>,
    stream_offsets: &StreamOffsets,
) -> font_cmaps::Index {
    font_cmaps::read(decode_budget, bytes, objects, stream_offsets)
}

/// TrueType/OpenType sfnt として再埋め込み可能な先頭signatureか判定する。
fn sfnt_font_signature_is_supported(bytes: &[u8]) -> bool {
    matches!(
        bytes.get(..4),
        Some(b"\0\x01\0\0" | b"true" | b"OTTO" | b"ttcf")
    )
}

/// sfntの埋め込み可否を表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SfntEmbeddingPermission {
    /// installableまたはeditable embeddingが明示されている。
    Allowed,
    /// restricted、bitmap-only、または不正なtable構造である。
    Prohibited,
    /// OS/2 tableがなく、PDF内subsetとToUnicodeの範囲だけ再利用できる。
    Unspecified,
}

/// OS/2 fsTypeからDOCXへの埋め込み可否を判定する。
fn sfnt_embedding_permission(bytes: &[u8]) -> SfntEmbeddingPermission {
    let Some(num_tables) = read_be_u16(bytes, 4).map(usize::from) else {
        return SfntEmbeddingPermission::Prohibited;
    };
    for index in 0..num_tables {
        let Some(record_start) = 12usize.checked_add(index.saturating_mul(16)) else {
            return SfntEmbeddingPermission::Prohibited;
        };
        let Some(tag) = bytes.get(record_start..record_start.saturating_add(4)) else {
            return SfntEmbeddingPermission::Prohibited;
        };
        if tag != b"OS/2" {
            continue;
        }
        let Some(table_offset) =
            read_be_u32(bytes, record_start.saturating_add(8)).map(|value| value as usize)
        else {
            return SfntEmbeddingPermission::Prohibited;
        };
        let Some(fs_type) = read_be_u16(bytes, table_offset.saturating_add(8)) else {
            return SfntEmbeddingPermission::Prohibited;
        };
        let restricted = fs_type & 0x0002 != 0;
        let editable = fs_type & 0x0008 != 0;
        let bitmap_only = fs_type & 0x0200 != 0;
        return if !restricted && !bitmap_only && (fs_type == 0 || editable) {
            SfntEmbeddingPermission::Allowed
        } else {
            SfntEmbeddingPermission::Prohibited
        };
    }
    SfntEmbeddingPermission::Unspecified
}

/// byte列からbig-endian u16を境界検証つきで読む。
fn read_be_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?,
    ))
}

/// byte列からbig-endian u32を境界検証つきで読む。
fn read_be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
    ))
}

/// PDF 2D affine matrix を点に適用する。
fn transform_point(matrix: [f32; 6], x: f32, y: f32) -> (f32, f32) {
    (
        matrix[0] * x + matrix[2] * y + matrix[4],
        matrix[1] * x + matrix[3] * y + matrix[5],
    )
}

/// 0.0-1.0 の PDF 色成分を 8bit 値へ変換する。
pub(crate) fn pdf_color_component_to_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// PDF DeviceCMYK成分を追加確保なしのRGB画素へ近似変換する。
pub(crate) fn pdf_cmyk_to_rgb(cyan: f32, magenta: f32, yellow: f32, black: f32) -> [u8; 3] {
    let black = black.clamp(0.0, 1.0);
    let red = 1.0 - (cyan.clamp(0.0, 1.0) + black).min(1.0);
    let green = 1.0 - (magenta.clamp(0.0, 1.0) + black).min(1.0);
    let blue = 1.0 - (yellow.clamp(0.0, 1.0) + black).min(1.0);
    [
        pdf_color_component_to_u8(red),
        pdf_color_component_to_u8(green),
        pdf_color_component_to_u8(blue),
    ]
}

/// font size から text element の概算幅を計算する。
fn estimate_text_width(text: &str, font_size: f32) -> f32 {
    text.chars()
        .map(|ch| {
            if ch.is_ascii() {
                font_size * 0.5
            } else {
                font_size * 0.78
            }
        })
        .sum()
}

/// 回転・斜交文字では標準 font metrics を使い、局所文字空間の概算幅を返す。
fn estimate_text_width_for_matrix(
    text: &str,
    font_size: f32,
    font_name: Option<&str>,
    matrix: [f32; 6],
) -> f32 {
    let has_rotation_or_skew = matrix[1].abs() > 0.0001 || matrix[2].abs() > 0.0001;
    if has_rotation_or_skew
        && let Some(width) = estimate_helvetica_text_width(text, font_size, font_name)
    {
        return width;
    }
    estimate_text_width(text, font_size)
}

/// Helvetica AFMのglyph幅からASCII textの幅をpt単位で見積もる。
fn estimate_helvetica_text_width(
    text: &str,
    font_size: f32,
    font_name: Option<&str>,
) -> Option<f32> {
    let font_name = font_name?.rsplit('+').next()?;
    if !matches!(font_name, "Helvetica" | "Helvetica-Oblique") {
        return None;
    }
    let width_units = text.chars().try_fold(0u32, |sum, ch| {
        Some(sum + u32::from(crate::font_widths::helvetica_glyph_width(ch)?))
    })?;
    Some(width_units as f32 * font_size / 1000.0)
}

/// `/Type /Page` の `/Contents` が参照する stream payload をページ番号付きで取り出す。
///
/// 本文一覧の保存配列は metadata 予算へ確保前に予約する。
/// `/Filter /FlateDecode` は展開し、Filter なしの stream はそのまま content stream として扱う。
pub(crate) fn extract_page_content_streams_with_page(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
) -> Vec<(usize, Vec<u8>)> {
    let page_ref_source = parser_source_with_compressed_objects(decode_budget, bytes);
    let references = parse_page_content_object_refs_with_page(decode_budget, &page_ref_source);
    let Some(mut streams) = decode_budget.metadata_vector(references.len()) else {
        return Vec::new();
    };
    let stream_offsets = stream_object_offsets(decode_budget, bytes);
    for (page, object_number) in references {
        if let Some(stream) =
            stream_payload_for_object_number(decode_budget, bytes, object_number, &stream_offsets)
        {
            streams.push((page, stream));
        }
    }
    streams
}

/// PDF バイト列から、容量制限付きのstream offset索引を作る。
pub(crate) fn stream_object_offsets(decode_budget: &DecodeBudget, bytes: &[u8]) -> StreamOffsets {
    StreamOffsets::build(decode_budget, bytes)
}

/// Form の未圧縮本文もコピー前に予算を検査し、有効 stream だけを復元する。
pub(crate) fn form_stream_payload(
    budget: &DecodeBudget,
    bytes: &[u8],
    number: u32,
    offsets: &StreamOffsets,
) -> Option<Vec<u8>> {
    let (dictionary, data) = offsets
        .get(&number)
        .and_then(|offset| extract_stream_object_at_offset(budget, bytes, *offset))
        .or_else(|| extract_stream_for_object(budget, bytes, number))?;
    if crate::metadata::value(dictionary, b"Filter").is_none() {
        return budget.copy_stream_bytes(data);
    }
    decode_content_stream_payload(budget, dictionary, data)
}

/// Font の Filter 方針を検査して、検査済み stream の完成 map を共有する。
fn font_cmap_for_object_number(
    budget: &DecodeBudget,
    bytes: &[u8],
    number: u32,
    offsets: &StreamOffsets,
) -> Option<ToUnicodeCMap> {
    if budget.is_stopped() {
        return None;
    }
    if let Some(index) = budget.stream_index(bytes) {
        let (dict, _) = index.for_object(bytes, number)?;
        if dict.windows(b"/Filter".len()).any(|w| w == b"/Filter")
            && !dict
                .windows(b"FlateDecode".len())
                .any(|w| w == b"FlateDecode")
        {
            return None;
        }
        drop(index);
        return crate::text::parse_tounicode_stream(budget, bytes, number);
    }
    let payload = cmap_payload_for_object_number(budget, bytes, number, offsets)?;
    Some(parse_tounicode_body(budget, &payload))
}

/// CMap の Filter 方針を維持し、未圧縮 payload は入力から借用する。
fn cmap_payload_for_object_number<'a>(
    decode_budget: &DecodeBudget,
    bytes: &'a [u8],
    object_number: u32,
    offsets: &StreamOffsets,
) -> Option<Cow<'a, [u8]>> {
    if decode_budget.is_stopped() {
        return None;
    }
    let (dict, data) = offsets
        .get(&object_number)
        .and_then(|offset| extract_stream_object_at_offset(decode_budget, bytes, *offset))
        .or_else(|| extract_stream_for_object(decode_budget, bytes, object_number))?;
    if dict
        .windows(b"FlateDecode".len())
        .any(|w| w == b"FlateDecode")
    {
        decode_zlib(data, decode_budget).map(Cow::Owned)
    } else if !dict.windows(b"/Filter".len()).any(|w| w == b"/Filter") {
        Some(Cow::Borrowed(data))
    } else {
        None
    }
}

/// FontFile2 の保存・展開配列を確保前に累積予算で検査する。
fn embedded_font_payload(
    budget: &DecodeBudget,
    bytes: &[u8],
    object_number: u32,
    offsets: &StreamOffsets,
) -> Option<Vec<u8>> {
    if budget.is_stopped() {
        return None;
    }
    let (dict, data) = offsets
        .get(&object_number)
        .and_then(|offset| extract_stream_object_at_offset(budget, bytes, *offset))
        .or_else(|| extract_stream_for_object(budget, bytes, object_number))?;
    if dict
        .windows(b"FlateDecode".len())
        .any(|word| word == b"FlateDecode")
    {
        decode_zlib_font(data, budget)
    } else if !dict
        .windows(b"/Filter".len())
        .any(|word| word == b"/Filter")
    {
        let mut saved = budget.font_vector(data.len())?;
        saved.extend_from_slice(data);
        Some(saved)
    } else {
        None
    }
}

/// 指定 object number の stream payload だけを Filter に応じて復元する。
fn stream_payload_for_object_number(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
    object_number: u32,
    offsets: &StreamOffsets,
) -> Option<Vec<u8>> {
    if let Some(offset) = offsets.get(&object_number)
        && let Some((dict, data)) = extract_stream_object_at_offset(decode_budget, bytes, *offset)
        && let Some(payload) = decode_content_stream_payload(decode_budget, dict, data)
    {
        return Some(payload);
    }
    let (dict, data) = extract_stream_for_object(decode_budget, bytes, object_number)?;
    decode_content_stream_payload(decode_budget, dict, data)
}

#[path = "rootless_streams.rs"]
mod rootless_streams;

/// Rootなし入力のFlate本文を番号順に保存し、追加配列なしでページ番号を付ける。
fn extract_flate_streams(decode_budget: &DecodeBudget, bytes: &[u8]) -> Vec<(usize, Vec<u8>)> {
    rootless_streams::extract(decode_budget, bytes)
}

/// stream dictionary の Filter に応じて content stream payload を復元する。
fn decode_content_stream_payload(
    decode_budget: &DecodeBudget,
    dict: &[u8],
    data: &[u8],
) -> Option<Vec<u8>> {
    if dict
        .windows(b"FlateDecode".len())
        .any(|w| w == b"FlateDecode")
    {
        let decoded = decode_zlib(data, decode_budget)?;
        Some(decoded)
    } else if !dict.windows(b"/Filter".len()).any(|w| w == b"/Filter") {
        decode_budget.copy_stream_bytes(data)
    } else {
        None
    }
}

/// stream の直前にある `n 0 obj` から object number を読む。
fn parse_object_number_before_stream(bytes: &[u8], stream_pos: usize) -> Option<u32> {
    let prefix = object_header_prefix_before_stream(bytes, stream_pos)?;
    let obj_marker_pos = rfind_bytes(prefix, b" obj")?;
    let header_start = prefix[..obj_marker_pos]
        .iter()
        .rposition(|byte| matches!(byte, b'\n' | b'\r'))
        .map_or(0, |index| index + 1);
    let header = prefix[header_start..obj_marker_pos]
        .split(|byte| byte.is_ascii_whitespace())
        .find(|token| !token.is_empty())?;
    parse_ascii_u32(header)
}

/// stream 直前の object header を含む探索範囲を返す。
fn object_header_prefix_before_stream(bytes: &[u8], stream_pos: usize) -> Option<&[u8]> {
    let local_start = stream_pos.saturating_sub(512);
    let local = bytes.get(local_start..stream_pos)?;
    if rfind_bytes(local, b" obj").is_some() {
        return Some(local);
    }
    bytes.get(..stream_pos)
}

/// xref stream の object entry。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum XrefStreamEntry {
    /// type 1: 通常 indirect object の byte offset。
    Uncompressed {
        /// object 開始 byte offset。
        offset: usize,
    },
    /// type 2: object stream 内に圧縮された indirect object。
    Compressed {
        /// object stream の object number。
        object_stream: u32,
        /// object stream 内の index。
        index: u32,
    },
}

/// PDF dictionary 内の `/Name n` を u32 として読む。
fn parse_pdf_u32_after_name(dict: &[u8], name: &[u8]) -> Option<u32> {
    let value = crate::metadata::value(dict, name.strip_prefix(b"/").unwrap_or(name))?;
    let mut lexer = crate::syntax::Lexer {
        bytes: value,
        cursor: 0,
    };
    let crate::syntax::Token::Word(word) = lexer.next()? else {
        return None;
    };
    let number = crate::syntax::integer(word).and_then(|n| u32::try_from(n).ok())?;
    lexer.next().is_none().then_some(number)
}

/// big-endian unsigned integer field を読む。
fn read_be_uint(bytes: &[u8]) -> Option<u64> {
    let mut value = 0u64;
    for byte in bytes {
        value = value.checked_mul(256)?.checked_add(u64::from(*byte))?;
    }
    Some(value)
}

/// 指定 offset の object が stream object なら辞書と payload を返す。
fn extract_stream_object_at_offset<'a>(
    decode_budget: &DecodeBudget,
    bytes: &'a [u8],
    object_offset: usize,
) -> Option<(&'a [u8], &'a [u8])> {
    if let Some(index) = decode_budget.stream_index(bytes) {
        return index.at_offset(bytes, object_offset);
    }
    let object = bytes.get(object_offset..)?;
    let stream_rel = find_bytes(object, b"stream")?;
    let stream_pos = object_offset + stream_rel;
    let endobj_rel = find_bytes(object, b"endobj")?;
    if stream_rel > endobj_rel {
        return None;
    }
    let dict_start = object[..stream_rel]
        .windows(2)
        .rposition(|window| window == b"<<")?;
    let dict = &object[dict_start..stream_rel];
    crate::legacy_stream::payload(bytes, dict, stream_pos).map(|(data, _)| (dict, data))
}

/// PDF の 1 行を byte slice と次の cursor に分ける。
fn next_pdf_line(bytes: &[u8], cursor: usize) -> (&[u8], usize) {
    let mut end = cursor;
    while end < bytes.len() && !matches!(bytes[end], b'\n' | b'\r') {
        end += 1;
    }
    let mut next = end;
    if bytes.get(next) == Some(&b'\r') {
        next += 1;
    }
    if bytes.get(next) == Some(&b'\n') {
        next += 1;
    }
    (&bytes[cursor..end], next)
}

/// ASCII whitespace を前後から取り除く。
fn trim_ascii(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|byte| !byte.is_ascii_whitespace())
        .map_or(start, |index| index + 1);
    &bytes[start..end]
}

/// PDF の page object から page 番号付き `/Contents n 0 R` 参照を抽出する。
fn parse_page_content_object_refs_with_page(
    decode_budget: &DecodeBudget,
    bytes: &[u8],
) -> Vec<(usize, u32)> {
    crate::content_refs::page_references(decode_budget, bytes)
}

/// ASCII decimal token を `u32` として読む。
fn parse_ascii_u32(token: &[u8]) -> Option<u32> {
    if token.is_empty() || token.iter().any(|byte| !byte.is_ascii_digit()) {
        return None;
    }
    let mut value = 0u32;
    for byte in token {
        value = value.checked_mul(10)?.checked_add(u32::from(byte - b'0'))?;
    }
    Some(value)
}

/// byte slice 内で needle の開始位置を探す。
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// byte slice 内で needle の最後の開始位置を探す。
fn rfind_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

/// 既存fixture向けに既定予算で罫線を抽出する。
#[cfg(test)]
fn extract_graphics_from_content_stream(page: usize, content: &str) -> Vec<ParsedGraphicElement> {
    graphics_with_budget(&DecodeBudget::default(), page, content.as_bytes())
}

/// 2 つの PDF affine matrix を連結する。
pub(crate) fn concat_matrix(left: [f32; 6], right: [f32; 6]) -> [f32; 6] {
    [
        left[0] * right[0] + left[2] * right[1],
        left[1] * right[0] + left[3] * right[1],
        left[0] * right[2] + left[2] * right[3],
        left[1] * right[2] + left[3] * right[3],
        left[0] * right[4] + left[2] * right[5] + left[4],
        left[1] * right[4] + left[3] * right[5] + left[5],
    ]
}

/// PDF 矩形へ CTM を適用した外接 bounds を返す。
pub(crate) fn transform_rect_bounds(
    matrix: [f32; 6],
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> ParsedBounds {
    let points = [
        transform_point(matrix, x, y),
        transform_point(matrix, x + w, y),
        transform_point(matrix, x, y + h),
        transform_point(matrix, x + w, y + h),
    ];
    let mut bounds = ParsedBounds {
        x0: f32::INFINITY,
        y0: f32::INFINITY,
        x1: f32::NEG_INFINITY,
        y1: f32::NEG_INFINITY,
    };
    for (x, y) in points {
        bounds.x0 = bounds.x0.min(x);
        bounds.y0 = bounds.y0.min(y);
        bounds.x1 = bounds.x1.max(x);
        bounds.y1 = bounds.y1.max(y);
    }
    bounds
}

/// 細い filled rectangle を中心線に正規化する。
fn thin_rect_to_line(x: f32, y: f32, w: f32, h: f32) -> Option<(ParsedBounds, f32)> {
    const MAX_RULE_THICKNESS: f32 = 2.5;

    let abs_w = w.abs();
    let abs_h = h.abs();
    if abs_w <= MAX_RULE_THICKNESS && abs_h > MAX_RULE_THICKNESS {
        let cx = x + w / 2.0;
        Some((
            ParsedBounds {
                x0: cx,
                y0: y,
                x1: cx,
                y1: y + h,
            },
            abs_w,
        ))
    } else if abs_h <= MAX_RULE_THICKNESS && abs_w > MAX_RULE_THICKNESS {
        let cy = y + h / 2.0;
        Some((
            ParsedBounds {
                x0: x,
                y0: cy,
                x1: x + w,
                y1: cy,
            },
            abs_h,
        ))
    } else {
        None
    }
}

/// `m/l/h/f` で描かれた細い矩形パスを中心線に正規化する。
fn thin_path_segments_to_line(path_segments: &[ParsedBounds]) -> Option<(ParsedBounds, f32)> {
    if path_segments.is_empty() {
        return None;
    }
    let mut x0 = f32::INFINITY;
    let mut y0 = f32::INFINITY;
    let mut x1 = f32::NEG_INFINITY;
    let mut y1 = f32::NEG_INFINITY;
    for segment in path_segments {
        x0 = x0.min(segment.x0).min(segment.x1);
        y0 = y0.min(segment.y0).min(segment.y1);
        x1 = x1.max(segment.x0).max(segment.x1);
        y1 = y1.max(segment.y0).max(segment.y1);
    }
    thin_rect_to_line(x0, y0, x1 - x0, y1 - y0)
}

/// 軸平行線分だけを扱う。
fn is_axis_aligned(bounds: ParsedBounds) -> bool {
    (bounds.x0 - bounds.x1).abs() < f32::EPSILON || (bounds.y0 - bounds.y1).abs() < f32::EPSILON
}

/// bounds の端点順を正規化する。
fn normalized_bounds(bounds: ParsedBounds) -> ParsedBounds {
    ParsedBounds {
        x0: bounds.x0.min(bounds.x1),
        y0: bounds.y0.min(bounds.y1),
        x1: bounds.x0.max(bounds.x1),
        y1: bounds.y0.max(bounds.y1),
    }
}

/// 軸平行線分を既存配列の中で結合する。
fn merge_axis_aligned_segments(segments: Vec<ParsedGraphicElement>) -> Vec<ParsedGraphicElement> {
    graphics_merge::merge(segments)
}

#[cfg(test)]
use crate::page_attribute_fixture;

#[cfg(test)]
#[path = "parser_metadata_output_tests.rs"]
mod metadata_output_tests;

#[cfg(test)]
#[path = "parser_font_metrics_tests.rs"]
mod font_metrics_tests;

#[cfg(test)]
#[path = "parser_tounicode_shared_tests.rs"]
mod tounicode_shared_tests;

#[cfg(test)]
#[path = "parser_image_limits.rs"]
mod image_limit_tests;

#[cfg(test)]
#[path = "parser_content_budget_tests.rs"]
mod content_budget_tests;

#[cfg(test)]
#[path = "rootless_stream_budget_tests.rs"]
mod rootless_stream_budget_tests;

#[cfg(test)]
mod tests {
    /// 入れ子 Descriptor によらず実 Font の直下参照へ CMap を対応付ける。
    #[test]
    fn descriptor_mapping_uses_root_font_reference() {
        let cmap = b"1 beginbfchar\n<41> <0041>\nendbfchar";
        let source = format!(
            "1 0 obj << /Type /Font /Note << /Type /FontDescriptor /FontDescriptor 3 0 R >> /FontDescriptor 4 0 R /ToUnicode 2 0 R >> endobj\n2 0 obj << /Length {} >>\nstream\n{}\nendstream\nendobj",
            cmap.len(),
            std::str::from_utf8(cmap).unwrap()
        );
        let budget = crate::decode::DecodeBudget::new(&crate::ParseLimits::default());
        let mappings = super::font_descriptor_tounicode_mappings(
            &budget,
            source.as_bytes(),
            &source,
            &super::StreamOffsets::default(),
        );
        assert_eq!(
            mappings.get(&4).unwrap().bmp_scalar_mappings(),
            vec![(0x41, 0x41)]
        );
        assert!(!mappings.contains_key(&3));
    }

    /// binary 内の偽 DecodeParms は正規の JBIG2 globals 参照を上書きしない。
    #[test]
    fn jbig2_params_ignore_fake_object_in_payload() {
        let fake = b"endstream\nendobj\n4 0 obj << /JBIG2Globals 3 0 R >> endobj";
        let mut bytes =
            format!("%PDF-1.7\n1 0 obj << /Length {} >>\nstream\n", fake.len()).into_bytes();
        bytes.extend_from_slice(fake);
        bytes.extend_from_slice(b"\nendstream\nendobj\n2 0 obj << /Length 4 >>\nstream\ngood\nendstream\nendobj\n3 0 obj << /Length 3 >>\nstream\nbad\nendstream\nendobj\n4 0 obj << /JBIG2Globals 2 0 R >> endobj\n%%EOF");
        let limits = crate::ParseLimits::default();
        let budget = crate::decode::DecodeBudget::new(&limits);
        let streams = crate::structure::validate_structure(
            std::path::Path::new("globals.pdf"),
            &bytes,
            9,
            &limits,
            &budget,
        )
        .unwrap();
        budget.cache_streams(&bytes, &streams);
        assert_eq!(
            super::jbig2_globals_stream(&budget, &bytes, "<< /DecodeParms 4 0 R >>"),
            Some(b"good".to_vec())
        );
    }

    /// 文字列・コメント内の偽 DecodeParms object と入れ子キーを除外する。
    #[test]
    fn jbig2_globals_ignore_metadata_and_nested_keys() {
        for metadata in [
            "1 0 obj << /Title (4 0 obj << /JBIG2Globals 3 0 R >> endobj) >> endobj\n",
            "1 0 obj << /Title (title) >> endobj\n% 4 0 obj << /JBIG2Globals 3 0 R >> endobj\n",
        ] {
            let bytes = format!("%PDF-1.7\n{metadata}2 0 obj << /Length 4 >>\nstream\ngood\nendstream\nendobj\n3 0 obj << /Length 3 >>\nstream\nbad\nendstream\nendobj\n4 0 obj << /Note << /JBIG2Globals 3 0 R >> /JBIG2Globals 2 0 R >> endobj\n%%EOF").into_bytes();
            let limits = crate::ParseLimits::default();
            let budget = crate::decode::DecodeBudget::new(&limits);
            let streams = crate::structure::validate_structure(
                std::path::Path::new("globals-metadata.pdf"),
                &bytes,
                9,
                &limits,
                &budget,
            )
            .unwrap();
            budget.cache_streams(&bytes, &streams);
            for dictionary in [
                "<< /DecodeParms 4 0 R >>",
                "<< /Note << /JBIG2Globals 3 0 R >> /DecodeParms 4 0 R >>",
            ] {
                assert_eq!(
                    super::jbig2_globals_stream(&budget, &bytes, dictionary),
                    Some(b"good".to_vec())
                );
            }
        }
    }

    use super::{
        ParsedPageViewport, SfntEmbeddingPermission, count_page_objects,
        extract_compressed_object_bodies, extract_graphics_from_content_stream,
        extract_graphics_from_pdf_bytes, extract_page_content_streams_with_page,
        extract_text_elements_from_pdf_bytes, has_more_than_image_xobject_streams,
        merge_axis_aligned_segments, normalize_bounds_for_page, parse_page_metadata,
        parse_page_viewports, parse_pdf_version_from_header, repair_sfnt_unicode_cmap,
        sfnt_embedding_permission, stream_object_offsets,
    };
    use crate::content_refs::from_page as parse_contents_refs_from_page_object_bytes;
    use crate::{ParsedBounds, ParsedGraphicElement, ParsedPage};
    use flate2::{Compression, write::ZlibEncoder};
    use proptest::prelude::*;
    use std::io::Write;

    proptest! {
        #[test]
        fn valid_single_digit_versions_are_parsed(major in 0u8..=9, minor in 0u8..=9) {
            let header = format!("%PDF-{}.{}", major, minor);
            let parsed = parse_pdf_version_from_header(&header);
            prop_assert!(parsed.is_some());
            let parsed = parsed.expect("must parse");
            prop_assert_eq!(parsed.major, major);
            prop_assert_eq!(parsed.minor, minor);
        }

        #[test]
        fn invalid_version_headers_are_rejected(
            a in "[^0-9]",
            b in "[^0-9]"
        ) {
            let header = format!("%PDF-{}.{}", a, b);
            prop_assert!(parse_pdf_version_from_header(&header).is_none());
        }

        #[test]
        fn arbitrary_sfnt_bytes_never_panic_during_embedding_permission_check(
            bytes in proptest::collection::vec(any::<u8>(), 0..4096)
        ) {
            let _ = sfnt_embedding_permission(&bytes);
        }

        /// 任意byte列とCID mappingでもcmap修復がpanicしないことを検証する。
        #[test]
        fn arbitrary_sfnt_bytes_never_panic_during_unicode_cmap_repair(
            bytes in proptest::collection::vec(any::<u8>(), 0..4096),
            mappings in proptest::collection::vec((any::<u16>(), 0u16..0xd800u16), 0..64)
        ) {
            let _ = repair_sfnt_unicode_cmap(&crate::decode::DecodeBudget::default(), &bytes, mappings.iter().copied());
        }
    }

    /// OS/2 fsTypeの明示的な埋め込み制約を優先する。
    #[test]
    fn sfnt_embedding_permission_rejects_restricted_and_bitmap_only_fonts() {
        assert_eq!(
            sfnt_embedding_permission(&sfnt_with_fs_type(0x0002)),
            SfntEmbeddingPermission::Prohibited
        );
        assert_eq!(
            sfnt_embedding_permission(&sfnt_with_fs_type(0x0200)),
            SfntEmbeddingPermission::Prohibited
        );
        assert_eq!(
            sfnt_embedding_permission(&sfnt_with_fs_type(0x0008)),
            SfntEmbeddingPermission::Allowed
        );
        assert_eq!(
            sfnt_embedding_permission(&sfnt_with_fs_type(0)),
            SfntEmbeddingPermission::Allowed
        );
    }

    /// 指定fsTypeを持つ最小sfnt風byte列をtest用に作る。
    fn sfnt_with_fs_type(fs_type: u16) -> Vec<u8> {
        let mut bytes = vec![0u8; 38];
        bytes[..4].copy_from_slice(b"\0\x01\0\0");
        bytes[4..6].copy_from_slice(&1u16.to_be_bytes());
        bytes[12..16].copy_from_slice(b"OS/2");
        bytes[20..24].copy_from_slice(&28u32.to_be_bytes());
        bytes[24..28].copy_from_slice(&10u32.to_be_bytes());
        bytes[36..38].copy_from_slice(&fs_type.to_be_bytes());
        bytes
    }

    #[test]
    fn short_header_is_rejected() {
        assert!(parse_pdf_version_from_header("%PDF-1").is_none());
    }

    #[test]
    fn non_pdf_prefix_is_rejected() {
        assert!(parse_pdf_version_from_header("NOT-1.7").is_none());
    }

    #[test]
    fn count_page_objects_excludes_pages_tree() {
        let bytes = b"2 0 obj << /Type /Pages /Count 1 >> endobj 3 0 obj << /Type /Page /Parent 2 0 R >> endobj";
        assert_eq!(count_page_objects(&Default::default(), bytes), 1);
    }

    #[test]
    fn count_page_objects_accepts_no_space_before_page_name_delimiter() {
        let bytes = b"<< /Type/Page/Parent 2 0 R >> << /Type/Pages/Count 1 >>";
        assert_eq!(count_page_objects(&Default::default(), bytes), 1);
    }

    #[test]
    fn parse_page_metadata_extracts_media_box_and_rotation() {
        let bytes = b"3 0 obj << /Type /Page /MediaBox [10 20 210 320] /Rotate 90 >> endobj";
        let pages = parse_page_metadata(&Default::default(), bytes);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].page_number, 0);
        assert_eq!(pages[0].width, 200.0);
        assert_eq!(pages[0].height, 300.0);
        assert_eq!(pages[0].rotation, 90);
    }

    #[test]
    fn parse_page_metadata_prefers_crop_box_display_size() {
        let bytes =
            b"3 0 obj << /Type /Page /MediaBox [0 0 300 200] /CropBox [50 25 250 175] >> endobj";
        let pages = parse_page_metadata(&Default::default(), bytes);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].width, 200.0);
        assert_eq!(pages[0].height, 150.0);
    }

    #[test]
    fn parse_page_metadata_inherits_media_and_crop_boxes_from_pages_parent() {
        let bytes = b"2 0 obj << /Type /Pages /MediaBox [0 0 300 200] /CropBox [50 25 250 175] /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R >> endobj";
        let pages = parse_page_metadata(&Default::default(), bytes);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].width, 200.0);
        assert_eq!(pages[0].height, 150.0);
    }

    #[test]
    fn parse_page_metadata_uses_nearest_box_across_multiple_parent_levels() {
        let bytes = b"6 0 obj << /Type /Pages /MediaBox [0 0 300 200] /CropBox [10 10 290 190] /Kids [2 0 R] /Count 1 >> endobj
2 0 obj << /Type /Pages /Parent 6 0 R /CropBox [50 25 250 175] /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R >> endobj";
        let pages = parse_page_metadata(&Default::default(), bytes);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].width, 200.0);
        assert_eq!(pages[0].height, 150.0);
    }

    #[test]
    fn parse_page_metadata_stops_on_cyclic_pages_parent_chain() {
        let bytes = b"2 0 obj << /Type /Pages /Parent 2 0 R /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R >> endobj";
        assert!(parse_page_metadata(&Default::default(), bytes).is_empty());
    }

    #[test]
    fn inherited_page_rotation_rotates_crop_relative_bounds_clockwise() {
        let bytes = b"2 0 obj << /Type /Pages /MediaBox [0 0 300 200] /CropBox [50 25 250 175] /Rotate 90 /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R >> endobj";
        let viewports = parse_page_viewports(&Default::default(), bytes);
        assert_eq!(viewports.len(), 1);
        assert_eq!(viewports[0].page.rotation, 90);

        let mut bounds = ParsedBounds {
            x0: 70.0,
            y0: 115.0,
            x1: 110.0,
            y1: 135.0,
        };
        assert!(normalize_bounds_for_page(0, &mut bounds, &viewports));
        assert_eq!(
            bounds,
            ParsedBounds {
                x0: 90.0,
                y0: 140.0,
                x1: 110.0,
                y1: 180.0,
            }
        );
    }

    #[test]
    fn crop_box_normalization_translates_clips_and_rejects_hidden_bounds() {
        let viewport = ParsedPageViewport {
            page: ParsedPage {
                page_number: 0,
                width: 200.0,
                height: 150.0,
                rotation: 0,
            },
            origin_x: 50.0,
            origin_y: 25.0,
            clip_to_page: true,
        };
        let mut partial = ParsedBounds {
            x0: 40.0,
            y0: 20.0,
            x1: 80.0,
            y1: 60.0,
        };
        assert!(normalize_bounds_for_page(
            0,
            &mut partial,
            &[viewport.clone()]
        ));
        assert_eq!(
            partial,
            ParsedBounds {
                x0: 0.0,
                y0: 0.0,
                x1: 30.0,
                y1: 35.0,
            }
        );

        let mut hidden = ParsedBounds {
            x0: 10.0,
            y0: 5.0,
            x1: 20.0,
            y1: 15.0,
        };
        assert!(!normalize_bounds_for_page(0, &mut hidden, &[viewport]));
    }

    #[test]
    fn parse_page_metadata_defaults_missing_rotation_to_zero() {
        let bytes = b"3 0 obj << /Type /Page /MediaBox [0 0 200 300] >> endobj";
        let pages = parse_page_metadata(&Default::default(), bytes);
        assert_eq!(pages[0].rotation, 0);
    }

    #[test]
    fn extract_graphics_from_content_stream_converts_thin_filled_rectangles_to_lines() {
        let graphics = extract_graphics_from_content_stream(
            0,
            "84.96 742.64 m 85.44 742.64 l 85.44 650.16 l 84.96 650.16 l h f",
        );
        assert_eq!(graphics.len(), 1);
        assert_eq!(graphics[0].bounds.x0.round(), 85.0);
        assert_eq!(graphics[0].bounds.y0.round(), 650.0);
        assert_eq!(graphics[0].bounds.x1.round(), 85.0);
        assert_eq!(graphics[0].bounds.y1.round(), 743.0);
    }

    #[test]
    fn extract_graphics_from_content_stream_converts_two_point_filled_rectangles_to_lines() {
        let graphics = extract_graphics_from_content_stream(0, "20 149 100 2 re f 19 60 2 90 re f");

        assert_eq!(graphics.len(), 2);
        assert_eq!(graphics[0].bounds.y0.round(), 150.0);
        assert_eq!(graphics[0].bounds.y1.round(), 150.0);
        assert_eq!(graphics[1].bounds.x0.round(), 20.0);
        assert_eq!(graphics[1].bounds.x1.round(), 20.0);
    }

    #[test]
    fn extract_graphics_from_content_stream_applies_ctm_to_table_rules() {
        let graphics = extract_graphics_from_content_stream(
            0,
            "q 0.75 0 0 0.75 72 86 cm 0 0 m 600 0 l S 0 0 m 0 96 l S Q",
        );

        assert_eq!(graphics.len(), 2);
        assert_eq!(graphics[0].bounds.x0.round(), 72.0);
        assert_eq!(graphics[0].bounds.x1.round(), 522.0);
        assert_eq!(graphics[1].bounds.x0.round(), 72.0);
        assert_eq!(graphics[1].bounds.y1.round(), 158.0);
    }

    #[test]
    fn image_stream_counter_skips_stream_words_inside_payload() {
        let pdf = b"%PDF-1.4
1 0 obj << /Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB /Length 47 >> stream
stream stream stream stream stream stream stream
endstream endobj
";

        assert!(
            !has_more_than_image_xobject_streams(&crate::decode::DecodeBudget::default(), pdf, 1),
            "single image stream payload bytes must not be counted as additional image streams"
        );
    }

    #[test]
    fn page_contents_ref_parser_accepts_delimiter_after_ref_marker() {
        let page_object =
            b"<< /Type/Page /Contents 4 0 R/Group << /Type/Group/S/Transparency >> >>";

        let refs = parse_contents_refs_from_page_object_bytes(page_object).collect::<Vec<_>>();

        assert_eq!(refs, vec![4]);
    }

    #[test]
    fn extract_graphics_from_pdf_bytes_skips_all_stream_fallback_for_many_image_streams() {
        let mut pdf = b"%PDF-1.7\n\
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >> endobj\n"
            .to_vec();
        let decoy_graphics = zlib_compress(b"0 10 m 100 10 l S");
        push_flate_stream_object(&mut pdf, 4, &decoy_graphics);
        for object_number in 5..27 {
            push_image_stream_object(&mut pdf, object_number);
        }
        pdf.extend_from_slice(b"%%EOF\n");

        let graphics = extract_graphics_from_pdf_bytes(&Default::default(), &pdf);

        assert!(
            graphics.is_empty(),
            "many image-heavy PDFs must not treat arbitrary Flate streams as page graphics"
        );
    }

    #[test]
    fn extract_graphics_from_pdf_bytes_uses_page_contents_instead_of_all_flate_streams() {
        let pdf =
            pdf_with_page_content_and_decoy_stream(b"0 10 m 100 10 l S", b"20 0 m 20 100 l S");

        let graphics = extract_graphics_from_pdf_bytes(&Default::default(), &pdf);

        assert_eq!(graphics.len(), 1);
        assert_eq!(graphics[0].page, 0);
        assert_eq!(graphics[0].bounds.x0, 0.0);
        assert_eq!(graphics[0].bounds.y0, 10.0);
        assert_eq!(graphics[0].bounds.x1, 100.0);
        assert_eq!(graphics[0].bounds.y1, 10.0);
    }

    #[test]
    fn extract_graphics_from_pdf_bytes_reads_contents_array_in_order() {
        let pdf = pdf_with_contents_array_and_decoy_streams(
            &[b"0 10 m 100 10 l S", b"20 0 m 20 100 l S"],
            b"40 0 m 40 100 l S",
        );

        let graphics = extract_graphics_from_pdf_bytes(&Default::default(), &pdf);

        assert_eq!(graphics.len(), 2);
        assert_eq!(graphics[0].page, 0);
        assert_eq!(graphics[1].page, 0);
        assert_eq!(graphics[0].bounds.y0, 10.0);
        assert_eq!(graphics[1].bounds.x0, 20.0);
    }

    #[test]
    fn page_contents_ref_parser_does_not_read_resource_refs_after_direct_contents() {
        let page = b"3 0 obj << /Type /Page /Contents 4 0 R /Resources << /Font << /F1 99 0 R >> >> >> endobj";

        let refs = parse_contents_refs_from_page_object_bytes(page).collect::<Vec<_>>();

        assert_eq!(refs, vec![4]);
    }

    #[test]
    fn xref_table_offsets_read_in_use_subsections() {
        let pdf = b"%PDF-1.7\n\
xref\n\
0 2\n\
0000000000 65535 f\x20\n\
0000000017 00000 n\x20\n\
4 2\n\
0000000242 00000 n\x20\n\
0000000310 00000 n\x20\n\
trailer\n\
<< /Size 6 >>\n\
startxref\n\
400\n\
%%EOF";

        let offsets = stream_object_offsets(&Default::default(), pdf);

        assert_eq!(offsets.get(&1), Some(&17));
        assert_eq!(offsets.get(&4), Some(&242));
        assert_eq!(offsets.get(&5), Some(&310));
        assert!(offsets.get(&0).is_none());
    }

    #[test]
    fn page_content_flate_streams_can_be_resolved_from_xref_offsets() {
        let pdf = pdf_with_xref_table(b"0 10 m 100 10 l S");

        let streams = extract_page_content_streams_with_page(&Default::default(), &pdf);

        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].0, 0);
        assert_eq!(streams[0].1, b"0 10 m 100 10 l S");
    }

    #[test]
    fn xref_stream_offsets_read_flate_encoded_in_use_entries() {
        let pdf = pdf_with_xref_stream(b"0 10 m 100 10 l S");

        let offsets = stream_object_offsets(&Default::default(), &pdf);

        assert!(offsets.get(&1).is_some());
        assert!(offsets.get(&4).is_some());
        assert!(offsets.get(&0).is_none());
    }

    #[test]
    fn page_content_flate_streams_can_be_resolved_from_xref_stream_offsets() {
        let pdf = pdf_with_xref_stream(b"20 0 m 20 100 l S");

        let streams = extract_page_content_streams_with_page(&Default::default(), &pdf);

        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].0, 0);
        assert_eq!(streams[0].1, b"20 0 m 20 100 l S");
    }

    #[test]
    fn compressed_object_body_can_be_extracted_from_object_stream_entry() {
        let pdf = pdf_with_compressed_page_object(b"30 0 m 30 100 l S");

        let bodies = extract_compressed_object_bodies(&Default::default(), &pdf);
        let page = String::from_utf8_lossy(bodies.get(&3).expect("page object body must exist"));

        assert!(page.contains("/Type /Page"));
        assert!(page.contains("/Contents 4 0 R"));
    }

    #[test]
    fn page_content_flate_streams_can_use_page_object_from_object_stream() {
        let pdf = pdf_with_compressed_page_object(b"30 0 m 30 100 l S");

        let streams = extract_page_content_streams_with_page(&Default::default(), &pdf);

        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].0, 0);
        assert_eq!(streams[0].1, b"30 0 m 30 100 l S");
    }

    #[test]
    fn page_count_reads_page_object_from_object_stream() {
        let pdf = pdf_with_compressed_page_object(b"30 0 m 30 100 l S");

        assert_eq!(count_page_objects(&Default::default(), &pdf), 1);
    }

    #[test]
    fn page_metadata_reads_media_box_from_object_stream() {
        let pdf = pdf_with_compressed_page_object(b"30 0 m 30 100 l S");

        let pages = parse_page_metadata(&Default::default(), &pdf);

        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].page_number, 0);
        assert_eq!(pages[0].width, 200.0);
        assert_eq!(pages[0].height, 200.0);
        assert_eq!(pages[0].rotation, 0);
    }

    #[test]
    fn font_resource_base_font_can_be_resolved_from_object_stream() {
        let pdf = pdf_with_compressed_font_object(b"BT /F7 12 Tf 1 0 0 1 10 20 Tm (Styled) Tj ET");

        let text = extract_text_elements_from_pdf_bytes(&Default::default(), &pdf);

        assert_eq!(text.len(), 1);
        assert_eq!(text[0].font_name.as_deref(), Some("CompressedFont"));
    }

    /// 通常ページの縦反転文字を既存の罫線座標系へ合わせる。
    #[test]
    fn y_flipped_text_matrix_keeps_legacy_table_cell_bounds() {
        let pdf = pdf_with_compressed_font_object(
            b".75 0 0 .75 77.25 92.546265 cm BT /F7 10 Tf 1 0 0 -1 4 1.15 Tm (Cell) Tj ET",
        );

        let text = extract_text_elements_from_pdf_bytes(&Default::default(), &pdf);

        assert_eq!(text.len(), 1);
        let bounds = text[0].bounds;
        assert!((bounds.x0 - 80.25).abs() < 0.001);
        assert!((bounds.y0 - 93.408_77).abs() < 0.001);
        assert!((bounds.x1 - 95.25).abs() < 0.001);
        assert!((bounds.y1 - 100.908_77).abs() < 0.001);
    }

    #[test]
    fn extract_graphics_from_pdf_bytes_keeps_page_number_during_merge() {
        let pdf = pdf_with_two_page_contents(
            b"0 10 m 100 10 l S",
            b"20 10 m 120 10 l S",
            b"40 0 m 40 100 l S",
        );

        let graphics = extract_graphics_from_pdf_bytes(&Default::default(), &pdf);

        assert_eq!(graphics.len(), 2);
        assert_eq!(graphics[0].page, 0);
        assert_eq!(graphics[0].bounds.x0, 0.0);
        assert_eq!(graphics[1].page, 1);
        assert_eq!(graphics[1].bounds.x0, 20.0);
    }

    #[test]
    fn merge_axis_aligned_segments_keeps_disjoint_same_axis_segments_separate() {
        let graphics = merge_axis_aligned_segments(vec![
            ParsedGraphicElement {
                page: 0,
                bounds: ParsedBounds {
                    x0: 20.0,
                    y0: 50.0,
                    x1: 20.0,
                    y1: 80.0,
                },
                stroke_width: 1.0,
            },
            ParsedGraphicElement {
                page: 0,
                bounds: ParsedBounds {
                    x0: 20.0,
                    y0: 120.0,
                    x1: 20.0,
                    y1: 150.0,
                },
                stroke_width: 1.0,
            },
            ParsedGraphicElement {
                page: 0,
                bounds: ParsedBounds {
                    x0: 20.0,
                    y0: 80.0,
                    x1: 20.0,
                    y1: 100.0,
                },
                stroke_width: 1.0,
            },
        ]);

        assert_eq!(graphics.len(), 2);
        assert_eq!(graphics[0].bounds.y0, 50.0);
        assert_eq!(graphics[0].bounds.y1, 100.0);
        assert_eq!(graphics[1].bounds.y0, 120.0);
        assert_eq!(graphics[1].bounds.y1, 150.0);
    }

    fn pdf_with_page_content_and_decoy_stream(
        page_content: &[u8],
        decoy_content: &[u8],
    ) -> Vec<u8> {
        let page_stream = zlib_compress(page_content);
        let decoy_stream = zlib_compress(decoy_content);
        let mut pdf = Vec::new();
        pdf.extend_from_slice(
            b"%PDF-1.7\n\
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >> endobj\n",
        );
        push_flate_stream_object(&mut pdf, 4, &page_stream);
        push_flate_stream_object(&mut pdf, 5, &decoy_stream);
        pdf.extend_from_slice(b"%%EOF\n");
        pdf
    }

    fn pdf_with_contents_array_and_decoy_streams(
        contents: &[&[u8]],
        decoy_content: &[u8],
    ) -> Vec<u8> {
        let encoded_contents = contents
            .iter()
            .map(|content| zlib_compress(content))
            .collect::<Vec<_>>();
        let decoy_stream = zlib_compress(decoy_content);
        let mut pdf = Vec::new();
        pdf.extend_from_slice(
            b"%PDF-1.7\n\
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents [4 0 R 5 0 R] >> endobj\n",
        );
        for (index, stream) in encoded_contents.iter().enumerate() {
            push_flate_stream_object(&mut pdf, 4 + index as u32, stream);
        }
        push_flate_stream_object(&mut pdf, 6, &decoy_stream);
        pdf.extend_from_slice(b"%%EOF\n");
        pdf
    }

    fn pdf_with_two_page_contents(
        page_0_content: &[u8],
        page_1_content: &[u8],
        decoy_content: &[u8],
    ) -> Vec<u8> {
        let page_0_stream = zlib_compress(page_0_content);
        let page_1_stream = zlib_compress(page_1_content);
        let decoy_stream = zlib_compress(decoy_content);
        let mut pdf = Vec::new();
        pdf.extend_from_slice(
            b"%PDF-1.7\n\
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 5 0 R >> endobj\n\
4 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 6 0 R >> endobj\n",
        );
        push_flate_stream_object(&mut pdf, 5, &page_0_stream);
        push_flate_stream_object(&mut pdf, 6, &page_1_stream);
        push_flate_stream_object(&mut pdf, 7, &decoy_stream);
        pdf.extend_from_slice(b"%%EOF\n");
        pdf
    }

    fn push_flate_stream_object(pdf: &mut Vec<u8>, object_number: u32, stream: &[u8]) {
        pdf.extend_from_slice(
            format!(
                "{object_number} 0 obj << /Length {} /Filter /FlateDecode >> stream\n",
                stream.len()
            )
            .as_bytes(),
        );
        pdf.extend_from_slice(stream);
        pdf.extend_from_slice(b"\nendstream endobj\n");
    }

    fn push_image_stream_object(pdf: &mut Vec<u8>, object_number: u32) {
        pdf.extend_from_slice(
            format!(
                "{object_number} 0 obj << /Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 /ColorSpace /DeviceRGB /Length 3 >> stream\n"
            )
            .as_bytes(),
        );
        pdf.extend_from_slice(&[255, 255, 255]);
        pdf.extend_from_slice(b"\nendstream endobj\n");
    }

    fn pdf_with_xref_table(page_content: &[u8]) -> Vec<u8> {
        let page_stream = zlib_compress(page_content);
        let mut pdf = Vec::new();
        let mut offsets = Vec::new();
        pdf.extend_from_slice(b"%PDF-1.7\n");
        offsets.push(pdf.len());
        pdf.extend_from_slice(b"1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n");
        offsets.push(pdf.len());
        pdf.extend_from_slice(b"2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n");
        offsets.push(pdf.len());
        pdf.extend_from_slice(
            b"3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >> endobj\n",
        );
        offsets.push(pdf.len());
        push_flate_stream_object(&mut pdf, 4, &page_stream);
        let xref_offset = pdf.len();
        pdf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n")
                .as_bytes(),
        );
        pdf
    }

    fn pdf_with_xref_stream(page_content: &[u8]) -> Vec<u8> {
        let page_stream = zlib_compress(page_content);
        let mut pdf = Vec::new();
        let mut offsets = Vec::new();
        pdf.extend_from_slice(b"%PDF-1.7\n");
        offsets.push(pdf.len());
        pdf.extend_from_slice(b"1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n");
        offsets.push(pdf.len());
        pdf.extend_from_slice(b"2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n");
        offsets.push(pdf.len());
        pdf.extend_from_slice(
            b"3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >> endobj\n",
        );
        offsets.push(pdf.len());
        push_flate_stream_object(&mut pdf, 4, &page_stream);
        offsets.push(pdf.len());
        let mut xref_entries = Vec::new();
        xref_entries.extend_from_slice(&[0, 0, 0, 0, 0, 255]);
        for offset in &offsets {
            xref_entries.push(1);
            xref_entries.extend_from_slice(&(*offset as u32).to_be_bytes());
            xref_entries.push(0);
        }
        let encoded_xref = zlib_compress(&xref_entries);
        pdf.extend_from_slice(
            format!(
                "5 0 obj << /Type /XRef /Size 6 /Index [0 6] /W [1 4 1] /Length {} /Filter /FlateDecode /Root 1 0 R >> stream\n",
                encoded_xref.len()
            )
            .as_bytes(),
        );
        pdf.extend_from_slice(&encoded_xref);
        pdf.extend_from_slice(
            format!("\nendstream endobj\nstartxref\n{}\n%%EOF\n", offsets[4]).as_bytes(),
        );
        pdf
    }

    fn pdf_with_compressed_page_object(page_content: &[u8]) -> Vec<u8> {
        let page_stream = zlib_compress(page_content);
        let page_object =
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>";
        let object_stream_header = b"3 0 ";
        let mut object_stream_payload = Vec::new();
        object_stream_payload.extend_from_slice(object_stream_header);
        object_stream_payload.extend_from_slice(page_object);
        let encoded_object_stream = zlib_compress(&object_stream_payload);
        let mut pdf = Vec::new();
        let mut offsets = Vec::new();
        pdf.extend_from_slice(b"%PDF-1.7\n");
        offsets.push(pdf.len());
        pdf.extend_from_slice(b"1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n");
        offsets.push(pdf.len());
        pdf.extend_from_slice(b"2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n");
        offsets.push(0);
        offsets.push(pdf.len());
        push_flate_stream_object(&mut pdf, 4, &page_stream);
        offsets.push(pdf.len());
        pdf.extend_from_slice(
            format!(
                "5 0 obj << /Type /ObjStm /N 1 /First {} /Length {} /Filter /FlateDecode >> stream\n",
                object_stream_header.len(),
                encoded_object_stream.len()
            )
            .as_bytes(),
        );
        pdf.extend_from_slice(&encoded_object_stream);
        pdf.extend_from_slice(b"\nendstream endobj\n");
        offsets.push(pdf.len());
        let mut xref_entries = Vec::new();
        xref_entries.extend_from_slice(&[0, 0, 0, 0, 0, 255]);
        for object_number in 1..=5 {
            if object_number == 3 {
                xref_entries.push(2);
                xref_entries.extend_from_slice(&5u32.to_be_bytes());
                xref_entries.push(0);
            } else {
                xref_entries.push(1);
                xref_entries.extend_from_slice(&(offsets[object_number - 1] as u32).to_be_bytes());
                xref_entries.push(0);
            }
        }
        let encoded_xref = zlib_compress(&xref_entries);
        pdf.extend_from_slice(
            format!(
                "6 0 obj << /Type /XRef /Size 7 /Index [0 6] /W [1 4 1] /Length {} /Filter /FlateDecode /Root 1 0 R >> stream\n",
                encoded_xref.len()
            )
            .as_bytes(),
        );
        pdf.extend_from_slice(&encoded_xref);
        pdf.extend_from_slice(
            format!("\nendstream endobj\nstartxref\n{}\n%%EOF\n", offsets[5]).as_bytes(),
        );
        pdf
    }

    fn pdf_with_compressed_font_object(page_content: &[u8]) -> Vec<u8> {
        let page_stream = zlib_compress(page_content);
        let font_object = b"<< /Type /Font /Subtype /Type1 /BaseFont /CompressedFont >>";
        let object_stream_header = b"7 0 ";
        let mut object_stream_payload = Vec::new();
        object_stream_payload.extend_from_slice(object_stream_header);
        object_stream_payload.extend_from_slice(font_object);
        let encoded_object_stream = zlib_compress(&object_stream_payload);
        let mut pdf = Vec::new();
        let mut offsets = Vec::new();
        pdf.extend_from_slice(b"%PDF-1.7\n");
        offsets.push(pdf.len());
        pdf.extend_from_slice(b"1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n");
        offsets.push(pdf.len());
        pdf.extend_from_slice(b"2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n");
        offsets.push(pdf.len());
        pdf.extend_from_slice(
            b"3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /Font << /F7 7 0 R >> >> /Contents 4 0 R >> endobj\n",
        );
        offsets.push(pdf.len());
        push_flate_stream_object(&mut pdf, 4, &page_stream);
        offsets.push(pdf.len());
        pdf.extend_from_slice(
            format!(
                "5 0 obj << /Type /ObjStm /N 1 /First {} /Length {} /Filter /FlateDecode >> stream\n",
                object_stream_header.len(),
                encoded_object_stream.len()
            )
            .as_bytes(),
        );
        pdf.extend_from_slice(&encoded_object_stream);
        pdf.extend_from_slice(b"\nendstream endobj\n");
        offsets.push(pdf.len());
        let mut xref_entries = Vec::new();
        xref_entries.extend_from_slice(&[0, 0, 0, 0, 0, 255]);
        for object_number in 1..=7 {
            if object_number == 6 {
                xref_entries.push(0);
                xref_entries.extend_from_slice(&0u32.to_be_bytes());
                xref_entries.push(0);
            } else if object_number == 7 {
                xref_entries.push(2);
                xref_entries.extend_from_slice(&5u32.to_be_bytes());
                xref_entries.push(0);
            } else {
                xref_entries.push(1);
                xref_entries.extend_from_slice(&(offsets[object_number - 1] as u32).to_be_bytes());
                xref_entries.push(0);
            }
        }
        let encoded_xref = zlib_compress(&xref_entries);
        pdf.extend_from_slice(
            format!(
                "6 0 obj << /Type /XRef /Size 8 /Index [0 8] /W [1 4 1] /Length {} /Filter /FlateDecode /Root 1 0 R >> stream\n",
                encoded_xref.len()
            )
            .as_bytes(),
        );
        pdf.extend_from_slice(&encoded_xref);
        pdf.extend_from_slice(
            format!("\nendstream endobj\nstartxref\n{}\n%%EOF\n", offsets[5]).as_bytes(),
        );
        pdf
    }

    fn zlib_compress(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(bytes).expect("zlib write must succeed");
        encoder.finish().expect("zlib finish must succeed")
    }
}
