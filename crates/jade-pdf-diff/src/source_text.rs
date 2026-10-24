//! Lower bounds on encoded glyph counts, independent of Pdfium's text page.
//!
//! ISO 32000-1 9.4.3: simple fonts consume one byte per code; Identity CMaps
//! consume two. Other composite CMaps consume at most four, so ceil(bytes/4)
//! is only a lower bound. Counts are not Unicode and cannot prove completeness:
//! ActualText, ligature expansion and unrelated surplus records can mask loss.
use anyhow::{ensure, Context, Result};
use lopdf::{content::Content, Dictionary, Document, Object, ObjectId, Stream};
use std::collections::HashSet;

pub(crate) mod structure;
pub(crate) mod unicode;

pub(crate) struct Facts {
    pub minimum_glyphs: usize,
    pub unicode: unicode::Facts,
    pub actual: structure::Plan,
}

const PAGE_BYTES: usize = 2 * 1024 * 1024;
const DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
const WORK: usize = 2_000_000;
const DEPTH: usize = 32;

pub(crate) struct Reader<'a> {
    doc: &'a Document,
    work: usize,
    bytes: usize,
    unicode: unicode::Reader<'a>,
    structure: structure::Index,
}

#[derive(Clone, Copy, Default)]
struct State {
    width: Option<usize>,
    font: Option<usize>,
}

#[derive(Clone, Copy, Default)]
struct Scope {
    actual: bool,
    empty: bool,
    group: Option<usize>,
    context: usize,
}
struct PageProjection {
    actual: structure::Plan,
    outside: unicode::Facts,
    parts: Vec<unicode::Facts>,
    glyphs: Vec<usize>,
    contexts: Vec<(usize, i32, usize)>,
}
fn merge_unicode(into: &mut unicode::Facts, from: unicode::Facts) {
    into.unavailable |= from.unavailable;
    for (c, count) in from.scalars {
        *into.scalars.entry(c).or_default() += count;
    }
}
impl PageProjection {
    fn new(actual: structure::Plan) -> Self {
        let count = actual.groups.len();
        Self {
            actual,
            outside: unicode::Facts::default(),
            parts: (0..count).map(|_| unicode::Facts::default()).collect(),
            glyphs: vec![0; count],
            contexts: vec![(0, -1, 0)],
        }
    }
    fn context(&mut self, parent: usize, mcid: i32) -> Result<usize> {
        ensure!(
            mcid >= 0 && self.contexts[parent].2 < 64,
            "Invalid or over-nested source MCID context"
        );
        let id = self.contexts.len();
        self.contexts
            .push((parent, mcid, self.contexts[parent].2 + 1));
        Ok(id)
    }
    fn mcids(&self, mut context: usize) -> Vec<i32> {
        let mut result = Vec::with_capacity(self.contexts[context].2);
        while context != 0 {
            let (parent, mcid, _) = self.contexts[context];
            result.push(mcid);
            context = parent;
        }
        result.reverse();
        result
    }
    fn add(&mut self, facts: unicode::Facts, group: Option<usize>) {
        let target = if let Some(g) = group {
            &mut self.parts[g]
        } else {
            &mut self.outside
        };
        merge_unicode(target, facts);
    }
    fn finish(mut self, mut minimum: usize) -> Facts {
        for (index, part) in self.parts.into_iter().enumerate() {
            if self.actual.groups[index].invalid {
                self.actual.unavailable = true;
                merge_unicode(&mut self.outside, part);
            } else {
                minimum = minimum.saturating_sub(self.glyphs[index]);
                for c in self.actual.groups[index].text.chars() {
                    unicode::add_scalar(&mut self.outside.scalars, c);
                }
            }
        }
        self.actual
            .ids
            .retain(|_, group| !self.actual.groups[*group].invalid);
        self.outside.unavailable |= self.actual.unavailable;
        Facts {
            minimum_glyphs: minimum,
            unicode: self.outside,
            actual: self.actual,
        }
    }
}

