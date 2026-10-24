//! Bounded, independent Unicode evidence from source font declarations.
//! Never expands a character-code range or repairs malformed UTF-16 silently.
use anyhow::{ensure, Context, Result};
use lopdf::{content::Content, dictionary, Document, Encoding, Object};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

const WORK: usize = 4_000_000;
const BYTES: usize = 8 * 1024 * 1024;
const FONT_LIMIT: usize = 512;
const MAP_ENTRIES: usize = 100_000;

#[derive(Default)]
pub(crate) struct Facts {
    pub scalars: BTreeMap<char, usize>,
    pub unavailable: bool,
}

/// Canonical equivalents and the SDK's common Latin ligature expansions.
/// Whitespace/control codes stay in the separate encoded-glyph count guard.
pub(crate) fn add_scalar(bag: &mut BTreeMap<char, usize>, scalar: char) {
    let expansion = match scalar {
        'ﬀ' => "ff",
        'ﬁ' => "fi",
        'ﬂ' => "fl",
        'ﬃ' => "ffi",
        'ﬄ' => "ffl",
        'ﬅ' | 'ﬆ' => "st",
        'ſ' => "s",
        _ => "",
    };
    if expansion.is_empty() {
        for c in scalar
            .to_string()
            .nfd()
            .filter(|c| !c.is_whitespace() && !c.is_control())
        {
            *bag.entry(c).or_default() += 1;
        }
    } else {
        for c in expansion.chars() {
            *bag.entry(c).or_default() += 1;
        }
    }
}

#[derive(Clone)]
enum Target {
    Increment(Vec<u8>),
    Array(Vec<Vec<u8>>),
}
#[derive(Clone)]
struct Entry {
    end: u32,
    origin: u32,
    target: Rc<Target>,
}
#[derive(Default)]
struct Cmap {
    entries: [BTreeMap<u32, Entry>; 4],
    spaces: [Vec<(u32, u32)>; 4],
}

fn code(object: &Object) -> Result<(u32, usize)> {
    let bytes = object.as_str()?;
    ensure!((1..=4).contains(&bytes.len()), "Invalid CMap source width");
    Ok((
        bytes.iter().fold(0, |v, b| (v << 8) | u32::from(*b)),
        bytes.len(),
    ))
}
fn utf16(bytes: &[u8]) -> Result<String> {
    ensure!(
        !bytes.is_empty() && bytes.len().is_multiple_of(2) && bytes.len() <= 1024,
        "Invalid CMap UTF16 destination"
    );
    let units: Vec<_> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .collect();
    Ok(String::from_utf16(&units)?)
}
fn increment(bytes: &[u8], amount: u32) -> Result<Vec<u8>> {
    let mut result = bytes.to_vec();
    let mut carry = u64::from(amount);
    for byte in result.iter_mut().rev() {
        carry += u64::from(*byte);
        *byte = carry as u8;
        carry >>= 8;
    }
    ensure!(carry == 0, "CMap destination overflow");
    Ok(result)
}

