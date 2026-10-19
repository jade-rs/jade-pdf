//! 幅表の allocation 予算、共有と公開 API の超過通知を検証する。
use jade_pdf2docx_parser::{ParseError, ParseLimits, parse_pdf_bytes_with_limits};
use proptest::prelude::*;

#[path = "support/page_attribute_pdf.rs"]
mod fixture;

/// Identity-H の幅表 payload は unsigned 16 bit CID ごとの f32 である。
const CID_BYTES: u64 = 65_536 * 4;
/// simple Font の幅表 payload は一 byte code ごとの f32 である。
const SIMPLE_BYTES: u64 = 256 * 4;

/// 指定数の間接 Font と二ページを持つ文書を生成する。
fn document(count: usize, aliases: usize, simple: bool, default_only: bool) -> Vec<String> {
    let mut fonts = String::new();
    for index in 0..count {
        for alias in 0..aliases {
            fonts.push_str(&format!(" /F{index}_{alias} {} 0 R", 10 + index * 2));
        }
    }
    let mut objects = fixture::pages(&format!(
        "/MediaBox [0 0 100 100] /Resources << /Font << {fonts} >> >>"
    ));
    objects[4] = fixture::stream("");
    objects[5] = fixture::stream("");
    for index in 0..count {
        objects.push(if simple {
            "<< /Type /Font /Subtype /Type1 /BaseFont /Custom /FirstChar 65 /LastChar 65 /Widths [600] >>".into()
        } else {
            format!("<< /Type /Font /Subtype /Type0 /BaseFont /Custom /Encoding /Identity-H /DescendantFonts [{} 0 R] >>", 11 + index * 2)
        });
        objects.push(if default_only {
            "<< /Type /Font /Subtype /CIDFontType2 /DW 600 >>".into()
        } else {
            "<< /Type /Font /Subtype /CIDFontType2 /W [65 [600]] >>".into()
        });
    }
    objects
}

/// 文書ごとに新しい公開 API の予算を作り解析する。
fn parse(objects: &[String], limit: u64) -> Result<(), ParseError> {
    parse_pdf_bytes_with_limits(
        "font-budget.pdf",
        &fixture::pdf(objects),
        &ParseLimits {
            max_total_font_metrics_bytes: limit,
            ..Default::default()
        },
    )
    .map(|_| ())
}

/// 超過を fallback で隠さず、資源名と inclusive 境界を公開する。
fn assert_limit(result: Result<(), ParseError>, limit: u64, actual: u64) {
    assert!(
        matches!(result, Err(ParseError::ResourceLimit {
        resource: "font metrics bytes", limit: found_limit, actual: found_actual,
    }) if found_limit == limit && found_actual == actual),
        "{result:?}"
    );
}

/// CID 幅表の一回の確保は上限ちょうどを許可し、一 byte 下では拒否する。
#[test]
fn cid_width_payload_limit_is_inclusive() {
    let objects = document(1, 1, false, false);
    assert!(parse(&objects, CID_BYTES).is_ok());
    assert_limit(parse(&objects, CID_BYTES - 1), CID_BYTES - 1, CID_BYTES);
}

/// 個々の Font が上限以下でも異なる幅表の合計が超えたら拒否する。
#[test]
fn distinct_fonts_consume_cumulative_budget() {
    assert_limit(
        parse(&document(2, 1, false, false), CID_BYTES),
        CID_BYTES,
        CID_BYTES * 2,
    );
}

/// ページと別名で共有する幅表は再加算しない。
#[test]
fn shared_font_aliases_are_charged_once() {
    assert!(parse(&document(1, 8, false, false), CID_BYTES).is_ok());
}

/// simple Font の256幅にも同じ累積予算を適用する。
#[test]
fn simple_width_payload_is_bounded() {
    let objects = document(1, 1, true, false);
    assert!(parse(&objects, SIMPLE_BYTES).is_ok());
    assert_limit(
        parse(&objects, SIMPLE_BYTES - 1),
        SIMPLE_BYTES - 1,
        SIMPLE_BYTES,
    );
}

/// DW のみの CID Font は幅表を確保しないためゼロ予算でも読める。
#[test]
fn default_only_cid_does_not_allocate_width_payload() {
    assert!(parse(&document(1, 1, false, true), 0).is_ok());
}

/// 上限超過後に別の解析を始めれば予算と停止状態は初期化される。
#[test]
fn failed_parse_does_not_poison_later_budget() {
    let objects = document(1, 1, false, false);
    assert_limit(parse(&objects, 0), 0, CID_BYTES);
    assert!(parse(&objects, CID_BYTES).is_ok());
}

