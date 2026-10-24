//! Check the live SDK bridge before accepting PDF work in a browser.
use anyhow::{ensure, Context, Result};
use pdfium_render::prelude::Pdfium;

const CONTRACT_ERROR: &str = "PDFIUM_WASM_ABI_MISMATCH: Pdfium character-box coordinates are incompatible; call preparePdfiumForJade() before initialize_pdfium_render()";

pub(crate) fn bind() -> Result<Pdfium> {
    let pdfium = Pdfium::bind_to_system_library().map(Pdfium::new)?;
    // Do not cache a successful check: initialize_pdfium_render() can rebind
    // the same application instance to a different SDK or ccall implementation.
    check_character_box(&pdfium).context(CONTRACT_ERROR)?;
    check_actual_text_blob(&pdfium).context("PDFIUM_WASM_ABI_MISMATCH: ActualText raw-byte bridge is incompatible; load the current preparePdfiumForJade() adapter")?;
    Ok(pdfium)
}

fn check_character_box(pdfium: &Pdfium) -> Result<()> {
    // One fixed-size, first-party Type3 glyph: no substitute/system fonts,
    // external resources, source-dependent work, or rendering allocations.
    let bytes = crate::sdk_probes::character_box();
    let document = pdfium.load_pdf_from_byte_slice(&bytes, None)?;
    ensure!(document.pages().len() == 1, "Invalid SDK probe page count");
    let page = document.pages().get(0)?;
    let text = page.text()?;
    let chars = text.chars();
    ensure!(chars.len() == 1, "Invalid SDK probe character count");
    let bounds = chars.get(0)?.tight_bounds()?;
    // 400x700 font units, FontMatrix=.001, font size=20, origin=(60,100).
    // Distinct coordinates detect the upstream wrapper's output-slot mix-up
    // before it can silently corrupt real character/highlight geometry.
    ensure!(
        [
            (bounds.left().value, 60.0),
            (bounds.bottom().value, 100.0),
            (bounds.right().value, 68.0),
            (bounds.top().value, 114.0),
        ]
        .into_iter()
        .all(|(actual, expected)| (actual - expected).abs() <= 0.001),
        "Invalid SDK probe character bounds"
    );
    Ok(())
}

fn check_actual_text_blob(pdfium: &Pdfium) -> Result<()> {
    // A fixed, first-party marked vector. Its PDF string has an odd UTF-16
    // tail, so a Unicode string decoder cannot pass as an exact raw-byte read.
    let bytes = crate::sdk_probes::actual_text();
    let b = pdfium.bindings();
    let document = b.FPDF_LoadMemDocument64(&bytes, None);
    ensure!(!document.is_null(), "Failed to load ActualText SDK probe");
    let page = b.FPDF_LoadPage(document, 0);
    let result = (|| {
        ensure!(
            !page.is_null() && b.FPDFPage_CountObjects(page) == 1,
            "Invalid ActualText SDK probe page"
        );
        let object = b.FPDFPage_GetObject(page, 0);
        ensure!(
            !object.is_null() && b.FPDFPageObj_CountMarks(object) == 1,
            "Invalid ActualText SDK probe object"
        );
        let mark = b.FPDFPageObj_GetMark(object, 0);
        ensure!(!mark.is_null(), "Invalid ActualText SDK probe mark");
        let mut budget = crate::actual_text::Budget::default();
        ensure!(
            crate::actual_text::marked_bytes(pdfium, mark.cast(), &mut budget)?.as_deref()
                == Some(&[0xfe, 0xff, 0x00, 0x41, 0x00]),
            "Invalid ActualText SDK probe bytes"
        );
        Ok(())
    })();
    if !page.is_null() {
        b.FPDF_ClosePage(page);
    }
    b.FPDF_CloseDocument(document);
    result
}