impl Cmap {
    fn put(
        &mut self,
        start: u32,
        end: u32,
        len: usize,
        target: Target,
        budget: &mut usize,
    ) -> Result<()> {
        ensure!(start <= end && (1..=4).contains(&len), "Invalid CMap range");
        let map = &mut self.entries[len - 1];
        let mut overlap = Vec::new();
        if let Some((&at, entry)) = map.range(..start).next_back() {
            if entry.end >= start {
                overlap.push(at)
            }
        }
        overlap.extend(map.range(start..=end).map(|(&at, _)| at));
        charge(budget, overlap.len() + 1)?;
        for at in overlap {
            let previous = map.remove(&at).context("Missing CMap interval")?;
            if at < start {
                let mut left = previous.clone();
                left.end = start - 1;
                map.insert(at, left);
            }
            if previous.end > end {
                map.insert(end + 1, previous);
            }
        }
        map.insert(
            start,
            Entry {
                end,
                origin: start,
                target: Rc::new(target),
            },
        );
        ensure!(
            self.entries.iter().map(BTreeMap::len).sum::<usize>() <= MAP_ENTRIES,
            "CMap exceeds interval budget"
        );
        Ok(())
    }
    fn parse(data: &[u8], budget: &mut usize) -> Result<Self> {
        ensure!(data.len() <= BYTES, "CMap exceeds byte budget");
        let content = Content::decode_strict(data)?;
        charge(budget, content.operations.len())?;
        let mut map = Self::default();
        let mut section: Option<(&str, usize)> = None;
        for op in &content.operations {
            let name = op.operator.as_str();
            let args = &op.operands;
            match name {
                "begincodespacerange" | "beginbfchar" | "beginbfrange" => {
                    ensure!(
                        section.is_none() && args.len() == 1,
                        "Invalid CMap section start"
                    );
                    let count = args[0].as_i64()?;
                    ensure!((0..=100_000).contains(&count), "Invalid CMap section count");
                    section = Some((name, count as usize));
                }
                "endcodespacerange" | "endbfchar" | "endbfrange" => {
                    let (begin, count) = section.take().context("Unbalanced CMap section")?;
                    ensure!(begin[5..] == name[3..], "Mismatched CMap section");
                    let width = if name == "endbfrange" { 3 } else { 2 };
                    ensure!(
                        args.len() == count * width,
                        "CMap section count disagrees with operands"
                    );
                    charge(budget, args.len())?;
                    for row in args.chunks_exact(width) {
                        let (start, len) = code(&row[0])?;
                        if name == "endbfchar" {
                            let bytes = row[1].as_str()?.to_vec();
                            utf16(&bytes)?;
                            map.put(start, start, len, Target::Increment(bytes), budget)?;
                        } else {
                            let (end, last_len) = code(&row[1])?;
                            ensure!(start <= end && len == last_len, "Invalid CMap range bounds");
                            if name == "endcodespacerange" {
                                map.spaces[len - 1].push((start, end));
                                continue;
                            }
                            let target = match &row[2] {
                                Object::String(bytes, _) => {
                                    utf16(bytes)?;
                                    increment(bytes, end - start)?;
                                    Target::Increment(bytes.clone())
                                }
                                Object::Array(array) => {
                                    ensure!(
                                        array.len() as u64 == u64::from(end - start) + 1,
                                        "CMap array length disagrees with range"
                                    );
                                    let mut values = Vec::new();
                                    for value in array {
                                        let bytes = value.as_str()?;
                                        utf16(bytes)?;
                                        charge(budget, bytes.len())?;
                                        values.push(bytes.to_vec());
                                    }
                                    Target::Array(values)
                                }
                                _ => anyhow::bail!("Invalid CMap target"),
                            };
                            map.put(start, end, len, target, budget)?;
                        }
                    }
                }
                "usecmap" => anyhow::bail!("External CMap inheritance is not verified"),
                "findresource" | "begin" | "dict" | "def" | "end" | "begincmap" | "endcmap"
                | "CMapName" | "currentdict" | "defineresource" | "pop" | "dup" | "readonly" => {
                    ensure!(
                        section.is_none(),
                        "Unexpected operator inside CMap mapping section"
                    )
                }
                _ => anyhow::bail!("Unsupported operator in source ToUnicode CMap"),
            }
        }
        ensure!(
            section.is_none() && map.spaces.iter().any(|spaces| !spaces.is_empty()),
            "Incomplete CMap codespaces"
        );
        for spaces in &mut map.spaces {
            charge(budget, spaces.len().saturating_mul(20))?;
            spaces.sort_unstable();
            let mut merged: Vec<(u32, u32)> = Vec::new();
            for &(lo, hi) in spaces.iter() {
                if let Some(last) = merged.last_mut() {
                    if lo <= last.1.saturating_add(1) {
                        last.1 = last.1.max(hi);
                        continue;
                    }
                }
                merged.push((lo, hi));
            }
            *spaces = merged;
        }
        Ok(map)
    }
    fn contains_code(&self, code: u32, len: usize) -> bool {
        let spaces = &self.spaces[len - 1];
        let at = spaces.partition_point(|&(lo, _)| lo <= code);
        at > 0 && code <= spaces[at - 1].1
    }
    fn get(&self, code: u32, len: usize) -> Result<Option<String>> {
        let Some((_, entry)) = self.entries[len - 1].range(..=code).next_back() else {
            return Ok(None);
        };
        if entry.end < code {
            return Ok(None);
        }
        let offset = code - entry.origin;
        let bytes = match entry.target.as_ref() {
            Target::Increment(base) => increment(base, offset)?,
            Target::Array(array) => array
                .get(offset as usize)
                .context("CMap array offset exceeds target")?
                .clone(),
        };
        Ok(Some(utf16(&bytes)?))
    }
}
fn charge(work: &mut usize, n: usize) -> Result<()> {
    *work = work.saturating_add(n);
    ensure!(*work <= WORK, "Source Unicode exceeds work budget");
    Ok(())
}

