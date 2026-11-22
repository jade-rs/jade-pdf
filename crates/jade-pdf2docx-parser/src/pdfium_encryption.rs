//! native SDKで開けるコピー許可付き入力だけを、上限付きの内部表現へ復号する。
use crate::{ParseError, ParseLimits, decode::DecodeBudget, limits::check_limit};
use pdfium_render::prelude::{FPDF_DOCUMENT, FPDF_FILEWRITE, PdfiumLibraryBindings};
use std::{
    io,
    os::raw::{c_int, c_ulong, c_void},
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
};

/// 入力byteとSDKを閉じるまで借用し、途中の失敗でもdocumentを必ず解放する。
struct OpenDocument<'a> {
    /// このhandleを生成したSDK。
    bindings: &'a dyn PdfiumLibraryBindings,
    /// 開いたdocument。nullは保存しない。
    handle: FPDF_DOCUMENT,
    /// SDKが借用する入力の寿命。
    _input: &'a [u8],
}

impl Drop for OpenDocument<'_> {
    /// SDKと入力が有効な間に、所有するdocumentを一度だけ閉じる。
    fn drop(&mut self) {
        self.bindings.FPDF_CloseDocument(self.handle);
    }
}

/// パスワードの試行や原本保存を行わず、SDKで開けるStandard securityだけを復号する。
pub(super) fn normalize(
    path: &Path,
    input: &[u8],
    limits: &ParseLimits,
    budget: &DecodeBudget,
) -> Result<Vec<u8>, ParseError> {
    budget.check()?;
    check_limit("input bytes", input.len() as u64, limits.max_input_bytes)?;
    let version = source_version(path, input)?;
    let pdfium = super::library::bind(path)?;
    let bindings = pdfium.bindings();
    let handle = bindings.FPDF_LoadMemDocument64(input, None);
    if handle.is_null() {
        return Err(ParseError::UnsupportedEncryption { path: path.into() });
    }
    let document = OpenDocument {
        bindings,
        handle,
        _input: input,
    };
    let revision = bindings.FPDF_GetSecurityHandlerRevision(handle);
    let permissions = bindings.FPDF_GetDocUserPermissions(handle);
    if revision < 2 || permissions & (1 << 4) == 0 {
        return Err(ParseError::UnsupportedEncryption { path: path.into() });
    }
    budget.check()?;
    let pages = u64::try_from(bindings.FPDF_GetPageCount(handle))
        .map_err(|_| super::pdfium_error(path, "invalid encrypted page count"))?;
    check_limit("page count", pages, limits.max_pages as u64)?;
    let mut writer = BoundedWriter::new(limits.max_input_bytes, budget);
    // pdfium-render 0.8.37の互換値。新しいSDKでもdeprecated値3を単独で受理する。
    // 他のsave flagとORせず、元入力のsecurityだけを内部コピーから除く。
    let saved = bindings.FPDF_SaveWithVersion(document.handle, &mut writer.header, 3, version);
    if let Some(error) = writer.error.take() {
        return Err(error);
    }
    budget.check()?;
    if !bindings.is_true(saved) {
        return Err(super::pdfium_error(
            path,
            "encrypted PDF normalization failed",
        ));
    }
    preserve_version(path, &mut writer.bytes, version)?;
    Ok(writer.bytes)
}

/// Catalogの実効版ではなく、原本のliteral header版を保存設定へ渡す。
fn source_version(path: &Path, input: &[u8]) -> Result<c_int, ParseError> {
    let header = crate::parser::pdf_header_offset(input)
        .and_then(|offset| input.get(offset..offset + 8))
        .filter(|header| {
            header[5].is_ascii_digit() && header[6] == b'.' && header[7].is_ascii_digit()
        })
        .ok_or_else(|| super::pdfium_error(path, "encrypted PDF header unavailable"))?;
    let version = c_int::from(header[5] - b'0') * 10 + c_int::from(header[7] - b'0');
    if !(10..=17).contains(&version) && version != 20 {
        return Err(super::pdfium_error(
            path,
            "unsupported normalization PDF version",
        ));
    }
    Ok(version)
}

/// SDKのPDF 2.0保存が1.0 headerになる既知の挙動だけを、同じ長さで補正する。
/// object・stream・xref offsetには触れず、未知の版変化は受け入れない。
fn preserve_version(path: &Path, bytes: &mut [u8], version: c_int) -> Result<(), ParseError> {
    if !(10..=17).contains(&version) && version != 20 {
        return Err(super::pdfium_error(
            path,
            "unsupported normalization PDF version",
        ));
    }
    let expected = [
        b'%',
        b'P',
        b'D',
        b'F',
        b'-',
        b'0' + (version / 10) as u8,
        b'.',
        b'0' + (version % 10) as u8,
    ];
    if bytes.get(..8) == Some(expected.as_slice()) {
        return Ok(());
    }
    if version == 20 && bytes.get(..8) == Some(b"%PDF-1.0") {
        bytes[..8].copy_from_slice(&expected);
        return Ok(());
    }
    Err(super::pdfium_error(
        path,
        "normalization changed PDF header unexpectedly",
    ))
}

