//! PDF本文の文字列byteをUTF-8置換せず解釈する回帰。
use super::*;
use proptest::prelude::*;

/// FontとToUnicodeを共有するページまたはFormを組み立てる。
pub(super) fn fixture(content: &[u8], mapping: &[u8], form: bool) -> Vec<u8> {
    let mut pdf = b"%PDF-1.7\n".to_vec();
    if form {
        pdf.extend_from_slice(b"1 0 obj << /Type /Page /Parent 7 0 R /MediaBox [0 0 600 800] /Resources << /XObject << /Fm 5 0 R >> >> /Contents 2 0 R >> endobj\n");
        stream(&mut pdf, 2, b"", b"/Fm Do");
        stream(&mut pdf, 5, b"/Type /XObject /Subtype /Form /BBox [0 0 600 800] /Resources << /Font << /F1 3 0 R >> >>", content);
    } else {
        pdf.extend_from_slice(b"1 0 obj << /Type /Page /Parent 7 0 R /MediaBox [0 0 600 800] /Resources << /Font << /F1 3 0 R >> >> /Contents 2 0 R >> endobj\n");
        stream(&mut pdf, 2, b"", content);
    }
    pdf.extend_from_slice(
        b"3 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 4 0 R >> endobj\n",
    );
    stream(&mut pdf, 4, b"", mapping);
    pdf.extend_from_slice(b"6 0 obj << /Type /Catalog /Pages 7 0 R >> endobj\n7 0 obj << /Type /Pages /Count 1 /Kids [1 0 R] >> endobj\n");
    let xref = pdf.len();
    pdf.extend_from_slice(b"xref\n0 8\n0000000000 65535 f \n");
    for number in 1..=7 {
        let needle = format!("{number} 0 obj");
        if let Some(offset) = pdf[..xref]
            .windows(needle.len())
            .position(|window| window == needle.as_bytes())
        {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        } else {
            pdf.extend_from_slice(b"0000000000 00000 f \n");
        }
    }
    pdf.extend_from_slice(
        format!("trailer << /Size 8 /Root 6 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    pdf
}

/// streamの長さを元byteから記録する。
fn stream(pdf: &mut Vec<u8>, number: usize, dictionary: &[u8], data: &[u8]) {
    pdf.extend_from_slice(format!("{number} 0 obj << /Length {} ", data.len()).as_bytes());
    pdf.extend_from_slice(dictionary);
    pdf.extend_from_slice(b" >> stream\n");
    pdf.extend_from_slice(data);
    pdf.extend_from_slice(b"\nendstream endobj\n");
}

/// 一byteのCMapを作る。
fn cmap(code: u8) -> Vec<u8> {
    format!("1 begincodespacerange\n<00> <FF>\nendcodespacerange\n1 beginbfchar\n<{code:02X}> <6587>\nendbfchar\n").into_bytes()
}

/// 非UTF-8のPDF文字列をToUnicodeへ渡す。
fn content(code: u8) -> Vec<u8> {
    let mut content = b"BT /F1 12 Tf 1 0 0 1 20 100 Tm (".to_vec();
    content.push(code);
    content.extend_from_slice(b") Tj ET");
    content
}

/// ページ本文の置換文字で元codeを破壊しない。
#[test]
fn page_preserves_binary_literal_codes() {
    let budget = DecodeBudget::default();
    let pdf = fixture(&content(0x80), &cmap(0x80), false);
    let texts = extract_text_elements_from_pdf_bytes(&budget, &pdf);
    assert_eq!(
        texts
            .iter()
            .map(|text| text.text.as_str())
            .collect::<Vec<_>>(),
        ["文"]
    );
    assert!(budget.check().is_ok());
}

/// Form本文もfont固有ToUnicodeの元codeを保持する。
#[test]
fn form_preserves_binary_literal_codes() {
    let pdf = fixture(&content(0x80), &cmap(0x80), true);
    let texts = parse_pdf_bytes("binary-form.pdf", &pdf)
        .unwrap()
        .text_elements;
    assert_eq!(
        texts
            .iter()
            .map(|text| text.text.as_str())
            .collect::<Vec<_>>(),
        ["文"]
    );
}

/// コメント・文字列・配列・辞書の偽命令を文字描画と診断しない。
#[test]
fn text_diagnostic_ignores_operator_lookalikes() {
    for source in [
        b"% Tj TJ ' \"\n".as_slice(),
        b"( Tj )",
        b"[ Tj ]",
        b"<< /Note ( TJ ) >>",
    ] {
        let budget = DecodeBudget::default();
        assert!(
            !pdf_bytes_have_text_showing_operators(&budget, &fixture(source, b"", false)),
            "{source:?}"
        );
        assert!(budget.check().is_ok());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// 一byte codeの上位半分もページ/Formで同じ文字へ復号する。
    #[test]
    fn binary_codes_survive_page_and_form(code in 128u8..=255, form in any::<bool>()) {
        let pdf = fixture(&content(code), &cmap(code), form);
        let texts = parse_pdf_bytes("binary-code.pdf", &pdf).unwrap().text_elements;
        prop_assert_eq!(texts.iter().map(|text| text.text.as_str()).collect::<Vec<_>>(), vec!["文"]);
    }
}

/// metadata容量だけを制限する。
fn budget(capacity: u64) -> DecodeBudget {
    DecodeBudget::new(&ParseLimits {
        max_total_metadata_workspace_bytes: capacity,
        ..Default::default()
    })
}

/// Font名と色の保存を容量不足で描画前に止める。
#[test]
fn drawing_state_strings_require_capacity() {
    for (source, capacity, actual) in [
        (b"BT /LongFont 12 Tf (x) Tj ET".as_slice(), 7, 8),
        (b"BT 1 0 0 rg (x) Tj ET", 5, 6),
        (b"BT 0.5 g (x) Tj ET", 5, 6),
        (b"BT 0 0 0 1 k (x) Tj ET", 5, 6),
    ] {
        let target = budget(capacity);
        let mut calls = 0;
        crate::text_draws::visit(&target, source, |_, _, _, _, _| {
            calls += 1;
            0.
        });
        assert_eq!(calls, 0);
        assert!(matches!(target.check(), Err(ParseError::ResourceLimit {
            resource: "metadata workspace bytes", limit, actual: requested,
        }) if limit == capacity && requested == actual));
    }
}

/// qの保存slotは初回全容量を予約し、再走査も累積する。
#[test]
fn text_q_storage_is_exact_and_cumulative() {
    let one = (4 * std::mem::size_of::<(crate::text_draws::State, Option<([f32; 6], [f32; 6])>)>())
        as u64;
    for limit in [one - 1, one, 2 * one - 1, 2 * one] {
        let target = budget(limit);
        let mut calls = 0;
        for scan in 1..=2 {
            crate::text_draws::visit(&target, b"q BT (x) Tj ET Q", |_, _, _, _, _| {
                calls += 1;
                0.
            });
            if limit < one * scan {
                assert_eq!(calls, scan - 1);
                assert!(matches!(target.check(), Err(ParseError::ResourceLimit {
                    resource: "metadata workspace bytes", actual, ..
                }) if actual == one * scan));
                break;
            }
            assert_eq!(calls, scan);
            assert!(target.check().is_ok());
        }
    }
}

/// 予算エラーを消費しても走査を再開せず、TJ配列内でも停止する。
#[test]
fn text_draws_obey_consumed_and_callback_stop() {
    let target = budget(0);
    assert!(target.metadata_vector::<u8>(1).is_none());
    assert!(target.check().is_err());
    let mut calls = 0;
    crate::text_draws::visit(&target, b"BT (x) Tj ET", |_, _, _, _, _| {
        calls += 1;
        0.
    });
    assert_eq!(calls, 0);
    let target = budget(0);
    crate::text_draws::visit(
        &target,
        b"BT [(x) (y) (z)] TJ (w) Tj ET",
        |_, _, _, _, _| {
            calls += 1;
            assert!(target.metadata_vector::<u8>(1).is_none());
            0.
        },
    );
    assert_eq!(calls, 1);
    assert!(target.is_stopped());
}

/// 後続qで停止した場合は先行する文字列も部分結果として返さない。
#[test]
fn text_elements_discard_partial_results_when_stopped() {
    let target = budget(12 + 4 * std::mem::size_of::<ParsedTextElement>() as u64);
    let names = BTreeMap::new();
    let cmaps = BTreeMap::new();
    let metrics = BTreeMap::new();
    assert!(
        extract_text_elements_from_content_stream(
            &target,
            0,
            b"BT /F 12 Tf (first) Tj ET q BT (last) Tj ET Q",
            &names,
            &names,
            &cmaps,
            &ToUnicodeCMap::default(),
            &metrics,
            |_| Default::default(),
        )
        .is_empty()
    );
    assert!(target.check().is_err());
    assert!(target.is_stopped());
}

/// prefixのコメント・文字列・container中のcmを位置と解釈しない。
#[test]
fn text_geometry_prefix_ignores_fake_cm() {
    for suffix in [
        b"% 2 0 0 2 90 90 cm\n".as_slice(),
        b"(2 0 0 2 90 90 cm)",
        b"[2 0 0 2 90 90 cm]",
        b"<< /Note (2 0 0 2 90 90 cm) >>",
    ] {
        let mut prefix = b"1 0 0 1 3 4 cm ".to_vec();
        prefix.extend_from_slice(suffix);
        prefix.extend_from_slice(b" n BT /F 10 Tf (A) Tj ET");
        let target = DecodeBudget::new(&Default::default());
        let matrices =
            crate::form_content::text_matrices(&target, &prefix, [1., 0., 0., 1., 0., 0.]);
        assert_eq!(matrices.get(&prefix.len()), Some(&[1., 0., 0., 1., 3., 4.]));
    }
}

/// UTF-8としても有効なPDF code列をUnicodeへ先行変換しない。
#[test]
fn utf8_shaped_codes_are_still_pdf_codes() {
    let mapping = b"1 begincodespacerange\n<00> <FF>\nendcodespacerange\n2 beginbfchar\n<C3> <7532>\n<A9> <4E59>\nendbfchar\n";
    for form in [false, true] {
        let pdf = fixture(b"BT /F1 12 Tf 20 100 Td (\xC3\xA9) Tj ET", mapping, form);
        let parsed = parse_pdf_bytes("utf8-codes.pdf", &pdf).unwrap();
        assert_eq!(
            parsed
                .text_elements
                .iter()
                .map(|text| text.text.as_str())
                .collect::<Vec<_>>(),
            ["甲乙"]
        );
    }
}

/// 各描画命令でliteral・hex・octalを同じFont CMapへ渡す。
#[test]
fn every_show_operator_preserves_binary_codes() {
    for operand in [
        b"(\x80) Tj".as_slice(),
        b"<80> Tj",
        b"[(\x80) <80>] TJ",
        b"(\\200) '",
        b"0 0 (\x80) \"",
    ] {
        for form in [false, true] {
            let mut content = b"BT /F1 12 Tf 20 100 Td ".to_vec();
            content.extend_from_slice(operand);
            content.extend_from_slice(b" ET");
            let parsed =
                parse_pdf_bytes("binary-show.pdf", &fixture(&content, &cmap(128), form)).unwrap();
            let expected = if operand.starts_with(b"[") {
                "文文"
            } else {
                "文"
            };
            assert_eq!(
                parsed
                    .text_elements
                    .iter()
                    .map(|text| text.text.as_str())
                    .collect::<Vec<_>>(),
                [expected]
            );
        }
    }
}

/// 選択Fontのexternal encodingもhex operandへ引き継ぐ。
#[test]
fn borrowed_operand_uses_selected_encoding() {
    let encodings = BTreeMap::from([("F".into(), "90ms-RKSJ-H".into())]);
    let cmaps = BTreeMap::new();
    assert_eq!(
        decode_text_show_operand(
            &DecodeBudget::default(),
            b"<82A0>",
            &ToUnicodeCMap::default(),
            &encodings,
            &cmaps,
            Some("F")
        ),
        "あ"
    );
}

/// q/Qで保存するFontと色のコピーも累積し、popでslotを再利用する。
#[test]
fn saved_state_strings_are_counted_on_each_q() {
    let slots =
        (4 * std::mem::size_of::<(crate::text_draws::State, Option<([f32; 6], [f32; 6])>)>())
            as u64;
    let first = slots + 14;
    for limit in [first - 1, first, first + 6, first + 7] {
        let target = budget(limit);
        let mut calls = 0;
        crate::text_draws::visit(
            &target,
            b"/F 12 Tf 0.5 g q BT (x) Tj ET Q q BT (y) Tj ET Q",
            |_, _, _, state, _| {
                calls += 1;
                assert_eq!(state.font.as_deref(), Some("F"));
                assert_eq!(state.color.as_deref(), Some("808080"));
                0.
            },
        );
        assert_eq!(
            calls,
            if limit < first {
                0
            } else if limit < first + 7 {
                1
            } else {
                2
            }
        );
        assert_eq!(target.is_stopped(), limit < first + 7);
    }
}

/// 数値operandと大きな文字列operandは元本文を借用し、metadata容量0で走査する。
#[test]
fn text_operands_borrow_source_without_temporary_show_copy() {
    let mut source = b"BT 1 0 0 1 20 30 Tm (".to_vec();
    source.extend(std::iter::repeat_n(b'x', 65536));
    source.extend_from_slice(b") Tj ET");
    let target = budget(0);
    let mut calls = 0;
    crate::text_draws::visit(&target, &source, |_, _, raw, _, matrix| {
        calls += 1;
        assert_eq!(raw.len(), 65538);
        assert!(raw.as_ptr() >= source.as_ptr());
        assert!(raw.as_ptr_range().end <= source.as_ptr_range().end);
        assert_eq!(matrix, [1., 0., 0., 1., 20., 30.]);
        0.
    });
    assert_eq!(calls, 1);
    assert!(target.check().is_ok());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    /// q列の初回・倍増容量を独立モデルと照合する。
    #[test]
    fn q_capacity_growth_matches_model(count in 1usize..13, short in any::<bool>()) {
        let slot = std::mem::size_of::<(crate::text_draws::State, Option<([f32; 6], [f32; 6])>)>() as u64;
        let mut capacity = 0;
        let mut total = 0;
        for index in 0..count {
            if index == capacity {
                capacity = (2 * capacity).max(4);
                total += capacity as u64 * slot;
            }
        }
        let target = budget(total - u64::from(short));
        let source = format!("{}BT (x) Tj ET {}", "q ".repeat(count), "Q ".repeat(count));
        let mut calls = 0;
        crate::text_draws::visit(&target, source.as_bytes(), |_, _, _, _, _| { calls += 1; 0. });
        prop_assert_eq!(calls, usize::from(!short));
        prop_assert_eq!(target.is_stopped(), short);
        if short {
            let exceeded = matches!(target.check(), Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", actual, .. }) if actual == total);
            prop_assert!(exceeded);
        } else { prop_assert!(target.check().is_ok()); }
    }
}