enum Font {
    Cmap {
        mapping: Cmap,
        encoding: CodeEncoding,
    },
    Unicode(CodeEncoding),
    Simple(Box<[Option<String>; 256]>),
    Unknown,
}
pub(super) struct ActualValue {
    pub text: Option<String>,
    pub encoded_bytes: usize,
}
pub(super) struct Reader<'a> {
    doc: &'a Document,
    fonts: HashMap<usize, Font>,
    work: usize,
    bytes: usize,
}
impl<'a> Reader<'a> {
    pub fn new(doc: &'a Document) -> Self {
        Self {
            doc,
            fonts: HashMap::new(),
            work: 0,
            bytes: 0,
        }
    }
    fn resolve(&mut self, mut value: &'a Object) -> Result<&'a Object> {
        let mut seen = std::collections::HashSet::new();
        while let Object::Reference(id) = value {
            charge(&mut self.work, 1)?;
            ensure!(
                seen.len() < 32 && seen.insert(*id),
                "Cyclic Unicode resource"
            );
            value = self
                .doc
                .objects
                .get(id)
                .context("Missing Unicode resource")?;
        }
        Ok(value)
    }
    pub fn font(&mut self, object: &'a Object, width: usize) -> usize {
        let object = match self.resolve(object) {
            Ok(object) => object,
            Err(_) => return usize::MAX,
        };
        // Stable addresses of our immutable lopdf objects, never SDK layouts.
        let key = object as *const Object as usize;
        if !self.fonts.contains_key(&key) && self.fonts.len() < FONT_LIMIT {
            let font = self.read_font(object, width).unwrap_or(Font::Unknown);
            self.fonts.insert(key, font);
        }
        key
    }
    fn read_font(&mut self, object: &'a Object, width: usize) -> Result<Font> {
        charge(&mut self.work, 256)?;
        let font = self.resolve(object)?.as_dict()?;
        if let Ok(cmap) = font.get(b"ToUnicode") {
            let encoding = source_encoding(font, width)?;
            let stream = self.resolve(cmap)?.as_stream()?;
            ensure!(
                !stream.dict.has(b"UseCMap"),
                "Inherited ToUnicode CMap is not verified"
            );
            if stream.dict.has(b"Filter") {
                stream.filters()?;
            }
            let bytes = stream.decompressed_content_with_limit(BYTES.saturating_sub(self.bytes))?;
            self.bytes += bytes.len();
            return Ok(Font::Cmap {
                mapping: Cmap::parse(&bytes, &mut self.work)?,
                encoding,
            });
        }
        if width != 1 {
            let encoding = source_encoding(font, width)?;
            ensure!(
                !matches!(encoding, CodeEncoding::Fixed(_)),
                "Composite source font lacks verified Unicode mapping"
            );
            return Ok(Font::Unicode(encoding));
        }
        let base = font
            .get(b"BaseFont")
            .and_then(Object::as_name)
            .unwrap_or_default();
        let standard = matches!(
            base,
            b"Helvetica"
                | b"Helvetica-Bold"
                | b"Helvetica-Oblique"
                | b"Helvetica-BoldOblique"
                | b"Times-Roman"
                | b"Times-Bold"
                | b"Times-Italic"
                | b"Times-BoldItalic"
                | b"Courier"
                | b"Courier-Bold"
                | b"Courier-Oblique"
                | b"Courier-BoldOblique"
        );
        let embedded = font
            .get(b"FontDescriptor")
            .ok()
            .map(|v| {
                let d = self.resolve(v)?.as_dict()?;
                Ok::<_, anyhow::Error>(
                    d.has(b"FontFile") || d.has(b"FontFile2") || d.has(b"FontFile3"),
                )
            })
            .transpose()?
            .unwrap_or(false);
        let default = (standard && !embedded).then_some(b"StandardEncoding".as_slice());
        let mut table: Box<[Option<String>; 256]> = Box::new(std::array::from_fn(|_| None));
        let encoding = font
            .get(b"Encoding")
            .ok()
            .map(|e| self.resolve(e))
            .transpose()?;
        let (base_encoding, differences) = match encoding {
            Some(Object::Name(name)) => {
                validate_encoding(name)?;
                (Some(name.as_slice()), None)
            }
            Some(Object::Dictionary(dict)) => {
                let base_encoding = match dict.get(b"BaseEncoding") {
                    Ok(value) => {
                        let name = self.resolve(value)?.as_name()?;
                        validate_encoding(name)?;
                        Some(name)
                    }
                    Err(_) => default,
                };
                (base_encoding, dict.get(b"Differences").ok())
            }
            None => (default, None),
            _ => anyhow::bail!("Invalid simple font encoding"),
        };
        if let Some(encoding) = base_encoding {
            let synthetic =
                dictionary! {"Type"=>"Font", "Encoding"=>Object::Name(encoding.to_vec())};
            let encoding = synthetic.get_font_encoding_with_limit(self.doc, 0)?;
            ensure!(
                matches!(encoding, Encoding::OneByteEncoding(_)),
                "Unexpected simple encoding decoder"
            );
            for (index, value) in table.iter_mut().enumerate() {
                *value = encoding
                    .bytes_to_string(&[index as u8])
                    .ok()
                    .filter(|text| !text.is_empty());
            }
        }
        if let Some(differences) = differences {
            let differences = self.resolve(differences)?.as_array()?;
            let mut at = None;
            charge(&mut self.work, differences.len())?;
            for item in differences {
                match item {
                    Object::Integer(n) => {
                        ensure!((0..256).contains(n), "Invalid Differences index");
                        at = Some(*n as usize)
                    }
                    Object::Name(name) => {
                        let index = at.context("Differences name lacks index")?;
                        ensure!(index < 256, "Differences exceeds byte codes");
                        charge(&mut self.work, name.len())?;
                        table[index] = glyph_name(name);
                        at = Some(index + 1)
                    }
                    _ => anyhow::bail!("Invalid Differences entry"),
                }
            }
        }
        let bytes = table.iter().flatten().map(String::len).sum::<usize>();
        ensure!(
            bytes <= BYTES.saturating_sub(self.bytes),
            "Simple Unicode mappings exceed cache budget"
        );
        self.bytes += bytes;
        Ok(Font::Simple(table))
    }
    pub fn show(&mut self, key: Option<usize>, bytes: &[u8], facts: &mut Facts) {
        if bytes.is_empty() {
            return;
        }
        if charge(&mut self.work, bytes.len()).is_err() {
            facts.unavailable = true;
            return;
        }
        let Some(font) = key.and_then(|key| self.fonts.get(&key)) else {
            facts.unavailable = true;
            return;
        };
        let encoding = match font {
            Font::Simple(_) => CodeEncoding::Fixed(1),
            Font::Cmap { encoding, .. } | Font::Unicode(encoding) => *encoding,
            Font::Unknown => {
                facts.unavailable = true;
                return;
            }
        };
        let mut at = 0;
        while at < bytes.len() {
            let width = match encoding.code_len(&bytes[at..]) {
                Ok(width) => width,
                Err(_) => {
                    facts.unavailable = true;
                    return;
                }
            };
            let code_bytes = &bytes[at..at + width];
            at += width;
            let decoded = match font {
                Font::Simple(table) => Ok(table[code_bytes[0] as usize].clone()),
                Font::Cmap { mapping, .. } => {
                    let code = code_bytes.iter().fold(0, |v, b| (v << 8) | u32::from(*b));
                    if !mapping.contains_code(code, width) {
                        facts.unavailable = true;
                    }
                    mapping.get(code, width)
                }
                Font::Unicode(_) => encoding.decode(code_bytes).map(Some),
                Font::Unknown => unreachable!(),
            };
            match decoded {
                Ok(Some(text)) => {
                    if charge(&mut self.work, text.len()).is_err() {
                        facts.unavailable = true;
                        return;
                    }
                    for c in text.chars() {
                        add_scalar(&mut facts.scalars, c);
                    }
                }
                _ => facts.unavailable = true,
            }
        }
    }
    pub fn actual_text(&mut self, object: &Object, facts: &mut Facts) -> ActualValue {
        let decoded = actual_bytes(self.doc, object).and_then(|bytes| {
            charge(&mut self.work, bytes.len())?;
            decode_actual_bytes(bytes).map(|text| (text, bytes.len()))
        });
        match decoded {
            Ok((text, encoded_bytes)) => {
                for c in text.chars() {
                    add_scalar(&mut facts.scalars, c)
                }
                ActualValue {
                    text: Some(text),
                    encoded_bytes,
                }
            }
            Err(_) => {
                facts.unavailable = true;
                ActualValue {
                    text: None,
                    encoded_bytes: 0,
                }
            }
        }
    }
}
fn validate_encoding(name: &[u8]) -> Result<()> {
    ensure!(
        matches!(
            name,
            b"WinAnsiEncoding" | b"MacRomanEncoding" | b"MacExpertEncoding" | b"StandardEncoding"
        ),
        "Unsupported simple encoding"
    );
    Ok(())
}
// AGL2.0, pinned in resources/adobe-glyphlist.json. Dependency font decoders
// also recognize nonstandard aliases; those are not independent source proof.
fn adobe_glyph_names() -> &'static BTreeMap<&'static str, &'static str> {
    static NAMES: OnceLock<BTreeMap<&'static str, &'static str>> = OnceLock::new();
    NAMES.get_or_init(|| {
        include_str!("../../resources/adobe-glyphlist.txt")
            .lines()
            .filter(|line| !line.starts_with('#'))
            .filter_map(|line| line.split_once(';'))
            .collect()
    })
}
fn glyph_name(name: &[u8]) -> Option<String> {
    if name.len() > 4096 {
        return None;
    }
    glyph_name_inner(name).filter(|s| s.len() <= 1024)
}
fn glyph_name_inner(name: &[u8]) -> Option<String> {
    let name = name.split(|b| *b == b'.').next()?;
    if name.contains(&b'_') {
        // Unknown components make this source mapping unavailable even though
        // the AGL text-conversion algorithm would discard those components.
        return name
            .split(|b| *b == b'_')
            .map(glyph_name)
            .collect::<Option<Vec<_>>>()
            .map(|v| v.concat());
    }
    let text = std::str::from_utf8(name).ok()?;
    // Names such as union, universal and uacute are AGL entries. They must
    // precede the generic uni/u naming rules, including the prefix checks.
    if let Some(values) = adobe_glyph_names().get(text) {
        return values
            .split_whitespace()
            .map(|hex| u32::from_str_radix(hex, 16).ok().and_then(char::from_u32))
            .collect();
    }
    let upper_hex = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b))
    };
    if let Some(hex) = text.strip_prefix("uni") {
        if !upper_hex(hex) || !hex.len().is_multiple_of(4) {
            return None;
        }
        return hex
            .as_bytes()
            .chunks_exact(4)
            .map(|s| {
                u32::from_str_radix(std::str::from_utf8(s).ok()?, 16)
                    .ok()
                    .and_then(char::from_u32)
            })
            .collect();
    }
    let hex = text
        .strip_prefix('u')
        .filter(|s| (4..=6).contains(&s.len()) && upper_hex(s))?;
    u32::from_str_radix(hex, 16)
        .ok()
        .and_then(char::from_u32)
        .map(|c| c.to_string())
}

