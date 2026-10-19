use jade_pdf2docx_docx::{
    ConvertError, MinimumPdfToDocxRequest, convert_minimum_pdf_to_docx,
    convert_minimum_pdf_to_docx_with_limits,
};
#[path = "../../jade-pdf2docx-parser/tests/support/page_attribute_pdf.rs"]
mod metadata_pdf;
use std::io::{Read, Write};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

/// テストごとに独立した一時ディレクトリを管理する。
struct TestDir(
    /// テストが所有するディレクトリ。
    PathBuf,
);
impl TestDir {
    /// 一意な一時ディレクトリを作る。
    fn new() -> Self {
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "jade-pdf2docx-output-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    /// 最小の入力 PDF と出力先を持つリクエストを作る。
    fn request(&self, output: &str) -> MinimumPdfToDocxRequest {
        let input = self.0.join("input.pdf");
        fs::write(&input, b"%PDF-1.7\n1 0 obj\n<< /Type /Page >>\nendobj\n").unwrap();
        MinimumPdfToDocxRequest {
            input_pdf_path: input,
            output_docx_path: self.0.join(output),
            ocr_text_lines: None,
            ocr_table: None,
            clean_output: true,
        }
    }
}
impl Drop for TestDir {
    /// テストで作ったファイルを掃除する。
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// file/bytes の strict 変換は白画像置換を拒否し、既存出力も新規ディレクトリも変更しない。
#[test]
fn strict_quality_rejection_preserves_output_and_reports_reason() {
    let dir = TestDir::new();
    let mut request = dir.request("output.docx");
    let pdf = b"%PDF-1.7\n1 0 obj << /Type /Page >> endobj\n2 0 obj << /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode /Length 1 >>\nstream\nx\nendstream\nendobj\n";
    fs::write(&request.input_pdf_path, pdf).unwrap();
    fs::write(&request.output_docx_path, b"existing output").unwrap();
    let options = jade_pdf2docx_parser::ParseOptions {
        strict: true,
        ..Default::default()
    };
    let result = jade_pdf2docx_docx::convert_minimum_pdf_to_docx_with_options(&request, &options);
    assert!(matches!(
        result,
        Err(ConvertError::Parse(
            jade_pdf2docx_parser::ParseError::QualityRejected { .. }
        ))
    ));
    assert_eq!(
        fs::read(&request.output_docx_path).unwrap(),
        b"existing output"
    );
    assert_eq!(fs::read(&request.input_pdf_path).unwrap(), pdf);
    assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 2);
    request.output_docx_path = dir.0.join("new/sub/output.docx");
    assert!(
        jade_pdf2docx_docx::convert_minimum_pdf_to_docx_with_options(&request, &options).is_err()
    );
    assert!(!dir.0.join("new").exists());
    assert!(matches!(
        jade_pdf2docx_docx::convert_minimum_pdf_bytes_to_docx_bytes_with_options(
            "image.pdf",
            pdf,
            &options
        ),
        Err(ConvertError::Parse(
            jade_pdf2docx_parser::ParseError::QualityRejected { .. }
        ))
    ));
    request.output_docx_path = dir.0.join("lenient.docx");
    let report = convert_minimum_pdf_to_docx(&request).unwrap();
    assert_eq!(
        report.quality.issues,
        vec![
            jade_pdf2docx_parser::QualityIssue::MissingPageGeometry { page: 0 },
            jade_pdf2docx_parser::QualityIssue::ImageReplaced {
                object_number: Some(2),
                reason: jade_pdf2docx_parser::ImageReplacementReason::UnsupportedFilter
            }
        ]
    );
    assert!(
        fs::read(&request.output_docx_path)
            .unwrap()
            .starts_with(b"PK")
    );
}

/// 破損/暗号化を呼び出し元が区別でき、失敗で既存の出力を消さない。
#[test]
fn invalid_structure_and_encryption_preserve_existing_output() {
    for encrypted in [false, true] {
        let dir = TestDir::new();
        let request = dir.request("output.docx");
        let input: &[u8] = if encrypted {
            b"%PDF-1.7\n1 0 obj << /Type /Catalog >> endobj\ntrailer << /Encrypt 5 0 R >>"
        } else {
            b"%PDF-1.7\n"
        };
        fs::write(&request.input_pdf_path, input).unwrap();
        fs::write(&request.output_docx_path, b"existing output").unwrap();
        let result = convert_minimum_pdf_to_docx(&request);
        if encrypted {
            assert!(matches!(
                result,
                Err(ConvertError::Parse(
                    jade_pdf2docx_parser::ParseError::UnsupportedEncryption { .. }
                ))
            ));
        } else {
            assert!(matches!(
                result,
                Err(ConvertError::Parse(
                    jade_pdf2docx_parser::ParseError::InvalidPdfStructure { .. }
                ))
            ));
        }
        assert_eq!(fs::read(&request.input_pdf_path).unwrap(), input);
        assert_eq!(
            fs::read(&request.output_docx_path).unwrap(),
            b"existing output"
        );
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 2);
    }
}

