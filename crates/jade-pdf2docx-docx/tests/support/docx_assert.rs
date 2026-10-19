#![allow(dead_code)]

use std::{fs::File, io::Read, path::PathBuf};

pub fn count_paragraph_tags_in_docx(path: &PathBuf) -> usize {
    let doc_xml = read_document_xml(path);
    doc_xml.matches("<w:p").count()
}

/// `<w:tbl>` 要素の開始タグのみを数える（`<w:tblGrid>` / `<w:tblPr>` 等は含めない）。
pub fn count_table_tags_in_docx(path: &PathBuf) -> usize {
    let doc_xml = read_document_xml(path);
    count_w_tbl_element_opens(&doc_xml)
}

/// `<w:tc>` 内に直接または子孫として含まれる `<w:tbl>` 要素数を数える。
pub fn count_nested_table_tags_in_docx(path: &PathBuf) -> usize {
    let doc_xml = read_document_xml(path);
    let mut cursor = 0usize;
    let mut table_cell_depth = 0usize;
    let mut count = 0usize;
    while let Some(tag_start_rel) = doc_xml[cursor..].find('<') {
        let tag_start = cursor + tag_start_rel;
        let Some(tag_end_rel) = doc_xml[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + tag_end_rel;
        let tag = &doc_xml[tag_start + 1..tag_end];
        let tag_name = tag
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_end_matches('/');
        match (tag.starts_with('/'), tag_name) {
            (false, "w:tc") => table_cell_depth += 1,
            (true, "w:tc") => table_cell_depth = table_cell_depth.saturating_sub(1),
            (false, "w:tbl") if table_cell_depth > 0 => count += 1,
            _ => {}
        }
        cursor = tag_end + 1;
    }
    count
}

/// top-level table の同一行内で、複数セルが直接の子表を持つ行数を数える。
pub fn count_top_level_rows_with_nested_tables_in_multiple_cells(path: &PathBuf) -> usize {
    let doc_xml = read_document_xml(path);
    let mut cursor = 0usize;
    let mut table_depth = 0usize;
    let mut top_row_cell_count_with_nested = 0usize;
    let mut top_cell_has_nested_table = false;
    let mut rows = 0usize;
    while let Some(tag_start_rel) = doc_xml[cursor..].find('<') {
        let tag_start = cursor + tag_start_rel;
        let Some(tag_end_rel) = doc_xml[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + tag_end_rel;
        let tag = &doc_xml[tag_start + 1..tag_end];
        let tag_name = tag
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_end_matches('/');
        match (tag.starts_with('/'), tag_name) {
            (false, "w:tbl") => {
                if table_depth == 1 {
                    top_cell_has_nested_table = true;
                }
                table_depth += 1;
            }
            (true, "w:tbl") => table_depth = table_depth.saturating_sub(1),
            (false, "w:tr") if table_depth == 1 => top_row_cell_count_with_nested = 0,
            (true, "w:tr") if table_depth == 1 => {
                if top_row_cell_count_with_nested >= 2 {
                    rows += 1;
                }
            }
            (false, "w:tc") if table_depth == 1 => top_cell_has_nested_table = false,
            (true, "w:tc") if table_depth == 1 => {
                if top_cell_has_nested_table {
                    top_row_cell_count_with_nested += 1;
                }
            }
            _ => {}
        }
        cursor = tag_end + 1;
    }
    rows
}

/// DOCX の `word/document.xml` を文字列として読み込む。
pub fn read_document_xml(path: &PathBuf) -> String {
    let file = File::open(path).expect("failed to open docx");
    let mut zip = zip::ZipArchive::new(file).expect("failed to open docx zip");
    let mut doc_xml = String::new();
    zip.by_name("word/document.xml")
        .expect("missing word/document.xml")
        .read_to_string(&mut doc_xml)
        .expect("failed to read document.xml");
    doc_xml
}

/// `word/_rels/document.xml.rels` を文字列として読み込む。
pub fn read_document_relationships_xml(path: &PathBuf) -> String {
    read_docx_entry(path, "word/_rels/document.xml.rels")
}

/// DOCX zip 内の指定 entry を文字列として読み込む。
pub fn read_docx_entry(path: &PathBuf, entry_name: &str) -> String {
    String::from_utf8(read_docx_entry_bytes(path, entry_name))
        .unwrap_or_else(|_| panic!("{entry_name} must be UTF-8"))
}

/// DOCX zip 内の指定 entry をバイト列として読み込む。
pub fn read_docx_entry_bytes(path: &PathBuf, entry_name: &str) -> Vec<u8> {
    let file = File::open(path).expect("failed to open docx");
    let mut zip = zip::ZipArchive::new(file).expect("failed to open docx zip");
    let mut entry_bytes = Vec::new();
    zip.by_name(entry_name)
        .unwrap_or_else(|_| panic!("missing {entry_name}"))
        .read_to_end(&mut entry_bytes)
        .unwrap_or_else(|_| panic!("failed to read {entry_name}"));
    entry_bytes
}

/// DOCX 内の media ファイル数を数える。
pub fn count_media_files_in_docx(path: &PathBuf) -> usize {
    let file = File::open(path).expect("failed to open docx");
    let mut zip = zip::ZipArchive::new(file).expect("failed to open docx zip");
    (0..zip.len())
        .filter(|index| {
            zip.by_index(*index)
                .map(|file| file.name().starts_with("word/media/") && file.name() != "word/media/")
                .unwrap_or(false)
        })
        .count()
}

/// DOCX 内の先頭 media ファイルをバイト列として読む。
pub fn read_first_media_file_in_docx(path: &PathBuf) -> Vec<u8> {
    let file = File::open(path).expect("failed to open docx");
    let mut zip = zip::ZipArchive::new(file).expect("failed to open docx zip");
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).expect("failed to read zip entry");
        if file.name().starts_with("word/media/") && file.name() != "word/media/" {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)
                .expect("failed to read media file");
            return bytes;
        }
    }
    panic!("missing media file in docx");
}

