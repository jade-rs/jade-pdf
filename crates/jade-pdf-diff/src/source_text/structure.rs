//! Source-side ActualText ownership. MCIDs are bound only after page/Form
//! namespaces and ParentTree back-pointers agree, and each source occurrence is
//! unique on this page. Ambiguous or cross-page replacements remain reviewable.
use anyhow::{ensure, Context, Result};
use lopdf::{content::Content, Dictionary, Document, Object, ObjectId};
use std::collections::{HashMap, HashSet};

const WORK: usize = 200_000;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(super) struct Reference {
    pub page: ObjectId,
    pub owner: ObjectId,
    pub mcid: i32,
    pub leaf: ObjectId,
}
#[derive(Clone, Default)]
pub(super) struct Group {
    pub text: String,
    pub refs: Vec<Reference>,
    pub invalid: bool,
}
#[derive(Default)]
pub(super) struct Index {
    groups: Vec<Group>,
    unknown: bool,
    pages: HashMap<ObjectId, Vec<usize>>,
}
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(crate) struct MarkedSignature {
    pub mcids: Vec<i32>,
    pub text: String,
}

#[derive(Clone, Default)]
pub(crate) struct Plan {
    pub groups: Vec<Replacement>,
    pub ids: HashMap<i32, usize>,
    pub unavailable: bool,
    pub marked_actual: bool,
    pub marked_scopes: HashMap<MarkedSignature, usize>,
}
#[derive(Clone, Default)]
pub(crate) struct Replacement {
    pub text: String,
    pub invalid: bool,
    pub(super) refs: Vec<Reference>,
}

