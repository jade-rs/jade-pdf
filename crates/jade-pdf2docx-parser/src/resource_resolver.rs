//! 抽出時の resource 参照解決と共有チェーンの処理量を管理する。
use crate::resource_cache::Cache;
use crate::{metadata, page_attributes::Objects};

/// 有効 object 世代と、解決した値または参照失敗の記録。
struct CacheEntry<'bytes> {
    /// 最新 object index と一致する世代。
    generation: u16,
    /// この走査で直前に記録した object。独立した path 配列を作らない。
    previous: Option<u32>,
    /// 解決結果。走査中と参照失敗は None として循環の再訪を止める。
    resolved: Option<(&'bytes [u8], u32)>,
}

/// 不変な object index から resource 値を取り出す解析ローカルの resolver。
pub(crate) struct ResourceResolver<'objects, 'bytes> {
    /// 最新の object 番号・世代と本文。
    objects: &'objects Objects<'bytes>,
    /// 解析全体の容量と停止状態を共有する。
    budget: &'objects crate::decode::DecodeBudget,
    /// 参照済みの有効 object ごとの最終結果。成功も失敗も共有する。
    cache: Cache<CacheEntry<'bytes>>,
    /// 回帰テストで数える実 object lookup の回数。
    #[cfg(test)]
    object_reads: usize,
}

impl<'objects, 'bytes> ResourceResolver<'objects, 'bytes> {
    /// 解析対象の object index を借用して空の参照 cache を作る。
    pub(crate) fn new(
        objects: &'objects Objects<'bytes>,
        budget: &'objects crate::decode::DecodeBudget,
    ) -> Self {
        Self {
            objects,
            budget,
            cache: Cache::default(),
            #[cfg(test)]
            object_reads: 0,
        }
    }