#[derive(Clone, Copy)]
enum CodeEncoding {
    Fixed(usize),
    Ucs2,
    Utf8,
    Utf16,
    Utf32,
}
impl CodeEncoding {
    fn code_len(self, bytes: &[u8]) -> Result<usize> {
        let len = match self {
            Self::Fixed(n) => n,
            Self::Ucs2 | Self::Utf16 => {
                ensure!(bytes.len() >= 2, "Truncated source Unicode code");
                let first = u16::from_be_bytes([bytes[0], bytes[1]]);
                if matches!(self, Self::Ucs2) {
                    ensure!(
                        !(0xd800..=0xdfff).contains(&first),
                        "Surrogate code in UCS2 source"
                    );
                    2
                } else if (0xd800..=0xdbff).contains(&first) {
                    ensure!(
                        bytes.len() >= 4
                            && (0xdc00..=0xdfff)
                                .contains(&u16::from_be_bytes([bytes[2], bytes[3]])),
                        "Invalid UTF16 source pair"
                    );
                    4
                } else {
                    ensure!(
                        !(0xdc00..=0xdfff).contains(&first),
                        "Unmatched UTF16 low source unit"
                    );
                    2
                }
            }
            Self::Utf32 => 4,
            Self::Utf8 => match bytes[0] {
                0..=0x7f => 1,
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf4 => 4,
                _ => anyhow::bail!("Invalid UTF8 source lead byte"),
            },
        };
        ensure!(len <= bytes.len(), "Truncated source code");
        if matches!(self, Self::Utf8 | Self::Utf32) {
            self.decode(&bytes[..len])?;
        }
        Ok(len)
    }
    fn decode(self, bytes: &[u8]) -> Result<String> {
        match self {
            Self::Utf8 => Ok(std::str::from_utf8(bytes)?.to_owned()),
            Self::Ucs2 | Self::Utf16 => utf16(bytes),
            Self::Utf32 => {
                ensure!(bytes.len() == 4, "Invalid UTF32 source width");
                let code = u32::from_be_bytes(bytes.try_into()?);
                Ok(char::from_u32(code)
                    .context("Invalid UTF32 source scalar")?
                    .to_string())
            }
            Self::Fixed(_) => anyhow::bail!("Fixed font codes are not Unicode"),
        }
    }
}
// Unicode forms and byte ranges: https://github.com/adobe-type-tools/cmap-resources
fn source_encoding(font: &lopdf::Dictionary, width: usize) -> Result<CodeEncoding> {
    if width <= 2 {
        return Ok(CodeEncoding::Fixed(width));
    }
    let name = std::str::from_utf8(font.get(b"Encoding")?.as_name()?)?;
    let stem = name
        .strip_suffix("-H")
        .or_else(|| name.strip_suffix("-V"))
        .context("Unsupported composite writing mode")?;
    let (family, kind) = stem
        .rsplit_once('-')
        .context("Unsupported composite code boundaries")?;
    let allowed = match family {
        "UniJISX0213" | "UniJISX02132004" => kind == "UTF32",
        "UniJIS2004" => matches!(kind, "UTF8" | "UTF16" | "UTF32"),
        "UniGB" | "UniCNS" | "UniJIS" | "UniKS" | "UniHojo" => true,
        _ => false,
    };
    ensure!(allowed, "Unknown Unicode composite CMap family");
    match kind {
        "UCS2" => Ok(CodeEncoding::Ucs2),
        "UTF8" => Ok(CodeEncoding::Utf8),
        "UTF16" => Ok(CodeEncoding::Utf16),
        "UTF32" => Ok(CodeEncoding::Utf32),
        _ => anyhow::bail!("Unsupported composite Unicode encoding"),
    }
}