/// 同一パスへの変換は入力を破壊せずに拒否する。
#[test]
fn rejects_identical_input_and_output_without_modifying_pdf() {
    let dir = TestDir::new();
    let request = dir.request("input.pdf");
    let before = fs::read(&request.input_pdf_path).unwrap();
    assert!(matches!(
        convert_minimum_pdf_to_docx(&request),
        Err(ConvertError::OutputAliasesInput)
    ));
    assert!(
        fs::read(&request.input_pdf_path).unwrap() == before,
        "input PDF changed"
    );
}

/// 別名の hardlink でも同一ファイルとして拒否する。
#[test]
fn rejects_hardlinked_output_without_modifying_pdf() {
    let dir = TestDir::new();
    let request = dir.request("alias.docx");
    fs::hard_link(&request.input_pdf_path, &request.output_docx_path).unwrap();
    let before = fs::read(&request.input_pdf_path).unwrap();
    assert!(matches!(
        convert_minimum_pdf_to_docx(&request),
        Err(ConvertError::OutputAliasesInput)
    ));
    assert!(
        fs::read(&request.input_pdf_path).unwrap() == before,
        "input PDF changed"
    );
}

/// symlink 経由の同一入力への出力を拒否する。
#[cfg(unix)]
#[test]
fn rejects_symlinked_output_without_modifying_pdf() {
    let dir = TestDir::new();
    let request = dir.request("alias.docx");
    std::os::unix::fs::symlink(&request.input_pdf_path, &request.output_docx_path).unwrap();
    let before = fs::read(&request.input_pdf_path).unwrap();
    assert!(matches!(
        convert_minimum_pdf_to_docx(&request),
        Err(ConvertError::OutputAliasesInput)
    ));
    assert!(
        fs::read(&request.input_pdf_path).unwrap() == before,
        "input PDF changed"
    );
}