/// Form の局所 Font も本文の幅表と同じ公開予算を使う。
#[test]
fn form_local_font_limit_is_not_hidden() {
    let mut objects = document(1, 1, false, false);
    objects[1] = "<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] /MediaBox [0 0 100 100] /Resources << /XObject << /Fm 7 0 R >> >> >>".into();
    objects[4] = fixture::stream("/Fm Do");
    let content = "BT /F 10 Tf <0041> Tj ET";
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Resources << /Font << /F 10 0 R >> >> /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    assert!(parse(&objects, CID_BYTES).is_ok());
    assert_limit(parse(&objects, CID_BYTES - 1), CID_BYTES - 1, CID_BYTES);
}

/// 同じ直接辞書でもページごとの確保は全て累積へ加算する。
#[test]
fn inline_font_widths_are_charged_for_each_allocation() {
    let mut objects = document(0, 0, true, false);
    objects[1] = "<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] /MediaBox [0 0 100 100] /Resources << /Font << /F << /Type /Font /Subtype /Type1 /FirstChar 65 /Widths [600] >> >> >> >>".into();
    assert!(parse(&objects, SIMPLE_BYTES * 2).is_ok());
    assert_limit(
        parse(&objects, SIMPLE_BYTES),
        SIMPLE_BYTES,
        SIMPLE_BYTES * 2,
    );
}

/// 別々に再作成するページと Form の幅表も解析共通の予算へ加算する。
#[test]
fn page_and_form_share_one_cumulative_budget() {
    let mut objects = document(1, 1, false, false);
    objects[1] = "<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] /MediaBox [0 0 100 100] /Resources << /Font << /F 10 0 R >> /XObject << /Fm 7 0 R >> >> >>".into();
    objects[4] = fixture::stream("/Fm Do");
    let content = "BT /F 10 Tf <0041> Tj ET";
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    assert!(parse(&objects, CID_BYTES * 2).is_ok());
    assert_limit(parse(&objects, CID_BYTES), CID_BYTES, CID_BYTES * 2);
}

