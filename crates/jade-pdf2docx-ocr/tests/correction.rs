use jade_pdf2docx_ocr::{OcrTextBlock, OcrTextKind};
use jade_pdf2docx_ocr_correction::{CorrectionOptions, Corrector};

/// 認識ブロックを変更せず、座標・信頼度・原文と対応する補正候補を取得する。
#[test]
fn recognized_blocks_can_be_reviewed_without_changing_ocr_output() {
    let block = OcrTextBlock {
        bounds: Some([10, 20, 300, 40]),
        text: "株式会杜の契約害 100万円".into(),
        confidence: 0.6,
        kind: OcrTextKind::Recognized,
    };
    let before = block.clone();
    let engine = Corrector::new(
        &["株式会社", "契約書"],
        &[('杜', '社'), ('害', '書')],
        CorrectionOptions::default(),
    )
    .unwrap();
    let report = block.suggest_corrections(&engine).unwrap().unwrap();
    assert_eq!(report.original(), block.text);
    assert_eq!(report.confidence(), block.confidence);
    assert_eq!(
        report.apply_selected(&[0, 1]).unwrap(),
        "株式会社の契約書 100万円"
    );
    assert_eq!(block, before);
}

/// mock・座標診断を認識本文の補正として扱わず、不正な実認識信頼度は拒否する。
#[test]
fn excludes_mock_and_diagnostic_blocks_from_correction() {
    let engine =
        Corrector::new(&["株式会社"], &[('杜', '社')], CorrectionOptions::default()).unwrap();
    for kind in [OcrTextKind::Mock, OcrTextKind::BandDiagnostic] {
        let block = OcrTextBlock {
            bounds: None,
            text: "株式会杜".into(),
            confidence: 0.0,
            kind,
        };
        assert!(block.suggest_corrections(&engine).unwrap().is_none());
    }
    let block = OcrTextBlock {
        bounds: None,
        text: "株式会杜".into(),
        confidence: f32::NAN,
        kind: OcrTextKind::Recognized,
    };
    assert!(block.suggest_corrections(&engine).is_err());
}
