use super::*;
use proptest::prelude::*;

/// ドメイン辞書と取り違え規則を明示した補正器を作る。
fn corrector() -> Corrector {
    Corrector::new(
        &["株式会社", "契約書", "東京都"],
        &[('杜', '社'), ('害', '書'), ('京', '亰')],
        CorrectionOptions::default(),
    )
    .unwrap()
}

/// UTF-8のbyte範囲を保持し、候補を選択した箇所だけを補正する。
#[test]
fn suggests_dictionary_terms_and_applies_only_selected_spans() {
    let source = "  株式会杜の契約害：100万円\t";
    let report = corrector().analyze(source, 0.6).unwrap();
    assert_eq!(report.original(), source);
    assert_eq!(report.confidence(), 0.6);
    assert_eq!(report.candidates().len(), 2);
    let candidate = &report.candidates()[0];
    assert_eq!(&source[candidate.span.clone()], "株式会杜");
    assert_eq!(candidate.original, "株式会杜");
    assert_eq!(candidate.replacement, "株式会社");
    assert_eq!(candidate.substitutions, 1);
    assert_eq!(report.apply_selected(&[]).unwrap(), source);
    assert_eq!(
        report.apply_selected(&[0]).unwrap(),
        "  株式会社の契約害：100万円\t"
    );
    assert_eq!(
        report.apply_selected(&[1, 0]).unwrap(),
        "  株式会社の契約書：100万円\t"
    );
    assert_eq!(report.original(), source);
}

/// 正しい語・未知の固有名詞・高信頼の認識を自動的に書き換えない。
#[test]
fn preserves_known_words_unknown_names_and_confident_text() {
    for source in ["株式会社の契約書", "杜山様", "東京都", "", "  \t\u{3000}"] {
        assert!(
            corrector()
                .analyze(source, 0.4)
                .unwrap()
                .candidates()
                .is_empty()
        );
    }
    let report = corrector().analyze("株式会杜", 0.95).unwrap();
    assert!(report.candidates().is_empty());
    // 同じ長さと異なる長さの正しい辞書語も候補から保護する。
    for dictionary in [&["契約書", "誓約書"][..], &["新契約書", "誓約書"][..]] {
        let engine =
            Corrector::new(dictionary, &[('契', '誓')], CorrectionOptions::default()).unwrap();
        assert!(
            engine
                .analyze("新契約書", 0.3)
                .unwrap()
                .candidates()
                .is_empty()
        );
    }
}

/// 同じ範囲の候補は決定的に並び、曖昧な同時選択は拒否する。
#[test]
fn rejects_overlapping_duplicate_and_invalid_selections() {
    let engine = Corrector::new(
        &["契約者", "契約書", "契約書"],
        &[('害', '書'), ('害', '者')],
        CorrectionOptions::default(),
    )
    .unwrap();
    let report = engine.analyze("契約害", 0.2).unwrap();
    assert_eq!(report.candidates().len(), 2);
    assert_eq!(report.candidates()[0].replacement, "契約書");
    assert_eq!(report.candidates()[1].replacement, "契約者");
    for selection in [&[0, 1][..], &[0, 0][..], &[2][..]] {
        assert!(report.apply_selected(selection).is_err());
    }
}

/// 数字・識別子・空白・句読点を変更する規則を受け付けない。
#[test]
fn rejects_rules_that_can_change_numbers_identifiers_or_formatting() {
    for pair in [
        ('0', 'O'),
        ('Ｏ', '０'),
        ('一', '二'),
        ('万', '方'),
        (' ', '社'),
        ('。', '社'),
        ('社', '社'),
    ] {
        assert!(Corrector::new(&["株式会社"], &[pair], CorrectionOptions::default()).is_err());
    }
    for confidence in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
        assert!(corrector().analyze("株式会杜", confidence).is_err());
    }
}

/// 入力・辞書・候補数・比較回数の超過は部分結果を返さず拒否する。
#[test]
fn enforces_limits_instead_of_silently_truncating() {
    let mut options = CorrectionOptions::default();
    options.limits.max_text_chars = 4;
    let engine = Corrector::new(&["株式会社"], &[('杜', '社')], options).unwrap();
    assert_eq!(
        engine.analyze("株式会杜", 0.3).unwrap().candidates().len(),
        1
    );
    assert!(engine.analyze("株式会杜。", 0.3).is_err());
    let mut options = CorrectionOptions::default();
    options.limits.max_candidates = 1;
    let engine = Corrector::new(&["株式会社"], &[('杜', '社')], options).unwrap();
    assert!(engine.analyze("株式会杜 株式会杜", 0.3).is_err());
    let mut options = CorrectionOptions::default();
    options.limits.max_comparisons = 1;
    let engine = Corrector::new(&["株式会社"], &[('杜', '社')], options).unwrap();
    assert!(engine.analyze("株式会杜", 0.3).is_err());
    let mut options = CorrectionOptions::default();
    options.limits.max_dictionary_terms = 1;
    assert!(Corrector::new(&["株式会社", "契約書"], &[], options).is_err());
    assert!(Corrector::new(&[""], &[], CorrectionOptions::default()).is_err());
}

proptest! {
    /// 任意のUnicode入力で原文を保持し、候補の範囲が必ず有効なUTF-8を指す。
    #[test]
    fn arbitrary_unicode_keeps_original_and_valid_spans(source in ".{0,80}") {
        let report = corrector().analyze(&source, 0.3).unwrap();
        prop_assert_eq!(report.original(), source.as_str());
        prop_assert_eq!(report.apply_selected(&[]).unwrap(), source.clone());
        for candidate in report.candidates() {
            prop_assert_eq!(&source[candidate.span.clone()], candidate.original.as_str());
            prop_assert_eq!(candidate.original.chars().count(), candidate.replacement.chars().count());
        }
    }

    /// 数字・空白・反復文字を含む周囲を保持し、同じ補正を二度提案しない。
    #[test]
    fn selected_correction_preserves_surroundings_and_is_idempotent(
        prefix in "[0-9 ０-９]{0,30}", suffix in "[0-9\\t ]{0,30}"
    ) {
        let source = format!("{prefix}株式会杜{suffix}");
        let report = corrector().analyze(&source, 0.3).unwrap();
        prop_assert_eq!(report.candidates().len(), 1);
        let corrected = report.apply_selected(&[0]).unwrap();
        prop_assert_eq!(&corrected, &format!("{prefix}株式会社{suffix}"));
        prop_assert!(corrector().analyze(&corrected, 0.3).unwrap().candidates().is_empty());
    }
}