fn actual_bytes<'a>(doc: &'a Document, object: &'a Object) -> Result<&'a [u8]> {
    let mut value = object;
    let mut seen = std::collections::HashSet::new();
    while let Object::Reference(id) = value {
        ensure!(
            seen.len() < 32 && seen.insert(*id),
            "Cyclic ActualText string"
        );
        value = doc.objects.get(id).context("Missing ActualText string")?;
    }
    Ok(value.as_str()?)
}
pub(crate) fn decode_actual(doc: &Document, object: &Object) -> Result<String> {
    decode_actual_bytes(actual_bytes(doc, object)?)
}
pub(crate) fn decode_actual_bytes(bytes: &[u8]) -> Result<String> {
    ensure!(
        bytes.len() <= 4 * 1024 * 1024,
        "ActualText exceeds string budget"
    );
    let text = if let Some(bytes) = bytes.strip_prefix(&[0xfe, 0xff]) {
        ensure!(bytes.len().is_multiple_of(2), "Odd ActualText UTF16");
        let units: Vec<_> = bytes
            .chunks_exact(2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
            .collect();
        String::from_utf16(&units)?
    } else if let Some(bytes) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        std::str::from_utf8(bytes)?.to_owned()
    } else {
        let mut text = String::new();
        for byte in bytes {
            let value = lopdf::decode_text_string(&Object::String(
                vec![*byte],
                lopdf::StringFormat::Literal,
            ))?;
            ensure!(
                value.chars().count() == 1,
                "Undefined ActualText PDFDocEncoding"
            );
            text.push_str(&value);
        }
        text
    };
    ensure!(
        !text.contains('\u{1b}'),
        "ActualText language escapes are not yet supported"
    );
    Ok(text)
}