/// ファイル API でも native backend に入る前に幅表予算の超過を返す。
#[test]
fn file_api_applies_font_metrics_limit() {
    let path = std::env::temp_dir().join(format!(
        "jade-pdf2docx-font-budget-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, fixture::pdf(&document(1, 1, false, false))).unwrap();
    let result = jade_pdf2docx_parser::parse_pdf_with_limits(
        &path,
        &ParseLimits {
            max_total_font_metrics_bytes: CID_BYTES - 1,
            ..Default::default()
        },
    );
    std::fs::remove_file(path).unwrap();
    assert_limit(result.map(|_| ()), CID_BYTES - 1, CID_BYTES);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    /// 異なる幅表だけを数え、resource 別名の数に予算を左右されない。
    #[test]
    fn budget_matches_unique_font_count(count in 1usize..6, aliases in 1usize..5, simple in any::<bool>()) {
        let bytes = if simple { SIMPLE_BYTES } else { CID_BYTES };
        let total = count as u64 * bytes;
        let objects = document(count, aliases, simple, false);
        prop_assert!(parse(&objects, total).is_ok());
        assert_limit(parse(&objects, total - 1), total - 1, total);
    }
}

/// W の一つの assignment と unsigned 16 bit CID の successor 配列のサイズ。
fn workspace_bytes(assignments: usize) -> u64 {
    (assignments * std::mem::size_of::<(usize, usize, f32)>()
        + 65_537 * std::mem::size_of::<usize>()) as u64
}

/// 解析全体へ CID 幅表の一時配列予算を渡す。
fn parse_workspace(objects: &[String], limit: u64) -> Result<(), ParseError> {
    parse_pdf_bytes_with_limits(
        "font-workspace.pdf",
        &fixture::pdf(objects),
        &ParseLimits {
            max_total_font_width_workspace_bytes: limit,
            ..Default::default()
        },
    )
    .map(|_| ())
}

/// payload とは独立した一時配列の inclusive 境界を検証する。
#[test]
fn cid_workspace_limit_is_inclusive() {
    let objects = document(1, 1, false, false);
    let bytes = workspace_bytes(1);
    assert!(parse_workspace(&objects, bytes).is_ok());
    assert!(matches!(parse_workspace(&objects, bytes - 1),
        Err(ParseError::ResourceLimit { resource: "font width workspace bytes", limit, actual })
        if limit == bytes - 1 && actual == bytes));
}

/// 別の Font で解放・再作成する workspace も累積で数える。
#[test]
fn distinct_fonts_consume_cumulative_workspace() {
    let bytes = workspace_bytes(1);
    assert!(parse_workspace(&document(2, 1, false, false), bytes * 2).is_ok());
    assert!(
        matches!(parse_workspace(&document(2, 1, false, false), bytes),
        Err(ParseError::ResourceLimit { resource: "font width workspace bytes", limit, actual })
        if limit == bytes && actual == bytes * 2)
    );
}

/// ページと別名の cache hit は workspace を再作成しない。
#[test]
fn shared_font_has_one_workspace_charge() {
    assert!(parse_workspace(&document(1, 8, false, false), workspace_bytes(1)).is_ok());
}

/// 配列幅と範囲幅が混在する W の assignment 数を独立に数える。
#[test]
fn mixed_widths_charge_each_workspace_assignment() {
    let mut objects = document(1, 1, false, false);
    objects[10] =
        "<< /Type /Font /Subtype /CIDFontType2 /W [0 [500 600 700] 10 65535 800] >>".into();
    let bytes = workspace_bytes(4);
    assert!(parse_workspace(&objects, bytes).is_ok());
    assert!(matches!(parse_workspace(&objects, bytes - 1),
        Err(ParseError::ResourceLimit { resource: "font width workspace bytes", actual, .. })
        if actual == bytes));
}

/// W なし・空 W・空の個別幅と simple Widths は heap workspace を使わない。
#[test]
fn stack_or_empty_widths_need_no_heap_workspace() {
    assert!(parse_workspace(&document(1, 1, true, false), 0).is_ok());
    assert!(parse_workspace(&document(1, 1, false, true), 0).is_ok());
    for w in ["[]", "[65 []]"] {
        let mut objects = document(1, 1, false, false);
        objects[10] = format!("<< /Type /Font /Subtype /CIDFontType2 /W {w} >>");
        assert!(parse_workspace(&objects, 0).is_ok());
    }
}

/// 失敗した解析の累積 workspace と停止状態を次の解析へ持ち越さない。
#[test]
fn workspace_failure_is_local_to_one_parse() {
    let objects = document(1, 1, false, false);
    assert!(parse_workspace(&objects, 0).is_err());
    assert!(parse_workspace(&objects, workspace_bytes(1)).is_ok());
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 24, failure_persistence: None, ..ProptestConfig::default() })]
    /// 範囲ごとに一つ、配列の幅ごとに一つの assignment が必要となる。
    #[test]
    fn workspace_matches_width_group_model(groups in prop::collection::vec((1usize..16, any::<bool>()), 1..16)) {
        let mut objects = document(1, 1, false, false);
        let mut entries = String::new();
        let mut count = 0;
        for (widths, array) in groups {
            if array {
                entries.push_str(&format!("0 [{}] ", "500 ".repeat(widths)));
                count += widths;
            } else {
                entries.push_str("0 65535 600 ");
                count += 1;
            }
        }
        objects[10] = format!("<< /Type /Font /Subtype /CIDFontType2 /W [{entries}] >>");
        let bytes = workspace_bytes(count);
        prop_assert!(parse_workspace(&objects, bytes).is_ok());
        prop_assert!(matches!(parse_workspace(&objects, bytes-1), Err(ParseError::ResourceLimit {resource: "font width workspace bytes", actual, ..}) if actual == bytes), "workspace limit was lost");
    }
}

/// Form とページで再作成する workspace も解析全体の上限で拒否する。
#[test]
fn form_and_page_share_one_workspace_budget() {
    let mut objects = document(1, 1, false, false);
    objects[1] = "<< /Type /Pages /Count 2 /Kids [3 0 R 4 0 R] /MediaBox [0 0 100 100] /Resources << /Font << /F 10 0 R >> /XObject << /Fm 7 0 R >> >> >>".into();
    objects[4] = fixture::stream("/Fm Do");
    let content = "BT /F 10 Tf <0041> Tj ET";
    objects[6] = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    let bytes = workspace_bytes(1);
    assert!(parse_workspace(&objects, bytes * 2).is_ok());
    assert!(
        matches!(parse_workspace(&objects, bytes), Err(ParseError::ResourceLimit {
        resource: "font width workspace bytes", actual, ..
    }) if actual == bytes * 2)
    );
}

/// ファイル API も Pdfium へ渡す前に一時配列の超過を拒否する。
#[test]
fn file_api_applies_font_width_workspace_limit() {
    let path = std::env::temp_dir().join(format!(
        "jade-pdf2docx-font-workspace-budget-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, fixture::pdf(&document(1, 1, false, false))).unwrap();
    let result = jade_pdf2docx_parser::parse_pdf_with_limits(
        &path,
        &ParseLimits {
            max_total_font_width_workspace_bytes: 0,
            ..Default::default()
        },
    );
    std::fs::remove_file(path).unwrap();
    assert!(matches!(result, Err(ParseError::ResourceLimit {
        resource: "font width workspace bytes", actual, ..
    }) if actual == workspace_bytes(1)));
}
