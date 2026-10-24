//! Physical page units missing from the public Pdfium page-geometry API.
//! Coordinates remain in PDF user space; one unit is UserUnit / 72 inch.
use anyhow::{ensure, Context, Result};
use lopdf::{Document, LoadOptions, Object, ObjectId};
use std::collections::HashSet;

pub(crate) struct PageUnits {
    units: Result<Vec<Result<f64, String>>, String>,
    forms: bool,
    source_text: Vec<Result<crate::source_text::Facts, String>>,
}

impl PageUnits {
    pub fn requires_form_analysis(&self) -> bool {
        self.forms
    }
    pub fn source_text_minimum(&self, index: usize) -> Option<usize> {
        self.source_text
            .get(index)
            .and_then(|v| v.as_ref().ok())
            .map(|facts| facts.minimum_glyphs)
    }
    pub fn source_actual(&self, index: usize) -> Option<&crate::source_text::structure::Plan> {
        self.source_text
            .get(index)
            .and_then(|facts| facts.as_ref().ok())
            .map(|facts| &facts.actual)
    }
    pub fn source_unicode(&self, index: usize) -> Option<&crate::source_text::unicode::Facts> {
        self.source_text
            .get(index)
            .and_then(|facts| facts.as_ref().ok())
            .map(|facts| &facts.unicode)
    }
    pub fn get(&self, index: usize) -> Result<f64> {
        let units = self.units.as_ref().map_err(|e| anyhow::anyhow!("{e}"))?;
        units
            .get(index)
            .context("Missing physical page unit")?
            .as_ref()
            .copied()
            .map_err(|e| anyhow::anyhow!("{e}"))
    }
}

pub(crate) fn validate(unit: f64) -> Result<()> {
    ensure!(
        unit.is_finite() && unit > 0.0 && unit <= 75_000.0,
        "UserUnit must be positive, finite and at most 75000"
    );
    Ok(())
}

pub(crate) fn default_unit() -> Option<f64> {
    Some(1.0)
}

const MAX_BYTES: usize = 64 * 1024 * 1024;

pub(crate) fn from_bytes(bytes: &[u8], page_count: usize) -> PageUnits {
    from_bytes_for_text(bytes, page_count, 0..0)
}