/// 出力 symlink のリンク先ファイルを直接上書きしない。
#[cfg(unix)]
#[test]
fn successful_conversion_replaces_output_entry_without_following_symlink() {
    let dir = TestDir::new();
    let request = dir.request("output.docx");
    let target = dir.0.join("other.docx");
    fs::write(&target, b"existing document").unwrap();
    std::os::unix::fs::symlink(&target, &request.output_docx_path).unwrap();
    convert_minimum_pdf_to_docx(&request).unwrap();
    assert_eq!(fs::read(target).unwrap(), b"existing document");
    assert!(
        fs::read(&request.output_docx_path)
            .unwrap()
            .starts_with(b"PK")
    );
    assert!(
        !fs::symlink_metadata(&request.output_docx_path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

/// clean output の空ページに内部パスやデバッグ見出しを混入させない。
#[test]
fn clean_empty_output_contains_no_debug_metadata() {
    let dir = TestDir::new();
    let request = dir.request("output.docx");
    convert_minimum_pdf_to_docx(&request).unwrap();
    let mut zip = zip::ZipArchive::new(fs::File::open(&request.output_docx_path).unwrap()).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(!xml.contains("minimum pdf to docx conversion"));
    assert!(!xml.contains("source:"));
    assert!(xml.contains("(no extractable content)"));
}

/// 失敗した atomic replace は既存ディレクトリとその内容を保持し、一時ファイルを残さない。
#[test]
fn failed_commit_preserves_destination_and_removes_temporary_files() {
    let dir = TestDir::new();
    let request = dir.request("output.docx");
    fs::create_dir(&request.output_docx_path).unwrap();
    let sentinel = request.output_docx_path.join("existing");
    fs::write(&sentinel, b"preserve me").unwrap();
    let before = fs::read_dir(&dir.0).unwrap().count();
    assert!(convert_minimum_pdf_to_docx(&request).is_err());
    assert_eq!(fs::read(sentinel).unwrap(), b"preserve me");
    assert_eq!(fs::read_dir(&dir.0).unwrap().count(), before);
}

/// 別の DOCX の hardlink を出力先にしても元の DOCX を変更しない。
#[test]
fn successful_conversion_replaces_hardlink_without_modifying_other_entry() {
    let dir = TestDir::new();
    let request = dir.request("output.docx");
    let target = dir.0.join("other.docx");
    fs::write(&target, b"existing document").unwrap();
    fs::hard_link(&target, &request.output_docx_path).unwrap();
    convert_minimum_pdf_to_docx(&request).unwrap();
    assert_eq!(fs::read(target).unwrap(), b"existing document");
    assert!(
        fs::read(&request.output_docx_path)
            .unwrap()
            .starts_with(b"PK")
    );
}

/// `.` を含む別表記でも入力の上書きを拒否する。
#[test]
fn rejects_path_alias_without_modifying_pdf() {
    let dir = TestDir::new();
    let request = dir.request("./input.pdf");
    let before = fs::read(&request.input_pdf_path).unwrap();
    assert!(matches!(
        convert_minimum_pdf_to_docx(&request),
        Err(ConvertError::OutputAliasesInput)
    ));
    assert!(
        fs::read(&request.input_pdf_path).unwrap() == before,
        "input PDF changed"
    );
}

/// 圧縮 stream の資源超過で変換が失敗しても既存 DOCX を保持する。
#[test]
fn decoded_stream_limit_preserves_existing_output() {
    let dir = TestDir::new();
    let request = dir.request("output.docx");
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&vec![b' '; 1025]).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut pdf = format!("%PDF-1.7\n1 0 obj\n<< /Type /Page /Contents 2 0 R >>\nendobj\n2 0 obj\n<< /Filter /FlateDecode /Length {} >>\nstream\n", compressed.len()).into_bytes();
    pdf.extend_from_slice(&compressed);
    pdf.extend_from_slice(b"\nendstream\nendobj\n");
    fs::write(&request.input_pdf_path, &pdf).unwrap();
    fs::write(&request.output_docx_path, b"existing document").unwrap();
    let limits = jade_pdf2docx_parser::ParseLimits {
        max_decoded_stream_bytes: 1024,
        ..Default::default()
    };
    assert!(matches!(
        convert_minimum_pdf_to_docx_with_limits(&request, &limits),
        Err(ConvertError::Parse(
            jade_pdf2docx_parser::ParseError::ResourceLimit {
                resource: "decoded stream bytes",
                ..
            }
        ))
    ));
    assert_eq!(
        fs::read(&request.output_docx_path).unwrap(),
        b"existing document"
    );
    assert_eq!(fs::read(&request.input_pdf_path).unwrap(), pdf);
    assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 2);
}

/// 幅表予算の超過は既存 DOCX と入力を保持し、一時出力も残さない。
#[test]
fn font_metrics_limit_preserves_existing_output() {
    let dir = TestDir::new();
    let request = dir.request("output.docx");
    let pdf = b"%PDF-1.7\n1 0 obj\n<< /Type /Page /Resources << /Font << /F 2 0 R >> >> >>\nendobj\n2 0 obj\n<< /Type /Font /Subtype /Type1 /FirstChar 65 /Widths [600] >>\nendobj\n";
    fs::write(&request.input_pdf_path, pdf).unwrap();
    fs::write(&request.output_docx_path, b"existing document").unwrap();
    let limits = jade_pdf2docx_parser::ParseLimits {
        max_total_font_metrics_bytes: 1023,
        ..Default::default()
    };
    assert!(matches!(
        convert_minimum_pdf_to_docx_with_limits(&request, &limits),
        Err(ConvertError::Parse(
            jade_pdf2docx_parser::ParseError::ResourceLimit {
                resource: "font metrics bytes",
                limit: 1023,
                actual: 1024,
            }
        ))
    ));
    assert_eq!(
        fs::read(&request.output_docx_path).unwrap(),
        b"existing document"
    );
    assert_eq!(fs::read(&request.input_pdf_path).unwrap(), pdf);
    assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 2);
}

