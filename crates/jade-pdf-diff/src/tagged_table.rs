//! Optional page-local Table/TR/TH/TD membership from the public SDK API.
//!
//! No SDK address escapes the diagnostic page pass. Membership is never
//! inferred from font names/geometry.
//! Ambiguous, nested, cyclic or unsupported trees keep geometric extraction.
//! Exceeding the aggregate work budget fails explicitly without partial text.
use pdfium_render::prelude::Pdfium;
use std::{
    collections::{HashMap, HashSet},
    ffi::c_void,
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Ord, PartialOrd)]
pub(crate) struct Cell {
    pub table: u32,
    pub row: u32,
    pub ordinal: u32,
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(crate) struct Content {
    pub cell: Option<Cell>,
    pub outside: Option<i32>,
}

#[derive(Clone, Copy)]
pub(crate) struct Spans {
    pub rows: u32,
    pub columns: u32,
}

#[derive(Clone, Copy)]
pub(crate) struct Declaration {
    pub cell: Cell,
    pub observed: bool,
    pub spans: Option<Spans>,
}

#[derive(Default)]
pub(crate) struct Facts {
    pub membership: Option<Membership>,
    pub declarations: Vec<Declaration>,
    pub requires_review: bool,
}

#[derive(Default)]
pub(crate) struct Budget {
    used: usize,
    exceeded: bool,
}
impl Budget {
    pub fn charge(&mut self, amount: usize) -> bool {
        if self.exceeded {
            return false;
        }
        match self.used.checked_add(amount).filter(|&n| n <= 200_000) {
            Some(n) => {
                self.used = n;
                true
            }
            None => {
                self.exceeded = true;
                false
            }
        }
    }
    pub fn check(&self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.exceeded, "PDF tagged structure exceeds the diagnostic work budget. No partial text was returned.");
        Ok(())
    }
}