impl<'a> Reader<'a> {
    pub fn new(doc: &'a Document, pages: &[ObjectId]) -> Self {
        Self {
            doc,
            work: 0,
            bytes: 0,
            unicode: unicode::Reader::new(doc),
            structure: structure::Index::read(doc, pages),
        }
    }

    fn charge(&mut self, amount: usize) -> Result<()> {
        self.work = self.work.saturating_add(amount);
        ensure!(self.work <= WORK, "Source text exceeds work budget");
        Ok(())
    }

    fn resolve<'v>(&mut self, mut value: &'v Object) -> Result<&'v Object>
    where
        'a: 'v,
    {
        let mut seen = HashSet::new();
        while let Object::Reference(id) = value {
            self.charge(1)?;
            ensure!(
                seen.len() < DEPTH && seen.insert(*id),
                "Cyclic source text reference"
            );
            value = self
                .doc
                .objects
                .get(id)
                .context("Missing source text object")?;
        }
        Ok(value)
    }

    #[cfg(test)]
    pub fn page(&mut self, id: ObjectId) -> Result<usize> {
        self.page_facts(id).map(|f| f.minimum_glyphs)
    }

    pub fn page_facts(&mut self, id: ObjectId) -> Result<Facts> {
        self.charge(1)?;
        let page = self
            .doc
            .objects
            .get(&id)
            .context("Missing source text page")?
            .as_dict()?;
        let mut node = page;
        let mut resources = None;
        let mut seen = HashSet::new();
        loop {
            self.charge(1)?;
            if let Ok(value) = node.get(b"Resources") {
                resources = Some(self.resolve(value)?.as_dict()?);
                break;
            }
            let Ok(parent) = node.get(b"Parent") else {
                break;
            };
            let id = parent.as_reference()?;
            ensure!(
                seen.len() < DEPTH && seen.insert(id),
                "Cyclic source text parent"
            );
            node = self.resolve(parent)?.as_dict()?;
        }
        let mut data = Vec::new();
        if let Ok(contents) = page.get(b"Contents") {
            self.contents(contents, &mut data, 0)?;
        }
        let mut actual = self.structure.for_page(id);
        structure::Scan::run(self.doc, id, &data, resources, &mut actual);
        let mut projection = PageProjection::new(actual);
        let minimum_glyphs = self.operations(
            &data,
            resources,
            State::default(),
            &mut HashSet::new(),
            &mut projection,
            Scope::default(),
        )?;
        Ok(projection.finish(minimum_glyphs))
    }

    fn stream(&mut self, stream: &Stream, remaining: usize) -> Result<Vec<u8>> {
        self.charge(1)?;
        let limit = remaining.min(DOCUMENT_BYTES.saturating_sub(self.bytes));
        // A malformed /Filter must not be silently interpreted as unfiltered data.
        if stream.dict.has(b"Filter") {
            stream.filters()?;
        }
        let data = stream.decompressed_content_with_limit(limit)?;
        self.bytes = self.bytes.saturating_add(data.len());
        ensure!(
            self.bytes <= DOCUMENT_BYTES,
            "Source text exceeds decoded byte budget"
        );
        Ok(data)
    }

    fn contents(&mut self, value: &'a Object, data: &mut Vec<u8>, depth: usize) -> Result<()> {
        self.charge(1)?;
        ensure!(depth < DEPTH, "Source text Contents nesting exceeds budget");
        match self.resolve(value)? {
            Object::Stream(stream) => {
                let bytes = self.stream(stream, PAGE_BYTES.saturating_sub(data.len()))?;
                ensure!(
                    data.len() + bytes.len() < PAGE_BYTES,
                    "Source text page exceeds byte budget"
                );
                data.extend_from_slice(&bytes);
                data.push(b'\n');
            }
            Object::Array(array) => {
                for item in array {
                    self.contents(item, data, depth + 1)?;
                }
            }
            Object::Null => {}
            _ => anyhow::bail!("Invalid source text Contents"),
        }
        Ok(())
    }

    fn resource(
        &mut self,
        resources: Option<&'a Dictionary>,
        kind: &[u8],
        name: &[u8],
    ) -> Result<&'a Object> {
        let dictionary = self
            .resolve(
                resources
                    .context("Missing source text resources")?
                    .get(kind)?,
            )?
            .as_dict()?;
        self.resolve(dictionary.get(name)?)
    }

    fn font_width(&mut self, font: &'a Object) -> Result<usize> {
        let font = self.resolve(font)?.as_dict()?;
        match font.get(b"Subtype")?.as_name()? {
            b"Type1" | b"MMType1" | b"TrueType" | b"Type3" => Ok(1),
            b"Type0" => {
                let encoding = self.resolve(font.get(b"Encoding")?)?;
                match encoding {
                    Object::Name(name) if name == b"Identity-H" || name == b"Identity-V" => Ok(2),
                    Object::Name(_) | Object::Stream(_) => Ok(4),
                    _ => anyhow::bail!("Invalid composite source font encoding"),
                }
            }
            _ => anyhow::bail!("Unsupported source font subtype"),
        }
    }

    fn show(
        &mut self,
        value: &Object,
        state: State,
        projection: &mut PageProjection,
        scope: Scope,
    ) -> Result<usize> {
        let bytes = value.as_str()?;
        self.charge(bytes.len())?;
        if bytes.is_empty() {
            return Ok(0);
        }
        let width = state.width.context("Source text has no known font")?;
        ensure!(
            width != 2 || bytes.len() % 2 == 0,
            "Odd-length Identity source text"
        );
        let count = bytes.len().div_ceil(width);
        if !scope.actual {
            let mut facts = unicode::Facts::default();
            self.unicode.show(state.font, bytes, &mut facts);
            projection.add(facts, scope.group);
            if let Some(group) = scope.group {
                projection.glyphs[group] += count;
            }
            Ok(count)
        } else {
            Ok(0)
        }
    }

    fn operations(
        &mut self,
        data: &[u8],
        resources: Option<&'a Dictionary>,
        mut state: State,
        forms: &mut HashSet<ObjectId>,
        projection: &mut PageProjection,
        mut scope: Scope,
    ) -> Result<usize> {
        ensure!(
            forms.len() < DEPTH,
            "Source text Form nesting exceeds budget"
        );
        // Strict parsing is essential: the permissive iterator can silently stop
        // before malformed trailing operators. Input is bounded before allocation.
        let content = Content::decode_strict(data)?;
        self.charge(content.operations.len())?;
        let mut stack = Vec::new();
        let mut in_text = false;
        let mut marked: Vec<Scope> = Vec::new();
        let mut count = 0usize;
        for op in content.operations {
            let args = op.operands.as_slice();
            match op.operator.as_str() {
                "q" => {
                    ensure!(args.is_empty(), "Invalid source q operands");
                    ensure!(
                        stack.len() < 64,
                        "Source text graphics stack exceeds budget"
                    );
                    stack.push(state);
                }
                "Q" => {
                    ensure!(args.is_empty(), "Invalid source Q operands");
                    state = stack
                        .pop()
                        .context("Unbalanced source text graphics stack")?
                }
                "BT" => {
                    ensure!(args.is_empty(), "Invalid source BT operands");
                    ensure!(!in_text, "Nested source text object");
                    in_text = true;
                }
                "ET" => {
                    ensure!(args.is_empty(), "Invalid source ET operands");
                    ensure!(in_text, "Unbalanced source text object");
                    in_text = false;
                }
                "Tf" => {
                    ensure!(args.len() == 2, "Invalid source Tf operands");
                    let font = self.resource(resources, b"Font", args[0].as_name()?)?;
                    let width = self.font_width(font)?;
                    state.width = Some(width);
                    state.font = Some(self.unicode.font(font, width));
                }
                "gs" => {
                    ensure!(args.len() == 1, "Invalid source gs operands");
                    let gs = self
                        .resource(resources, b"ExtGState", args[0].as_name()?)?
                        .as_dict()?;
                    if let Ok(font) = gs.get(b"Font") {
                        let font = self.resolve(font)?.as_array()?;
                        ensure!(font.len() == 2, "Invalid source ExtGState Font");
                        let width = self.font_width(&font[0])?;
                        state.width = Some(width);
                        state.font = Some(self.unicode.font(&font[0], width));
                    }
                }
                "Tj" | "'" | "\"" => {
                    let expected = if op.operator == "\"" { 3 } else { 1 };
                    ensure!(
                        in_text && args.len() == expected,
                        "Invalid source text-showing operator"
                    );
                    count += self.show(&args[expected - 1], state, projection, scope)?;
                }
                "TJ" => {
                    ensure!(in_text && args.len() == 1, "Invalid source TJ operands");
                    for part in args[0].as_array()? {
                        match part {
                            Object::String(..) => {
                                count += self.show(part, state, projection, scope)?
                            }
                            Object::Integer(_) | Object::Real(_) => {}
                            _ => anyhow::bail!("Invalid source TJ array"),
                        }
                    }
                }
                "Do" => {
                    ensure!(args.len() == 1, "Invalid source Do operands");
                    let object = self
                        .resource(resources, b"XObject", args[0].as_name()?)?
                        .as_stream()?;
                    match object.dict.get(b"Subtype")?.as_name()? {
                        b"Image" => {}
                        b"Form" => {
                            // Pointer identity is unnecessary: resolve the named reference
                            // before recursion and detect repeated active Form IDs.
                            let names = self
                                .resolve(
                                    resources
                                        .context("Missing Form resources")?
                                        .get(b"XObject")?,
                                )?
                                .as_dict()?;
                            let id = names.get(args[0].as_name()?)?.as_reference()?;
                            ensure!(forms.insert(id), "Cyclic source text Form");
                            let nested = match object.dict.get(b"Resources") {
                                Ok(value) => Some(self.resolve(value)?.as_dict()?),
                                Err(_) => resources,
                            };
                            let bytes = self.stream(object, PAGE_BYTES)?;
                            count +=
                                self.operations(&bytes, nested, state, forms, projection, scope)?;
                            forms.remove(&id);
                        }
                        _ => anyhow::bail!("Unsupported source XObject"),
                    }
                }
                "BMC" => {
                    ensure!(
                        args.len() == 1 && args[0].as_name().is_ok(),
                        "Invalid BMC operands"
                    );
                    ensure!(
                        marked.len() < 64,
                        "Source marked-content stack exceeds budget"
                    );
                    marked.push(scope);
                }
                "BDC" => {
                    ensure!(
                        args.len() == 2 && args[0].as_name().is_ok(),
                        "Invalid BDC operands"
                    );
                    ensure!(
                        marked.len() < 64,
                        "Source marked-content stack exceeds budget"
                    );
                    let properties = match &args[1] {
                        Object::Dictionary(dict) => Some(dict),
                        Object::Name(name) => self
                            .resource(resources, b"Properties", name)
                            .ok()
                            .and_then(|v| v.as_dict().ok()),
                        _ => None,
                    };
                    marked.push(scope);
                    if let Some(dict) = properties {
                        if let Ok(value) = dict.get(b"MCID") {
                            self.charge(1)?;
                            let mcid = i32::try_from(self.resolve(value)?.as_i64()?)?;
                            scope.context = projection.context(scope.context, mcid)?;
                        }
                        if let Some(group) = dict
                            .get(b"MCID")
                            .ok()
                            .and_then(|v| v.as_i64().ok())
                            .and_then(|id| i32::try_from(id).ok())
                            .and_then(|id| projection.actual.ids.get(&id).copied())
                        {
                            if scope.actual || scope.group.is_some_and(|old| old != group) {
                                projection.actual.groups[group].invalid = true;
                                if let Some(old) = scope.group {
                                    projection.actual.groups[old].invalid = true;
                                }
                                projection.actual.unavailable = true;
                            } else {
                                scope.group = Some(group);
                            }
                        }
                        if let Ok(actual) = dict.get(b"ActualText") {
                            projection.actual.marked_actual = true;
                            let mut facts = unicode::Facts::default();
                            let value = self.unicode.actual_text(actual, &mut facts);
                            let empty = value.text.as_ref().is_some_and(|text| text.is_empty());
                            if !scope.actual && scope.group.is_none() && value.encoded_bytes <= 4096
                            {
                                if let Some(text) = value.text.filter(|text| !text.is_empty()) {
                                    let mcids = projection.mcids(scope.context);
                                    self.charge(mcids.len().saturating_add(text.len()))?;
                                    let signature = structure::MarkedSignature { mcids, text };
                                    *projection
                                        .actual
                                        .marked_scopes
                                        .entry(signature)
                                        .or_default() += 1;
                                }
                            }
                            if let Some(group) = scope.group {
                                projection.actual.groups[group].invalid = true;
                                projection.actual.unavailable = true;
                            }
                            if scope.actual {
                                if !(scope.empty && empty) {
                                    projection.actual.unavailable = true;
                                }
                                scope.empty &= empty;
                            } else {
                                projection.add(facts, scope.group);
                                scope.empty = empty;
                            }
                            scope.actual = true;
                        }
                    } else {
                        projection.actual.unavailable = true;
                        scope.actual = true;
                        scope.empty = false;
                    }
                }
                "EMC" => {
                    ensure!(args.is_empty(), "Invalid EMC operands");
                    scope = marked.pop().context("Unbalanced source marked content")?;
                }
                // Type3 programs and tiling patterns can themselves paint text.
                // Their glyph/program recursion is not covered by this reader.
                "SCN" | "scn" if args.iter().any(|arg| matches!(arg, Object::Name(_))) => {
                    anyhow::bail!("Source text in patterns is not accounted")
                }
                _ => {}
            }
        }
        ensure!(
            !in_text && stack.is_empty() && marked.is_empty(),
            "Unbalanced source text stream"
        );
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    fn source(content: &[u8]) -> (Document, ObjectId) {
        let mut doc = Document::with_version("1.7");
        let simple = doc
            .add_object(dictionary! {"Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica"});
        let identity = doc
            .add_object(dictionary! {"Type"=>"Font", "Subtype"=>"Type0", "Encoding"=>"Identity-H"});
        let generic = doc
            .add_object(dictionary! {"Type"=>"Font", "Subtype"=>"Type0", "Encoding"=>"SomeCMap"});
        let stream = doc.add_object(Stream::new(Dictionary::new(), content.to_vec()));
        let resources = dictionary! {"Font"=>dictionary!{"F1"=>simple, "F2"=>identity, "F3"=>generic}, "ExtGState"=>dictionary!{"GS"=>dictionary!{"Font"=>vec![identity.into(),12.into()]}}};
        let parent = doc.add_object(dictionary! {"Resources"=>resources});
        let page =
            doc.add_object(dictionary! {"Type"=>"Page", "Parent"=>parent, "Contents"=>stream});
        (doc, page)
    }

    #[test]
    fn all_show_operators_escapes_fonts_and_saved_state_count_original_codes() {
        let (doc, page) = source(b"BT /F1 12 Tf (A\\050B\\051) Tj [(C) 722 (D)] TJ (EF) ' 0 0 (G) \" q /F2 12 Tf <00480049> Tj Q (J) Tj /GS gs <004b> Tj ET");
        assert_eq!(Reader::new(&doc, &[page]).page(page).unwrap(), 13);
        let (doc, page) = source(b"BT /F3 12 Tf <0102030405> Tj ET");
        assert_eq!(Reader::new(&doc, &[page]).page(page).unwrap(), 2); // lower bound, not five Unicode characters
    }

    #[test]
    fn forms_are_counted_per_invocation_with_their_own_resources() {
        let (mut doc, page) = source(b"BT /F1 12 Tf ET /Outer Do /Outer Do");
        let inner = doc.add_object(Stream::new(
            dictionary! {"Subtype"=>"Form"},
            b"BT (AB) Tj ET".to_vec(),
        ));
        let outer = doc.add_object(Stream::new(dictionary!{"Subtype"=>"Form", "Resources"=>dictionary!{"XObject"=>dictionary!{"Inner"=>inner}}}, b"/Inner Do".to_vec()));
        // A Form without Resources inherits the active resource dictionary.
        let parent = doc
            .get_dictionary(page)
            .unwrap()
            .get(b"Parent")
            .unwrap()
            .as_reference()
            .unwrap();
        let resources = doc
            .get_dictionary_mut(parent)
            .unwrap()
            .get_mut(b"Resources")
            .unwrap()
            .as_dict_mut()
            .unwrap();
        resources.set("XObject", dictionary! {"Outer"=>outer});
        assert_eq!(Reader::new(&doc, &[page]).page(page).unwrap(), 4);
        doc.get_object_mut(inner)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .content = b"/Inner Do".to_vec();
        assert!(Reader::new(&doc, &[page]).page(page).is_err());
    }

    #[test]
    fn malformed_or_unsupported_source_is_never_a_successful_zero_count() {
        for bytes in [
            b"BT /F1 12 Tf (C) Tj ET (tail)".as_slice(),
            b"BT /F2 12 Tf <43> Tj ET",
            b"BT (C) Tj ET",
            b"BT /Missing 12 Tf (C) Tj ET",
            b"BT /F1 12 Tf [(A) /Bad] TJ ET",
            b"Q BT /F1 12 Tf (C) Tj ET",
            b"BT /F1 12 Tf (C) Tj",
            b"/Missing Do",
            b"/Pattern cs /P scn",
        ] {
            let (doc, page) = source(bytes);
            assert!(
                Reader::new(&doc, &[page]).page(page).is_err(),
                "{:?}",
                bytes
            );
        }
    }

    #[test]
    fn concatenated_contents_preserve_state_and_do_not_skip_bad_references() {
        let (mut doc, page) = source(b"BT /F1 12 Tf (A) Tj");
        let first = doc
            .get_dictionary(page)
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .clone();
        let second = doc.add_object(Stream::new(Dictionary::new(), b"(B) Tj ET".to_vec()));
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Contents", vec![first, second.into()]);
        assert_eq!(Reader::new(&doc, &[page]).page(page).unwrap(), 2);
        doc.objects.remove(&second);
        assert!(Reader::new(&doc, &[page]).page(page).is_err());
    }

    #[test]
    fn budgets_apply_before_decompression_and_across_pages_and_forms() {
        let (mut doc, page) = source(b"BT /F1 12 Tf (C) Tj ET");
        let mut reader = Reader::new(&doc, &[page]);
        reader.work = WORK;
        assert!(reader.page(page).is_err());
        let mut reader = Reader::new(&doc, &[page]);
        reader.bytes = DOCUMENT_BYTES;
        assert!(reader.page(page).is_err());
        let id = doc
            .get_dictionary(page)
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .as_reference()
            .unwrap();
        let stream = doc.get_object_mut(id).unwrap().as_stream_mut().unwrap();
        stream.content = vec![b' '; PAGE_BYTES + 1];
        stream.compress().unwrap();
        assert!(Reader::new(&doc, &[page]).page(page).is_err());
    }
}
