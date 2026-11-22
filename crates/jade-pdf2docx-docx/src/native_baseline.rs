//! glyphの字面とbaselineを混同せず、既存の自作・pure IRへは互換の行座標を返す。
use jade_pdf2docx_parser::ParsedTextElement;

/// 実SDKの有限baselineを優先し、未確認・不正な値だけを従来の矩形下端へ戻す。
pub(super) fn line_y(text: &ParsedTextElement) -> f32 {
    text.baseline_y
        .filter(|y| y.is_finite())
        .unwrap_or(text.bounds.y0)
}
