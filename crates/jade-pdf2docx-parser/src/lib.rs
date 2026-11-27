mod cancellation;
#[cfg(test)]
mod cancellation_tests;
mod catalog_version;
mod cid_encoding;
mod cid_widths;
mod content_clip;
mod content_refs;
mod decode;
mod font_diagnostics;
mod font_fallback;
mod font_metrics_cache;
mod font_references;
mod font_repair;
mod font_validation;
mod font_weight;
mod font_widths;
mod form_attributes;
mod form_clip;
mod form_content;
mod form_diagnostics;
mod form_image_state;
mod form_matrices;
mod form_operands;
mod form_storage;
mod form_text_state;
mod glyph_names;
mod image_color_space;
mod image_filter;
mod image_metadata;
mod indexed_predictor;
mod indexed_stream;
mod input;
mod legacy_font_names;
mod legacy_page_geometry_validation;
mod legacy_page_resources;
mod legacy_stream;
mod limits;
mod lossy_chunks;
mod lzw_content;
mod metadata;
#[cfg(test)]
mod metadata_extraction_tests;
mod metadata_input;
#[cfg(test)]
mod metadata_input_budget_tests;
mod metadata_map;
mod metadata_names;
mod metadata_source;
#[cfg(test)]
mod metadata_validation_tests;
mod metadata_workspace;
mod model;
mod named_font_encoding;
mod numeric_tj_gap;
mod object_bytes;
mod object_index;
#[cfg(test)]
#[path = "../tests/support/page_attribute_pdf.rs"]
mod page_attribute_fixture;
mod page_attributes;
mod page_contents;
mod page_geometry_diagnostics;
mod page_resources;
mod page_tree;
mod parser;
mod pdf_binary_string;
#[path = "type1_outline_pdf_encodings.rs"]
mod pdf_font_encodings;
#[cfg(not(target_arch = "wasm32"))]
mod pdfium_backend;
#[cfg(not(target_arch = "wasm32"))]
mod pdfium_clip;
#[cfg(not(target_arch = "wasm32"))]
mod pdfium_clip_overlap;
#[cfg(not(target_arch = "wasm32"))]
mod pdfium_font_tags;
#[cfg(not(target_arch = "wasm32"))]
mod pdfium_overprint_groups;
#[cfg(not(target_arch = "wasm32"))]
mod pdfium_overprint_page;
#[cfg(not(target_arch = "wasm32"))]
mod pdfium_printed_hyphens;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod pdfium_subset_native_tests;
#[cfg(not(target_arch = "wasm32"))]
mod pdfium_subset_weight;
#[cfg(not(target_arch = "wasm32"))]
mod pdfium_text;
#[cfg(not(target_arch = "wasm32"))]
mod pdfium_visible_text;
mod quality;
mod registered_cid;
mod resource_cache;
mod resource_resolver;
mod stream_index;
mod structure;
mod syntax;
#[cfg(test)]
#[path = "pdfium_need_allocations.rs"]
mod test_allocations;
mod text;
mod text_alpha;
mod text_baseline;
mod text_clipping;
mod text_draws;
mod text_geometry;
mod text_rendering;
mod text_result_storage;
mod text_stroke_paint;
mod text_word_gap;
mod tounicode_cache;
mod truetype_winansi;
#[cfg(test)]
#[path = "type1_cluster_embedding_tests.rs"]
mod type1_cluster_embedding_tests;
#[cfg(test)]
mod type1_embedding_tests;
mod type1_font;
mod type1_outline;
mod type1_program;
mod type1_standard_encoding;
mod unsupported_content;
mod xobject_refs;
mod xref;

pub use cancellation::ParseCancellation;
pub use limits::ParseLimits;
pub use model::{
    ParseError, ParsedBounds, ParsedDocument, ParsedEmbeddedFont, ParsedGraphicElement,
    ParsedImageElement, ParsedPage, ParsedSourceDiagonal, ParsedTextBackground, ParsedTextElement,
    PdfVersion,
};
pub use parser::{
    parse_pdf, parse_pdf_bytes, parse_pdf_bytes_for_word, parse_pdf_bytes_with_limits,
    parse_pdf_bytes_with_options, parse_pdf_for_word, parse_pdf_with_limits,
    parse_pdf_with_options,
};
pub use quality::{
    ImageReplacementReason, ParseOptions, QualityIssue, QualityReport, UnsupportedFontReason,
    UnsupportedXObjectReason, image_requires_ocr, text_is_visible,
};
