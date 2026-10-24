//! Direct SDK stream inventory for the separately fetched public corpus.
//! This checks conservation/diagnostic transport, not independent Unicode truth.
use anyhow::{ensure, Result};
use pdfium_render::prelude::Pdfium;

pub fn pages(pdfium: &Pdfium, bytes: &[u8]) -> Result<Vec<(String, usize)>> {
    let b = pdfium.bindings();
    let document = b.FPDF_LoadMemDocument64(bytes, None);
    ensure!(!document.is_null(), "Failed to open SDK inventory source");
    let result = (|| {
        let count = b.FPDF_GetPageCount(document);
        ensure!(
            (1..=5000).contains(&count),
            "SDK inventory page count exceeded"
        );
        let mut pages = Vec::new();
        for number in 0..count {
            let page = b.FPDF_LoadPage(document, number);
            ensure!(!page.is_null(), "Failed to load inventory page");
            let page_result = (|| {
                let text = b.FPDFText_LoadPage(page);
                ensure!(!text.is_null(), "Failed to load inventory text");
                let text_result = (|| {
                    let count = b.FPDFText_CountChars(text);
                    ensure!(
                        (0..=2_000_000).contains(&count),
                        "SDK inventory character count exceeded"
                    );
                    let mut output = String::new();
                    let mut errors = 0;
                    let mut paired_low = None;
                    for index in 0..count {
                        let scalar = char::from_u32(b.FPDFText_GetUnicode(text, index));
                        let error = b.FPDFText_HasUnicodeMapError(text, index);
                        ensure!(error == 0 || error == 1, "Invalid SDK mapping flag");
                        errors += usize::from(error == 1);
                        if paired_low == Some(index) {
                            paired_low = None;
                            continue;
                        }
                        let raw = b.FPDFText_GetUnicode(text, index);
                        let scalar = if (0xd800..=0xdbff).contains(&raw) && index + 1 < count {
                            let next = b.FPDFText_GetUnicode(text, index + 1);
                            // The reference decoder uses Rust's strict UTF-16
                            // decoder and independently checks source identity
                            // and geometry before treating two records as one.
                            let object = b.FPDFText_GetTextObject(text, index);
                            let mut bounds = Vec::new();
                            for at in [index, index + 1] {
                                let (mut l, mut r, mut y0, mut y1) = (0.0, 0.0, 0.0, 0.0);
                                let (mut x, mut y) = (0.0, 0.0);
                                let ok = b.FPDFText_GetCharBox(
                                    text, at, &mut l, &mut r, &mut y0, &mut y1,
                                ) != 0
                                    && b.FPDFText_GetCharOrigin(text, at, &mut x, &mut y) != 0;
                                bounds.push((
                                    ok,
                                    [l as f32, r as f32, y0 as f32, y1 as f32, x as f32, y as f32],
                                ));
                            }
                            let same = !object.is_null()
                                && object == b.FPDFText_GetTextObject(text, index + 1)
                                && (0xdc00..=0xdfff).contains(&next)
                                && [index, index + 1]
                                    .into_iter()
                                    .all(|i| b.FPDFText_IsGenerated(text, i) == 0)
                                && bounds
                                    .iter()
                                    .all(|(ok, v)| *ok && v.iter().all(|x| x.is_finite()))
                                && bounds[0].1 == bounds[1].1;
                            if same {
                                paired_low = Some(index + 1);
                                Some(
                                    char::decode_utf16([raw as u16, next as u16])
                                        .next()
                                        .unwrap()
                                        .unwrap(),
                                )
                            } else {
                                scalar
                            }
                        } else {
                            scalar
                        };
                        if error == 1
                            && scalar.is_none_or(|c| {
                                c.is_control() || c.is_whitespace() || c == '\u{ad}'
                            })
                        {
                            output.push('\u{fffd}');
                        } else if b.FPDFText_IsHyphen(text, index) == 1 {
                            output.push('-');
                        } else if let Some(scalar) = scalar {
                            output.push(scalar);
                        } else if b.FPDFText_IsGenerated(text, index) == 0 {
                            let (mut l, mut r, mut y0, mut y1) = (0.0, 0.0, 0.0, 0.0);
                            if b.FPDFText_GetCharBox(text, index, &mut l, &mut r, &mut y0, &mut y1)
                                != 0
                                && [l, r, y0, y1].into_iter().all(f64::is_finite)
                                && l != r
                                && y0 != y1
                            {
                                output.push('\u{fffd}');
                            }
                        }
                    }
                    Ok((output, errors))
                })();
                b.FPDFText_ClosePage(text);
                text_result
            })();
            b.FPDF_ClosePage(page);
            pages.push(page_result?);
        }
        Ok(pages)
    })();
    b.FPDF_CloseDocument(document);
    result
}
