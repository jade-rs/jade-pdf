//! Rootのない互換PDFのResourcesと幾何属性を共有索引で検査する。
use crate::{
    ParseError, ParseLimits,
    decode::DecodeBudget,
    limits::check_limit,
    metadata,
    metadata_workspace::{Set, ensure_active, vector},
    page_attributes::{Objects, dictionary_value, is_null, resolve_value},
};
use std::path::Path;

/// 互換ページのResourcesと幾何属性を検査し、保存容量も予算へ予約する。
pub(crate) fn validate(
    path: &Path,
    source: &[u8],
    limits: &ParseLimits,
    budget: &DecodeBudget,
) -> Result<Vec<crate::legacy_page_geometry_validation::Geometry>, ParseError> {
    ensure_active(budget)?;
    let mut pages = metadata::page_iter(source).peekable();
    if pages.peek().is_none() {
        return Ok(Vec::new());
    }
    let objects = crate::object_index::build(budget, source);
    budget.check()?;
    ensure_active(budget)?;
    let objects = objects.ok_or_else(|| invalid(path, "resource index unavailable"))?;
    let mut geometry = vector(budget, metadata::page_iter(source).count())?;
    for page in pages {
        validate_page(path, &objects, page, limits, budget)?;
        geometry.push(crate::legacy_page_geometry_validation::resolve(
            path, &objects, page, limits, budget,
        )?);
    }
    Ok(geometry)
}

/// 最寄りの非null辞書を選び、親の深度・世代・循環も検査する。
fn validate_page<'a>(
    path: &Path,
    objects: &Objects<'a>,
    mut node: &'a [u8],
    limits: &ParseLimits,
    budget: &DecodeBudget,
) -> Result<(), ParseError> {
    let mut visited = Set::default();
    let mut depth = 0u64;
    loop {
        if let Some(resources) = metadata::value(node, b"Resources") {
            let (resources, _) = resolve_value(path, objects, resources, limits, budget)?;
            if !is_null(resources) {
                let resources = dictionary_value(path, resources)?;
                return crate::page_resources::validate(path, objects, resources, limits, budget);
            }
        }
        let Some(parent) = metadata::value(node, b"Parent") else {
            return Ok(());
        };
        if is_null(parent) {
            return Ok(());
        }
        let reference = metadata::reference_with_generation(parent)
            .ok_or_else(|| invalid(path, "resource parent is not an indirect reference"))?;
        depth += 1;
        check_limit(
            "page inheritance depth",
            depth,
            limits.max_container_nesting as u64,
        )?;
        if !visited.insert(budget, reference)? {
            return Err(invalid(path, "cyclic resource parent reference"));
        }
        let (_, body) = objects
            .get(&reference.0)
            .filter(|(generation, _)| *generation == reference.1)
            .ok_or_else(|| invalid(path, "missing or wrong-generation resource parent"))?;
        node = dictionary_value(path, body)?;
    }
}

/// 継承の破損を公開APIが判別できる構造エラーとして返す。
fn invalid(path: &Path, reason: &'static str) -> ParseError {
    ParseError::InvalidPdfStructure {
        path: path.into(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 検査済み幾何属性の保存容量も索引と同じ予算へ確保前に予約する。
    #[test]
    fn geometry_storage_is_charged_before_allocation() {
        let source = b"1 0 obj << /Type /Page >> endobj";
        let probe = DecodeBudget::new(&ParseLimits {
            max_total_metadata_workspace_bytes: 0,
            ..Default::default()
        });
        assert!(crate::object_index::build(&probe, source).is_none());
        let Err(ParseError::ResourceLimit {
            actual: index_bytes,
            ..
        }) = probe.check()
        else {
            panic!("索引容量が未検査");
        };
        let limits = ParseLimits {
            max_total_metadata_workspace_bytes: index_bytes,
            ..Default::default()
        };
        let budget = DecodeBudget::new(&limits);
        assert!(matches!(
            validate(Path::new("geometry-budget.pdf"), source, &limits, &budget),
            Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", limit, actual }) if limit == index_bytes && actual > limit
        ));
        let total =
            index_bytes + size_of::<crate::legacy_page_geometry_validation::Geometry>() as u64;
        for limit in [total - 1, total] {
            let limits = ParseLimits {
                max_total_metadata_workspace_bytes: limit,
                ..Default::default()
            };
            let budget = DecodeBudget::new(&limits);
            let result = validate(Path::new("geometry-budget.pdf"), source, &limits, &budget);
            assert_eq!(result.is_ok(), limit == total);
            if let Ok(geometry) = result {
                assert_eq!(geometry.len(), 1);
            } else {
                let _ = budget.check();
                assert!(validate(Path::new("stopped.pdf"), b"", &limits, &budget).is_err());
            }
        }
    }

    /// 索引の確保前にmetadata予算を検査し、失敗後も処理を再開しない。
    #[test]
    fn index_budget_failure_is_sticky() {
        let limits = ParseLimits {
            max_total_metadata_workspace_bytes: 0,
            ..Default::default()
        };
        let budget = DecodeBudget::new(&limits);
        let source = b"1 0 obj << /Type /Page /Resources << >> >> endobj";
        assert!(matches!(
            validate(Path::new("budget.pdf"), source, &limits, &budget),
            Err(ParseError::ResourceLimit { resource: "metadata workspace bytes", limit: 0, actual }) if actual > 0
        ));
        let _ = budget.check();
        assert!(validate(Path::new("budget.pdf"), b"", &limits, &budget).is_err());
    }

    /// ページがない互換PDFはResources用の索引を確保しない。
    #[test]
    fn no_pages_need_no_metadata_capacity() {
        let limits = ParseLimits {
            max_total_metadata_workspace_bytes: 0,
            ..Default::default()
        };
        let budget = DecodeBudget::new(&limits);
        assert!(
            validate(
                Path::new("empty.pdf"),
                b"1 0 obj << >> endobj",
                &limits,
                &budget
            )
            .is_ok()
        );
        assert!(budget.check().is_ok());
    }
}
