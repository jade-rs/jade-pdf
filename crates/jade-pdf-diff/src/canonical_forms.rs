//! Logical AcroForm fields, independent of whether a widget is placed on a page.
//! Traversal/output budgets do not isolate every internal parser allocation.
use anyhow::{bail, ensure, Context, Result};
use lopdf::{Dictionary, Document, LoadOptions, Object, ObjectId};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

use crate::{Limits, Status};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalFormField {
    /// Components preserve the distinction between partial and qualified names.
    pub name: Vec<String>,
    pub properties: BTreeMap<String, Value>,
    /// Unreadable properties are not interpreted as missing or deleted values.
    pub unavailable_properties: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub(crate) struct FormSnapshot {
    pub fields: Vec<CanonicalFormField>,
    pub review_reasons: Vec<String>,
    /// Strictly bounded empty-shell proof, never inferred from zero outputs.
    pub empty_shell: bool,
    /// Independent catalog record, including records the SDK may not expose.
    pub form_present: bool,
    /// Recovered orphan records do not prove absence of other fields.
    pub inventory_complete: bool,
}

impl Default for FormSnapshot {
    fn default() -> Self {
        Self {
            fields: Vec::new(),
            review_reasons: Vec::new(),
            empty_shell: false,
            form_present: false,
            inventory_complete: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalFormChange {
    pub status: Status,
    pub old: Option<CanonicalFormField>,
    pub new: Option<CanonicalFormField>,
}

const PROPERTIES: &[&[u8]] = &[
    b"FT", b"Ff", b"V", b"DV", b"Opt", b"I", b"TU", b"TM", b"MaxLen", b"Q", b"DA", b"A", b"AA",
];
// MaxLen is inheritable for text fields (ISO 32000-1, Table 229).
const INHERITED: &[&[u8]] = &[b"FT", b"Ff", b"V", b"DV", b"DA", b"Q", b"MaxLen"];

struct Walker<'a> {
    doc: &'a Document,
    steps: usize,
    bytes: usize,
    budget: usize,
    seen: HashSet<ObjectId>,
    nodes: HashSet<usize>,
    fields: Vec<CanonicalFormField>,
    exhausted: bool,
}

impl<'a> Walker<'a> {
    fn charge(&mut self, bytes: usize) -> Result<()> {
        self.steps += 1;
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .context("Canonical form size overflow")?;
        self.exhausted |= self.steps > 200_000 || self.bytes > self.budget;
        ensure!(
            self.steps <= 200_000 && self.bytes <= self.budget,
            "Canonical forms exceed traversal/output budget"
        );
        Ok(())
    }

    fn resolve(&mut self, mut object: &'a Object) -> Result<&'a Object> {
        let mut visited = HashSet::new();
        while let Object::Reference(id) = object {
            self.charge(0)?;
            ensure!(
                visited.len() < 64 && visited.insert(*id),
                "Cyclic or over-deep form reference"
            );
            object = match self.doc.objects.get(id) {
                Some(object) => object,
                None => return Ok(&Object::Null), // ISO 32000-1, 7.3.9.
            };
        }
        Ok(object)
    }

    /// Dictionary null entries are semantically absent. Null array items
    /// remain values and are preserved by value(), rather than being dropped.
    fn entry(&mut self, dictionary: &'a Dictionary, key: &[u8]) -> Result<Option<&'a Object>> {
        let Ok(value) = dictionary.get(key) else {
            return Ok(None);
        };
        let value = self.resolve(value)?;
        Ok((!matches!(value, Object::Null)).then_some(value))
    }

    /// Empty Fields alone cannot exclude orphaned widgets, unplaced fields or
    /// actions. Scan every parsed object, including unused and direct nested
    /// dictionaries; references are covered by the outer object inventory.
    /// The existing latched traversal/output budget also bounds this proof.
    fn empty_shell(&mut self, catalog: &'a Dictionary, form: &'a Dictionary) -> Result<bool> {
        for key in [b"AA".as_slice(), b"OpenAction", b"Names", b"Perms"] {
            if self.entry(catalog, key)?.is_some() {
                return Ok(false);
            }
        }
        if self.entry(&self.doc.trailer, b"Encrypt")?.is_some() {
            return Ok(false);
        }
        for (key, value) in form {
            self.charge(key.len())?;
            let value = self.resolve(value)?;
            if matches!(value, Object::Null) {
                continue;
            }
            match key.as_slice() {
                b"Fields" => {}
                b"DA" => {
                    self.resolve(value)?.as_str()?;
                }
                b"DR" => {
                    self.resolve(value)?.as_dict()?;
                }
                b"NeedAppearances" if !self.resolve(value)?.as_bool()? => {}
                _ => return Ok(false),
            }
        }
        // /T is also a structure-element title. Identify actual elements by
        // following the catalog's structure root, not a guessed dictionary shape.
        let mut structure = HashSet::new();
        if let Some(root) = self.entry(catalog, b"StructTreeRoot")? {
            let root = root.as_dict()?;
            let mut stack = Vec::new();
            if let Ok(k) = root.get(b"K") {
                stack.push((k, 0usize));
            }
            while let Some((object, depth)) = stack.pop() {
                self.charge(0)?;
                ensure!(depth < 64, "Empty form structure proof nesting exceeds 64");
                match self.resolve(object)? {
                    Object::Array(values) => {
                        self.charge(values.len())?;
                        ensure!(
                            values.len()
                                <= 200_000usize
                                    .saturating_sub(self.steps)
                                    .saturating_sub(stack.len()),
                            "Empty form array exceeds remaining traversal budget"
                        );
                        stack.extend(values.iter().map(|v| (v, depth + 1)));
                    }
                    Object::Dictionary(d) if d.has(b"S") => {
                        self.resolve(d.get(b"S")?)?.as_name()?;
                        ensure!(
                            structure.insert(d as *const Dictionary as usize),
                            "Repeated structure element in empty form proof"
                        );
                        if let Ok(k) = d.get(b"K") {
                            stack.push((k, depth + 1));
                        }
                    }
                    Object::Dictionary(_) | Object::Integer(_) => {}
                    _ => return Ok(false),
                }
            }
        }
        let doc = self.doc;
        for object in doc.objects.values() {
            let mut stack = vec![(object, 0usize)];
            while let Some((object, depth)) = stack.pop() {
                self.charge(0)?;
                ensure!(depth < 64, "Empty form proof nesting exceeds 64");
                let dictionary = match object {
                    Object::Dictionary(d) => Some(d),
                    Object::Stream(stream) => Some(&stream.dict),
                    Object::Array(values) => {
                        self.charge(values.len())?;
                        ensure!(
                            values.len()
                                <= 200_000usize
                                    .saturating_sub(self.steps)
                                    .saturating_sub(stack.len()),
                            "Empty form array exceeds remaining traversal budget"
                        );
                        stack.extend(values.iter().map(|v| (v, depth + 1)));
                        None
                    }
                    _ => None,
                };
                if let Some(d) = dictionary {
                    self.charge(d.len())?;
                    // A field can be malformed or unreferenced by Fields. A
                    // bare partial name is not accepted as proof of no fields.
                    for key in [
                        b"FT".as_slice(),
                        b"V",
                        b"DV",
                        b"Ff",
                        b"Opt",
                        b"MaxLen",
                        b"SV",
                        b"Lock",
                        b"JS",
                        b"AA",
                        b"XFA",
                    ] {
                        if self.entry(d, key)?.is_some() {
                            return Ok(false);
                        }
                    }
                    let is_structure = structure.contains(&(d as *const Dictionary as usize));
                    if let Some(title) = self.entry(d, b"T")? {
                        let linearization_offset =
                            d.has(b"Linearized") && self.resolve(title)?.as_i64().is_ok();
                        if !is_structure && !linearization_offset {
                            return Ok(false);
                        }
                    }
                    // /I is also an integer offset in linearization hint streams.
                    if let Some(indices) = self.entry(d, b"I")? {
                        if matches!(self.resolve(indices)?, Object::Array(_)) {
                            return Ok(false);
                        }
                    }
                    if let Some(annotations) = self.entry(d, b"Annots")? {
                        if !self.resolve(annotations)?.as_array()?.is_empty() {
                            return Ok(false);
                        }
                    }
                    if let Some(subtype) = self.entry(d, b"Subtype")? {
                        if self.resolve(subtype)?.as_name()? == b"Widget" {
                            return Ok(false);
                        }
                    }
                    if let Some(kind) = self.entry(d, b"Type")? {
                        if matches!(
                            self.resolve(kind)?.as_name()?,
                            b"Sig" | b"DocTimeStamp" | b"Action"
                        ) {
                            return Ok(false);
                        }
                    }
                    // StructElem /A is an attribute, and GoTo merely navigates.
                    // Other action contexts cannot establish an empty-shell proof.
                    if let Some(action) = self.entry(d, b"A")? {
                        if !is_structure {
                            let action = self.resolve(action)?.as_dict()?;
                            if self.resolve(action.get(b"S")?)?.as_name()? != b"GoTo" {
                                return Ok(false);
                            }
                        }
                    }
                    if let Some(action) = self.entry(d, b"S")? {
                        if matches!(
                            self.resolve(action)?.as_name().ok(),
                            Some(
                                b"JavaScript"
                                    | b"SubmitForm"
                                    | b"ResetForm"
                                    | b"ImportData"
                                    | b"Rendition"
                            )
                        ) {
                            return Ok(false);
                        }
                    }
                    for (key, value) in d {
                        self.charge(key.len())?;
                        stack.push((value, depth + 1));
                    }
                }
            }
        }
        Ok(true)
    }

    fn text(&mut self, object: &Object) -> Result<String> {
        let bytes = object.as_str()?;
        ensure!(
            bytes.len() <= 4_000_002,
            "Canonical form string exceeds 2000000 UTF-16 units"
        );
        // JSON can escape one control scalar as six ASCII bytes.
        self.charge(bytes.len().saturating_mul(6))?;
        if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
            ensure!(bytes.len() % 2 == 0, "Odd UTF-16 form string");
            let units: Vec<_> = bytes
                .chunks_exact(2)
                .map(|b| u16::from_be_bytes([b[0], b[1]]))
                .collect();
            return Ok(String::from_utf16(&units)?);
        }
        if let Some(bytes) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
            let value = std::str::from_utf8(bytes)?;
            ensure!(
                value.encode_utf16().count() <= 2_000_000,
                "Canonical form string exceeds 2000000 UTF-16 units"
            );
            return Ok(value.to_owned());
        }
        ensure!(
            bytes.len() <= 2_000_000,
            "Canonical form string exceeds 2000000 UTF-16 units"
        );
        static PDF_DOC: std::sync::OnceLock<[Option<char>; 256]> = std::sync::OnceLock::new();
        let table = PDF_DOC.get_or_init(|| {
            std::array::from_fn(|byte| {
                let object = Object::String(vec![byte as u8], lopdf::StringFormat::Literal);
                let text = lopdf::decode_text_string(&object).ok()?;
                let mut scalars = text.chars();
                let scalar = scalars.next()?;
                scalars.next().is_none().then_some(scalar)
            })
        });
        let mut result = String::new();
        for byte in bytes {
            result.push(table[*byte as usize].context("Undefined PDFDocEncoding form character")?);
        }
        Ok(result)
    }

    fn value(
        &mut self,
        object: &'a Object,
        depth: usize,
        binary_strings: bool,
        text_stream: bool,
    ) -> Result<Value> {
        ensure!(depth < 64, "Canonical form value nesting exceeds 64");
        self.charge(64)?;
        let object = self.resolve(object)?;
        Ok(match object {
            Object::Null => Value::Null,
            Object::Boolean(value) => json!(value),
            Object::Integer(value) => json!(value),
            Object::Real(value) => {
                ensure!(value.is_finite(), "Non-finite canonical field number");
                json!(value)
            }
            Object::Name(value) => {
                self.charge(value.len().saturating_mul(2))?;
                json!({"name_hex":hex(value)})
            }
            Object::String(value, _) if binary_strings => {
                self.charge(value.len().saturating_mul(2))?;
                json!({"binary_hex":hex(value)})
            }
            Object::String(_, _) => json!(self.text(object)?),
            Object::Array(values) => {
                ensure!(
                    values.len() <= 100_000,
                    "Canonical field array is too large"
                );
                let mut result = Vec::new();
                for value in values {
                    result.push(self.value(value, depth + 1, binary_strings, text_stream)?);
                }
                json!(result)
            }
            Object::Dictionary(dictionary) => {
                let mut result = BTreeMap::new();
                for (name, value) in dictionary {
                    self.charge(name.len().saturating_mul(2))?;
                    let value = self.resolve(value)?;
                    if matches!(value, Object::Null) {
                        continue;
                    }
                    result.insert(
                        hex(name),
                        self.value(
                            value,
                            depth + 1,
                            binary_strings,
                            name == b"JS" && !binary_strings,
                        )?,
                    );
                }
                json!({"dictionary_hex_keys":result})
            }
            Object::Stream(stream) if text_stream && !binary_strings => {
                for (key, value) in &stream.dict {
                    self.charge(key.len())?;
                    let value = self.resolve(value)?;
                    ensure!(
                        matches!(value, Object::Null)
                            || matches!(key.as_slice(), b"Length" | b"Filter" | b"DecodeParms"),
                        "Text stream has unsupported dictionary semantics"
                    );
                }
                let raw_filter = stream.dict.get(b"Filter").ok();
                let has_filter = match raw_filter {
                    Some(filter) => !matches!(self.resolve(filter)?, Object::Null),
                    None => false,
                };
                let filters = if has_filter {
                    let filter = raw_filter.unwrap();
                    ensure!(
                        matches!(filter, Object::Name(_))
                            || matches!(filter,Object::Array(values) if values.len()<=16 && values.iter().all(|v|matches!(v,Object::Name(_)))),
                        "Malformed or indirect text stream Filter requires review"
                    );
                    stream.filters()?
                } else {
                    Vec::new()
                };
                ensure!(
                    filters
                        .iter()
                        .all(|filter| matches!(*filter, b"FlateDecode" | b"ASCIIHexDecode")),
                    "Text stream filter has no strict decoder"
                );
                if let Some(parameters) = self.entry(&stream.dict, b"DecodeParms")? {
                    let no_op = |value: &Object| {
                        matches!(value, Object::Null)
                            || matches!(value,Object::Dictionary(d) if d.is_empty())
                    };
                    ensure!(
                        no_op(parameters)
                            || matches!(parameters,Object::Array(values) if values.len()<=16 && values.iter().all(no_op)),
                        "Nontrivial text stream DecodeParms requires review"
                    );
                    if let Object::Array(values) = parameters {
                        ensure!(
                            values.len() == filters.len(),
                            "Text stream DecodeParms/filter counts differ"
                        );
                    }
                }
                // Charge each layer's input and output work. Failed decoding
                // retains its reserved output charge, bounding repeated bombs.
                if filters.is_empty() {
                    ensure!(
                        stream.content.len() <= 8 * 1024 * 1024,
                        "Canonical text stream exceeds 8MiB"
                    );
                }
                self.charge(stream.content.len())?;
                let mut bytes = stream.content.clone();
                for filter in filters {
                    self.charge(bytes.len())?;
                    let limit = self
                        .budget
                        .saturating_sub(self.bytes)
                        .saturating_sub(512)
                        .min(8 * 1024 * 1024);
                    ensure!(
                        limit > 0,
                        "Canonical text stream has no remaining work budget"
                    );
                    self.charge(limit)?;
                    let decoded = strict_text_layer(&bytes, filter, limit)?;
                    self.bytes -= limit - decoded.len();
                    bytes = decoded;
                }
                json!(self.text(&Object::String(bytes, lopdf::StringFormat::Literal))?)
            }
            Object::Stream(_) => bail!("Stream-valued canonical form property requires review"),
            Object::Reference(_) => unreachable!(),
        })
    }

    /// Visit only the actual page tree. Unused page dictionaries are not a
    /// source of logical fields. Traversal shares the canonical work budget.
    fn page_widgets(&mut self) -> Result<Vec<&'a Object>> {
        let Ok(root) = self.doc.catalog()?.get(b"Pages") else {
            return Ok(Vec::new());
        };
        let mut stack = vec![(root, 0usize)];
        let mut pages = HashSet::new();
        let mut widgets = Vec::new();
        let mut seen_widgets = HashSet::new();
        while let Some((object, depth)) = stack.pop() {
            self.charge(0)?;
            ensure!(depth < 64, "Canonical page tree exceeds 64 levels");
            let node = self.resolve(object)?.as_dict()?;
            ensure!(
                pages.insert(node as *const Dictionary as usize),
                "Repeated or cyclic canonical page node"
            );
            match self
                .entry(node, b"Type")?
                .context("Missing page node type")?
                .as_name()?
            {
                b"Pages" => {
                    let kids = self
                        .entry(node, b"Kids")?
                        .context("Missing page children")?
                        .as_array()?;
                    self.charge(kids.len())?;
                    ensure!(
                        kids.len() <= 100_000
                            && kids.len()
                                <= 200_000usize
                                    .saturating_sub(self.steps)
                                    .saturating_sub(stack.len()),
                        "Canonical page children exceed remaining work budget"
                    );
                    stack.extend(kids.iter().rev().map(|v| (v, depth + 1)));
                }
                b"Page" => {
                    if let Some(annotations) = self.entry(node, b"Annots")? {
                        let annotations = annotations.as_array()?;
                        ensure!(
                            annotations.len() <= 100_000,
                            "Canonical annotation array exceeds 100000"
                        );
                        for annotation in annotations {
                            self.charge(0)?;
                            let dictionary = self.resolve(annotation)?.as_dict()?;
                            if self
                                .entry(dictionary, b"Subtype")?
                                .and_then(|o| o.as_name().ok())
                                == Some(b"Widget")
                            {
                                ensure!(
                                    widgets.len() < 100_000,
                                    "Canonical page widgets exceed 100000"
                                );
                                if seen_widgets.insert(dictionary as *const Dictionary as usize) {
                                    widgets.push(annotation);
                                }
                            }
                        }
                    }
                }
                _ => bail!("Unknown canonical page node type"),
            }
        }
        Ok(widgets)
    }

    /// Missing catalog links do not erase known field declarations. Follow
    /// each page widget's Parent chain, then reuse the normal field parser so
    /// inherited values, property availability and all limits stay identical.
    fn recover_widgets(
        &mut self,
        widgets: &[&'a Object],
        defaults: &BTreeMap<&'static [u8], &'a Object>,
    ) -> Result<usize> {
        let mut recovered = 0usize;
        for &widget in widgets {
            let dictionary = self.resolve(widget)?.as_dict()?;
            if self
                .nodes
                .contains(&(dictionary as *const Dictionary as usize))
            {
                continue;
            }
            recovered += 1;
            let mut chain = Vec::new();
            let mut seen = HashSet::new();
            let mut current = widget;
            loop {
                self.charge(0)?;
                let dictionary = self.resolve(current)?.as_dict()?;
                ensure!(
                    chain.len() < 64 && seen.insert(dictionary as *const Dictionary as usize),
                    "Cyclic or over-deep orphan widget Parent chain"
                );
                chain.push(current);
                let Some(parent) = self.entry(dictionary, b"Parent")? else {
                    break;
                };
                current = parent;
            }
            let mut names = Vec::new();
            let mut inherited = defaults.clone();
            for (depth, object) in chain.into_iter().rev().enumerate() {
                let dictionary = self.resolve(object)?.as_dict()?;
                if !self
                    .nodes
                    .contains(&(dictionary as *const Dictionary as usize))
                {
                    self.field(object, &names, &inherited, depth)?;
                }
                if let Some(name) = self.entry(dictionary, b"T")? {
                    names.push(self.text(name)?);
                }
                for key in INHERITED {
                    if let Some(value) = self.entry(dictionary, key)? {
                        inherited.insert(*key, value);
                    }
                }
            }
        }
        Ok(recovered)
    }

    /// A widget under a field may repeat inherited field entries without
    /// introducing another qualified name. Only semantically equal inherited
    /// properties may be omitted. Conflicting or independent properties keep
    /// the normal duplicate-name/review path rather than losing stored values.
    fn repeats_parent(
        &mut self,
        dictionary: &'a Dictionary,
        inherited: &BTreeMap<&'static [u8], &'a Object>,
    ) -> Result<bool> {
        let kind = inherited
            .get(b"FT".as_slice())
            .and_then(|o| self.resolve(o).ok())
            .and_then(|o| o.as_name().ok());
        for key in PROPERTIES {
            let Some(child) = self.entry(dictionary, key)? else {
                continue;
            };
            if !INHERITED.contains(key) {
                return Ok(false);
            }
            let binary = kind == Some(b"Sig") && matches!(*key, b"V" | b"DV");
            let text = kind == Some(b"Tx") && matches!(*key, b"V" | b"DV");
            let parent = match inherited.get(key) {
                Some(parent) => self.value(parent, 0, binary, text)?,
                None if matches!(*key, b"Ff" | b"Q") => json!(0),
                None => return Ok(false),
            };
            if self.value(child, 0, binary, text)? != parent {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn field(
        &mut self,
        object: &'a Object,
        parents: &[String],
        inherited: &BTreeMap<&'static [u8], &'a Object>,
        depth: usize,
    ) -> Result<()> {
        ensure!(depth < 64, "Canonical field tree exceeds 64 levels");
        self.charge(128)?;
        if let Object::Reference(id) = object {
            ensure!(
                self.seen.insert(*id),
                "Repeated or cyclic canonical field node"
            );
        }
        let dictionary = self.resolve(object)?.as_dict()?;
        ensure!(
            self.nodes.insert(dictionary as *const Dictionary as usize),
            "Repeated or aliased canonical field node"
        );
        let widget = self
            .entry(dictionary, b"Subtype")?
            .and_then(|o| o.as_name().ok())
            == Some(b"Widget");
        let named = self.entry(dictionary, b"T")?.is_some();
        if widget
            && !named
            && (self.entry(dictionary, b"FT")?.is_none()
                || (depth > 0 && self.repeats_parent(dictionary, inherited)?))
        {
            return Ok(()); // A control of its parent field, not another field.
        }
        self.charge(parents.iter().map(|p| p.len().saturating_mul(6)).sum())?;
        let mut name = parents.to_vec();
        if let Some(partial) = self.entry(dictionary, b"T")? {
            name.push(self.text(partial)?);
        }
        let mut effective = inherited.clone();
        for key in INHERITED {
            if let Some(value) = self.entry(dictionary, key)? {
                effective.insert(*key, value);
            }
        }
        let mut properties = BTreeMap::new();
        let kind = effective
            .get(b"FT".as_slice())
            .and_then(|o| self.resolve(o).ok())
            .and_then(|o| o.as_name().ok());
        let signature = kind == Some(b"Sig");
        let mut unavailable_properties = BTreeMap::new();
        for key in PROPERTIES {
            let property = if INHERITED.contains(key) {
                effective.get(key).copied()
            } else {
                self.entry(dictionary, key)?
            };
            if let Some(value) = property {
                let name = std::str::from_utf8(key)?.to_owned();
                match self.value(
                    value,
                    0,
                    signature && matches!(*key, b"V" | b"DV"),
                    kind == Some(b"Tx") && matches!(*key, b"V" | b"DV"),
                ) {
                    Ok(value) => {
                        properties.insert(name, value);
                    }
                    Err(error) => {
                        if self.exhausted {
                            return Err(error);
                        }
                        let reason = format!("{error:#}").chars().take(240).collect::<String>();
                        self.charge(name.len() + reason.len() + 64)?;
                        unavailable_properties.insert(name, reason);
                    }
                }
            }
        }
        // Named branch nodes have meaningful defaults and inherited state too.
        if named || !properties.is_empty() || !unavailable_properties.is_empty() {
            self.charge(name.iter().map(|p| p.len().saturating_mul(6)).sum())?;
            for key in ["Ff", "Q"] {
                if !unavailable_properties.contains_key(key) {
                    properties.entry(key.into()).or_insert(json!(0));
                }
            }
            ensure!(
                self.fields.len() < 100_000,
                "Canonical field count exceeds 100000"
            );
            self.fields.push(CanonicalFormField {
                name: name.clone(),
                properties,
                unavailable_properties,
            });
        }
        if let Some(kids) = self.entry(dictionary, b"Kids")? {
            let kids = kids.as_array()?;
            ensure!(
                kids.len() <= 100_000,
                "Canonical child array exceeds 100000"
            );
            for kid in kids {
                self.field(kid, &name, &effective, depth + 1)?;
            }
        }
        Ok(())
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn strict_text_layer(input: &[u8], filter: &[u8], limit: usize) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    match filter {
        b"FlateDecode" => {
            let mut decoder = flate2::Decompress::new(true);
            let mut buffer = [0u8; 8192];
            loop {
                let before_in = decoder.total_in();
                let before_out = decoder.total_out();
                let status = decoder.decompress(
                    &input[before_in as usize..],
                    &mut buffer,
                    flate2::FlushDecompress::None,
                )?;
                let written = (decoder.total_out() - before_out) as usize;
                ensure!(
                    output.len().saturating_add(written) <= limit,
                    "Canonical decoded text exceeds work budget"
                );
                output.extend_from_slice(&buffer[..written]);
                if status == flate2::Status::StreamEnd {
                    ensure!(
                        decoder.total_in() as usize == input.len(),
                        "Trailing bytes after canonical Flate stream"
                    );
                    break;
                }
                ensure!(
                    decoder.total_in() != before_in || written > 0,
                    "Truncated canonical Flate stream"
                );
            }
        }
        b"ASCIIHexDecode" => {
            let mut high = None;
            let mut ended = false;
            for &byte in input {
                if matches!(byte, 0 | 9 | 10 | 12 | 13 | 32) {
                    continue;
                }
                ensure!(!ended, "Trailing data after canonical ASCIIHex stream");
                if byte == b'>' {
                    ended = true;
                    continue;
                }
                let nibble = match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'f' => byte - b'a' + 10,
                    b'A'..=b'F' => byte - b'A' + 10,
                    _ => bail!("Invalid canonical ASCIIHex digit"),
                };
                if let Some(previous) = high.take() {
                    ensure!(
                        output.len() < limit,
                        "Canonical decoded text exceeds work budget"
                    );
                    output.push(previous * 16 + nibble);
                } else {
                    high = Some(nibble);
                }
            }
            ensure!(ended, "Missing canonical ASCIIHex end marker");
            if let Some(high) = high {
                ensure!(
                    output.len() < limit,
                    "Canonical decoded text exceeds work budget"
                );
                output.push(high * 16);
            }
        }
        _ => bail!("Unsupported strict canonical text filter"),
    }
    Ok(output)
}

// ISO 32000-1:2008, 7.3.9: null dictionary values and references to
// nonexistent objects mean the entry is absent. Cycles/deep chains are not
// absence proofs. The caller charges the full walk to its own analysis budget.
fn form_object(doc: &Document, mut charge: impl FnMut() -> Result<()>) -> Result<Option<&Object>> {
    charge()?;
    let Ok(mut object) = doc.catalog()?.get(b"AcroForm") else {
        return Ok(None);
    };
    let mut seen = HashSet::new();
    while let Object::Reference(id) = object {
        charge()?;
        ensure!(
            seen.len() < 64 && seen.insert(*id),
            "Cyclic or over-deep AcroForm reference"
        );
        let Some(next) = doc.objects.get(id) else {
            return Ok(None);
        };
        object = next;
    }
    Ok((!matches!(object, Object::Null)).then_some(object))
}

/// Independent preflight over the already parsed page-unit source. An unknown
/// result requires analysis; SDK form-type detection is not an absence proof.
pub(crate) fn requires_analysis(doc: &Document) -> bool {
    !matches!(form_object(doc, || Ok(())), Ok(None))
}

pub(crate) fn from_bytes(bytes: &[u8], limits: &Limits) -> Result<FormSnapshot> {
    ensure!(
        bytes.len() <= 64 * 1024 * 1024,
        "Canonical form source exceeds 64MiB analysis budget"
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
        "Canonical form parser has too many objects"
    );
    let mut walker = Walker {
        doc: &doc,
        steps: 0,
        bytes: 0,
        budget: usize::try_from(limits.max_input_mb.saturating_mul(1024 * 1024))
            .unwrap_or(usize::MAX)
            .min(64 * 1024 * 1024),
        seen: HashSet::new(),
        nodes: HashSet::new(),
        fields: Vec::new(),
        exhausted: false,
    };
    let catalog = doc.catalog()?;
    let widgets = walker.page_widgets()?;
    let form = form_object(&doc, || walker.charge(0))?
        .map(Object::as_dict)
        .transpose()?;
    if form.is_none() && widgets.is_empty() {
        return Ok(FormSnapshot::default());
    }
    let mut review_reasons = Vec::new();
    if let Some(form) = form {
        if walker.entry(form, b"XFA")?.is_some() {
            review_reasons.push(
                "XFA semantics remain unverified; available AcroForm records are compared"
                    .to_owned(),
            );
        }
    }
    let root_fields = match form {
        Some(form) => walker.entry(form, b"Fields")?,
        None => None,
    };
    let mut inherited = BTreeMap::new();
    if let Some(form) = form {
        for key in [b"DA".as_slice(), b"Q".as_slice()] {
            if let Some(value) = walker.entry(form, key)? {
                inherited.insert(key, value);
            }
        }
    }
    let fields = match root_fields {
        Some(fields) => fields.as_array()?.as_slice(),
        None => {
            ensure!(
                !widgets.is_empty() || !review_reasons.is_empty(),
                "AcroForm Fields entry is missing"
            );
            review_reasons.push("Catalog field inventory is missing; page widget records are compared without assuming a complete inventory".to_owned());
            &[]
        }
    };
    ensure!(
        fields.len() <= 100_000,
        "Canonical root field array exceeds 100000"
    );
    for field in fields {
        walker.field(field, &[], &inherited, 0)?;
    }
    let recovered = walker.recover_widgets(&widgets, &inherited)?;
    if recovered > 0 {
        review_reasons.push("Page widgets outside the catalog field tree were recovered; field inventory and interactive semantics remain unverified".to_owned());
    }
    walker.fields.sort_by(|a, b| a.name.cmp(&b.name));
    ensure!(
        walker.fields.windows(2).all(|w| w[0].name != w[1].name),
        "Duplicate canonical field names require review"
    );
    let empty_shell = fields.is_empty()
        && review_reasons.is_empty()
        && match form {
            Some(form) => walker.empty_shell(catalog, form)?,
            None => false,
        };
    if fields.is_empty() && review_reasons.is_empty() && !empty_shell {
        review_reasons.push(
            "Empty AcroForm has unverified field, annotation, action or dictionary semantics"
                .to_owned(),
        );
    }
    Ok(FormSnapshot {
        fields: walker.fields,
        review_reasons,
        empty_shell,
        form_present: true,
        inventory_complete: root_fields.is_some() && recovered == 0,
    })
}

pub(crate) fn pair(
    old: Vec<CanonicalFormField>,
    new: Vec<CanonicalFormField>,
    old_inventory_complete: bool,
    new_inventory_complete: bool,
) -> Vec<CanonicalFormChange> {
    let mut old: BTreeMap<_, _> = old.into_iter().map(|f| (f.name.clone(), f)).collect();
    let mut result = Vec::new();
    for new in new {
        match old.remove(&new.name) {
            Some(previous) if comparable_properties(&previous, &new) => {}
            Some(previous) => result.push(CanonicalFormChange {
                status: Status::Modified,
                old: Some(previous),
                new: Some(new),
            }),
            None if old_inventory_complete => result.push(CanonicalFormChange {
                status: Status::Added,
                old: None,
                new: Some(new),
            }),
            None => {}
        }
    }
    result.extend(
        old.into_values()
            .filter(|_| new_inventory_complete)
            .map(|old| CanonicalFormChange {
                status: Status::Removed,
                old: Some(old),
                new: None,
            }),
    );
    result
}

fn comparable_properties(old: &CanonicalFormField, new: &CanonicalFormField) -> bool {
    let available = |key: &str| {
        !old.unavailable_properties.contains_key(key)
            && !new.unavailable_properties.contains_key(key)
    };
    old.name == new.name
        && old
            .properties
            .iter()
            .filter(|(key, _)| available(key))
            .eq(new.properties.iter().filter(|(key, _)| available(key)))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn from_file(path: &std::path::Path, limits: &Limits) -> Result<FormSnapshot> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    ensure!(
        file.metadata()?.len() <= 64 * 1024 * 1024,
        "Canonical form source exceeds 64MiB analysis budget"
    );
    let mut bytes = Vec::new();
    file.take(64 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    from_bytes(&bytes, limits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    fn tree(cycle: bool) -> Vec<u8> {
        tree_with_value(cycle, "inherited")
    }

    fn tree_with_value(cycle: bool, value: &str) -> Vec<u8> {
        let mut doc = Document::with_version("1.7");
        let field = doc.new_object_id();
        let child = doc.new_object_id();
        doc.objects.insert(field,Object::Dictionary(dictionary! {"T"=>lopdf::text_string("Parent"),"FT"=>Object::Name(b"Tx".to_vec()),"V"=>lopdf::text_string(value),"Ff"=>1,"Kids"=>vec![Object::Reference(child)]}));
        let mut kid = dictionary! {"T"=>lopdf::text_string("Child")};
        if cycle {
            kid.set("Kids", vec![Object::Reference(field)]);
        }
        doc.objects.insert(child, Object::Dictionary(kid));
        let form = doc.add_object(dictionary! {"Fields"=>vec![Object::Reference(field)]});
        let root =
            doc.add_object(dictionary! {"Type"=>"Catalog","AcroForm"=>Object::Reference(form)});
        doc.trailer.set("Root", root);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn page_side_recovery_bounds_parent_depth_and_annotation_expansion() {
        for depth in [1usize, 64, 65] {
            let mut doc = Document::with_version("1.7");
            let mut parent = None;
            for index in 0..depth {
                let mut field = dictionary! {
                    "FT" => "Tx", "T" => lopdf::text_string(&format!("Level{index}")),
                    "V" => lopdf::text_string("known")
                };
                if let Some(parent) = parent {
                    field.set("Parent", Object::Reference(parent));
                }
                if index == depth - 1 {
                    field.set("Subtype", "Widget");
                }
                parent = Some(doc.add_object(field));
            }
            let page = doc.add_object(
                dictionary! {"Type"=>"Page","Annots"=>vec![Object::Reference(parent.unwrap())]},
            );
            let pages = doc.add_object(
                dictionary! {"Type"=>"Pages","Kids"=>vec![Object::Reference(page)],"Count"=>1},
            );
            let root = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
            doc.trailer.set("Root", root);
            let mut bytes = Vec::new();
            doc.save_to(&mut bytes).unwrap();
            let result = from_bytes(&bytes, &Limits::default());
            if depth <= 64 {
                let snapshot = result.unwrap();
                assert_eq!(snapshot.fields.len(), depth);
                assert!(!snapshot.inventory_complete);
                assert!(!snapshot.review_reasons.is_empty());
            } else {
                assert!(result.unwrap_err().to_string().contains("Parent chain"));
            }
            doc.get_object_mut(page)
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("Annots", vec![Object::Reference(parent.unwrap()); 100_001]);
            let mut bytes = Vec::new();
            doc.save_to(&mut bytes).unwrap();
            assert!(from_bytes(&bytes, &Limits::default())
                .unwrap_err()
                .to_string()
                .contains("annotation array"));
        }
    }

    #[test]
    fn empty_shell_proof_stops_before_expanding_overlarge_or_deep_containers() {
        for deep in [false, true] {
            let mut doc = Document::with_version("1.7");
            let form = doc.add_object(dictionary! { "Fields" => Vec::<Object>::new() });
            let mut value = Object::Null;
            if deep {
                for _ in 0..65 {
                    value = Object::Array(vec![value]);
                }
            } else {
                value = Object::Array(vec![Object::Null; 200_001]);
            }
            doc.add_object(value);
            let root = doc.add_object(dictionary! { "Type" => "Catalog", "AcroForm" => form });
            doc.trailer.set("Root", root);
            let mut bytes = Vec::new();
            doc.save_to(&mut bytes).unwrap();
            let error = from_bytes(&bytes, &Limits::default())
                .unwrap_err()
                .to_string();
            assert!(
                error.contains(if deep { "nesting" } else { "budget" }),
                "{error}"
            );
        }
    }

    #[test]
    fn optional_form_reference_bounds_do_not_turn_cycles_into_absence() {
        for depth in [0, 1, 64, 65] {
            let mut doc = Document::with_version("1.7");
            let mut value = Object::Null;
            for _ in 0..depth {
                let id = doc.add_object(value);
                value = Object::Reference(id);
            }
            let root = doc.add_object(dictionary! { "Type" => "Catalog", "AcroForm" => value });
            doc.trailer.set("Root", root);
            assert_eq!(requires_analysis(&doc), depth > 64);
            let mut bytes = Vec::new();
            doc.save_to(&mut bytes).unwrap();
            assert_eq!(from_bytes(&bytes, &Limits::default()).is_err(), depth > 64);
        }
        let mut doc = Document::with_version("1.7");
        let loop_id = doc.new_object_id();
        doc.objects.insert(loop_id, Object::Reference(loop_id));
        let root = doc.add_object(dictionary! { "Type" => "Catalog", "AcroForm" => loop_id });
        doc.trailer.set("Root", root);
        assert!(requires_analysis(&doc));
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        assert!(from_bytes(&bytes, &Limits::default())
            .unwrap_err()
            .to_string()
            .contains("Cyclic"));
    }

    #[test]
    fn named_tree_nodes_inherit_type_flags_and_value() {
        let mut doc = Document::load_mem(&tree(false)).unwrap();
        doc.get_object_mut((1, 0))
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("MaxLen", 36);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let fields = from_bytes(&bytes, &Limits::default()).unwrap().fields;
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[1].name, ["Parent", "Child"]);
        assert_eq!(fields[1].properties["V"], "inherited");
        assert_eq!(fields[1].properties["Ff"], 1);
        assert_eq!(fields[1].properties["MaxLen"], 36);
        assert_eq!(fields[1].properties["FT"], json!({"name_hex":"5478"}));
    }

    #[test]
    fn pdf_doc_encoding_preserves_ligature_euro_and_bullet_scalars() {
        let mut doc = Document::load_mem(&tree(false)).unwrap();
        doc.get_object_mut((1, 0))
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set(
                "V",
                Object::String(vec![0x93, 0xa0, 0x80], lopdf::StringFormat::Hexadecimal),
            );
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let fields = from_bytes(&bytes, &Limits::default()).unwrap().fields;
        assert_eq!(fields[0].properties["V"], "ﬁ€•");
    }

    #[test]
    fn cyclic_field_trees_return_an_error_without_partial_snapshots() {
        let error = from_bytes(&tree(true), &Limits::default()).unwrap_err();
        assert!(error.to_string().contains("cyclic"));
    }

    #[test]
    fn traversal_and_output_budget_is_checked() {
        let limits = Limits {
            max_input_mb: 1,
            ..Limits::default()
        };
        let bytes = tree_with_value(false, &"x".repeat(400_000));
        assert!(bytes.len() < 1024 * 1024);
        assert!(from_bytes(&bytes, &limits)
            .unwrap_err()
            .to_string()
            .contains("budget"));
    }

    #[test]
    fn strict_text_streams_reject_corruption_truncation_and_overexpansion() {
        use std::io::Write;
        let text = b"A compressed value".repeat(200);
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&text).unwrap();
        let encoded = encoder.finish().unwrap();
        assert_eq!(
            strict_text_layer(&encoded, b"FlateDecode", text.len()).unwrap(),
            text
        );
        assert!(strict_text_layer(&encoded, b"FlateDecode", text.len() - 1).is_err());
        assert!(
            strict_text_layer(&encoded[..encoded.len() - 1], b"FlateDecode", text.len()).is_err()
        );
        let mut damaged = encoded.clone();
        let last = damaged.len() - 1;
        damaged[last] ^= 1;
        assert!(strict_text_layer(&damaged, b"FlateDecode", text.len()).is_err());
        let mut trailing = encoded;
        trailing.push(0);
        assert!(strict_text_layer(&trailing, b"FlateDecode", text.len()).is_err());
        assert_eq!(
            strict_text_layer(b"41 6c706861>", b"ASCIIHexDecode", 5).unwrap(),
            b"Alpha"
        );
        assert!(strict_text_layer(b"416c706861", b"ASCIIHexDecode", 5).is_err());
        assert!(strict_text_layer(b"416c706861>00", b"ASCIIHexDecode", 5).is_err());
    }

    #[test]
    fn malformed_filters_keep_other_properties_available() {
        use lopdf::{dictionary, Stream};
        let mut doc = Document::load_mem(&tree(false)).unwrap();
        let stream = doc.add_object(Stream::new(dictionary! {"Filter"=>7}, b"Alpha".to_vec()));
        doc.get_object_mut((1, 0))
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("V", Object::Reference(stream));
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let snapshot = from_bytes(&bytes, &Limits::default()).unwrap();
        assert_eq!(snapshot.fields.len(), 2);
        for field in snapshot.fields {
            assert_eq!(field.properties["Ff"], 1);
            assert!(field.unavailable_properties["V"].contains("Filter"));
        }
    }
}
