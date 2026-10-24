use pdf_diff::{
    load_document, load_document_from_bytes, render_page_at_dpi_from_bytes, render_page_from_bytes,
    text_layer_from_bytes, Limits,
};

// Distinct page objects and a correct page-tree count; this is a complete PDF,
// not just an inflated /Count entry or repeated references to one page object.
fn pages(count: usize) -> Vec<u8> {
    let mut objects = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!(
            "<< /Type /Pages /Count {count} /MediaBox [0 0 200 200] /Resources << >> /Kids [{}] >>",
            (0..count)
                .map(|i| format!("{} 0 R", i + 3))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    ];
    objects.extend((0..count).map(|_| "<< /Type /Page /Parent 2 0 R >>".to_owned()));
    let mut source = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(source.len());
        source.push_str(&format!("{} 0 obj\n{o}\nendobj\n", i + 1));
    }
    let start = source.len();
    source.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for offset in offsets {
        source.push_str(&format!("{offset:010} 00000 n \n"));
    }
    source.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{start}\n%%EOF\n",
        objects.len() + 1
    ));
    source.into_bytes()
}

#[test]
fn full_page_count_is_checked_before_u16_truncation_in_every_input_route() {
    for count in [65_536, 65_537] {
        let bytes = pages(count);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("many.pdf");
        std::fs::write(&path, &bytes).unwrap();
        // A caller cannot bypass the backend index limit by increasing max_pages.
        let limits = Limits {
            max_pages: 100_000,
            ..Limits::default()
        };
        for error in [
            load_document(&path, &limits).unwrap_err(),
            load_document_from_bytes(&bytes, &limits).unwrap_err(),
        ] {
            assert!(error.to_string().contains(&count.to_string()), "{error:#}");
        }
        assert!(render_page_from_bytes(&bytes, 1, 400).is_err());
        assert!(render_page_at_dpi_from_bytes(&bytes, 1, 72).is_err());
        assert!(text_layer_from_bytes(&bytes, 1).is_err());
        assert!(pdf_diff::render_page(&path, 1, 400).is_err());
        assert!(pdf_diff::render_page_at_dpi(&path, 1, 72).is_err());
        assert!(pdf_diff::text_layer(&path, 1).is_err());
        let cli = std::process::Command::new(env!("CARGO_BIN_EXE_pdf-diff"))
            .arg(&path)
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(cli.status.code(), Some(2));
        assert!(cli.stdout.is_empty());
    }
    let valid = pages(1);
    assert_eq!(
        load_document_from_bytes(&valid, &Limits::default())
            .unwrap()
            .pages
            .len(),
        1
    );
}

#[test]
fn checked_file_access_preserves_unicode_paths() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("日本語 résumé.pdf");
    std::fs::write(&path, pages(1)).unwrap();
    assert_eq!(
        load_document(&path, &Limits::default())
            .unwrap()
            .pages
            .len(),
        1
    );
    assert!(pdf_diff::render_page(&path, 1, 400).is_ok());
}

#[test]
fn input_size_limits_precede_pdf_parsing_for_files_and_memory() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("oversized.pdf");
    // Sparse file: verify the default limit without allocating a 513MB buffer.
    std::fs::File::create(&path)
        .unwrap()
        .set_len(513 * 1024 * 1024)
        .unwrap();
    let errors = [
        load_document(&path, &Limits::default()).unwrap_err(),
        pdf_diff::render_page(&path, 1, 400).err().unwrap(),
        pdf_diff::render_page_at_dpi(&path, 1, 72).err().unwrap(),
        pdf_diff::text_layer(&path, 1).unwrap_err(),
    ];
    for error in errors {
        assert!(error.to_string().contains("max_input_mb"), "{error:#}");
    }
    let bytes = vec![b'X'; 1024 * 1024 + 1];
    let error = load_document_from_bytes(
        &bytes,
        &Limits {
            max_input_mb: 1,
            ..Limits::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("max_input_mb"));
}

#[cfg(target_os = "linux")]
#[test]
fn checked_file_access_preserves_non_utf8_paths_on_linux() {
    use std::os::unix::ffi::OsStringExt;
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .join(std::ffi::OsString::from_vec(b"\xff.pdf".to_vec()));
    std::fs::write(&path, pages(1)).unwrap();
    assert_eq!(
        load_document(&path, &Limits::default())
            .unwrap()
            .pages
            .len(),
        1
    );
    assert!(pdf_diff::render_page(&path, 1, 400).is_ok());
}