pub(crate) fn from_bytes_for_text(
    bytes: &[u8],
    page_count: usize,
    pages: std::ops::Range<usize>,
) -> PageUnits {
    match read(bytes, page_count, pages) {
        Ok(units) => units,
        Err(e) => PageUnits {
            units: Err(format!("Physical page units unavailable: {e:#}")),
            forms: true,
            source_text: Vec::new(),
        },
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn from_file(path: &std::path::Path, page_count: usize) -> PageUnits {
    from_file_for_text(path, page_count, 0..0)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn from_file_for_text(
    path: &std::path::Path,
    page_count: usize,
    pages: std::ops::Range<usize>,
) -> PageUnits {
    use std::io::Read;
    let read_file = || -> Result<Vec<u8>> {
        let file = std::fs::File::open(path)?;
        ensure!(
            file.metadata()?.len() <= MAX_BYTES as u64,
            "Page-unit source exceeds 64MiB analysis budget"
        );
        let mut bytes = Vec::new();
        file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= MAX_BYTES,
            "Page-unit source exceeds 64MiB analysis budget"
        );
        Ok(bytes)
    };
    match read_file() {
        Ok(bytes) => from_bytes_for_text(&bytes, page_count, pages),
        Err(e) => PageUnits {
            units: Err(format!("Physical page units unavailable: {e:#}")),
            forms: true,
            source_text: Vec::new(),
        },
    }
}

fn read(bytes: &[u8], page_count: usize, pages: std::ops::Range<usize>) -> Result<PageUnits> {
    ensure!(
        bytes.len() <= MAX_BYTES,
        "Page-unit source exceeds 64MiB analysis budget"
    );
    let doc = Document::load_mem_with_options(
        bytes,
        LoadOptions {
            max_decompressed_size: Some(8 * 1024 * 1024),
            ..Default::default()
        },
    )?;
    ensure!(
        doc.objects.len() <= 200_000,
        "Too many objects in page-unit source"
    );
    let mut tree = Tree {
        doc: &doc,
        seen: HashSet::new(),
        steps: 0,
        units: Vec::new(),
        page_count,
        annotations_present: false,
        page_ids: Vec::new(),
    };
    tree.visit(doc.catalog()?.get(b"Pages")?, 0)?;
    ensure!(
        tree.units.len() == page_count,
        "Page-unit tree disagrees with SDK page count"
    );
    let mut source = crate::source_text::Reader::new(&doc, &tree.page_ids);
    let source_text = tree
        .page_ids
        .into_iter()
        .enumerate()
        .map(|(index, id)| {
            if pages.contains(&index) {
                source.page_facts(id).map_err(|e| format!("{e:#}"))
            } else {
                Err("Source text analysis not requested".into())
            }
        })
        .collect();
    Ok(PageUnits {
        units: Ok(tree.units),
        forms: tree.annotations_present || crate::canonical_forms::requires_analysis(&doc),
        source_text,
    })
}

struct Tree<'a> {
    doc: &'a Document,
    seen: HashSet<ObjectId>,
    steps: usize,
    units: Vec<Result<f64, String>>,
    page_count: usize,
    annotations_present: bool,
    page_ids: Vec<ObjectId>,
}

impl<'a> Tree<'a> {
    fn resolve(&mut self, mut object: &'a Object) -> Result<&'a Object> {
        let mut seen = HashSet::new();
        while let Object::Reference(id) = object {
            self.steps += 1;
            ensure!(
                self.steps <= 200_000 && seen.len() < 64 && seen.insert(*id),
                "Cyclic or over-budget page-unit reference"
            );
            object = self
                .doc
                .objects
                .get(id)
                .context("Missing page-unit object")?;
        }
        Ok(object)
    }

    fn visit(&mut self, object: &'a Object, depth: usize) -> Result<()> {
        self.steps += 1;
        ensure!(
            depth < 64 && self.steps <= 200_000,
            "Page-unit tree exceeds traversal budget"
        );
        let id = object
            .as_reference()
            .context("Page-unit tree node is not indirect")?;
        ensure!(
            self.seen.insert(id),
            "Repeated or cyclic page-unit tree node"
        );
        let dictionary = self.resolve(object)?.as_dict()?;
        match dictionary.get(b"Type")?.as_name()? {
            b"Page" => {
                self.page_ids.push(id);
                self.annotations_present |= dictionary.has(b"Annots");
                ensure!(
                    self.units.len() < self.page_count,
                    "Page-unit tree exceeds SDK page count"
                );
                // UserUnit is a page entry, not an inheritable page-tree attribute.
                let unit = match dictionary.get(b"UserUnit") {
                    Ok(value) => self
                        .resolve(value)
                        .and_then(|v| match v {
                            Object::Integer(n) => Ok(*n as f64),
                            Object::Real(n) => Ok(f64::from(*n)),
                            _ => anyhow::bail!("UserUnit is not a number"),
                        })
                        .and_then(|n| {
                            validate(n)?;
                            Ok(n)
                        }),
                    Err(_) => Ok(1.0),
                };
                self.units.push(unit.map_err(|e| {
                    format!("Invalid UserUnit on page {}: {e:#}", self.units.len() + 1)
                }));
            }
            b"Pages" => {
                let count = self.resolve(dictionary.get(b"Count")?)?.as_i64()?;
                ensure!(
                    count >= 0 && count as u64 <= self.page_count as u64,
                    "Invalid page-unit subtree Count"
                );
                let before = self.units.len();
                let kids = self.resolve(dictionary.get(b"Kids")?)?.as_array()?;
                ensure!(kids.len() <= 100_000, "Page-unit child array too large");
                for kid in kids {
                    self.visit(kid, depth + 1)?;
                }
                ensure!(
                    self.units.len() - before == count as usize,
                    "Page-unit subtree Count disagrees with children"
                );
            }
            _ => anyhow::bail!("Invalid page-unit tree type"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    fn source(unit: Option<Object>, parent_unit: Option<Object>) -> Vec<u8> {
        let mut doc = Document::with_version("1.7");
        let root = doc.new_object_id();
        let mut page = dictionary! {"Type"=>"Page", "Parent"=>root, "MediaBox"=>vec![0.into(),0.into(),100.into(),100.into()]};
        if let Some(unit) = unit {
            page.set("UserUnit", unit);
        }
        let page = doc.add_object(page);
        let mut pages = dictionary! {"Type"=>"Pages", "Count"=>1, "Kids"=>vec![page.into()]};
        if let Some(unit) = parent_unit {
            pages.set("UserUnit", unit);
        }
        doc.objects.insert(root, pages.into());
        let catalog = doc.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>root});
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn units_are_page_local_and_fractional_values_are_valid() {
        assert_eq!(
            from_bytes(&source(None, Some(72.into())), 1)
                .get(0)
                .unwrap(),
            1.0
        );
        assert_eq!(
            from_bytes(&source(Some(Object::Real(0.5)), None), 1)
                .get(0)
                .unwrap(),
            0.5
        );
    }

    #[test]
    fn invalid_units_are_unknown_instead_of_defaulted() {
        for value in [
            Object::Integer(0),
            Object::Integer(-1),
            Object::Integer(75_001),
            Object::Name(b"72".to_vec()),
        ] {
            let units = from_bytes(&source(Some(value), None), 1);
            assert!(units
                .get(0)
                .unwrap_err()
                .to_string()
                .contains("Invalid UserUnit"));
        }
    }

    #[test]
    fn tree_count_aliases_cycles_and_reference_errors_are_rejected() {
        let bytes = source(Some(Object::Reference((99, 0))), None);
        assert!(from_bytes(&bytes, 1).get(0).is_err());
        assert!(from_bytes(&source(None, None), 2).get(0).is_err());
        let bytes = source(None, None);
        for kids in [
            vec![Object::Reference((1, 0))],
            vec![Object::Reference((2, 0)), Object::Reference((2, 0))],
        ] {
            let mut doc = Document::load_mem(&bytes).unwrap();
            doc.objects
                .get_mut(&(1, 0))
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("Kids", kids);
            let mut malformed = Vec::new();
            doc.save_to(&mut malformed).unwrap();
            assert!(from_bytes(&malformed, 1).get(0).is_err());
        }
    }
}