#[cfg(test)]
mod tests {
    #[test]
    fn actual_text_decoding_is_strict_and_preserves_explicit_empty_strings() {
        assert_eq!(super::decode_actual_bytes(b"").unwrap(), "");
        assert_eq!(super::decode_actual_bytes(&[0xfe, 0xff]).unwrap(), "");
        assert_eq!(
            super::decode_actual_bytes(&[0xfe, 0xff, 0xd8, 0x3e, 0xdd, 0x8a]).unwrap(),
            "🦊"
        );
        assert_eq!(
            super::decode_actual_bytes("\u{feff}日本語".as_bytes()).unwrap(),
            "日本語"
        );
        assert_eq!(super::decode_actual_bytes(b"A\tB").unwrap(), "A\tB");
        for bytes in [
            &[0xfe, 0xff, 0x41][..],
            &[0xfe, 0xff, 0xd8, 0x00],
            &[0xef, 0xbb, 0xbf, 0xff],
            &[0x7f],
            &[0x9f],
            &[0xad],
            &[0xfe, 0xff, 0x00, 0x1b],
        ] {
            assert!(super::decode_actual_bytes(bytes).is_err(), "{bytes:?}");
        }
    }

    use super::*;
    fn cmap(body: &str) -> Cmap {
        let data=format!("/CIDInit /ProcSet findresource begin 12 dict begin begincmap 1 begincodespacerange <00> <ff> endcodespacerange {body} endcmap CMapName currentdict /CMap defineresource pop end end");
        Cmap::parse(data.as_bytes(), &mut 0).unwrap()
    }
    #[test]
    fn later_ranges_arrays_and_chars_override_in_source_order() {
        let map=cmap("1 beginbfrange <41> <43> [<00660069> <D83DDE00> <0045>] endbfrange 1 beginbfchar <42> <0043> endbfchar 1 beginbfrange <41> <42> <0061> endbfrange 1 beginbfchar <41> <00630063> endbfchar");
        assert_eq!(map.get(0x41, 1).unwrap().as_deref(), Some("cc"));
        assert_eq!(map.get(0x42, 1).unwrap().as_deref(), Some("b"));
        assert_eq!(map.get(0x43, 1).unwrap().as_deref(), Some("E"));
        assert_eq!(map.get(0x44, 1).unwrap(), None);
    }
    #[test]
    fn four_billion_code_range_is_kept_as_one_interval() {
        let data=b"begincmap 1 begincodespacerange <00000000> <ffffffff> endcodespacerange 1 beginbfrange <00000000> <ffffffff> <0000000000000000> endbfrange endcmap";
        let map = Cmap::parse(data, &mut 0).unwrap();
        assert_eq!(map.entries[3].len(), 1);
        assert_eq!(map.get(1, 4).unwrap().as_deref(), Some("\0\0\0\u{1}"));
        assert_eq!(
            map.get(u32::MAX, 4).unwrap().as_deref(),
            Some("\0\0\u{ffff}\u{ffff}")
        );
    }
    #[test]
    fn malformed_or_executable_cmaps_do_not_become_replacement_strings() {
        for data in [
            "1 begincodespacerange <00> <ff> endcodespacerange 2 beginbfchar <41> <0043> endbfchar",
            "1 begincodespacerange <00> <ff> endcodespacerange 1 beginbfchar <41> <D800> endbfchar",
            "1 begincodespacerange <00> <ff> endcodespacerange 1 beginbfchar <41> <00> endbfchar",
            "1 begincodespacerange <00> <ff> endcodespacerange 1 beginbfrange <41> <42> [<0041>] endbfrange",
            "1 begincodespacerange <00> <ff> endcodespacerange 1 beginbfrange <41> <ff> <ffff> endbfrange",
            "1 begincodespacerange <00> <ff> endcodespacerange /External usecmap",
            "1 begincodespacerange <00> <ff> endcodespacerange /Anything run",
        ] {assert!(Cmap::parse(data.as_bytes(),&mut 0).is_err(),"{data}");}
    }
    #[test]
    fn supplementary_increment_and_invalid_later_utf16_are_strict() {
        let map = cmap("1 beginbfrange <41> <42> <D83DDE00> endbfrange");
        assert_eq!(map.get(0x41, 1).unwrap().as_deref(), Some("😀"));
        assert_eq!(map.get(0x42, 1).unwrap().as_deref(), Some("😁"));
        let map = cmap("1 beginbfrange <41> <42> <D7FF> endbfrange");
        assert!(map.get(0x42, 1).is_err());
    }
    #[test]
    fn unknown_glyph_names_never_accept_dependency_fallback_encoding() {
        for (name, expected) in [
            ("A", "A"),
            ("f_f_i", "ffi"),
            ("uni00410042", "AB"),
            ("u1F600", "😀"),
            ("parenleft.cap", "("),
            ("union", "∪"),
            ("universal", "∀"),
            ("uacute", "ú"),
            ("ugrave", "ù"),
            ("Lcommaaccent_uni20AC0308_u1040C.alternate", "Ļ€̈𐐌"),
        ] {
            assert_eq!(glyph_name(name.as_bytes()).as_deref(), Some(expected));
        }
        for name in [
            "nonsense_glyph",
            "uniD800",
            "u110000",
            "uni123",
            "uniD801DC0C",
            "uni20ac",
            "u1f600",
            "circlecopyrt",
            "square",
            "triangle",
            "A_unknown",
        ] {
            assert_eq!(glyph_name(name.as_bytes()), None);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn pinned_adobe_table_has_no_nonstandard_aliases_and_matches_its_provenance() {
        use sha2::{Digest, Sha256};
        let raw = include_bytes!("../../resources/adobe-glyphlist.txt");
        let pin: serde_json::Value =
            serde_json::from_str(include_str!("../../resources/adobe-glyphlist.json")).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(raw)),
            pin["sha256"].as_str().unwrap()
        );
        assert_eq!(adobe_glyph_names().len(), 4281);
        for (name, expected) in adobe_glyph_names() {
            let decoded = expected
                .split_whitespace()
                .map(|s| char::from_u32(u32::from_str_radix(s, 16).unwrap()).unwrap())
                .collect::<String>();
            assert_eq!(
                glyph_name(name.as_bytes()).as_deref(),
                Some(decoded.as_str()),
                "{name}"
            );
        }
    }