/// C callbackが先頭headerを渡す同期writer。最初の失敗以降は書き込まない。
#[repr(C)]
struct BoundedWriter<'a> {
    /// SDKへ渡す公開C構造。必ず先頭に置く。
    header: FPDF_FILEWRITE,
    /// 復号済みbyte。内部の一時表現で、原本や外部ファイルへ保存しない。
    bytes: Vec<u8>,
    /// 元入力と内部入力の両方へ適用するbyte上限。
    limit: u64,
    /// 元入力の解析と共有する確保・キャンセル予算。
    budget: &'a DecodeBudget,
    /// callbackの失敗をC戻り値だけで失わないための最初のエラー。
    error: Option<ParseError>,
}

impl<'a> BoundedWriter<'a> {
    /// 保存開始前には出力bufferを確保しない。
    fn new(limit: u64, budget: &'a DecodeBudget) -> Self {
        Self {
            header: FPDF_FILEWRITE {
                version: 1,
                WriteBlock: Some(write_block),
            },
            bytes: Vec::new(),
            limit: limit.min(isize::MAX as u64),
            budget,
            error: None,
        }
    }

    /// 最初の失敗を保持し、後続callbackの再開を拒否する。
    fn fail(&mut self, error: ParseError) -> bool {
        if self.error.is_none() {
            self.error = Some(error);
        }
        false
    }

    /// Cのsliceを作る前に、停止と加算後の容量を検査する。
    fn check_length(&mut self, additional: u64) -> bool {
        if self.error.is_some() {
            return false;
        }
        if let Err(error) = self.budget.check() {
            return self.fail(error);
        }
        let length = (self.bytes.len() as u64).saturating_add(additional);
        match check_limit("normalized input bytes", length, self.limit) {
            Ok(()) => true,
            Err(error) => self.fail(error),
        }
    }

    /// 全capacityを共有予算へ先に予約し、失敗した部分出力を成功として返さない。
    fn append(&mut self, data: &[u8]) -> bool {
        if !self.check_length(data.len() as u64) {
            return false;
        }
        let needed = self.bytes.len().saturating_add(data.len());
        if needed > self.bytes.capacity() {
            let capacity = self
                .bytes
                .capacity()
                .saturating_mul(2)
                .max(8192)
                .max(needed)
                .min(self.limit as usize);
            let Some(mut next) = self.budget.try_vector_capacity(capacity) else {
                let error = self.budget.check().err().unwrap_or_else(|| {
                    ParseError::Io(io::Error::other("native PDF writer allocation stopped"))
                });
                return self.fail(error);
            };
            next.append(&mut self.bytes);
            self.bytes = next;
        }
        self.bytes.extend_from_slice(data);
        true
    }
}

