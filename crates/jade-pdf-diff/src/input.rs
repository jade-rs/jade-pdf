//! Check the full Pdfium page count before pdfium-render narrows it to u16.

use anyhow::{anyhow, ensure, Result};
use pdfium_render::prelude::{PdfDocument, Pdfium};

use crate::Limits;

fn open_error(code: u64) -> anyhow::Error {
    // Stable FPDF_ERR_* values in Pdfium's public fpdfview.h.
    match code {
        4 => anyhow!("PDF requires a password or valid credentials; password-protected PDFs are not supported by this API"),
        5 => anyhow!("PDF uses an unsupported security scheme"),
        3 => anyhow!("Pdfium could not open the PDF: invalid or damaged PDF format"),
        _ => anyhow!("Pdfium could not open the PDF (error {code})"),
    }
}

fn check_page_count(count: i32, limits: &Limits) -> Result<()> {
    ensure!(count > 0, "PDF has no pages or has an invalid page count");
    ensure!(
        count as usize <= limits.max_pages && count <= i32::from(u16::MAX),
        "PDF has {count} pages, exceeding max_pages {} or the backend limit {}. No partial comparison was performed.",
        limits.max_pages, u16::MAX
    );
    Ok(())
}

pub(crate) fn load_bytes<'a>(
    pdfium: &'a Pdfium,
    bytes: &'a [u8],
    limits: &Limits,
) -> Result<PdfDocument<'a>> {
    load_bytes_with(pdfium, bytes, limits, |_| Ok(())).map(|(document, ())| document)
}

pub(crate) fn load_bytes_with<'a, T>(
    pdfium: &'a Pdfium,
    bytes: &'a [u8],
    limits: &Limits,
    action: impl FnOnce(*mut std::ffi::c_void) -> Result<T>,
) -> Result<(PdfDocument<'a>, T)> {
    let (count, data) = with_raw_bytes(pdfium, bytes, limits, |handle| {
        Ok((
            pdfium.bindings().FPDF_GetPageCount(handle.cast()),
            action(handle)?,
        ))
    })?;
    let document = pdfium.load_pdf_from_byte_slice(bytes, None)?;
    ensure!(
        usize::from(document.pages().len()) == count as usize,
        "PDF page count changed while opening"
    );
    Ok((document, data))
}

pub(crate) fn with_raw_bytes<T>(
    pdfium: &Pdfium,
    bytes: &[u8],
    limits: &Limits,
    action: impl FnOnce(*mut std::ffi::c_void) -> Result<T>,
) -> Result<T> {
    limits.validate()?;
    crate::scan::ensure_size(bytes.len() as u64, limits)?;
    let bindings = pdfium.bindings();
    let handle = bindings.FPDF_LoadMemDocument64(bytes, None);
    if handle.is_null() {
        #[allow(clippy::unnecessary_cast)]
        return Err(open_error(bindings.FPDF_GetLastError() as u64));
    }
    let result = check_page_count(bindings.FPDF_GetPageCount(handle), limits)
        .and_then(|()| action(handle.cast()));
    bindings.FPDF_CloseDocument(handle);
    result
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn load_file<'a>(
    pdfium: &'a Pdfium,
    path: &std::path::Path,
    limits: &Limits,
) -> Result<PdfDocument<'a>> {
    load_file_with(pdfium, path, limits, |_| Ok(())).map(|(document, ())| document)
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn load_file_with<'a, T>(
    pdfium: &'a Pdfium,
    path: &std::path::Path,
    limits: &Limits,
    action: impl FnOnce(*mut std::ffi::c_void) -> Result<T>,
) -> Result<(PdfDocument<'a>, T)> {
    let (file, (count, data)) = with_owned_file(pdfium, path, limits, |handle| {
        Ok((
            pdfium.bindings().FPDF_GetPageCount(handle.cast()),
            action(handle)?,
        ))
    })?;
    let document = pdfium.load_pdf_from_reader(file, None)?;
    ensure!(
        usize::from(document.pages().len()) == count as usize,
        "PDF page count changed while opening"
    );
    Ok((document, data))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn with_raw_file<T>(
    pdfium: &Pdfium,
    path: &std::path::Path,
    limits: &Limits,
    action: impl FnOnce(*mut std::ffi::c_void) -> Result<T>,
) -> Result<T> {
    with_owned_file(pdfium, path, limits, action).map(|(_, result)| result)
}

#[cfg(not(target_arch = "wasm32"))]
fn with_owned_file<T>(
    pdfium: &Pdfium,
    path: &std::path::Path,
    limits: &Limits,
    action: impl FnOnce(*mut std::ffi::c_void) -> Result<T>,
) -> Result<(std::fs::File, T)> {
    use std::fs::File;
    use std::os::raw::{c_int, c_uchar, c_ulong, c_void};

    // Public FPDF_FILEACCESS ABI from fpdfview.h. pdfium-render exposes the
    // function but keeps the generated struct's module private. repr(C) and the
    // exact C field types let us pass the documented callback interface without
    // reading private Rust layouts or copying the entire file into memory.
    #[repr(C)]
    struct FileAccess {
        length: c_ulong,
        read_block:
            Option<unsafe extern "C" fn(*mut c_void, c_ulong, *mut c_uchar, c_ulong) -> c_int>,
        parameter: *mut c_void,
    }

    unsafe extern "C" fn read_block(
        parameter: *mut c_void,
        position: c_ulong,
        buffer: *mut c_uchar,
        size: c_ulong,
    ) -> c_int {
        use std::io::{Read, Seek, SeekFrom};
        if size == 0 {
            return 1;
        }
        let Ok(length) = usize::try_from(size) else {
            return 0;
        };
        if parameter.is_null() || buffer.is_null() || length > isize::MAX as usize {
            return 0;
        }
        // SAFETY: parameter points to the exclusively borrowed File below, and
        // Pdfium supplies a writable buffer of size bytes. The File and access
        // struct stay alive until FPDF_CloseDocument returns. No Rust references
        // to either are used during a Pdfium call or callback.
        let file = unsafe { &mut *parameter.cast::<File>() };
        let bytes = unsafe { std::slice::from_raw_parts_mut(buffer, length) };
        #[allow(clippy::unnecessary_cast)] // c_ulong is 32 bits on Windows.
        let offset = position as u64;
        (file.seek(SeekFrom::Start(offset)).is_ok() && file.read_exact(bytes).is_ok()) as c_int
    }

    limits.validate()?;
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    crate::scan::ensure_size(size, limits)?;
    #[allow(clippy::useless_conversion)] // c_ulong is 32 bits on Windows.
    let length = size
        .try_into()
        .map_err(|_| anyhow!("PDF exceeds the platform file-access limit"))?;
    let mut access = FileAccess {
        length,
        read_block: Some(read_block),
        parameter: (&mut file as *mut File).cast(),
    };
    let bindings = pdfium.bindings();
    let handle = bindings.FPDF_LoadCustomDocument((&mut access as *mut FileAccess).cast(), None);
    if handle.is_null() {
        #[allow(clippy::unnecessary_cast)]
        return Err(open_error(bindings.FPDF_GetLastError() as u64));
    }
    let result = check_page_count(bindings.FPDF_GetPageCount(handle), limits)
        .and_then(|()| action(handle.cast()));
    bindings.FPDF_CloseDocument(handle);
    Ok((file, result?))
}

/// Rendering and a single text layer do not extract the whole document, but
/// still cannot address pages outside pdfium-render's index range.
pub(crate) fn single_page_limits() -> Limits {
    Limits {
        max_pages: usize::from(u16::MAX),
        ..Limits::default()
    }
}