/// CID 幅の一時配列予算が超えた場合も既存 DOCX と入力を保持する。
#[test]
fn font_width_workspace_limit_preserves_existing_output() {
    let dir = TestDir::new();
    let request = dir.request("output.docx");
    let pdf = b"%PDF-1.7\n1 0 obj\n<< /Type /Page /Resources << /Font << /F 2 0 R >> >> >>\nendobj\n2 0 obj\n<< /Type /Font /Subtype /Type0 /Encoding /Identity-H /DescendantFonts [3 0 R] >>\nendobj\n3 0 obj\n<< /Type /Font /Subtype /CIDFontType2 /W [65 [600]] >>\nendobj\n";
    fs::write(&request.input_pdf_path, pdf).unwrap();
    fs::write(&request.output_docx_path, b"existing document").unwrap();
    let limits = jade_pdf2docx_parser::ParseLimits {
        max_total_font_width_workspace_bytes: 0,
        ..Default::default()
    };
    assert!(matches!(
        convert_minimum_pdf_to_docx_with_limits(&request, &limits),
        Err(ConvertError::Parse(
            jade_pdf2docx_parser::ParseError::ResourceLimit {
                resource: "font width workspace bytes",
                limit: 0,
                ..
            }
        ))
    ));
    assert_eq!(
        fs::read(&request.output_docx_path).unwrap(),
        b"existing document"
    );
    assert_eq!(fs::read(&request.input_pdf_path).unwrap(), pdf);
    assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 2);
}

/// ToUnicode の上限超過と演算 overflow は既存 DOCX・入力・一時出力なしを保証する。
#[test]
fn tounicode_failure_preserves_existing_output() {
    let dir = TestDir::new();
    let request = dir.request("output.docx");
    for mode in 0..5 {
        let overflow = mode == 1;
        let cmap = if overflow {
            "1 beginbfrange\n<01> <02> <FFFFFFFF>\nendbfrange"
        } else {
            "1 beginbfchar\n<01> <0041>\nendbfchar"
        };
        let pdf = format!(
            "%PDF-1.7\n1 0 obj\n<< /Type /Page /Resources << /Font << /F 2 0 R >> >> >>\nendobj\n2 0 obj\n<< /Type /Font /Subtype /Type1 /ToUnicode 3 0 R >>\nendobj\n3 0 obj\n<< /Length {} >>\nstream\n{cmap}\nendstream\nendobj\n",
            cmap.len()
        );
        fs::write(&request.input_pdf_path, &pdf).unwrap();
        fs::write(&request.output_docx_path, b"existing document").unwrap();
        let limits = jade_pdf2docx_parser::ParseLimits {
            max_total_tounicode_entries: if mode == 0 { 0 } else { 64 },
            max_total_tounicode_bytes: if mode == 2 { 0 } else { 64 },
            max_total_tounicode_storage_bytes: if mode == 3 { 0 } else { 64 * 1024 * 1024 },
            max_total_tounicode_input_bytes: if mode == 4 { 0 } else { 512 * 1024 * 1024 },
            ..Default::default()
        };
        let error = convert_minimum_pdf_to_docx_with_limits(&request, &limits).unwrap_err();
        if overflow {
            assert!(
                matches!(
                    error,
                    ConvertError::Parse(jade_pdf2docx_parser::ParseError::InvalidToUnicode { .. })
                ),
                "{error}"
            );
        } else {
            assert!(
                matches!(
                    error,
                    ConvertError::Parse(jade_pdf2docx_parser::ParseError::ResourceLimit {
                        resource,
                        limit: 0,
                        actual,
                    }) if (mode == 0 && resource == "ToUnicode entries" && actual == 1)
                        || (mode == 2 && resource == "ToUnicode mapping bytes" && actual == 3)
                        || (mode == 3 && resource == "ToUnicode storage bytes"
                            && actual == std::mem::size_of::<(String, String, usize)>() as u64)
                        || (mode == 4 && resource == "ToUnicode input bytes" && actual > 0)
                ),
                "{error}"
            );
        }
        assert_eq!(
            fs::read(&request.output_docx_path).unwrap(),
            b"existing document"
        );
        assert_eq!(fs::read(&request.input_pdf_path).unwrap(), pdf.as_bytes());
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 2);
    }
}

