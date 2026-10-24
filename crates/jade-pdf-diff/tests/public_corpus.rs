//! Network-free tests over the separately fetched, SHA-256-pinned corpus.
//! Run `python3 tools/pdf_corpus.py` before `cargo test --test public_corpus -- --ignored`.
#[path = "support/sdk_text_oracle.rs"]
mod sdk_text_oracle;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pdf_diff::{diff_documents, load_document, load_document_from_bytes, DiffOptions, Limits};
use serde::Deserialize;
use unicode_normalization::UnicodeNormalization;

#[derive(Deserialize)]
struct Manifest {
    documents: Vec<Entry>,
    #[serde(default)]
    robustness: Vec<RobustnessEntry>,
}

#[derive(Deserialize)]
struct RobustnessEntry {
    id: String,
    outcome: String,
    pages: Option<usize>,
    invalid_page: Option<usize>,
}

#[derive(Deserialize)]
struct Entry {
    id: String,
    pages: usize,
    text_fragments: Vec<String>,
    full_page_text: Option<String>,
    expected_user_units: Option<Vec<f64>>,
    #[serde(default)]
    reading_order: Vec<ReadingOrder>,
    #[serde(default)]
    right_to_left_spaces: bool,
    word_group_gold: Option<WordGroupGold>,
    actual_text_gold: Option<ActualTextGold>,
    #[serde(default)]
    actual_text_golds: Vec<ActualTextGold>,
    source_replacement_gold: Option<SourceReplacementGold>,
}

#[derive(Deserialize)]
struct SourceReplacementGold {
    page: usize,
    raw_sdk_text: String,
    replacement: String,
    removed_sdk_text: Option<String>,
}

#[derive(Deserialize)]
struct ActualTextGold {
    page: usize,
    expected_periods: usize,
    #[serde(default)]
    recovered_periods: usize,
    raw_sdk_periods: Option<usize>,
}

