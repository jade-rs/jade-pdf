//! 解析時の座標を変えず、Wordの22in上限に合わせて出力OOXMLを等比縮小する。
use crate::{ConvertError, ParsedDocument, QualityReport};
use docx_rs::XMLDocx;
use jade_pdf2docx_parser::{ParseError, QualityIssue};
use quick_xml::{
    Reader, Writer,
    events::{BytesStart, Event},
};
use std::collections::BTreeMap;

mod parts;
mod xml;

/// Wordが許容するページ各辺の最大値（22in、twips）。
const MAX_TWIPS: u32 = 31_680;

/// 1つのsectionの縮小率と、明示または継承されたheader/footer参照。
#[derive(Clone, Default)]
struct Plan {
    /// 元ページの長辺。上限以下なら1倍として扱う。
    longest: u32,
    /// (要素名、default/first/even)からpackage内relationship IDへの対応。
    references: BTreeMap<(String, String), String>,
}

impl Plan {
    /// 各長さを同一の有理数倍率で丸め、位置の符号とゼロを維持する。
    fn length(&self, value: i64, positive: bool) -> i64 {
        if self.longest <= MAX_TWIPS {
            return value;
        }
        let numerator = i128::from(value) * i128::from(MAX_TWIPS);
        let half = i128::from(self.longest) / 2;
        let scaled = (numerator + if value < 0 { -half } else { half }) / i128::from(self.longest);
        let scaled = scaled as i64;
        if positive && value > 0 {
            scaled.max(1)
        } else {
            scaled
        }
    }
}

/// 解析診断にWord固有の原寸変更を加える。PDF解析結果自体や文字・幾何は変更しない。
pub fn docx_quality_report(parsed: &ParsedDocument) -> QualityReport {
    let mut report = parsed.quality.clone();
    for page in &parsed.pages {
        if crate::parsed_page_size_twips(page).is_some_and(|(w, h)| w.max(h) > MAX_TWIPS) {
            let issue = QualityIssue::PageResizedForWord {
                page: page.page_number,
            };
            if !report.issues.contains(&issue) {
                report.issues.push(issue);
            }
        }
    }
    report
}

/// 変換前にWord診断を記録し、strictではレイアウトやOCRを開始せず拒否する。
pub(super) fn assess(parsed: &mut ParsedDocument, strict: bool) -> Result<(), ConvertError> {
    parsed.quality = docx_quality_report(parsed);
    if strict && !parsed.quality.issues.is_empty() {
        return Err(ParseError::QualityRejected {
            path: parsed.path.clone(),
            report: parsed.quality.clone(),
        }
        .into());
    }
    Ok(())
}

/// 自前builderのOOXMLだけを変換する。画像・font payloadや解析IRは複製・変更しない。
pub(super) fn fit(package: &mut XMLDocx) -> Result<(), ConvertError> {
    let mut plans = plans(&package.document)?;
    if !plans.iter().any(|plan| plan.longest > MAX_TWIPS) {
        return Ok(());
    }
    parts::fit_references(package, &mut plans)?;
    package.document = xml::scale(&package.document, &plans, true)?;
    Ok(())
}

/// section終端に記載されたページ寸法と、前sectionから継承する参照を先に集める。
fn plans(document: &[u8]) -> Result<Vec<Plan>, ConvertError> {
    let mut reader = Reader::from_reader(document);
    let mut result = Vec::new();
    let mut active = BTreeMap::new();
    let mut current: Option<Plan> = None;
    loop {
        match reader.read_event().map_err(error)? {
            Event::Start(ref e) if e.name().as_ref() == b"w:sectPr" => {
                current = Some(Plan {
                    references: active.clone(),
                    ..Plan::default()
                });
            }
            Event::Empty(ref e) if current.is_some() => {
                let plan = current.as_mut().unwrap();
                match e.name().as_ref() {
                    b"w:pgSz" => {
                        plan.longest = attr(e, b"w:w")?
                            .parse::<u32>()
                            .map_err(error)?
                            .max(attr(e, b"w:h")?.parse::<u32>().map_err(error)?);
                    }
                    b"w:headerReference" | b"w:footerReference" => {
                        let tag = String::from_utf8(e.name().as_ref().to_vec()).map_err(error)?;
                        plan.references
                            .insert((tag, attr(e, b"w:type")?), attr(e, b"r:id")?);
                    }
                    _ => {}
                }
            }
            Event::End(ref e) if e.name().as_ref() == b"w:sectPr" => {
                let plan = current.take().ok_or_else(|| error("unpaired section"))?;
                active = plan.references.clone();
                result.push(plan);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(result)
}

/// 制御されたOOXMLの必須属性をUTF-8で取得する。
fn attr(e: &BytesStart<'_>, key: &[u8]) -> Result<String, ConvertError> {
    let attribute = e
        .try_get_attribute(key)
        .map_err(error)?
        .ok_or_else(|| error("missing OOXML attribute"))?;
    String::from_utf8(attribute.value.into_owned()).map_err(error)
}

/// OOXML処理の失敗を変換エラーに統一する。
fn error(message: impl std::fmt::Display) -> ConvertError {
    ConvertError::Pack(format!("Word page fit: {message}"))
}

/// 属性を適切にescapeして小さな空要素を書き込む。
fn empty(
    writer: &mut Writer<Vec<u8>>,
    name: &str,
    attributes: &[(&str, &str)],
) -> Result<(), ConvertError> {
    let mut element = BytesStart::new(name);
    for &(key, value) in attributes {
        element.push_attribute((key, value));
    }
    writer.write_event(Event::Empty(element)).map_err(error)
}

#[cfg(test)]
mod tests;