    /// 最終値と間接 object 番号を返し、共有 prefix と循環を再走査しない。
    pub(crate) fn resolve(
        &mut self,
        mut value: &'bytes [u8],
    ) -> Option<(&'bytes [u8], Option<u32>)> {
        if self.budget.is_stopped() {
            return None;
        }
        let mut final_number = None;
        let mut path = None;
        let result = loop {
            let Some((number, generation)) = metadata::reference_with_generation(value) else {
                break Some((value, final_number));
            };
            if let Some(entry) = self.cache.get(&number) {
                break if entry.generation == generation {
                    entry.resolved.map(|(value, number)| (value, Some(number)))
                } else {
                    None
                };
            }
            #[cfg(test)]
            {
                self.object_reads += 1;
            }
            let Some(&(actual, body)) = self.objects.get(&number) else {
                break None;
            };
            if actual != generation {
                break None;
            }
            // 本文を辿る前に走査中と記録し、同じ object への循環を検出する。
            self.cache.insert(
                self.budget,
                number,
                CacheEntry {
                    generation,
                    previous: path,
                    resolved: None,
                },
            )?;
            path = Some(number);
            value = body;
            final_number = Some(number);
        };
        let resolved = result.and_then(|(value, number)| number.map(|number| (value, number)));
        while let Some(number) = path {
            let entry = self.cache.get_mut(&number)?;
            path = entry.previous.take();
            entry.resolved = resolved;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// cache を使わない有限走査で参照解決の期待値を独立に計算する。
    fn uncached_resolution<'bytes>(
        objects: &Objects<'bytes>,
        mut value: &'bytes [u8],
    ) -> Option<(&'bytes [u8], Option<u32>)> {
        let mut final_number = None;
        for _ in 0..=objects.len() {
            let Some((number, generation)) = metadata::reference_with_generation(value) else {
                return Some((value, final_number));
            };
            let &(actual, body) = objects.get(&number)?;
            if actual != generation {
                return None;
            }
            final_number = Some(number);
            value = body;
        }
        None
    }

    /// 多数の Font が共有する長い Type チェーンは一度だけ走査する。
    #[test]
    fn shared_type_chain_is_read_once() {
        let bodies = (1..=96)
            .map(|number| {
                if number == 96 {
                    "/Font".to_owned()
                } else {
                    format!("{} 0 R", number + 1)
                }
            })
            .collect::<Vec<_>>();
        let objects = bodies
            .iter()
            .enumerate()
            .map(|(index, body)| ((index + 1) as u32, (0, body.as_bytes())))
            .collect::<Objects>();
        let budget = crate::decode::DecodeBudget::default();
        let mut resolver = ResourceResolver::new(&objects, &budget);
        for _ in 0..64 {
            assert_eq!(
                resolver.resolve(b"1 0 R"),
                Some((b"/Font".as_slice(), Some(96)))
            );
        }
        assert!(
            resolver.object_reads <= objects.len(),
            "{} reads for {} objects",
            resolver.object_reads,
            objects.len()
        );
    }

    /// 循環の失敗も共有し、Font ごとに同じ循環を再走査しない。
    #[test]
    fn shared_cycle_is_read_once() {
        let objects = [(1, (0, b"2 0 R".as_slice())), (2, (0, b"1 0 R".as_slice()))]
            .into_iter()
            .collect::<Objects>();
        let budget = crate::decode::DecodeBudget::default();
        let mut resolver = ResourceResolver::new(&objects, &budget);
        for _ in 0..64 {
            assert_eq!(resolver.resolve(b"1 0 R"), None);
        }
        assert!(
            resolver.object_reads <= objects.len(),
            "{} cycle reads",
            resolver.object_reads
        );
    }

    /// 不正世代の失敗は prefix だけへ伝播し、正しい世代の解決を汚染しない。
    #[test]
    fn generation_failure_does_not_poison_valid_target() {
        let objects = [(1, (0, b"2 7 R".as_slice())), (2, (8, b"/Font".as_slice()))]
            .into_iter()
            .collect::<Objects>();
        let budget = crate::decode::DecodeBudget::default();
        let mut resolver = ResourceResolver::new(&objects, &budget);
        for _ in 0..64 {
            assert_eq!(resolver.resolve(b"1 0 R"), None);
        }
        assert_eq!(
            resolver.resolve(b"2 8 R"),
            Some((b"/Font".as_slice(), Some(2)))
        );
        assert_eq!(resolver.resolve(b"2 7 R"), None);
        assert_eq!(
            resolver.resolve(b"2 8 R"),
            Some((b"/Font".as_slice(), Some(2)))
        );
        assert!(
            resolver.object_reads <= 3,
            "{} generation reads",
            resolver.object_reads
        );
    }

    /// 直接値、間接 null、欠落参照と成功 cache の世代照合を保持する。
    #[test]
    fn terminal_values_and_missing_targets_keep_their_identity() {
        let objects = [
            (1, (0, b"null".as_slice())),
            (2, (0, b"3 0 R".as_slice())),
            (3, (0, b"4 0 R".as_slice())),
            (5, (7, b"/Font".as_slice())),
        ]
        .into_iter()
        .collect::<Objects>();
        let budget = crate::decode::DecodeBudget::default();
        let mut resolver = ResourceResolver::new(&objects, &budget);
        assert_eq!(resolver.resolve(b"null"), Some((b"null".as_slice(), None)));
        assert_eq!(
            resolver.resolve(b"1 0 R"),
            Some((b"null".as_slice(), Some(1)))
        );
        assert_eq!(
            resolver.resolve(b"5 7 R"),
            Some((b"/Font".as_slice(), Some(5)))
        );
        assert_eq!(resolver.resolve(b"5 0 R"), None);
        for _ in 0..64 {
            assert_eq!(resolver.resolve(b"2 0 R"), None);
            assert_eq!(resolver.resolve(b"3 0 R"), None);
        }
        assert_eq!(resolver.object_reads, 5);
        assert_eq!(resolver.cache.len(), objects.len());
    }

    /// 参照 cache の容量を確保する前に止め、エラー取得後も直接値を返さない。
    #[test]
    fn denied_reference_storage_stops_before_cache_creation() {
        let objects = Objects::from([(1, (0, b"/Font".as_slice()))]);
        let budget = crate::decode::DecodeBudget::new(&crate::ParseLimits {
            max_total_resource_workspace_bytes: 0,
            ..Default::default()
        });
        let mut resolver = ResourceResolver::new(&objects, &budget);
        assert!(resolver.resolve(b"1 0 R").is_none());
        assert!(resolver.cache.is_empty());
        assert!(
            matches!(budget.check(), Err(crate::ParseError::ResourceLimit {
            resource: "resource workspace bytes", limit: 0, actual
        }) if actual > 0)
        );
        assert!(resolver.resolve(b"/Font").is_none());
    }

    proptest! {
        /// 任意の共有・循環・欠落・世代不一致でも値を保ち、lookup 数を線形に抑える。
        #[test]
        fn graph_results_and_lookup_budget_are_order_independent(
            nodes in prop::collection::vec((any::<u8>(), 0_u16..2, 0_u16..2), 1..80),
            queries in prop::collection::vec((any::<u8>(), 0_u16..2), 0..160),
        ) {
            let count = nodes.len();
            let bodies = nodes.iter().map(|&(edge, _, generation)| {
                let target = usize::from(edge) % (count + 3);
                match target {
                    value if value == count => "/Font".to_owned(),
                    value if value == count + 1 => "null".to_owned(),
                    value => format!("{} {generation} R", value + 1),
                }
            }).collect::<Vec<_>>();
            let objects = bodies.iter().enumerate().map(|(index, body)| {
                ((index + 1) as u32, (nodes[index].1, body.as_bytes()))
            }).collect::<Objects>();
            let queries = queries.iter().map(|&(number, generation)| {
                format!("{} {generation} R", usize::from(number) % (count + 1) + 1)
            }).collect::<Vec<_>>();
            let budget = crate::decode::DecodeBudget::default();
        let mut resolver = ResourceResolver::new(&objects, &budget);
            for query in queries.iter().chain(queries.iter().rev()) {
                prop_assert_eq!(resolver.resolve(query.as_bytes()), uncached_resolution(&objects, query.as_bytes()));
            }
            prop_assert!(resolver.object_reads <= 2 * objects.len() + 2 * queries.len());
            prop_assert!(resolver.cache.len() <= objects.len());
        }
    }
}