#[derive(Deserialize)]
struct WordGroupGold {
    page: usize,
    numeric_labels: Option<Vec<String>>,
    expected_blocks: Option<Vec<String>>,
    selected_labels: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct ReadingOrder {
    page: usize,
    anchors: Vec<String>,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn entries() -> Vec<Entry> {
    serde_json::from_slice::<Manifest>(&std::fs::read(root().join("corpus/manifest.json")).unwrap())
        .unwrap()
        .documents
}

#[test]
#[ignore = "requires python3 tools/pdf_corpus.py; no network during cargo test"]
fn public_geometry_errors_are_explicit_and_recoverable() {
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(root().join("corpus/manifest.json")).unwrap())
            .unwrap();
    let limits = Limits::default();
    for entry in manifest.robustness {
        let path = root().join(format!("corpus/cache/{}.pdf", entry.id));
        let bytes = std::fs::read(&path).unwrap();
        match entry.outcome.as_str() {
            "geometry_rejected" | "password_required" => {
                assert!(load_document(&path, &limits).is_err(), "{}", entry.id);
                assert!(
                    load_document_from_bytes(&bytes, &limits).is_err(),
                    "{}",
                    entry.id
                );
                assert!(
                    pdf_diff::render_page_from_bytes(&bytes, entry.invalid_page.unwrap(), 400)
                        .is_err()
                );
                let output = std::process::Command::new(env!("CARGO_BIN_EXE_pdf-diff"))
                    .arg(&path)
                    .arg(&path)
                    .output()
                    .unwrap();
                assert_eq!(output.status.code(), Some(2));
                assert!(
                    output.stdout.is_empty(),
                    "A failed document emitted a partial diff"
                );
                if entry.outcome == "password_required" {
                    let error = load_document_from_bytes(&bytes, &limits).unwrap_err();
                    assert!(
                        error.to_string().contains("password"),
                        "{}: {error:#}",
                        entry.id
                    );
                    assert!(String::from_utf8_lossy(&output.stderr).contains("password"));
                    assert!(pdf_diff::render_page(&path, 1, 400).is_err());
                    assert!(pdf_diff::render_page_at_dpi(&path, 1, 72).is_err());
                    assert!(pdf_diff::render_page_at_dpi_from_bytes(&bytes, 1, 72).is_err());
                    assert!(pdf_diff::text_layer(&path, 1).is_err());
                    assert!(pdf_diff::text_layer_from_bytes(&bytes, 1).is_err());
                }
            }
            "accepted" => {
                let ir = load_document_from_bytes(&bytes, &limits).unwrap();
                assert_eq!(ir.pages.len(), entry.pages.unwrap());
                for page in &ir.pages {
                    assert!(pdf_diff::render_page_from_bytes(&bytes, page.index, 400).is_ok());
                }
            }
            other => panic!("Unknown robustness outcome: {other}"),
        }
    }
    // A rejected page must not poison the engine for the next valid document.
    let valid = std::fs::read(root().join("corpus/cache/hello.pdf")).unwrap();
    assert_eq!(
        load_document_from_bytes(&valid, &limits)
            .unwrap()
            .pages
            .len(),
        1
    );
}

fn canonical(text: &str) -> String {
    text.replace('\u{ad}', "")
        .replace('ﬀ', "ff")
        .replace('ﬁ', "fi")
        .replace('ﬂ', "fl")
        .replace('ﬃ', "ffi")
        .replace('ﬄ', "ffl")
        .replace(['ﬅ', 'ﬆ'], "st")
        .replace('ſ', "s")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .nfc()
        .collect()
}

#[test]
#[ignore = "requires python3 tools/pdf_corpus.py; no network during cargo test"]
fn public_pdfs_preserve_geometry_text_and_native_memory_parity() {
    let limits = Limits::default();
    for entry in entries() {
        let path = root().join(format!("corpus/cache/{}.pdf", entry.id));
        let bytes = std::fs::read(&path).expect("fetch corpus with tools/pdf_corpus.py first");
        let native = load_document(&path, &limits).unwrap();
        let memory = load_document_from_bytes(&bytes, &limits).unwrap();
        assert_eq!(native.pages.len(), entry.pages, "{}", entry.id);
        native.validate(&limits).unwrap();
        if let Some(expected) = &entry.expected_user_units {
            assert_eq!(
                native.pages.iter().map(|p| p.user_unit).collect::<Vec<_>>(),
                expected.iter().map(|&u| Some(u)).collect::<Vec<_>>()
            );
            let image = pdf_diff::render_page_at_dpi(&path, 1, 72).unwrap();
            assert_eq!((image.width, image.height), (612, 792));
            assert_eq!(native.pages[0].physical_size_points(), Some((612.0, 792.0)));
        }
        let all_text: String = native
            .pages
            .iter()
            .map(|p| p.text())
            .collect::<Vec<_>>()
            .join("");
        let compact: String = all_text.chars().filter(|c| !c.is_whitespace()).collect();
        if let Some(expected) = &entry.full_page_text {
            assert_eq!(native.pages.len(), 1, "Full-page gold expects one page");
            assert_eq!(
                &native.pages[0].text(),
                expected,
                "{} full-page Unicode gold",
                entry.id
            );
        }
        for gold in &entry.reading_order {
            // Match the geometric anchor contract used by the CLI audit and
            // browser: different-size inline runs can have separator newlines.
            // Literal full-page Unicode and scalar conservation are checked
            // separately; these anchors do not establish word spacing.
            let ordered = |text: &str| {
                canonical(text)
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>()
            };
            let text = ordered(&native.pages[gold.page - 1].text());
            let mut offset = 0;
            for anchor in &gold.anchors {
                let anchor = ordered(anchor);
                let position = text[offset..].find(&anchor).unwrap_or_else(|| {
                    panic!(
                        "{} page {}: reading-order anchor {anchor:?} is missing or out of order",
                        entry.id, gold.page
                    )
                });
                offset += position + anchor.len();
            }
        }
        for gold in entry
            .actual_text_gold
            .iter()
            .chain(&entry.actual_text_golds)
        {
            assert_eq!(
                native.pages[gold.page - 1]
                    .text()
                    .chars()
                    .filter(|c| *c == '.')
                    .count(),
                gold.expected_periods,
                "{} ActualText periods",
                entry.id
            );
        }
        if let Some(gold) = &entry.word_group_gold {
            let page = &native.pages[gold.page - 1];
            let mut actual = page
                .blocks
                .iter()
                .map(|b| b.text.clone())
                .collect::<Vec<_>>();
            if let Some(expected) = &gold.numeric_labels {
                actual.retain(|s| s.chars().all(|c| c.is_ascii_digit() || c == ' '));
                let mut expected = expected.clone();
                expected.sort();
                actual.sort();
                assert_eq!(actual, expected, "{} complete numeric labels", entry.id);
            } else if let Some(expected) = &gold.expected_blocks {
                let mut expected = expected.clone();
                expected.sort();
                actual.sort();
                assert_eq!(actual, expected, "{} source word groups", entry.id);
            } else if let Some(expected) = &gold.selected_labels {
                assert!(
                    expected.iter().all(|s| actual.contains(s)),
                    "{} selected source labels",
                    entry.id
                );
            }
        }
        for fragment in &entry.text_fragments {
            let fragment: String = fragment.chars().filter(|c| !c.is_whitespace()).collect();
            assert!(
                compact.contains(&fragment),
                "{}: missing text {fragment:?}",
                entry.id
            );
        }
        assert_eq!(
            serde_json::to_value(&native).unwrap(),
            serde_json::to_value(&memory).unwrap(),
            "{}",
            entry.id
        );
        let diff = diff_documents("a", "a", &native, &memory, &DiffOptions::default());
        assert!(!diff.has_changes(), "{}: {:?}", entry.id, diff.summary);

        for page in &native.pages {
            let result = pdf_diff::text_layer_from_bytes(&bytes, page.index);
            if page.blocks.is_empty() && page.sdk_text_objects_without_characters > 0 {
                assert!(result
                    .unwrap_err()
                    .to_string()
                    .contains("no character records"));
                continue;
            }
            let runs = result.unwrap();
            assert!(runs.iter().all(|r| r.sdk_text_objects_without_characters
                == page.sdk_text_objects_without_characters));
            if entry.right_to_left_spaces {
                for run in &runs {
                    for (i, c) in run.text.chars().enumerate() {
                        if c.is_whitespace() && i > 0 {
                            assert_eq!(run.chars[i].x0, run.chars[i].x1);
                            assert!((run.chars[i].x0-run.chars[i-1].x0).abs()<0.001,
                                "RTL word-space anchor must follow the left edge of the preceding glyph");
                        }
                    }
                }
            }
            assert_eq!(
                runs.iter()
                    .map(|run| run.sdk_unicode_mapping_errors)
                    .sum::<usize>(),
                page.sdk_unicode_mapping_errors,
                "{} page {} text layer lost Unicode diagnostics",
                entry.id,
                page.index
            );
            let text: Vec<_> = runs.iter().map(|r| canonical(&r.text)).collect();
            let expected: Vec<_> = page.blocks.iter().map(|b| b.text.clone()).collect();
            assert_eq!(
                text, expected,
                "{} page {} text layer must contain exactly the extracted text",
                entry.id, page.index
            );
            for run in &runs {
                assert!(run.chars.is_empty() || run.chars.len() == run.text.chars().count());
                for span in &run.chars {
                    assert!(span.x0.is_finite() && span.x1.is_finite() && span.x0 <= span.x1);
                }
            }
        }
        eprintln!("{}: {} pages verified", entry.id, entry.pages);
    }
}

#[test]
#[ignore = "requires python3 tools/pdf_corpus.py; no network during cargo test"]
fn cli_uses_identical_source_proof_even_for_textless_or_mixed_documents() {
    for id in ["empty", "vertical", "freeculture"] {
        let path = root().join(format!("corpus/cache/{id}.pdf"));
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_pdf-diff"))
            .arg(&path)
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(0), "{id}");
        let diff: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert!(
            diff["diagnostics"]["old_pages_without_text"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert_eq!(diff["comparison"]["identical_source"], true);
    }
}

fn character_bag(text: &str) -> BTreeMap<char, usize> {
    let mut bag = BTreeMap::new();
    for c in canonical(text)
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
    {
        *bag.entry(c).or_default() += 1;
    }
    bag
}

#[test]
#[ignore = "requires python3 tools/pdf_corpus.py; character conservation, not independent Unicode ground truth"]
fn public_text_matches_sdk_and_independently_declared_source_gold() {
    let mut characters = 0usize;
    for entry in entries() {
        let path = root().join(format!("corpus/cache/{}.pdf", entry.id));
        let ir = load_document(&path, &Limits::default()).unwrap();
        // The public reader has dropped its Pdfium handle before this one is acquired.
        let pdfium = pdfium_render::prelude::Pdfium::bind_to_library(
            pdf_diff::pdfium_library_path().unwrap(),
        )
        .map(pdfium_render::prelude::Pdfium::new)
        .unwrap();
        let oracle = sdk_text_oracle::pages(&pdfium, &std::fs::read(&path).unwrap()).unwrap();
        for (i, page) in ir.pages.iter().enumerate() {
            let expected = &oracle[i].0;
            assert_eq!(
                page.sdk_unicode_mapping_errors, oracle[i].1,
                "SDK mapping diagnostic count differs"
            );
            let actual: String = page.blocks.iter().map(|b| b.text.as_str()).collect();
            let mut expected = character_bag(expected);
            if let Some(gold) = entry
                .source_replacement_gold
                .as_ref()
                .filter(|g| g.page == i + 1)
            {
                assert_eq!(
                    expected,
                    character_bag(&gold.raw_sdk_text),
                    "{} original SDK inventory",
                    entry.id
                );
                if let Some(removed) = &gold.removed_sdk_text {
                    for (c, n) in character_bag(removed) {
                        let available = expected
                            .get_mut(&c)
                            .expect("Source correction is absent from raw SDK gold");
                        assert!(*available >= n);
                        *available -= n;
                    }
                    expected.retain(|_, n| *n > 0);
                }
                for (c, n) in character_bag(&gold.replacement) {
                    *expected.entry(c).or_default() += n;
                }
            }
            for gold in entry
                .actual_text_gold
                .iter()
                .chain(&entry.actual_text_golds)
                .filter(|g| g.page == i + 1 && g.recovered_periods > 0)
            {
                let raw = gold
                    .raw_sdk_periods
                    .expect("Recovered period gold needs an independent SDK count");
                assert_eq!(*expected.get(&'.').unwrap_or(&0), raw);
                assert_eq!(raw + gold.recovered_periods, gold.expected_periods);
                *expected.entry('.').or_default() += gold.recovered_periods;
            }
            characters += expected.values().sum::<usize>();
            assert_eq!(
                expected,
                character_bag(&actual),
                "{} page {} lost or duplicated characters",
                entry.id,
                i + 1
            );
        }
    }
    eprintln!("{characters} semantic characters checked against the SDK and independently declared source gold");
}

#[test]
#[ignore = "requires python3 tools/pdf_corpus.py; verifies SDK mapping diagnostics on pinned sources"]
fn public_sdk_mapping_errors_require_review_after_a_harmless_rewrite() {
    for id in ["cjk", "tracemonkey"] {
        let source = root().join(format!("corpus/cache/{id}.pdf"));
        let bytes = std::fs::read(&source).unwrap();
        let mut rewritten = bytes.clone();
        rewritten.extend_from_slice(b"\n% Jade representation-only rewrite\n");
        let diff = pdf_diff::diff_bytes(
            id,
            id,
            &bytes,
            &rewritten,
            &Limits::default(),
            &DiffOptions::default(),
        )
        .unwrap();
        assert!(!diff.has_changes(), "{id}");
        assert!(diff.diagnostics.has_unmapped_text(), "{id}");
        assert!(!diff.is_text_comparison_complete(), "{id}");
        assert!(!diff.is_comparison_complete(), "{id}");
        assert!(
            pdf_diff::text_layer_from_bytes(&bytes, 1)
                .unwrap()
                .iter()
                .any(|run| run.sdk_unicode_mapping_errors > 0),
            "{id}"
        );
    }
}