    #[test]
    fn glyph_name_expansion_and_shared_font_caches_charge_document_limits() {
        assert!(glyph_name(format!("uni{}", "20AC".repeat(342)).as_bytes()).is_none());
        assert!(glyph_name(&vec![b'A'; 4097]).is_none());
        let doc = Document::new();
        let font:Object=dictionary!{"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica","Encoding"=>dictionary!{"BaseEncoding"=>"StandardEncoding","Differences"=>vec![65.into(),Object::Name(format!("uni{}","0041".repeat(16)).into_bytes())]}}.into();
        let mut reader = Reader::new(&doc);
        reader.work = WORK - 300;
        let key = reader.font(&font, 1);
        let mut facts = Facts::default();
        reader.show(Some(key), b"A", &mut facts);
        assert!(facts.unavailable && facts.scalars.is_empty());
        let mut reader = Reader::new(&doc);
        reader.bytes = BYTES - 1;
        let key = reader.font(&font, 1);
        let mut facts = Facts::default();
        reader.show(Some(key), b"A", &mut facts);
        assert!(facts.unavailable && facts.scalars.is_empty());
        assert!(reader.bytes <= BYTES);
    }

    #[test]
    fn unicode_cmap_boundaries_are_independent_of_utf16_destinations() {
        assert_eq!(CodeEncoding::Utf8.code_len("日".as_bytes()).unwrap(), 3);
        assert_eq!(
            CodeEncoding::Utf16
                .code_len(&[0xd8, 0x3d, 0xde, 0])
                .unwrap(),
            4
        );
        assert_eq!(CodeEncoding::Utf32.decode(&[0, 1, 0xf6, 0]).unwrap(), "😀");
        assert!(CodeEncoding::Ucs2.code_len(&[0xd8, 0x3d, 0xde, 0]).is_err());
        assert!(CodeEncoding::Utf16
            .code_len(&[0xd8, 0x3d, 0x41, 0])
            .is_err());
        assert!(CodeEncoding::Utf8.code_len(&[0xc0, 0x80]).is_err());
        assert!(CodeEncoding::Utf32.code_len(&[0, 0x11, 0, 0]).is_err());
    }
    #[test]
    fn actual_text_strictly_decodes_utf16_utf8_and_pdfdoc_encoding() {
        let doc = Document::new();
        let mut reader = Reader::new(&doc);
        for bytes in [
            vec![0xfe, 0xff, 0xd8, 0x3d, 0xde, 0],
            vec![0xef, 0xbb, 0xbf, 0xf0, 0x9f, 0x98, 0x80],
        ] {
            let mut facts = Facts::default();
            reader.actual_text(
                &Object::String(bytes, lopdf::StringFormat::Hexadecimal),
                &mut facts,
            );
            assert!(!facts.unavailable);
            assert_eq!(facts.scalars, BTreeMap::from([('😀', 1)]));
        }
        let mut facts = Facts::default();
        reader.actual_text(
            &Object::String(vec![0xfe, 0xff, 0, 0x43, 0], lopdf::StringFormat::Literal),
            &mut facts,
        );
        assert!(facts.unavailable);
        assert!(facts.scalars.is_empty());
        let mut facts = Facts::default();
        reader.actual_text(
            &Object::String(vec![b'C', 0x95], lopdf::StringFormat::Literal),
            &mut facts,
        );
        assert!(!facts.unavailable);
        assert_eq!(facts.scalars.get(&'C'), Some(&1));
    }
    #[test]
    fn unicode_work_and_cache_limits_remain_explicit() {
        let doc = Document::new();
        let mut reader = Reader::new(&doc);
        reader.work = WORK;
        let mut facts = Facts::default();
        reader.show(None, b"C", &mut facts);
        assert!(facts.unavailable);
        let mut work = WORK;
        assert!(Cmap::parse(b"begin", &mut work).is_err());
    }
}
