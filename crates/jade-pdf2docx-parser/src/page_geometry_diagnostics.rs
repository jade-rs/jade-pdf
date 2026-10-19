//! ページ寸法の欠落を、元ページ番号と共有保存予算に従って報告する。
use crate::{ParseError, ParsedPage, QualityIssue, QualityReport, decode::DecodeBudget};

/// 昇順の既知ページを一度走査し、寸法のないページを診断する。
pub(crate) fn collect(
    budget: &DecodeBudget,
    page_count: usize,
    pages: &[ParsedPage],
    report: &mut QualityReport,
) -> Result<(), ParseError> {
    crate::metadata_workspace::ensure_active(budget)?;
    let mut known = pages.iter().peekable();
    for page in 0..page_count {
        if known.peek().is_some_and(|known| known.page_number == page) {
            known.next();
        } else {
            crate::metadata_workspace::push(
                budget,
                &mut report.issues,
                QualityIssue::MissingPageGeometry { page },
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ParseLimits;

    /// 診断保存容量だけを制限する予算を作る。
    fn budget(bytes: u64) -> DecodeBudget {
        DecodeBudget::new(&ParseLimits {
            max_total_metadata_workspace_bytes: bytes,
            ..Default::default()
        })
    }

    /// 保存容量の境界はinclusiveで、失敗した予算を再利用できない。
    #[test]
    fn diagnostics_charge_capacity_and_keep_failure_sticky() {
        let required = (4 * std::mem::size_of::<QualityIssue>()) as u64;
        for capacity in [0, required - 1] {
            let target = budget(capacity);
            let mut report = QualityReport::default();
            assert!(matches!(
                collect(&target, 1, &[], &mut report),
                Err(ParseError::ResourceLimit {
                    resource: "metadata workspace bytes",
                    ..
                })
            ));
            assert!(report.issues.is_empty());
            let _ = target.check();
            assert!(collect(&target, 0, &[], &mut report).is_err());
        }
        let mut report = QualityReport::default();
        collect(&budget(required), 4, &[], &mut report).unwrap();
        assert_eq!(report.issues.len(), 4);
    }

    /// 空文書と全ページ既知の文書では診断保存を確保しない。
    #[test]
    fn known_geometry_needs_no_diagnostic_capacity() {
        let mut report = QualityReport::default();
        let pages = [ParsedPage {
            page_number: 0,
            width: 100.0,
            height: 200.0,
            rotation: 0,
        }];
        collect(&budget(0), 0, &[], &mut report).unwrap();
        collect(&budget(0), 1, &pages, &mut report).unwrap();
        assert!(report.issues.is_empty());
    }

    /// 拡張は置換先容量も累積予約し、容量不足後は再収集を停止する。
    #[test]
    fn expanded_diagnostics_charge_both_vectors() {
        let required = (12 * std::mem::size_of::<QualityIssue>()) as u64;
        let target = budget(required - 1);
        let mut report = QualityReport::default();
        assert!(collect(&target, 5, &[], &mut report).is_err());
        assert_eq!(report.issues.len(), 4);
        let _ = target.check();
        assert!(collect(&target, 0, &[], &mut report).is_err());
        let mut report = QualityReport::default();
        collect(&budget(required), 5, &[], &mut report).unwrap();
        assert_eq!(report.issues.len(), 5);
    }
}