pub(crate) struct Membership {
    cells: HashMap<i32, Cell>,
    outside: HashSet<i32>,
    // Form XObjects have independent MCID namespaces. Only page-level text
    // objects can be associated by the page-scoped structure API used here.
    objects: HashSet<usize>,
}
impl Membership {
    pub fn content(&self, pdfium: &Pdfium, object: *mut c_void) -> Content {
        if !self.objects.contains(&(object as usize)) {
            return Content::default();
        }
        let id = pdfium
            .bindings()
            .FPDFPageObj_GetMarkedContentID(object.cast());
        Content {
            cell: self.cells.get(&id).copied(),
            outside: self.outside.contains(&id).then_some(id),
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Context {
    table: Option<u32>,
    row: Option<u32>,
    row_first_cell: u32,
    cell: Option<Cell>,
}

#[derive(Default)]
struct TableCounts {
    rows: u32,
    cells: u32,
}

struct Walk<'a> {
    pdfium: &'a Pdfium,
    budget: &'a mut Budget,
    seen: HashSet<usize>,
    tables: Vec<TableCounts>,
    ids: HashMap<i32, Option<Cell>>,
    handles: HashMap<Cell, *mut c_void>,
}
impl Walk<'_> {
    fn element(&mut self, handle: *mut c_void, mut context: Context, depth: usize) -> Option<()> {
        if handle.is_null()
            || depth >= 64
            || !self.budget.charge(1)
            || !self.seen.insert(handle as usize)
        {
            return None;
        }
        let b = self.pdfium.bindings();
        let mut bytes = [0u8; 64];
        let size = b.FPDF_StructElement_GetType(
            handle.cast(),
            bytes.as_mut_ptr().cast(),
            bytes.len() as _,
        ) as usize;
        // Only short standard roles are recognized; no allocation for names.
        let role = |name: &[u8]| {
            size == (name.len() + 1) * 2
                && size <= bytes.len()
                && bytes[size - 2..size] == [0, 0]
                && bytes[..size - 2]
                    .chunks_exact(2)
                    .zip(name)
                    .all(|(pair, &c)| pair == [c, 0])
        };
        if role(b"Table") {
            if context.table.is_some() {
                return None;
            }
            let table = u32::try_from(self.tables.len()).ok()?;
            self.tables.push(TableCounts::default());
            context = Context {
                table: Some(table),
                row: None,
                row_first_cell: 0,
                cell: None,
            };
        } else if role(b"TR") && context.table.is_some() {
            if context.row.is_some() || context.cell.is_some() {
                return None;
            }
            let table = &mut self.tables[context.table? as usize];
            context.row = Some(table.rows);
            table.rows = table.rows.checked_add(1)?;
            context.row_first_cell = table.cells;
        } else if (role(b"TH") || role(b"TD")) && context.table.is_some() {
            if context.cell.is_some() {
                return None;
            }
            let table = context.table?;
            let row = context.row?;
            let counts = &mut self.tables[table as usize];
            let ordinal = counts.cells.checked_sub(context.row_first_cell)?;
            counts.cells = counts.cells.checked_add(1)?;
            context.cell = Some(Cell {
                table,
                row,
                ordinal,
            });
            self.handles.insert(context.cell?, handle);
        }
        let count = b.FPDF_StructElement_CountChildren(handle.cast());
        if count < 0 || !self.budget.charge(count as usize) {
            return None;
        }
        for index in 0..count {
            let child = b.FPDF_StructElement_GetChildAtIndex(handle.cast(), index);
            if !child.is_null() {
                self.element(child.cast(), context, depth + 1)?;
            } else {
                // This API is explicitly scoped to the current page. Off-page
                // marked content and non-content children return -1.
                let id = b.FPDF_StructElement_GetChildMarkedContentID(handle.cast(), index);
                if id >= 0 {
                    // Blank row separators may be tagged directly under TR.
                    // Visible content in such regions invalidates the table
                    // later instead of being assigned to an arbitrary cell.
                    let cell = context.cell.or_else(|| {
                        context.table.map(|table| Cell {
                            table,
                            row: context.row.unwrap_or(u32::MAX),
                            ordinal: u32::MAX,
                        })
                    });
                    if let Some(previous) = self.ids.insert(id, cell) {
                        if previous != cell {
                            return None;
                        }
                    }
                }
            }
        }
        Some(())
    }
}

fn spans(pdfium: &Pdfium, element: *mut c_void, budget: &mut Budget) -> Option<Spans> {
    let b = pdfium.bindings();
    let count = b.FPDF_StructElement_GetAttributeCount(element.cast());
    if !budget.charge(1) {
        return None;
    }
    // The pinned SDK also returns -1 when /A is absent. Unit defaults are a
    // geometric hypothesis; recovery requires an explicitly spanning missing
    // cell and always requests review.
    if count == -1 {
        return Some(Spans {
            rows: 1,
            columns: 1,
        });
    }
    if count < 0 || !budget.charge((count as usize).saturating_mul(3)) {
        return None;
    }
    let mut rows = None;
    let mut columns = None;
    for index in 0..count {
        let attr = b.FPDF_StructElement_GetAttributeAtIndex(element.cast(), index);
        if attr.is_null() {
            return None;
        }
        let owner = b.FPDF_StructElement_Attr_GetValue(attr, "O");
        if owner.is_null() {
            continue;
        }
        let mut name = [0u8; 16];
        let mut length = 0;
        let ok = b.FPDF_StructElement_Attr_GetStringValue(
            owner,
            name.as_mut_ptr().cast(),
            name.len() as _,
            &mut length,
        );
        if ok == 0 || length != 12 || &name[..12] != b"T\0a\0b\0l\0e\0\0\0" {
            continue;
        }
        for (key, output) in [("RowSpan", &mut rows), ("ColSpan", &mut columns)] {
            let value = b.FPDF_StructElement_Attr_GetValue(attr, key);
            if value.is_null() {
                continue;
            }
            let mut number = 0.0f32;
            if b.FPDF_StructElement_Attr_GetNumberValue(value, &mut number) == 0
                || !number.is_finite()
                || number.fract() != 0.0
                || !(1.0..=64.0).contains(&number)
            {
                return None;
            }
            let number = number as u32;
            if output.is_some_and(|old| old != number) {
                return None;
            }
            *output = Some(number);
        }
    }
    Some(Spans {
        rows: rows.unwrap_or(1),
        columns: columns.unwrap_or(1),
    })
}

pub(crate) fn read(pdfium: &Pdfium, page: *mut c_void, budget: &mut Budget) -> Facts {
    let b = pdfium.bindings();
    let tree = b.FPDF_StructTree_GetForPage(page.cast());
    if tree.is_null() {
        return Facts::default();
    }
    let result = (|| {
        let count = b.FPDF_StructTree_CountChildren(tree);
        if count < 0 || !budget.charge(count as usize) {
            return None;
        }
        let mut walk = Walk {
            pdfium,
            budget,
            seen: HashSet::new(),
            tables: Vec::new(),
            ids: HashMap::new(),
            handles: HashMap::new(),
        };
        for index in 0..count {
            walk.element(
                b.FPDF_StructTree_GetChildAtIndex(tree, index).cast(),
                Context::default(),
                0,
            )?;
        }
        let mut cells = HashMap::new();
        let mut outside = HashSet::new();
        for (id, cell) in walk.ids {
            if let Some(cell) = cell {
                cells.insert(id, cell);
            } else {
                outside.insert(id);
            }
        }
        if cells.is_empty() {
            return Some(Facts::default());
        }
        let count = b.FPDFPage_CountObjects(page.cast());
        if count < 0 || !walk.budget.charge(count as usize) {
            return None;
        }
        let mut objects = HashSet::new();
        let mut observed = HashSet::new();
        for index in 0..count {
            let object = b.FPDFPage_GetObject(page.cast(), index);
            if object.is_null() {
                return None;
            }
            if b.FPDFPageObj_GetType(object) == 1 {
                objects.insert(object as usize);
                let id = b.FPDFPageObj_GetMarkedContentID(object);
                if let Some(cell) = cells.get(&id) {
                    observed.insert(*cell);
                }
            }
        }
        let declared: HashSet<_> = cells
            .values()
            .copied()
            .filter(|c| c.ordinal != u32::MAX)
            .collect();
        let missing_tables: HashSet<_> = declared
            .iter()
            .filter(|c| !observed.contains(c))
            .map(|c| c.table)
            .collect();
        // Outside marks are needed only for a missing-cell hypothesis.
        // Ordinary complete tables keep the previous diagnostic span cost.
        if missing_tables.is_empty() {
            outside.clear();
        }
        let mut declarations = Vec::new();
        for cell in declared {
            if missing_tables.contains(&cell.table) {
                let span = walk
                    .handles
                    .get(&cell)
                    .and_then(|&handle| spans(pdfium, handle, walk.budget));
                declarations.push(Declaration {
                    cell,
                    observed: observed.contains(&cell),
                    spans: span,
                });
            }
        }
        Some(Facts {
            membership: Some(Membership {
                cells,
                outside,
                objects,
            }),
            declarations,
            requires_review: false,
        })
    })();
    b.FPDF_StructTree_Close(tree);
    result.unwrap_or(Facts {
        requires_review: true,
        ..Facts::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exhausted_or_overflowing_tag_work_cannot_become_a_partial_success() {
        let mut budget = Budget::default();
        assert!(budget.charge(200_000));
        assert!(budget.check().is_ok());
        assert!(!budget.charge(1));
        assert!(budget.check().is_err());
        assert!(!budget.charge(0));
        let mut budget = Budget::default();
        assert!(budget.charge(1));
        assert!(!budget.charge(usize::MAX));
        assert!(budget.check().is_err());
    }
}
