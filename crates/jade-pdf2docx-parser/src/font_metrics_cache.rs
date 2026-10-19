//! 不変の object index 内で、間接 Font の幅表を一度だけ展開する。
use crate::resource_cache;
use crate::{font_widths::Metrics, page_attributes::Objects};
use std::sync::Arc;

/// 単一 PDF の object index に結び付いた Font metrics cache。
pub(crate) struct Cache<'objects, 'bytes> {
    /// 解決済み generation と辞書本文を持つ不変 index。
    objects: &'objects Objects<'bytes>,
    /// ページと Form をまたいで allocation の累積量と失敗を共有する。
    budget: &'objects crate::decode::DecodeBudget,
    /// 最終 Font object number ごとの幅表。局所 resource 名はキーにしない。
    metrics: resource_cache::Cache<Arc<Metrics>>,
    /// stream 境界の検証対象になった元 PDF。
    source: &'objects [u8],
}

impl<'objects, 'bytes> Cache<'objects, 'bytes> {
    /// 同じ index を使うページや Form の resource 解決に限定して共有する。
    pub(crate) fn new(
        objects: &'objects Objects<'bytes>,
        budget: &'objects crate::decode::DecodeBudget,
        source: &'objects [u8],
    ) -> Self {
        Self {
            objects,
            budget,
            source,
            metrics: resource_cache::Cache::default(),
        }
    }

    /// cache と同じ index で直接 Font 辞書の参照先も解決する。
    pub(crate) fn objects(&self) -> &'objects Objects<'bytes> {
        self.objects
    }

    /// 検査済みの最終 Font object の幅表を取得し、初回だけ読み込む。
    pub(crate) fn get(&mut self, number: u32) -> Option<Arc<Metrics>> {
        if self.budget.is_stopped() {
            return None;
        }
        if let Some(metrics) = self.metrics.get(&number) {
            return Some(Arc::clone(metrics));
        }
        let (_, dictionary) = self.objects.get(&number)?;
        self.metrics.prepare(self.budget, number)?;
        let metrics = self.read(dictionary)?;
        self.metrics
            .insert(self.budget, number, Arc::clone(&metrics))?;
        Some(metrics)
    }
    /// 直接Fontも本体とArc counterの容量を予約し、停止時には部分幅表を返さない。
    pub(crate) fn read(&self, dictionary: &[u8]) -> Option<Arc<Metrics>> {
        if self.budget.is_stopped() {
            return None;
        }
        self.budget.charge_resource_workspace_bytes(
            (std::mem::size_of::<Metrics>() + 2 * std::mem::size_of::<usize>()) as u64,
        )?;
        let metrics = Metrics::read(self.objects, dictionary, self.budget, self.source);
        if self.budget.is_stopped() {
            return None;
        }
        self.budget.budgeted_arc(metrics)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParseError, ParseLimits, decode::DecodeBudget};

    /// 幅表を持たない Font も cache 本体の容量超過を公開エラーにする。
    #[test]
    fn denied_cache_storage_stops_before_font_creation() {
        let objects = Objects::from([(
            1,
            (
                0,
                b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".as_slice(),
            ),
        )]);
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_resource_workspace_bytes: 0,
            ..Default::default()
        });
        let mut cache = Cache::new(&objects, &budget, &[]);
        assert!(cache.get(1).is_none());
        assert!(cache.metrics.is_empty());
        assert!(
            matches!(budget.check(), Err(ParseError::ResourceLimit { resource: "resource workspace bytes", limit: 0, actual }) if actual > 0)
        );
        assert!(cache.get(1).is_none());
    }
    /// 最初の slot 予約量を、確保を禁止した公開エラーから取得する。
    fn slot_bytes(objects: &Objects<'_>) -> u64 {
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_resource_workspace_bytes: 0,
            ..Default::default()
        });
        let mut cache = Cache::new(objects, &budget, &[]);
        assert!(cache.get(1).is_none());
        match budget.check().unwrap_err() {
            ParseError::ResourceLimit { actual, .. } => actual,
            error => panic!("unexpected {error}"),
        }
    }

    /// slot と Arc 本体の境界は inclusive で、共有 hit と欠落 object は再予約しない。
    #[test]
    fn font_body_budget_is_inclusive_and_alias_hits_share_allocation() {
        let objects = Objects::from([(
            1,
            (
                0,
                b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".as_slice(),
            ),
        )]);
        let capacity = slot_bytes(&objects)
            + (std::mem::size_of::<Metrics>() + 2 * std::mem::size_of::<usize>()) as u64;
        for limit in [capacity - 1, capacity] {
            let budget = DecodeBudget::new(&ParseLimits {
                max_total_resource_workspace_bytes: limit,
                ..Default::default()
            });
            let mut cache = Cache::new(&objects, &budget, &[]);
            assert!(cache.get(99).is_none());
            let metrics = cache.get(1);
            assert_eq!(metrics.is_some(), limit == capacity);
            if let Some(metrics) = metrics {
                for _ in 0..64 {
                    assert!(Arc::ptr_eq(&metrics, &cache.get(1).unwrap()));
                }
                assert!(budget.check().is_ok());
                let mut separate = Cache::new(&objects, &budget, &[]);
                assert!(separate.get(1).is_none());
            } else {
                assert!(cache.metrics.is_empty());
                assert!(
                    matches!(budget.check(), Err(ParseError::ResourceLimit { actual, .. }) if actual == capacity)
                );
                assert!(cache.get(1).is_none());
            }
        }
    }

    /// 幅表の読み込みが停止した場合に部分的な Metrics を Arc/cache へ保存しない。
    #[test]
    fn failed_width_payload_is_not_cached_as_fallback() {
        let objects = Objects::from([(
            1,
            (
                0,
                b"<< /Type /Font /Subtype /Type1 /FirstChar 65 /Widths [600] >>".as_slice(),
            ),
        )]);
        let budget = DecodeBudget::new(&ParseLimits {
            max_total_font_metrics_bytes: 0,
            ..Default::default()
        });
        let mut cache = Cache::new(&objects, &budget, &[]);
        assert!(cache.get(1).is_none());
        assert!(cache.metrics.is_empty());
        assert!(matches!(
            budget.check(),
            Err(ParseError::ResourceLimit {
                resource: "font metrics bytes",
                ..
            })
        ));
    }
}