/// SDKの同期write callback。長さ検査をslice構築より先に行い、panicをCへ越境させない。
unsafe extern "C" fn write_block(
    this: *mut FPDF_FILEWRITE,
    data: *const c_void,
    size: c_ulong,
) -> c_int {
    if this.is_null() {
        return 0;
    }
    // repr(C)の先頭headerを、呼出し中だけ元のwriterへ戻す。
    let writer = unsafe { &mut *this.cast::<BoundedWriter<'_>>() };
    let result = catch_unwind(AssertUnwindSafe(|| {
        // Windowsではc_ulongが32bit、Unix 64bit環境ではu64になる。
        #[allow(clippy::unnecessary_cast)]
        let length = size as u64;
        if !writer.check_length(length) {
            return false;
        }
        if size == 0 {
            return true;
        }
        if data.is_null() {
            return writer.fail(ParseError::Io(io::Error::other(
                "null native PDF write data",
            )));
        }
        let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size as usize) };
        writer.append(bytes)
    }));
    match result {
        Ok(success) => c_int::from(success),
        Err(_) => {
            writer.fail(ParseError::Io(io::Error::other(
                "native PDF writer panicked",
            )));
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 製品と同じ内部復号で、公開原本の全5ページの表示画素を変更しない。
    #[test]
    #[ignore = "requires hash-pinned external NTA corpus and native Pdfium"]
    fn native_encryption_normalization_retains_rendering() {
        use pdfium_render::prelude::PdfRenderConfig;
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../corpus/cache/nta-invoice-domestic-english-2026.pdf");
        let input = std::fs::read(&path).unwrap();
        let limits = ParseLimits::default();
        let budget = DecodeBudget::new(&limits);
        let bytes = normalize(&path, &input, &limits, &budget).unwrap();
        let pdfium = super::super::library::bind(&path).unwrap();
        let original = pdfium.load_pdf_from_byte_slice(&input, None).unwrap();
        let normalized = pdfium.load_pdf_from_byte_slice(&bytes, None).unwrap();
        assert_eq!(original.pages().len(), 5);
        assert_eq!(normalized.pages().len(), 5);
        for page in 0..5 {
            let before = original.pages().get(page).unwrap();
            let after = normalized.pages().get(page).unwrap();
            assert_eq!(before.width(), after.width());
            assert_eq!(before.height(), after.height());
            let config = PdfRenderConfig::new().set_target_width(400);
            let before = before.render_with_config(&config).unwrap();
            let after = after.render_with_config(&config).unwrap();
            assert_eq!(before.width(), after.width());
            assert_eq!(before.height(), after.height());
            assert_eq!(before.as_raw_bytes(), after.as_raw_bytes(), "page {page}");
        }
        assert_eq!(std::fs::read(&path).unwrap(), input);
    }

    /// CatalogのVersionと先頭余白を、原本header版の値へ混同しない。
    #[test]
    fn source_header_version_is_separate_from_catalog_version() {
        assert_eq!(
            source_version(Path::new("owned.pdf"), b"%PDF-1.7\n<< /Version /2.0 >>").unwrap(),
            17
        );
        assert_eq!(
            source_version(Path::new("owned.pdf"), b"prefix %PDF-2.0\n").unwrap(),
            20
        );
        for input in [b"%PDF-2.".as_slice(), b"%PDF-x.0", b"%PDF-1x7", b"%PDF-9.9"] {
            assert!(source_version(Path::new("owned.pdf"), input).is_err());
        }
    }

    /// 2.0 headerの補正では、長さと後続byteを変えない。
    #[test]
    fn version_adapter_keeps_offsets_and_original_version() {
        let mut bytes = b"%PDF-1.0\r\nowned body".to_vec();
        let original = bytes.clone();
        preserve_version(Path::new("owned.pdf"), &mut bytes, 20).unwrap();
        assert_eq!(&bytes[..8], b"%PDF-2.0");
        assert_eq!(bytes.len(), original.len());
        assert_eq!(&bytes[8..], &original[8..]);
        preserve_version(Path::new("owned.pdf"), &mut bytes, 20).unwrap();
        let mut bytes = b"%PDF-1.7\nbody".to_vec();
        let original = bytes.clone();
        preserve_version(Path::new("owned.pdf"), &mut bytes, 17).unwrap();
        assert_eq!(bytes, original);
    }

    /// 既知の2.0変化以外は、後から版を推測して受け入れない。
    #[test]
    fn version_adapter_rejects_unexpected_or_truncated_headers() {
        for (header, version) in [
            (b"%PDF-1.7".as_slice(), 20),
            (b"%PDF-1.0", 17),
            (b"%PDF-", 20),
            (b"%PDF-1.0", 0),
        ] {
            let mut bytes = header.to_vec();
            let original = bytes.clone();
            assert!(preserve_version(Path::new("owned.pdf"), &mut bytes, version).is_err());
            assert_eq!(bytes, original);
        }
    }

    /// 上限ちょうどの書込だけを受け入れ、超過後の小さい書込も再開しない。
    #[test]
    fn writer_enforces_exact_byte_limit_and_sticky_failure() {
        let budget = DecodeBudget::default();
        let mut writer = BoundedWriter::new(8, &budget);
        assert!(writer.append(b"1234"));
        assert!(writer.append(b"5678"));
        assert!(!writer.append(b"9"));
        assert!(!writer.append(b""));
        assert_eq!(writer.bytes, b"12345678");
        assert!(matches!(
            writer.error,
            Some(ParseError::ResourceLimit { .. })
        ));
    }

    /// 全capacityの予約を拒否した場合は、bufferも部分本文も確保しない。
    #[test]
    fn writer_reserves_capacity_before_copying() {
        let limits = ParseLimits {
            max_total_allocation_bytes: 7,
            ..Default::default()
        };
        let budget = DecodeBudget::new(&limits);
        let mut writer = BoundedWriter::new(8, &budget);
        assert!(!writer.append(b"1"));
        assert!(writer.bytes.is_empty());
        assert_eq!(writer.bytes.capacity(), 0);
        assert!(matches!(
            writer.error,
            Some(ParseError::ResourceLimit { .. })
        ));
    }

    /// 停止を観測したcallbackは、空の書込でも成功へ戻らない。
    #[test]
    fn writer_keeps_cancellation() {
        let cancellation = crate::ParseCancellation::default();
        let limits = ParseLimits::default();
        let budget = DecodeBudget::with_cancellation(&limits, Some(&cancellation));
        let mut writer = BoundedWriter::new(8, &budget);
        cancellation.cancel();
        assert!(!writer.append(b"1"));
        assert!(matches!(writer.error, Some(ParseError::Cancelled)));
    }

    /// callbackの空・null・巨大な長さを、無効なsliceを作らずに検査する。
    #[test]
    fn callback_rejects_invalid_data_before_slice_creation() {
        let budget = DecodeBudget::default();
        let mut writer = BoundedWriter::new(8, &budget);
        unsafe {
            assert_eq!(write_block(&mut writer.header, std::ptr::null(), 0), 1);
            assert_eq!(
                write_block(&mut writer.header, std::ptr::null(), c_ulong::MAX),
                0
            );
            assert_eq!(write_block(&mut writer.header, std::ptr::null(), 0), 0);
            assert_eq!(write_block(std::ptr::null_mut(), std::ptr::null(), 0), 0);
        }
        assert!(writer.bytes.is_empty());
    }
}
