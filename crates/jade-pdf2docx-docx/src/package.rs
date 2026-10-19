//! DOCXの梱包とPDF subset font埋め込みを担当する。
use crate::ConvertError;
use docx_rs::Docx;
use jade_pdf2docx_parser::ParsedEmbeddedFont;
use std::io::{Cursor, Read, Seek, Write};

/// Docx オブジェクトをPDF埋め込みfontとともにバイト列へ保存する。
pub(super) fn pack_docx_to_bytes(
    docx: Docx,
    embedded_fonts: &[ParsedEmbeddedFont],
) -> Result<Vec<u8>, ConvertError> {
    let package = docx.build();
    // Stored ZIPの主要payloadを先に予約し、巨大画像でのVec倍増時の同時保持を避ける。
    // その他のXMLやZIP header用に余裕を持たせ、必要なら通常どおり拡張する。
    let capacity = package
        .media
        .iter()
        .try_fold(package.document.len(), |size, (_, bytes)| {
            size.checked_add(bytes.len())
        })
        .and_then(|size| size.checked_add(1024 * 1024))
        .ok_or_else(|| ConvertError::Pack("DOCX package size overflow".into()))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|error| ConvertError::Pack(error.to_string()))?;
    let mut cursor = Cursor::new(bytes);
    package
        .pack(&mut cursor)
        .map_err(|err| ConvertError::Pack(err.to_string()))?;
    embed_fonts_in_docx_package(cursor.into_inner(), embedded_fonts)
}

/// DOCX packageへ本文で参照したPDF subset fontをOOXML埋め込みfontとして追加する。
fn embed_fonts_in_docx_package(
    docx_bytes: Vec<u8>,
    embedded_fonts: &[ParsedEmbeddedFont],
) -> Result<Vec<u8>, ConvertError> {
    if embedded_fonts.is_empty() {
        return Ok(docx_bytes);
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(docx_bytes))
        .map_err(|err| ConvertError::Pack(err.to_string()))?;
    let mut output = Cursor::new(Vec::new());
    if embed_fonts_in_archive(&mut archive, embedded_fonts, &mut output)? {
        Ok(output.into_inner())
    } else {
        Ok(archive.into_inner().into_inner())
    }
}

/// 使用fontがある場合だけpackageを書き換え、画像等は固定bufferで転送する。
fn embed_fonts_in_archive<R: Read + Seek, W: Write + Seek>(
    archive: &mut zip::ZipArchive<R>,
    embedded_fonts: &[ParsedEmbeddedFont],
    output: W,
) -> Result<bool, ConvertError> {
    let document_xml = {
        let mut entry = archive
            .by_name("word/document.xml")
            .map_err(|err| ConvertError::Pack(err.to_string()))?;
        let mut xml = String::new();
        entry
            .read_to_string(&mut xml)
            .map_err(|err| ConvertError::Pack(err.to_string()))?;
        xml
    };
    let used_fonts = embedded_fonts
        .iter()
        .filter(|font| document_xml.contains(font.font_name.as_str()))
        .collect::<Vec<_>>();
    if used_fonts.is_empty() {
        return Ok(false);
    }
    {
        let mut writer = zip::ZipWriter::new(output);
        let options = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);
        for index in 0..archive.len() {
            let mut entry = archive
                .by_index(index)
                .map_err(|err| ConvertError::Pack(err.to_string()))?;
            let name = entry.name().to_string();
            if name == "word/_rels/fontTable.xml.rels" || name.starts_with("word/fonts/") {
                continue;
            }
            if entry.is_dir() {
                writer
                    .add_directory(name, options)
                    .map_err(|err| ConvertError::Pack(err.to_string()))?;
                continue;
            }
            let modified_xml = if name == "word/fontTable.xml" || name == "[Content_Types].xml" {
                let mut bytes = Vec::new();
                entry
                    .read_to_end(&mut bytes)
                    .map_err(|err| ConvertError::Pack(err.to_string()))?;
                Some(if name == "word/fontTable.xml" {
                    add_embedded_fonts_to_font_table(&bytes, &used_fonts)?
                } else {
                    add_embedded_font_content_type(&bytes)?
                })
            } else {
                None
            };
            writer
                .start_file(name, options)
                .map_err(|err| ConvertError::Pack(err.to_string()))?;
            if let Some(bytes) = modified_xml {
                writer
                    .write_all(&bytes)
                    .map_err(|err| ConvertError::Pack(err.to_string()))?;
            } else {
                std::io::copy(&mut entry, &mut writer)
                    .map_err(|err| ConvertError::Pack(err.to_string()))?;
            }
        }

        writer
            .add_directory("word/fonts/", options)
            .map_err(|err| ConvertError::Pack(err.to_string()))?;
        writer
            .start_file("word/_rels/fontTable.xml.rels", options)
            .map_err(|err| ConvertError::Pack(err.to_string()))?;
        writer
            .write_all(embedded_font_relationships_xml(used_fonts.len()).as_bytes())
            .map_err(|err| ConvertError::Pack(err.to_string()))?;
        for (index, font) in used_fonts.iter().enumerate() {
            let key = embedded_font_key(font, index);
            let font_bytes = obfuscate_embedded_font(&font.bytes, &key);
            writer
                .start_file(format!("word/fonts/font{}.odttf", index + 1), options)
                .map_err(|err| ConvertError::Pack(err.to_string()))?;
            writer
                .write_all(&font_bytes)
                .map_err(|err| ConvertError::Pack(err.to_string()))?;
        }
        writer
            .finish()
            .map_err(|err| ConvertError::Pack(err.to_string()))?;
    }
    Ok(true)
}

