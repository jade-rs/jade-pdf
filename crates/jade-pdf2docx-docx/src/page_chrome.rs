//! 反復見出しを文字列だけで統合せず、書式と各ページの出現位置を保持する。
use super::*;
use docx_rs::BuildXML;

/// 各原本ページから実際に移すheader/footerの一覧。
pub(super) struct Plan {
    /// ページ番号ごとの移動対象行。
    pages: BTreeMap<usize, RepeatedPageChrome>,
    /// 検出されないページ用の空の設定。
    empty: RepeatedPageChrome,
}

impl Plan {
    /// 指定ページだけの見出しを返す。
    pub(super) fn page(&self, page: usize) -> &RepeatedPageChrome {
        self.pages.get(&page).unwrap_or(&self.empty)
    }

    /// 全ページが同じWord見出しと端からの距離を共有できるか判定する。
    pub(super) fn is_uniform(&self) -> bool {
        let mut keys = self
            .pages
            .values()
            .map(|chrome| (chrome.clear_header, chrome.clear_footer, output_key(chrome)));
        let first = keys.next();
        keys.all(|key| Some(key) == first)
    }
}

/// 原本の異なるrun分割を正規化した、実際のWord書式と文字列の列。
fn profile(line: &StructuredTextLine) -> (String, Vec<(Vec<u8>, String)>) {
    let mut runs: Vec<(Vec<u8>, String)> = Vec::new();
    for run in &line.runs {
        let properties = docx_run_from_structured_run(run).run_property.build();
        if let Some(last) = runs.last_mut().filter(|last| last.0 == properties) {
            last.1.push_str(&run.text);
        } else {
            runs.push((properties, run.text.clone()));
        }
    }
    (format!("{:?}", line.alignment), runs)
}

/// 自動生成される段落IDを含めず、書式とページ絶対位置を比較する。
#[derive(PartialEq, Eq)]
struct OutputLine {
    /// 実際に出力するrunの書式・文字列と段落の整列。
    style: (String, Vec<(Vec<u8>, String)>),
    /// 原本ページ端からの縦距離。
    edge: i32,
    /// 原本の左端をWordと同じ単位へ丸めた値。
    left: Option<i32>,
    /// 原本の右端をWordと同じ単位へ丸めた値。
    right: Option<i32>,
    /// sectionと同じ表示ページ幅。
    width: Option<u32>,
}

/// 書式と位置が同じ出現だけを、共有可能な見出しとして扱う。
fn output_key(chrome: &RepeatedPageChrome) -> Vec<Option<OutputLine>> {
    [&chrome.header, &chrome.footer]
        .into_iter()
        .map(|line| {
            line.as_ref().map(|line| OutputLine {
                style: profile(&line.line),
                edge: line.edge_distance_twips,
                left: pdf_points_to_indent_twips(line.line.left_x),
                right: pdf_points_to_indent_twips(line.line.right_x),
                width: line.page_width_twips,
            })
        })
        .collect()
}

/// 原本の一出現について、文字・書式と各ページ端からの距離を共通の方法で保持する。
fn occurrence(
    parsed: &ParsedDocument,
    page: usize,
    line: &StructuredTextLine,
    header: bool,
) -> Option<RepeatedPageChromeLine> {
    let parsed_page = parsed.pages.iter().find(|p| p.page_number == page)?;
    let (width, height) = parsed_page_display_size_points(parsed_page);
    let distance = if header {
        height - line.top_y
    } else {
        line.top_y - structured_line_max_font_size(line)
    };
    Some(RepeatedPageChromeLine {
        line: line.clone(),
        edge_distance_twips: pdf_points_to_floor_twips(distance.max(0.))?,
        right_indent_twips: pdf_points_to_indent_twips((width - line.right_x).max(0.))?,
        page_width_twips: parsed_page_size_twips(parsed_page).map(|size| size.0),
    })
}

/// 全ページで見つかる文字列のうち、同じ書式が複数ページにある行だけを移す。
pub(super) fn extract(parsed: &ParsedDocument, pages: &[StructuredTextPage]) -> Plan {
    let mut plan = Plan {
        pages: pages
            .iter()
            .map(|page| (page.page, RepeatedPageChrome::default()))
            .collect(),
        empty: RepeatedPageChrome::default(),
    };
    if pages.len() < 2 {
        return plan;
    }
    for header in [true, false] {
        let boundary = |page, line: &StructuredTextLine| {
            if header {
                is_pdf_header_line(parsed, page, line)
            } else {
                is_pdf_footer_line(parsed, page, line)
            }
        };
        let mut counts: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
        for page in pages {
            for line in &page.lines {
                if !line.text.trim().is_empty() && boundary(page.page, line) {
                    counts
                        .entry(line.text.trim().into())
                        .or_default()
                        .insert(page.page);
                }
            }
        }
        let Some(text) = counts
            .into_iter()
            .find_map(|(text, seen)| (seen.len() == pages.len()).then_some(text))
        else {
            continue;
        };
        let mut profiles = BTreeMap::new();
        for page in pages {
            for line in &page.lines {
                if line.text.trim() == text && boundary(page.page, line) {
                    profiles
                        .entry(profile(line))
                        .or_insert_with(BTreeSet::new)
                        .insert(page.page);
                }
            }
        }
        for page in pages {
            let Some(line) = page
                .lines
                .iter()
                .filter(|line| line.text.trim() == text && boundary(page.page, line))
                .filter(|line| profiles[&profile(line)].len() >= 2)
                .max_by_key(|line| profiles[&profile(line)].len())
            else {
                continue;
            };
            let Some(chrome) = occurrence(parsed, page.page, line, header) else {
                continue;
            };
            let target = plan.pages.get_mut(&page.page).expect("source page exists");
            if header {
                target.header = Some(chrome);
            } else {
                target.footer = Some(chrome);
            }
        }
    }
    // 既存の反復footerと競合させず、全出現を組み立てられた場合にだけ連番footerを採用する。
    if plan.pages.values().all(|chrome| chrome.footer.is_none()) {
        let selected = numbered_page_footers::select(parsed, pages);
        let occurrences = selected
            .iter()
            .map(|(page, line)| occurrence(parsed, *page, line, false).map(|c| (*page, c)))
            .collect::<Option<Vec<_>>>();
        if let Some(occurrences) = occurrences {
            for (page, footer) in occurrences {
                plan.pages
                    .get_mut(&page)
                    .expect("source page exists")
                    .footer = Some(footer);
            }
        }
    }
    let (mut seen_header, mut seen_footer) = (false, false);
    for page in pages {
        let chrome = plan.pages.get_mut(&page.page).expect("source page exists");
        chrome.clear_header = seen_header && chrome.header.is_none();
        chrome.clear_footer = seen_footer && chrome.footer.is_none();
        seen_header |= chrome.header.is_some();
        seen_footer |= chrome.footer.is_some();
    }
    plan
}

/// 選んだ出現だけを本文から除き、同文の題名や他の本文行を残す。
pub(super) fn remove(mut pages: Vec<StructuredTextPage>, plan: &Plan) -> Vec<StructuredTextPage> {
    for page in &mut pages {
        let chrome = plan.page(page.page);
        page.lines.retain(|line| {
            ![&chrome.header, &chrome.footer]
                .into_iter()
                .any(|selected| {
                    selected
                        .as_ref()
                        .is_some_and(|selected| selected.line == *line)
                })
        });
    }
    pages
}