struct Tree<'a> {
    doc: &'a Document,
    steps: usize,
    bytes: usize,
    seen: HashSet<ObjectId>,
    groups: Vec<Group>,
    unknown: bool,
}
impl<'a> Tree<'a> {
    fn charge(&mut self, n: usize) -> Result<()> {
        self.steps = self.steps.saturating_add(n);
        ensure!(
            self.steps <= WORK,
            "Source ActualText structure exceeds work budget"
        );
        Ok(())
    }
    fn resolve(&mut self, mut o: &'a Object) -> Result<&'a Object> {
        let mut seen = HashSet::new();
        while let Object::Reference(id) = o {
            self.charge(1)?;
            ensure!(
                seen.len() < 64 && seen.insert(*id),
                "Cyclic ActualText structure reference"
            );
            o = self
                .doc
                .objects
                .get(id)
                .context("Missing ActualText structure object")?;
        }
        Ok(o)
    }
    fn walk(
        &mut self,
        o: &'a Object,
        page: Option<ObjectId>,
        parent: Option<ObjectId>,
        active: Option<usize>,
        ancestry_valid: bool,
        depth: usize,
    ) -> Result<()> {
        self.charge(1)?;
        ensure!(
            depth < 64,
            "Source ActualText structure nesting exceeds budget"
        );
        let indirect = o.as_reference().ok();
        if let Some(id) = indirect {
            if !self.seen.insert(id) {
                if let Some(g) = active {
                    self.groups[g].invalid = true;
                }
                self.unknown = true;
                return Ok(());
            }
        }
        match self.resolve(o)? {
            Object::Array(a) => {
                for o in a {
                    self.walk(o, page, parent, active, ancestry_valid, depth + 1)?;
                }
            }
            Object::Integer(id) => {
                if let Some(g) = active {
                    self.content_ref(g, page, None, *id, parent)?;
                }
            }
            Object::Dictionary(d) => {
                let page = d
                    .get(b"Pg")
                    .ok()
                    .map(Object::as_reference)
                    .transpose()?
                    .or(page);
                if d.get(b"Type").and_then(Object::as_name).ok() == Some(b"MCR".as_slice())
                    || d.has(b"MCID")
                {
                    if let Some(g) = active {
                        if d.has(b"StmOwn") {
                            self.groups[g].invalid = true;
                        }
                        let owner = d.get(b"Stm").ok().map(Object::as_reference).transpose()?;
                        let mcid = self.resolve(d.get(b"MCID")?)?.as_i64()?;
                        self.content_ref(g, page, owner, mcid, parent)?;
                    }
                    return Ok(());
                }
                if d.get(b"Type").and_then(Object::as_name).ok() == Some(b"OBJR".as_slice()) {
                    if let Some(g) = active {
                        self.groups[g].invalid = true;
                    }
                    return Ok(());
                }
                let parent_valid = ancestry_valid
                    && indirect.is_some()
                    && d.get(b"P").ok().and_then(|o| o.as_reference().ok()) == parent;
                let mut group = active;
                if let Ok(value) = d.get(b"ActualText") {
                    let text = super::unicode::decode_actual(self.doc, value)?;
                    self.bytes = self.bytes.saturating_add(text.len());
                    ensure!(
                        self.bytes <= 2 * 1024 * 1024,
                        "Structure ActualText exceeds string budget"
                    );
                    if let Some(g) = active {
                        self.groups[g].invalid = true;
                    }
                    group = Some(self.groups.len());
                    self.groups.push(Group {
                        text,
                        refs: Vec::new(),
                        invalid: active.is_some() || !parent_valid,
                    });
                }
                if group.is_some() {
                    let actual_parent = d.get(b"P").ok().and_then(|o| o.as_reference().ok());
                    if indirect.is_none() || actual_parent != parent {
                        if let Some(g) = group {
                            self.groups[g].invalid = true;
                        }
                    }
                }
                if let Ok(k) = d.get(b"K") {
                    self.walk(k, page, indirect, group, parent_valid, depth + 1)?;
                }
            }
            Object::Null => {}
            _ => {
                if let Some(g) = active {
                    self.groups[g].invalid = true;
                }
            }
        }
        Ok(())
    }
    fn content_ref(
        &mut self,
        g: usize,
        page: Option<ObjectId>,
        stream: Option<ObjectId>,
        mcid: i64,
        leaf: Option<ObjectId>,
    ) -> Result<()> {
        let Some(page) = page else {
            self.groups[g].invalid = true;
            return Ok(());
        };
        let Some(leaf) = leaf else {
            self.groups[g].invalid = true;
            return Ok(());
        };
        ensure!(
            (0..WORK as i64).contains(&mcid),
            "ActualText MCID exceeds source work budget"
        );
        ensure!(
            self.doc.get_dictionary(page)?.get(b"Type")?.as_name()? == b"Page",
            "ActualText refers to a non-page"
        );
        let owner = if let Some(stream) = stream {
            let object = self.doc.get_object(stream)?.as_stream()?;
            if object.dict.get(b"Subtype").and_then(Object::as_name).ok()
                == Some(b"Form".as_slice())
            {
                stream
            } else {
                ensure!(
                    self.doc.get_page_contents(page).contains(&stream),
                    "ActualText stream does not belong to page"
                );
                page
            }
        } else {
            page
        };
        self.groups[g].refs.push(Reference {
            page,
            owner,
            mcid: mcid as i32,
            leaf,
        });
        Ok(())
    }
    fn parents(
        &mut self,
        o: &'a Object,
        out: &mut HashMap<i64, &'a Object>,
        seen: &mut HashSet<ObjectId>,
        depth: usize,
    ) -> Result<()> {
        self.charge(1)?;
        ensure!(depth < 64, "ActualText ParentTree nesting exceeds budget");
        if let Ok(id) = o.as_reference() {
            ensure!(seen.insert(id), "Cyclic ActualText ParentTree");
        }
        let d = self.resolve(o)?.as_dict()?;
        ensure!(!(d.has(b"Kids") && d.has(b"Nums")), "Mixed ParentTree node");
        if let Ok(nums) = d.get(b"Nums") {
            let a = self.resolve(nums)?.as_array()?;
            self.charge(a.len())?;
            ensure!(a.len() % 2 == 0, "Invalid ParentTree pairs");
            for p in a.chunks_exact(2) {
                let key = p[0].as_i64()?;
                ensure!(
                    key >= 0 && out.insert(key, &p[1]).is_none(),
                    "Repeated ParentTree key"
                );
            }
        }
        if let Ok(kids) = d.get(b"Kids") {
            for k in self.resolve(kids)?.as_array()? {
                self.parents(k, out, seen, depth + 1)?;
            }
        }
        Ok(())
    }
    fn validate(&mut self, root: &'a Dictionary) -> Result<()> {
        if self.groups.is_empty() {
            return Ok(());
        }
        let mut parents = HashMap::new();
        self.parents(
            root.get(b"ParentTree")?,
            &mut parents,
            &mut HashSet::new(),
            0,
        )?;
        let mut ownership = HashMap::new();
        for g in 0..self.groups.len() {
            let refs = self.groups[g].refs.clone();
            if refs.is_empty() {
                self.groups[g].invalid = true;
                self.unknown = true;
                continue;
            }
            let pages: HashSet<_> = refs.iter().map(|r| r.page).collect();
            if pages.len() != 1 {
                self.groups[g].invalid = true;
            }
            for r in refs {
                self.charge(1)?;
                let owner = self.doc.get_object(r.owner)?;
                let dict = match owner {
                    Object::Dictionary(d) => d,
                    Object::Stream(s) => &s.dict,
                    _ => anyhow::bail!("Invalid ActualText content owner"),
                };
                let key = dict.get(b"StructParents")?.as_i64()?;
                let array = self
                    .resolve(
                        parents
                            .get(&key)
                            .context("Missing ActualText ParentTree entry")?,
                    )?
                    .as_array()?;
                ensure!(
                    array.len() <= WORK,
                    "ActualText ParentTree array exceeds budget"
                );
                let parent = array
                    .get(r.mcid as usize)
                    .context("Missing ActualText parent back-pointer")?
                    .as_reference()?;
                if parent != r.leaf {
                    self.groups[g].invalid = true;
                }
                let key = (r.page, r.owner, r.mcid);
                if let Some(other) = ownership.insert(key, g) {
                    self.groups[g].invalid = true;
                    self.groups[other].invalid = true;
                }
            }
        }
        Ok(())
    }
}
impl Index {
    pub fn read(doc: &Document, pages: &[ObjectId]) -> Self {
        let mut tree = Tree {
            doc,
            steps: 0,
            bytes: 0,
            seen: HashSet::new(),
            groups: Vec::new(),
            unknown: false,
        };
        let live_pages: HashSet<_> = pages.iter().copied().collect();
        let result = (|| -> Result<()> {
            let Ok(catalog) = doc.catalog() else {
                return Ok(());
            };
            let Ok(value) = catalog.get(b"StructTreeRoot") else {
                return Ok(());
            };
            if matches!(value, Object::Null) {
                return Ok(());
            }
            let id = value.as_reference().ok();
            let root = tree.resolve(value)?.as_dict()?;
            if let Ok(k) = root.get(b"K") {
                tree.walk(k, None, id, None, true, 0)?;
            }
            tree.validate(root)?;
            ensure!(
                tree.groups
                    .iter()
                    .flat_map(|g| g.refs.iter())
                    .all(|r| live_pages.contains(&r.page)),
                "ActualText refers to a page outside the validated tree"
            );
            Ok(())
        })();
        if result.is_err() {
            for group in &mut tree.groups {
                group.invalid = true;
            }
        }
        let mut pages: HashMap<ObjectId, Vec<usize>> = HashMap::new();
        for (index, group) in tree.groups.iter().enumerate() {
            for page in group.refs.iter().map(|r| r.page).collect::<HashSet<_>>() {
                pages.entry(page).or_default().push(index);
            }
        }
        Self {
            pages,
            unknown: result.is_err() || (!tree.groups.is_empty() && tree.unknown),
            groups: tree.groups,
        }
    }
    pub fn for_page(&self, page: ObjectId) -> Plan {
        let groups = self
            .pages
            .get(&page)
            .into_iter()
            .flatten()
            .map(|&index| &self.groups[index])
            .map(|g| Replacement {
                text: if g.invalid {
                    String::new()
                } else {
                    g.text.clone()
                },
                invalid: g.invalid,
                refs: g.refs.iter().copied().filter(|r| r.page == page).collect(),
            })
            .collect();
        Plan {
            groups,
            unavailable: self.unknown,
            ..Plan::default()
        }
    }
}

/// This independent source walk counts occurrences across *all* namespaces,
/// including non-replacement marked content, before SDK MCIDs may be matched.
pub(super) struct Scan<'a> {
    doc: &'a Document,
    steps: usize,
    bytes: usize,
    counts: HashMap<i32, (ObjectId, usize)>,
    forms: HashSet<ObjectId>,
}
impl<'a> Scan<'a> {
    pub fn run(
        doc: &'a Document,
        page: ObjectId,
        data: &[u8],
        resources: Option<&'a Dictionary>,
        plan: &mut Plan,
    ) {
        if plan.groups.is_empty() {
            return;
        }
        let mut scan = Self {
            doc,
            steps: 0,
            bytes: 0,
            counts: HashMap::new(),
            forms: HashSet::new(),
        };
        if scan.walk(page, data, resources).is_err() {
            plan.unavailable = true;
            for group in &mut plan.groups {
                group.invalid = true;
            }
            return;
        }
        for (index, g) in plan.groups.iter_mut().enumerate() {
            if g.refs
                .iter()
                .any(|r| scan.counts.get(&r.mcid) != Some(&(r.owner, 1)))
            {
                g.invalid = true;
            }
            if g.invalid {
                plan.unavailable = true;
                continue;
            }
            for r in &g.refs {
                if let Some(other) = plan.ids.insert(r.mcid, index) {
                    if other != index {
                        plan.unavailable = true;
                    }
                }
            }
        }
    }
    fn charge(&mut self, n: usize) -> Result<()> {
        self.steps = self.steps.saturating_add(n);
        ensure!(
            self.steps <= 2_000_000,
            "ActualText source occurrence scan exceeds budget"
        );
        Ok(())
    }
    fn resolve(&mut self, mut o: &'a Object) -> Result<&'a Object> {
        let mut seen = HashSet::new();
        while let Object::Reference(id) = o {
            self.charge(1)?;
            ensure!(
                seen.len() < 64 && seen.insert(*id),
                "Cyclic ActualText resource"
            );
            o = self
                .doc
                .objects
                .get(id)
                .context("Missing ActualText resource")?;
        }
        Ok(o)
    }
    fn walk(
        &mut self,
        owner: ObjectId,
        data: &[u8],
        resources: Option<&'a Dictionary>,
    ) -> Result<()> {
        ensure!(
            self.forms.len() < 32,
            "ActualText Form nesting exceeds budget"
        );
        self.bytes = self.bytes.saturating_add(data.len());
        ensure!(
            data.len() <= 2 * 1024 * 1024 && self.bytes <= 64 * 1024 * 1024,
            "ActualText source scan exceeds decoded byte budget"
        );
        let content = Content::decode_strict(data)?;
        self.charge(content.operations.len())?;
        for op in content.operations {
            if op.operator == "BDC" && op.operands.len() == 2 {
                let p = match &op.operands[1] {
                    Object::Dictionary(d) => Some(d),
                    Object::Name(n) => {
                        let d = self
                            .resolve(
                                resources
                                    .context("Missing marked properties")?
                                    .get(b"Properties")?,
                            )?
                            .as_dict()?;
                        Some(self.resolve(d.get(n)?)?.as_dict()?)
                    }
                    _ => None,
                };
                if let Some(p) = p {
                    if let Ok(id) = p.get(b"MCID") {
                        let id = id.as_i64()?;
                        ensure!(
                            (0..WORK as i64).contains(&id),
                            "ActualText source MCID exceeds budget"
                        );
                        let entry = self.counts.entry(id as i32).or_insert((owner, 0));
                        entry.1 += 1;
                        if entry.0 != owner {
                            entry.1 = entry.1.max(2);
                        }
                    }
                }
            } else if op.operator == "Do" && op.operands.len() == 1 {
                let r = resources.context("Missing Form resources")?;
                let x = self.resolve(r.get(b"XObject")?)?.as_dict()?;
                let reference = x.get(op.operands[0].as_name()?)?;
                let object = self.resolve(reference)?.as_stream()?;
                if object.dict.get(b"Subtype").and_then(Object::as_name).ok()
                    == Some(b"Form".as_slice())
                {
                    let id = reference.as_reference()?;
                    ensure!(self.forms.insert(id), "Cyclic ActualText Form");
                    let nested = match object.dict.get(b"Resources") {
                        Ok(v) => Some(self.resolve(v)?.as_dict()?),
                        Err(_) => resources,
                    };
                    if object.dict.has(b"Filter") {
                        object.filters()?;
                    }
                    let bytes = object.decompressed_content_with_limit(2 * 1024 * 1024)?;
                    self.walk(id, &bytes, nested)?;
                    self.forms.remove(&id);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
use crate::appearance_fixture as fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    fn doc() -> Document {
        Document::load_mem(&fixture::structure_actual(1.0, 0, "page", true)).unwrap()
    }
    fn plan(doc: &Document) -> Plan {
        let mut reader = super::super::Reader::new(doc, &[(3, 0)]);
        reader.page_facts((3, 0)).unwrap().actual
    }
    #[test]
    fn number_tree_kids_and_content_stream_references_keep_valid_ownership() {
        let mut d = doc();
        d.objects.insert(
            (8, 0),
            Object::Dictionary(dictionary! { "Kids" => vec![Object::Reference((14, 0))] }),
        );
        d.objects.insert((14, 0), Object::Dictionary(dictionary! { "Nums" => vec![Object::Integer(0), Object::Array(vec![Object::Reference((7, 0))])] }));
        d.get_object_mut((7, 0))
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set(
                "K",
                Object::Dictionary(
                    dictionary! {"Type" => "MCR", "Pg" => (3, 0), "Stm" => (5, 0), "MCID" => 0},
                ),
            );
        let p = plan(&d);
        assert!(!p.unavailable);
        assert_eq!(p.ids.get(&0), Some(&0));
        assert_eq!(p.groups[0].text, "B");
    }
    #[test]
    fn cyclic_or_repeated_parent_tree_nodes_invalidate_all_replacements() {
        for repeated in [false, true] {
            let mut d = doc();
            let kids = if repeated {
                vec![Object::Reference((14, 0)); 2]
            } else {
                vec![Object::Reference((8, 0))]
            };
            d.objects
                .insert((8, 0), Object::Dictionary(dictionary! { "Kids" => kids }));
            d.objects.insert((14, 0), Object::Dictionary(dictionary! { "Nums" => vec![Object::Integer(0), Object::Array(vec![Object::Reference((7, 0))])] }));
            let p = plan(&d);
            assert!(p.unavailable);
            assert!(p.ids.is_empty());
            assert!(p.groups.iter().all(|g| g.invalid));
        }
    }
    #[test]
    fn source_ownership_does_not_include_detached_pages_or_object_references() {
        let mut d = doc();
        d.get_object_mut((7, 0))
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set(
                "K",
                Object::Dictionary(dictionary! {"Type" => "OBJR", "Obj" => (4, 0)}),
            );
        let p = plan(&d);
        assert!(p.unavailable);
        assert!(p.ids.is_empty());
        let d = doc();
        let index = Index::read(&d, &[(11, 0)]);
        assert!(index.unknown);
        assert!(index.groups[0].invalid);
    }
    #[test]
    fn bounded_walks_never_publish_partial_valid_replacements() {
        let d = doc();
        let mut tree = Tree {
            doc: &d,
            steps: WORK,
            bytes: 0,
            seen: HashSet::new(),
            groups: Vec::new(),
            unknown: false,
        };
        assert!(tree
            .walk(
                &Object::Integer(0),
                Some((3, 0)),
                Some((7, 0)),
                None,
                true,
                0
            )
            .is_err());
        let mut scan = Scan {
            doc: &d,
            steps: 2_000_000,
            bytes: 0,
            counts: HashMap::new(),
            forms: HashSet::new(),
        };
        assert!(scan
            .walk((3, 0), b"/Span << /MCID 0 >> BDC EMC", None)
            .is_err());
        let mut nested = doc();
        let mut kid = Object::Integer(0);
        for _ in 0..65 {
            kid = Object::Array(vec![kid]);
        }
        nested
            .get_object_mut((7, 0))
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("K", kid);
        let p = plan(&nested);
        assert!(p.unavailable);
        assert!(p.ids.is_empty());
    }
}