/// fontTable.xmlへsubset font定義と埋め込みrelationship参照を追加する。
fn add_embedded_fonts_to_font_table(
    font_table_xml: &[u8],
    fonts: &[&ParsedEmbeddedFont],
) -> Result<Vec<u8>, ConvertError> {
    let mut xml = String::from_utf8(font_table_xml.to_vec())
        .map_err(|err| ConvertError::Pack(err.to_string()))?;
    let mut definitions = String::new();
    for (index, font) in fonts.iter().enumerate() {
        let key = embedded_font_key(font, index);
        definitions.push_str(&format!(
            r#"<w:font w:name="{}"><w:embedRegular r:id="rIdEmbeddedFont{}" w:fontKey="{}" w:subsetted="true" /></w:font>"#,
            escape_xml_attribute(font.font_name.as_str()),
            index + 1,
            embedded_font_key_guid(&key),
        ));
    }
    let Some(close_pos) = xml.rfind("</w:fonts>") else {
        return Err(ConvertError::Pack(
            "word/fontTable.xml is missing </w:fonts>".to_string(),
        ));
    };
    xml.insert_str(close_pos, definitions.as_str());
    Ok(xml.into_bytes())
}

/// `[Content_Types].xml`へodttfのMIME型を追加する。
fn add_embedded_font_content_type(content_types_xml: &[u8]) -> Result<Vec<u8>, ConvertError> {
    let mut xml = String::from_utf8(content_types_xml.to_vec())
        .map_err(|err| ConvertError::Pack(err.to_string()))?;
    if xml.contains(r#"Extension="odttf""#) {
        return Ok(xml.into_bytes());
    }
    let Some(close_pos) = xml.rfind("</Types>") else {
        return Err(ConvertError::Pack(
            "[Content_Types].xml is missing </Types>".to_string(),
        ));
    };
    xml.insert_str(
        close_pos,
        r#"<Default Extension="odttf" ContentType="application/vnd.openxmlformats-officedocument.obfuscatedFont" />"#,
    );
    Ok(xml.into_bytes())
}

/// fontTable.xml.relsの埋め込みfont relationship群を生成する。
fn embedded_font_relationships_xml(font_count: usize) -> String {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for index in 0..font_count {
        xml.push_str(&format!(
            r#"<Relationship Id="rIdEmbeddedFont{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/font" Target="fonts/font{}.odttf" />"#,
            index + 1,
            index + 1,
        ));
    }
    xml.push_str("</Relationships>");
    xml
}

/// font内容と連番から再現可能な16byteのfont keyを生成する。
fn embedded_font_key(font: &ParsedEmbeddedFont, index: usize) -> [u8; 16] {
    let mut key: [u8; 16] = [
        0x6d, 0x8f, 0x2a, 0x41, 0x73, 0xc5, 0x4e, 0x90, 0xa7, 0x19, 0x5b, 0xe3, 0x0c, 0xd4, 0x86,
        0xf2,
    ];
    for (offset, byte) in font
        .font_name
        .bytes()
        .chain(font.bytes.iter().copied())
        .enumerate()
    {
        let slot = offset % key.len();
        key[slot] = key[slot]
            .rotate_left(((offset + index) % 7 + 1) as u32)
            .wrapping_add(byte)
            ^ (offset as u8).wrapping_mul(31);
    }
    key[6] = (key[6] & 0x0f) | 0x40;
    key[8] = (key[8] & 0x3f) | 0x80;
    key
}

/// 16byte font keyをOOXMLのGUID文字列表現へ変換する。
fn embedded_font_key_guid(key: &[u8; 16]) -> String {
    format!(
        "{{{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        key[0],
        key[1],
        key[2],
        key[3],
        key[4],
        key[5],
        key[6],
        key[7],
        key[8],
        key[9],
        key[10],
        key[11],
        key[12],
        key[13],
        key[14],
        key[15],
    )
}

/// OOXML仕様に従いfont先頭32byteをfont keyの逆順byteでXORする。
fn obfuscate_embedded_font(font_bytes: &[u8], key: &[u8; 16]) -> Vec<u8> {
    let mut bytes = font_bytes.to_vec();
    for index in 0..bytes.len().min(32) {
        bytes[index] ^= key[15 - index % key.len()];
    }
    bytes
}

/// XML属性値として危険な文字をentityへ変換する。
fn escape_xml_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// ファイル変換では梱包済みZIP全体をVecへ保持せず、削除保証付き一時ファイルへ保存する。
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn pack_docx_to_file(
    docx: Docx,
    embedded_fonts: &[ParsedEmbeddedFont],
) -> Result<std::fs::File, ConvertError> {
    use std::io::{BufReader, BufWriter};
    let mut output = BufWriter::new(tempfile::tempfile()?);
    docx.build()
        .pack(&mut output)
        .map_err(|err| ConvertError::Pack(err.to_string()))?;
    let file = output
        .into_inner()
        .map_err(|err| ConvertError::Io(err.into_error()))?;
    let mut file = if embedded_fonts.is_empty() {
        file
    } else {
        let mut archive = zip::ZipArchive::new(BufReader::new(file))
            .map_err(|err| ConvertError::Pack(err.to_string()))?;
        let mut output = BufWriter::new(tempfile::tempfile()?);
        if embed_fonts_in_archive(&mut archive, embedded_fonts, &mut output)? {
            output
                .into_inner()
                .map_err(|err| ConvertError::Io(err.into_error()))?
        } else {
            archive.into_inner().into_inner()
        }
    };
    file.rewind()?;
    Ok(file)
}
