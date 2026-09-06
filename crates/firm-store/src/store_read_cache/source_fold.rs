use super::*;
const MAX_COMBINED_PROJECTIONS: usize = 32;

struct Combined<D> {
    inputs: Vec<Arc<dyn Any + Send + Sync>>,
    projection: Arc<D>,
}
impl HarnessStore {
    /// Dispose/rebuild on any file replacement, retaining a stable Arc on
    /// unchanged (including missing) ledgers. Fold every decoded source row,
    /// never latest-by-id before the caller's temporal/provenance validation.
    pub(crate) fn cached_jsonl_source_fold<
        T: DeserializeOwned + Clone + Send + Sync + 'static,
        D: Default + Clone + Send + Sync + 'static,
    >(
        &self,
        ledger: &str,
        ignore_incomplete_tail: bool,
        fold: fn(&mut D, &T),
    ) -> StoreResult<Arc<D>> {
        Ok(self
            .cached_latest_jsonl_rows(
                ledger,
                ignore_incomplete_tail,
                |_| String::new(),
                |_| Ok(()),
                ReadPolicy {
                    selection: CacheSelection::Groups,
                    group_key: None,
                    fold,
                },
            )?
            .2)
    }

    /// Pure merge of already-read snapshots. Inputs are retained strongly so
    /// pointer reuse cannot create a false cache hit. The builder must not read
    /// a fresh ledger or perform effects; clocks/leases stay outside this memo.
    pub(crate) fn cached_combined_projection<D: Send + Sync + 'static>(
        &self,
        name: &str,
        inputs: Vec<Arc<dyn Any + Send + Sync>>,
        build: impl FnOnce() -> StoreResult<D>,
    ) -> StoreResult<Arc<D>> {
        let key = format!("combined:{name}");
        {
            let cache = self.read_cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(old) = cache
                .entries
                .get(&key)
                .and_then(|e| e.downcast_ref::<Combined<D>>())
            {
                if old.inputs.len() == inputs.len()
                    && old
                        .inputs
                        .iter()
                        .zip(&inputs)
                        .all(|(a, b)| Arc::ptr_eq(a, b))
                {
                    return Ok(old.projection.clone());
                }
            }
        }
        let projection = Arc::new(build()?);
        let mut cache = self.read_cache.lock().unwrap_or_else(|e| e.into_inner());
        // Scope combinations must not retain an unbounded set of old source
        // snapshots. Eviction changes only recomputation cost, never truth.
        if !cache.entries.contains_key(&key) {
            let combined_keys = cache
                .entries
                .keys()
                .filter(|name| name.starts_with("combined:"))
                .cloned()
                .collect::<Vec<_>>();
            if combined_keys.len() >= MAX_COMBINED_PROJECTIONS {
                cache.entries.remove(&combined_keys[0]);
            }
        }
        cache.entries.insert(
            key,
            Box::new(Combined {
                inputs,
                projection: projection.clone(),
            }),
        );
        Ok(projection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn combined_scope_combinations_are_bounded_and_eviction_only_recomputes() {
        let store = HarnessStore::new(std::env::temp_dir());
        let input = Arc::new(7u64);
        for i in 0..64u64 {
            let value = store
                .cached_combined_projection(&format!("bounded-{i:02}"), vec![input.clone()], || {
                    Ok(i)
                })
                .unwrap();
            assert_eq!(*value, i);
        }
        {
            let cache = store.read_cache.lock().unwrap();
            assert_eq!(
                cache
                    .entries
                    .keys()
                    .filter(|k| k.starts_with("combined:"))
                    .count(),
                MAX_COMBINED_PROJECTIONS
            );
            assert!(!cache.entries.contains_key("combined:bounded-00"));
        }
        let mut rebuilt = false;
        let value = store
            .cached_combined_projection("bounded-00", vec![input], || {
                rebuilt = true;
                Ok(0u64)
            })
            .unwrap();
        assert!(rebuilt);
        assert_eq!(*value, 0);
    }

    #[test]
    fn source_and_combined_arcs_are_stable_until_inputs_change() {
        let root = std::env::temp_dir().join(format!(
            "source-fold-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let store = HarnessStore::new(&root);
        let source = || {
            store
                .cached_jsonl_source_fold("rows.jsonl", false, |rows: &mut Vec<u64>, row: &u64| {
                    rows.push(*row)
                })
                .unwrap()
        };
        let missing = source();
        assert!(Arc::ptr_eq(&missing, &source()));
        fs::write(root.join("rows.jsonl"), "1\n2\n").unwrap();
        let first = source();
        assert_eq!(*first, vec![1, 2]);
        assert!(Arc::ptr_eq(&first, &source()));
        let combined = store
            .cached_combined_projection("sum-test", vec![first.clone()], || {
                Ok(first.iter().sum::<u64>())
            })
            .unwrap();
        let again = store
            .cached_combined_projection("sum-test", vec![source()], || -> StoreResult<u64> {
                panic!("unchanged inputs must not refold")
            })
            .unwrap();
        assert!(Arc::ptr_eq(&combined, &again));
        writeln!(
            OpenOptions::new()
                .append(true)
                .open(root.join("rows.jsonl"))
                .unwrap(),
            "3"
        )
        .unwrap();
        let appended = source();
        assert_eq!(*appended, vec![1, 2, 3]);
        let merged = store
            .cached_combined_projection("sum-test", vec![appended.clone()], || {
                Ok(appended.iter().sum::<u64>())
            })
            .unwrap();
        assert_eq!(*merged, 6);
        fs::write(root.join("next"), "4\n").unwrap();
        fs::rename(root.join("next"), root.join("rows.jsonl")).unwrap();
        assert_eq!(*source(), vec![4]);
        fs::remove_file(root.join("rows.jsonl")).unwrap();
        assert!(source().is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