/// 索引構築後の page tree 検証で失敗しても、出力・入力・一時ファイルを保護する。
#[test]
fn page_tree_workspace_limit_preserves_existing_output() {
    let dir = TestDir::new();
    let request = dir.request("output.docx");
    let pdf = metadata_pdf::pdf(&metadata_pdf::pages("/MediaBox [0 0 500 500]"));
    let Err(jade_pdf2docx_parser::ParseError::ResourceLimit {
        resource: "metadata workspace bytes",
        actual: index_bytes,
        ..
    }) = jade_pdf2docx_parser::parse_pdf_bytes_with_limits(
        "budget.pdf",
        &pdf,
        &jade_pdf2docx_parser::ParseLimits {
            max_total_metadata_workspace_bytes: 0,
            ..Default::default()
        },
    )
    else {
        panic!("索引容量が不明");
    };
    fs::write(&request.input_pdf_path, &pdf).unwrap();
    fs::write(&request.output_docx_path, b"existing document").unwrap();
    assert!(
        matches!(convert_minimum_pdf_to_docx_with_limits(&request, &jade_pdf2docx_parser::ParseLimits {
        max_total_metadata_workspace_bytes: index_bytes, ..Default::default()
    }), Err(ConvertError::Parse(jade_pdf2docx_parser::ParseError::ResourceLimit {
        resource: "metadata workspace bytes", limit, actual
    })) if limit == index_bytes && actual > index_bytes)
    );
    assert_eq!(
        fs::read(&request.output_docx_path).unwrap(),
        b"existing document"
    );
    assert_eq!(fs::read(&request.input_pdf_path).unwrap(), pdf);
    assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 2);
}

/// 未作成directory作成後に成立する入力aliasも、通常保存とworker予約で拒否する。
#[test]
fn future_output_alias_is_rejected_before_parent_creation() {
    for hardlink in [false, true] {
        let dir = TestDir::new();
        let name = if hardlink { "alias.docx" } else { "input.pdf" };
        let request = dir.request(&format!("parent/missing/../../{name}"));
        let original = fs::read(&request.input_pdf_path).unwrap();
        fs::create_dir(dir.0.join("parent")).unwrap();
        if hardlink {
            fs::hard_link(&request.input_pdf_path, dir.0.join(name)).unwrap();
        }
        assert!(matches!(
            convert_minimum_pdf_to_docx(&request),
            Err(ConvertError::OutputAliasesInput)
        ));
        assert!(matches!(
            jade_pdf2docx_docx::StagedDocxOutput::new(
                &request.input_pdf_path,
                &request.output_docx_path
            ),
            Err(ConvertError::OutputAliasesInput)
        ));
        assert_eq!(fs::read(&request.input_pdf_path).unwrap(), original);
        assert!(!dir.0.join("parent/missing").exists());
    }
}

/// 既存directoryリンクの実体を解決してから、未作成成分をまたぐ入力aliasを検査する。
#[cfg(unix)]
#[test]
fn future_output_alias_resolves_symlink_before_parent_reference() {
    let dir = TestDir::new();
    let mut request = dir.request("output.docx");
    let links = TestDir::new();
    fs::create_dir(dir.0.join("parent")).unwrap();
    std::os::unix::fs::symlink(dir.0.join("parent"), links.0.join("alias")).unwrap();
    request.output_docx_path = links.0.join("alias/missing/../../input.pdf");
    let original = fs::read(&request.input_pdf_path).unwrap();
    assert!(matches!(
        convert_minimum_pdf_to_docx(&request),
        Err(ConvertError::OutputAliasesInput)
    ));
    assert!(matches!(
        jade_pdf2docx_docx::StagedDocxOutput::new(
            &request.input_pdf_path,
            &request.output_docx_path
        ),
        Err(ConvertError::OutputAliasesInput)
    ));
    assert_eq!(fs::read(&request.input_pdf_path).unwrap(), original);
    assert!(!dir.0.join("parent/missing").exists());
}