/// `<w:tc>` 要素の開始タグを数える。
pub fn count_table_cell_tags_in_docx(path: &PathBuf) -> usize {
    let doc_xml = read_document_xml(path);
    doc_xml.matches("<w:tc>").count()
}

/// `<w:gridSpan>` 要素数を数える。
pub fn count_grid_span_tags_in_docx(path: &PathBuf) -> usize {
    let doc_xml = read_document_xml(path);
    doc_xml.matches("<w:gridSpan").count()
}

/// `<w:vMerge>` 要素数を数える。
pub fn count_vertical_merge_tags_in_docx(path: &PathBuf) -> usize {
    let doc_xml = read_document_xml(path);
    doc_xml.matches("<w:vMerge").count()
}

/// DOCX 本文内の page break 数を数える。
pub fn count_page_breaks_in_docx(path: &PathBuf) -> usize {
    let doc_xml = read_document_xml(path);
    doc_xml.matches(r#"<w:br w:type="page""#).count()
}

/// `word/document.xml` 内の `<w:t>` テキストを出現順で取り出す。
pub fn text_nodes_in_docx(path: &PathBuf) -> Vec<String> {
    let doc_xml = read_document_xml(path);
    text_nodes_in_xml(&doc_xml)
}

/// `word/document.xml` 内の段落ごとの text を run 境界をまたいで結合する。
pub fn paragraph_texts_in_docx(path: &PathBuf) -> Vec<String> {
    let doc_xml = read_document_xml(path);
    let mut result = Vec::new();
    let mut cursor = 0usize;
    while let Some(start_rel) = doc_xml[cursor..].find("<w:p") {
        let start = cursor + start_rel;
        let after_name = doc_xml.as_bytes().get(start + "<w:p".len()).copied();
        if !matches!(
            after_name,
            Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r')
        ) {
            cursor = start + "<w:p".len();
            continue;
        }
        let Some(close_rel) = doc_xml[start..].find("</w:p>") else {
            break;
        };
        let end = start + close_rel + "</w:p>".len();
        let text = text_nodes_in_xml(&doc_xml[start..end]).join("");
        if !text.is_empty() {
            result.push(text);
        }
        cursor = end;
    }
    result
}

/// 入れ子の表も除外し、通常本文の text node を出現順で読む。
pub fn text_nodes_outside_tables_in_docx(path: &PathBuf) -> Vec<String> {
    let xml = read_document_xml(path);
    let mut cursor = 0;
    let mut depth = 0usize;
    let mut texts = Vec::new();
    while let Some(start) = xml[cursor..].find('<').map(|offset| cursor + offset) {
        let Some(end) = xml[start..].find('>').map(|offset| start + offset) else {
            break;
        };
        let tag = &xml[start + 1..end];
        let name = tag
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or_default();
        match name {
            "w:tbl" if tag.starts_with('/') => depth = depth.saturating_sub(1),
            "w:tbl" => depth += 1,
            "w:t" if !tag.starts_with('/') && depth == 0 => {
                let text_start = end + 1;
                let text_end =
                    xml[text_start..].find("</w:t>").expect("text must close") + text_start;
                texts.push(unescape_xml_text(&xml[text_start..text_end]));
            }
            _ => {}
        }
        cursor = end + 1;
    }
    texts
}

/// XML 文字列内の `<w:t>` テキストを出現順で取り出す。
fn text_nodes_in_xml(doc_xml: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut cursor = 0usize;
    while let Some(start_rel) = doc_xml[cursor..].find("<w:t") {
        let start = cursor + start_rel;
        let after_name = doc_xml.as_bytes().get(start + "<w:t".len()).copied();
        if !matches!(
            after_name,
            Some(b'>') | Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r')
        ) {
            cursor = start + "<w:t".len();
            continue;
        }
        let Some(open_end_rel) = doc_xml[start..].find('>') else {
            break;
        };
        let text_start = start + open_end_rel + 1;
        let Some(close_rel) = doc_xml[text_start..].find("</w:t>") else {
            break;
        };
        let text_end = text_start + close_rel;
        result.push(unescape_xml_text(&doc_xml[text_start..text_end]));
        cursor = text_end + "</w:t>".len();
    }
    result
}

fn count_w_tbl_element_opens(doc_xml: &str) -> usize {
    const NEEDLE: &str = "<w:tbl";
    doc_xml
        .match_indices(NEEDLE)
        .filter(|(pos, _)| {
            let after = &doc_xml[*pos + NEEDLE.len()..];
            let c = after.chars().next();
            matches!(
                c,
                Some('>') | Some(' ') | Some('\t') | Some('\n') | Some('\r') | Some('/')
            )
        })
        .count()
}

fn unescape_xml_text(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}
