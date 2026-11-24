//! 段落sectPrに登録したテキストheader/footerも実際のZIP部品として保存する。
use crate::ConvertError;
use docx_rs::{BuildXML, DocumentChild, Docx, SectionProperty, XMLDocx};
use std::collections::BTreeMap;

/// ライブラリが収集しない段落sectionの部品を、relationshipの数値順で補完する。
pub(super) fn build(docx: Docx) -> Result<XMLDocx, ConvertError> {
    let header_count = docx.document_rels.header_count;
    let footer_count = docx.document_rels.footer_count;
    let root = &docx.document.section_property;
    let properties: Vec<_> = docx
        .document
        .children
        .iter()
        .filter_map(|child| {
            if let DocumentChild::Paragraph(paragraph) = child {
                paragraph.property.section_property.as_ref()
            } else {
                None
            }
        })
        .collect();
    let repair_headers = properties.iter().any(|property| {
        property
            .get_headers()
            .iter()
            .any(|header| !root.get_headers().iter().any(|known| known.0 == header.0))
    });
    let repair_footers = properties.iter().any(|property| {
        property
            .get_footers()
            .iter()
            .any(|footer| !root.get_footers().iter().any(|known| known.0 == footer.0))
    });
    let headers = repair_headers
        .then(|| collect(&properties, root, docx.document_rels.header_count, true))
        .transpose()?;
    let footers = repair_footers
        .then(|| collect(&properties, root, docx.document_rels.footer_count, false))
        .transpose()?;
    let mut package = docx.build();
    if let Some(headers) = headers {
        package.header_rels = vec![docx_rs::HeaderRels::new().build(); headers.len()];
        package.headers = headers;
    } else if header_count > 9 {
        package.headers = numeric_order(package.headers, header_count, "rIdHeader")?;
    }
    if let Some(footers) = footers {
        package.footer_rels = vec![docx_rs::FooterRels::new().build(); footers.len()];
        package.footers = footers;
    } else if footer_count > 9 {
        package.footers = numeric_order(package.footers, footer_count, "rIdFooter")?;
    }
    Ok(package)
}

/// docx-rsのrId文字列順をrelationship targetの1始まりの番号順へ戻す。
fn numeric_order(
    parts: Vec<Vec<u8>>,
    count: usize,
    prefix: &str,
) -> Result<Vec<Vec<u8>>, ConvertError> {
    if parts.len() != count {
        return Err(ConvertError::Pack("incomplete page chrome parts".into()));
    }
    let mut indices: Vec<_> = (1..=count).collect();
    indices.sort_by_key(|index| format!("{prefix}{index}"));
    Ok(indices
        .into_iter()
        .zip(parts)
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect())
}

/// ページ見出しの全登録を照合し、欠番や非テキスト参照の黙示的な欠落を拒否する。
fn collect(
    properties: &[&SectionProperty],
    root: &SectionProperty,
    count: usize,
    header: bool,
) -> Result<Vec<Vec<u8>>, ConvertError> {
    let mut parts = BTreeMap::new();
    for property in properties.iter().copied().chain(std::iter::once(root)) {
        let entries: Vec<_> = if header {
            property
                .get_headers()
                .into_iter()
                .map(|(id, part)| (id, part.build()))
                .collect()
        } else {
            property
                .get_footers()
                .into_iter()
                .map(|(id, part)| (id, part.build()))
                .collect()
        };
        for (id, xml) in entries {
            let prefix = if header { "rIdHeader" } else { "rIdFooter" };
            let index = id
                .strip_prefix(prefix)
                .and_then(|id| id.parse::<usize>().ok())
                .ok_or_else(|| ConvertError::Pack("invalid page chrome relationship".into()))?;
            let text = std::str::from_utf8(&xml).map_err(|e| ConvertError::Pack(e.to_string()))?;
            if ["<w:drawing", "<w:pict", "<w:hyperlink"]
                .iter()
                .any(|tag| text.contains(tag))
            {
                return Err(ConvertError::Pack(
                    "paragraph-section chrome requires text-only parts".into(),
                ));
            }
            if parts.get(&index).is_some_and(|known| known != &xml) {
                return Err(ConvertError::Pack(
                    "conflicting page chrome relationship".into(),
                ));
            }
            parts.insert(index, xml);
        }
    }
    if parts.len() != count || !(1..=count).all(|index| parts.contains_key(&index)) {
        return Err(ConvertError::Pack(
            "incomplete paragraph-section chrome parts".into(),
        ));
    }
    Ok(parts.into_values().collect())
}
