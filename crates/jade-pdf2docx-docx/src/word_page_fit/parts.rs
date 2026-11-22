//! 縮小するsectionだけへheader/footerのXMLを複製し、共有画像は元partを参照する。
use super::*;

/// 参照先と倍率の組ごとに独立partを作り、通常サイズsectionの参照を保持する。
pub(super) fn fit_references(
    package: &mut XMLDocx,
    plans: &mut [Plan],
) -> Result<(), ConvertError> {
    let relationships = relationships(&package.document_rels)?;
    let mut clones: BTreeMap<(bool, String, u32), String> = BTreeMap::new();
    let mut additions = Vec::new();
    let mut overrides = Vec::new();
    for plan in plans {
        if plan.longest <= MAX_TWIPS {
            continue;
        }
        for ((tag, _), id) in &mut plan.references {
            let header = tag == "w:headerReference";
            let key = (header, id.clone(), plan.longest);
            if let Some(new_id) = clones.get(&key) {
                *id = new_id.clone();
                continue;
            }
            let target = relationships
                .get(id)
                .ok_or_else(|| error("missing header/footer relationship"))?;
            let stem = if header { "header" } else { "footer" };
            let index = target
                .strip_prefix(stem)
                .and_then(|s| s.strip_suffix(".xml"))
                .and_then(|s| s.parse::<usize>().ok())
                .and_then(|n| n.checked_sub(1))
                .ok_or_else(|| error("unexpected header/footer part target"))?;
            let (parts, rels) = if header {
                (&mut package.headers, &mut package.header_rels)
            } else {
                (&mut package.footers, &mut package.footer_rels)
            };
            let original = parts
                .get(index)
                .ok_or_else(|| error("missing header/footer part"))?;
            let scaled = xml::scale(
                original,
                &[Plan {
                    longest: plan.longest,
                    ..Plan::default()
                }],
                false,
            )?;
            let number = parts.len() + 1;
            let mut new_id = format!("jadePageFit{stem}{number}");
            while relationships.contains_key(&new_id) {
                new_id.push('_');
            }
            // 元partと同じword/階層なので、画像等へのrelative relationshipは変更しない。
            let rel = rels.get(index).cloned().unwrap_or_else(empty_relationships);
            rels.resize_with(parts.len(), empty_relationships);
            parts.push(scaled);
            rels.push(rel);
            let new_target = format!("{stem}{number}.xml");
            empty_bytes(
                &mut additions,
                "Relationship",
                &[
                    ("Id", &new_id),
                    (
                        "Type",
                        &format!(
                            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/{stem}"
                        ),
                    ),
                    ("Target", &new_target),
                ],
            )?;
            empty_bytes(
                &mut overrides,
                "Override",
                &[
                    ("PartName", &format!("/word/{new_target}")),
                    (
                        "ContentType",
                        &format!(
                            "application/vnd.openxmlformats-officedocument.wordprocessingml.{stem}+xml"
                        ),
                    ),
                ],
            )?;
            clones.insert(key, new_id.clone());
            *id = new_id;
        }
    }
    append_before_close(&mut package.document_rels, b"</Relationships>", &additions)?;
    append_before_close(&mut package.content_type, b"</Types>", &overrides)?;
    Ok(())
}

/// document.xml.relsの小さい参照表だけを読み、画像payloadには触れない。
fn relationships(input: &[u8]) -> Result<BTreeMap<String, String>, ConvertError> {
    let mut reader = Reader::from_reader(input);
    let mut result = BTreeMap::new();
    loop {
        match reader.read_event().map_err(error)? {
            Event::Empty(ref e) if e.name().as_ref() == b"Relationship" => {
                result.insert(attr(e, b"Id")?, attr(e, b"Target")?);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(result)
}

/// 画像参照のないheader/footerにも有効な空relsを対応させる。
fn empty_relationships() -> Vec<u8> {
    b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".to_vec()
}

/// 小さな追加要素をXML writerでescapeして連結する。
fn empty_bytes(
    output: &mut Vec<u8>,
    name: &str,
    attributes: &[(&str, &str)],
) -> Result<(), ConvertError> {
    let mut writer = Writer::new(Vec::new());
    empty(&mut writer, name, attributes)?;
    output.extend(writer.into_inner());
    Ok(())
}

/// 自前builderの最終閉じタグの直前へ追加し、元の属性・namespaceを保持する。
fn append_before_close(
    xml: &mut Vec<u8>,
    close: &[u8],
    addition: &[u8],
) -> Result<(), ConvertError> {
    if addition.is_empty() {
        return Ok(());
    }
    let offset = xml
        .windows(close.len())
        .rposition(|s| s == close)
        .ok_or_else(|| error("missing package part closing tag"))?;
    xml.splice(offset..offset, addition.iter().copied());
    Ok(())
}
