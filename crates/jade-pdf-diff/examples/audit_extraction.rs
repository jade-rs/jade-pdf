//! Compare segmented extraction with Pdfium's complete character stream.
//! This is a conservation check, not an independent Unicode ground truth.

#[path = "../tests/support/sdk_text_oracle.rs"]
mod sdk_text_oracle;

use std::collections::BTreeMap;
use std::path::PathBuf;

use pdf_diff::{load_document, Limits};
use unicode_normalization::UnicodeNormalization;

fn bag(text: &str) -> BTreeMap<char, usize> {
    let mut bag = BTreeMap::new();
    let text = text
        .replace('\u{ad}', "")
        .replace('ﬀ', "ff")
        .replace('ﬁ', "fi")
        .replace('ﬂ', "fl")
        .replace('ﬃ', "ffi")
        .replace('ﬄ', "ffl")
        .replace(['ﬅ', 'ﬆ'], "st")
        .replace('ſ', "s");
    for c in text.nfc().filter(|c| !c.is_whitespace() && !c.is_control()) {
        *bag.entry(c).or_default() += 1;
    }
    bag
}

fn main() -> anyhow::Result<()> {
    let paths: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    for path in paths {
        let ir = load_document(&path, &Limits::default())?;
        // pdfium-render serializes the complete lifetime of each Pdfium handle.
        // Do not hold a second handle while invoking the library's reader.
        let pdfium = pdfium_render::prelude::Pdfium::bind_to_library(
            pdf_diff::pdfium_library_path().unwrap(),
        )
        .map(pdfium_render::prelude::Pdfium::new)?;
        let doc = pdfium.load_pdf_from_file(&path, None)?;
        let oracle = sdk_text_oracle::pages(&pdfium, &std::fs::read(&path)?)?;
        let mut missing = 0usize;
        let mut excess = 0usize;
        let mut total = 0usize;
        let mut legacy_missing = 0usize;
        let mut legacy_excess = 0usize;
        let mut pages = Vec::new();
        for (i, page) in ir.pages.iter().enumerate() {
            let loaded_page = doc.pages().get(i as u16)?;
            let text = loaded_page.text()?;
            let raw = &oracle[i].0;
            anyhow::ensure!(
                page.sdk_unicode_mapping_errors == oracle[i].1,
                "SDK mapping diagnostic transport failed"
            );
            let expected = bag(raw);
            let legacy: String = text
                .segments()
                .iter()
                .map(|segment| segment.text())
                .collect();
            let legacy = bag(&legacy);
            legacy_missing += expected
                .iter()
                .map(|(c, n)| n.saturating_sub(*legacy.get(c).unwrap_or(&0)))
                .sum::<usize>();
            legacy_excess += legacy
                .iter()
                .map(|(c, n)| n.saturating_sub(*expected.get(c).unwrap_or(&0)))
                .sum::<usize>();
            let actual = bag(&page
                .blocks
                .iter()
                .map(|b| b.text.as_str())
                .collect::<String>());
            let lost: usize = expected
                .iter()
                .map(|(c, n)| n.saturating_sub(*actual.get(c).unwrap_or(&0)))
                .sum();
            let extra: usize = actual
                .iter()
                .map(|(c, n)| n.saturating_sub(*expected.get(c).unwrap_or(&0)))
                .sum();
            let count: usize = expected.values().sum();
            missing += lost;
            excess += extra;
            total += count;
            if lost + extra > 0 {
                let missing_chars: BTreeMap<String, usize> = expected
                    .iter()
                    .filter_map(|(c, n)| {
                        let delta = n.saturating_sub(*actual.get(c).unwrap_or(&0));
                        (delta > 0).then(|| (c.to_string(), delta))
                    })
                    .collect();
                let excess_chars: BTreeMap<String, usize> = actual
                    .iter()
                    .filter_map(|(c, n)| {
                        let delta = n.saturating_sub(*expected.get(c).unwrap_or(&0));
                        (delta > 0).then(|| (c.to_string(), delta))
                    })
                    .collect();
                pages.push(serde_json::json!({"page": i+1, "missing":lost,"excess":extra,"characters":count,"missing_chars":missing_chars,"excess_chars":excess_chars}));
            }
        }
        println!(
            "{}",
            serde_json::json!({"file":path,"pages":ir.pages.len(),"characters":total,"missing":missing,"excess":excess,"legacy_rectangle_missing":legacy_missing,"legacy_rectangle_excess":legacy_excess,"affected_pages":pages})
        );
    }
    Ok(())
}
