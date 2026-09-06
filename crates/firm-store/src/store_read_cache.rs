//! Volatile observation accelerators. Full-history readers remain unchanged.
//! Changed append ledgers still read/compare their prefix bytes: metadata alone
//! cannot prove that an external writer only appended. Only decoding is O(delta).
use super::*;
use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
mod source_fold;

#[derive(Debug, Clone, Default, Serialize)]
pub struct StoreReadScanMetric {
    pub ledger: String,
    pub ledger_bytes: u64,
    pub bytes_read: u64,
    pub decoded_bytes: u64,
    pub decoded_rows: u64,
    pub cloned_rows: u64,
    pub total_cloned_rows: u64,
    pub total_bytes_read: u64,
    pub total_decoded_rows: u64,
    pub reason: String,
}

#[derive(Default)]
pub(super) struct StoreReadCache {
    entries: BTreeMap<String, Box<dyn Any + Send + Sync>>,
    metrics: BTreeMap<String, StoreReadScanMetric>,
}
impl std::fmt::Debug for StoreReadCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoreReadCache")
            .field("metrics", &self.metrics)
            .finish()
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    dev: u64,
    ino: u64,
    len: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}
impl From<fs::Metadata> for Stamp {
    fn from(m: fs::Metadata) -> Self {
        Self {
            dev: m.dev(),
            ino: m.ino(),
            len: m.len(),
            modified: (m.mtime(), m.mtime_nsec()),
            changed: (m.ctime(), m.ctime_nsec()),
        }
    }
}
struct Missing<D>(Arc<D>);
struct Latest<T, D> {
    stamp: Stamp,
    bytes: Vec<u8>,
    values: BTreeMap<String, (u64, T)>,
    next_row: u64,
    groups: BTreeSet<String>,
    derived: Arc<D>,
}

struct ReadPolicy<'a, T, D> {
    selection: CacheSelection<'a>,
    group_key: Option<fn(&T) -> String>,
    fold: fn(&mut D, &T),
}

#[derive(Clone, Copy)]
pub(super) enum CacheSelection<'a> {
    #[cfg(test)]
    All,
    AppendOrder,
    Prefix(&'a str),
    Groups,
}
impl HarnessStore {
    pub(super) fn record_jsonl_read(
        &self,
        ledger: &str,
        ledger_bytes: u64,
        bytes_read: u64,
        decoded_bytes: u64,
        decoded_rows: u64,
        reason: &str,
    ) {
        let mut cache = self.read_cache.lock().unwrap_or_else(|e| e.into_inner());
        record(
            &mut cache,
            ledger,
            ledger_bytes,
            bytes_read,
            decoded_bytes,
            decoded_rows,
            reason,
        );
    }

    /// Per-ledger observation cost, scoped to this store handle and its clones.
    /// Counters never authorize an effect or replace a durable fence.
    pub fn read_scan_metrics(&self) -> Vec<StoreReadScanMetric> {
        self.read_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .metrics
            .values()
            .cloned()
            .collect()
    }

    #[cfg(test)]
    pub(super) fn cached_latest_jsonl<T: DeserializeOwned + Clone + Send + Sync + 'static>(
        &self,
        ledger: &str,
        ignore_incomplete_tail: bool,
        key: impl Fn(&T) -> String,
        validate: impl Fn(&T) -> StoreResult<()>,
    ) -> StoreResult<BTreeMap<String, T>> {
        Ok(self
            .cached_latest_jsonl_rows(
                ledger,
                ignore_incomplete_tail,
                &key,
                validate,
                ReadPolicy {
                    selection: CacheSelection::All,
                    group_key: None,
                    fold: |_: &mut (), _| {},
                },
            )?
            .0
            .into_iter()
            .map(|v| (key(&v), v))
            .collect())
    }

    pub(super) fn cached_latest_jsonl_in_append_order<
        T: DeserializeOwned + Clone + Send + Sync + 'static,
    >(
        &self,
        ledger: &str,
        key: impl Fn(&T) -> String,
        validate: impl Fn(&T) -> StoreResult<()>,
    ) -> StoreResult<Vec<T>> {
        Ok(self
            .cached_latest_jsonl_rows(
                ledger,
                false,
                key,
                validate,
                ReadPolicy {
                    selection: CacheSelection::AppendOrder,
                    group_key: None,
                    fold: |_: &mut (), _| {},
                },
            )?
            .0)
    }

    /// One typed cache with an optional group inventory and range selection.
    /// Every caller of this index must use the same row key and group key.
    #[cfg(test)]
    pub(super) fn cached_latest_jsonl_indexed<
        T: DeserializeOwned + Clone + Send + Sync + 'static,
    >(
        &self,
        ledger: &str,
        key: impl Fn(&T) -> String,
        group_key: fn(&T) -> String,
        selection: CacheSelection<'_>,
    ) -> StoreResult<(Vec<T>, Vec<String>)> {
        let (rows, groups, _) = self.cached_latest_jsonl_derived(
            ledger,
            key,
            group_key,
            selection,
            |_: &mut (), _| {},
        )?;
        Ok((rows, groups))
    }

    pub(super) fn cached_latest_jsonl_derived<
        T: DeserializeOwned + Clone + Send + Sync + 'static,
        D: Default + Clone + Send + Sync + 'static,
    >(
        &self,
        ledger: &str,
        key: impl Fn(&T) -> String,
        group_key: fn(&T) -> String,
        selection: CacheSelection<'_>,
        fold: fn(&mut D, &T),
    ) -> StoreResult<(Vec<T>, Vec<String>, Arc<D>)> {
        self.cached_latest_jsonl_rows(
            ledger,
            true,
            key,
            |_| Ok(()),
            ReadPolicy {
                selection,
                group_key: Some(group_key),
                fold,
            },
        )
    }

    fn cached_latest_jsonl_rows<
        T: DeserializeOwned + Clone + Send + Sync + 'static,
        D: Default + Clone + Send + Sync + 'static,
    >(
        &self,
        ledger: &str,
        ignore_incomplete_tail: bool,
        key: impl Fn(&T) -> String,
        validate: impl Fn(&T) -> StoreResult<()>,
        policy: ReadPolicy<'_, T, D>,
    ) -> StoreResult<(Vec<T>, Vec<String>, Arc<D>)> {
        let ReadPolicy {
            selection,
            group_key,
            fold,
        } = policy;
        let mut cache = self.read_cache.lock().unwrap_or_else(|e| e.into_inner());
        let path = self.root.join(ledger);
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut bytes_read = 0;
        loop {
            let mut file = match File::open(&path) {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    let derived = cache
                        .entries
                        .get(ledger)
                        .and_then(|e| e.downcast_ref::<Missing<D>>())
                        .map(|m| m.0.clone())
                        .unwrap_or_else(|| Arc::new(D::default()));
                    cache
                        .entries
                        .insert(ledger.to_owned(), Box::new(Missing(derived.clone())));
                    record(&mut cache, ledger, 0, bytes_read, 0, 0, "missing");
                    return Ok((Vec::new(), Vec::new(), derived));
                }
                Err(e) => return Err(e.into()),
            };
            let stamp = Stamp::from(file.metadata()?);
            let old = cache
                .entries
                .get(ledger)
                .and_then(|v| v.downcast_ref::<Latest<T, D>>());
            if old.is_some_and(|old| old.stamp == stamp)
                && fs::metadata(&path).map(Stamp::from).ok().as_ref() == Some(&stamp)
            {
                let old = old.expect("checked above");
                let result = selected_values(&old.values, &old.groups, selection);
                let derived = old.derived.clone();
                record(&mut cache, ledger, stamp.len, bytes_read, 0, 0, "unchanged");
                record_clones(&mut cache, ledger, result.0.len() as u64);
                return Ok((result.0, result.1, derived));
            }
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            bytes_read += bytes.len() as u64;
            let stable = Stamp::from(file.metadata()?) == stamp
                && fs::metadata(&path).map(Stamp::from).ok().as_ref() == Some(&stamp);
            if !stable {
                if Instant::now() >= deadline {
                    cache.entries.remove(ledger);
                    return Err(StoreError::Conflict(format!(
                        "observation snapshot kept changing: {ledger}"
                    )));
                }
                continue;
            }
            if !ignore_incomplete_tail
                && !bytes.is_empty()
                && !bytes.ends_with(b"\n")
                && Instant::now() < deadline
            {
                thread::sleep(Duration::from_millis(5));
                continue;
            }
            let durable_len = if ignore_incomplete_tail {
                bytes.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1)
            } else {
                bytes.len()
            };
            let append = old.is_some_and(|old| {
                old.stamp.dev == stamp.dev
                    && old.stamp.ino == stamp.ino
                    && bytes.len() > old.bytes.len()
                    && old.bytes.ends_with(b"\n")
                    && bytes.starts_with(&old.bytes)
            });
            let (offset, mut values, reason) = if append {
                let old = old.expect("checked above");
                (old.bytes.len(), old.values.clone(), "append_delta")
            } else {
                (
                    0,
                    BTreeMap::new(),
                    if old.is_some() {
                        "replace_or_rewrite"
                    } else {
                        "cold"
                    },
                )
            };
            let mut next_row = if append {
                old.expect("checked above").next_row
            } else {
                0
            };
            let mut groups = if append {
                old.expect("checked above").groups.clone()
            } else {
                BTreeSet::new()
            };
            let mut derived = if append {
                (*old.expect("checked above").derived).clone()
            } else {
                D::default()
            };
            let mut rows = 0;
            for row in bytes[offset..durable_len].split(|b| *b == b'\n') {
                if row.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                let value: T = match serde_json::from_slice(row) {
                    Ok(value) => value,
                    Err(e) => {
                        cache.entries.remove(ledger);
                        return Err(e.into());
                    }
                };
                if let Err(e) = validate(&value) {
                    cache.entries.remove(ledger);
                    return Err(e);
                }
                if let Some(group_key) = group_key {
                    groups.insert(group_key(&value));
                }
                fold(&mut derived, &value);
                values.insert(key(&value), (next_row, value));
                next_row += 1;
                rows += 1;
            }
            let result = selected_values(&values, &groups, selection);
            let cloned_rows = result.0.len() as u64
                + if append {
                    old.expect("checked above").values.len() as u64
                } else {
                    0
                };
            let decoded = durable_len.saturating_sub(offset) as u64;
            // Keep the full snapshot, including an ignored trust crash tail.
            // Such a tail prevents delta reuse until the next complete rebuild.
            let derived = Arc::new(derived);
            cache.entries.insert(
                ledger.to_owned(),
                Box::new(Latest {
                    stamp: stamp.clone(),
                    bytes,
                    values,
                    next_row,
                    groups,
                    derived: derived.clone(),
                }),
            );
            record(
                &mut cache, ledger, stamp.len, bytes_read, decoded, rows, reason,
            );
            record_clones(&mut cache, ledger, cloned_rows);
            return Ok((result.0, result.1, derived));
        }
    }
}
fn selected_values<T: Clone>(
    values: &BTreeMap<String, (u64, T)>,
    groups: &BTreeSet<String>,
    selection: CacheSelection<'_>,
) -> (Vec<T>, Vec<String>) {
    let mut rows = match selection {
        CacheSelection::Groups => return (Vec::new(), groups.iter().cloned().collect()),
        CacheSelection::Prefix(prefix) => values
            .range(prefix.to_owned()..)
            .take_while(|(key, _)| key.starts_with(prefix))
            .map(|(_, row)| row)
            .collect::<Vec<_>>(),
        _ => values.values().collect::<Vec<_>>(),
    };
    if matches!(selection, CacheSelection::AppendOrder) {
        rows.sort_by_key(|(ordinal, _)| *ordinal);
    }
    (
        rows.into_iter().map(|(_, row)| row.clone()).collect(),
        Vec::new(),
    )
}
fn record_clones(cache: &mut StoreReadCache, ledger: &str, cloned_rows: u64) {
    let m = cache.metrics.entry(ledger.to_owned()).or_default();
    m.cloned_rows = cloned_rows;
    m.total_cloned_rows += cloned_rows;
}

fn record(
    cache: &mut StoreReadCache,
    ledger: &str,
    ledger_bytes: u64,
    bytes_read: u64,
    decoded_bytes: u64,
    decoded_rows: u64,
    reason: &str,
) {
    let m = cache.metrics.entry(ledger.to_owned()).or_default();
    m.ledger = ledger.to_owned();
    m.ledger_bytes = ledger_bytes;
    m.bytes_read = bytes_read;
    m.decoded_bytes = decoded_bytes;
    m.decoded_rows = decoded_rows;
    m.cloned_rows = 0;
    m.reason = reason.to_owned();
    m.total_bytes_read += bytes_read;
    m.total_decoded_rows += decoded_rows;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Debug, Clone, Deserialize)]
    struct Row {
        id: String,
        value: u64,
    }
    fn fixture() -> HarnessStore {
        static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "read-cache-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        fs::create_dir_all(&root).unwrap();
        HarnessStore::new(root)
    }
    fn read(store: &HarnessStore) -> StoreResult<BTreeMap<String, Row>> {
        store.cached_latest_jsonl("test.jsonl", false, |r: &Row| r.id.clone(), |_| Ok(()))
    }
    #[test]
    fn unchanged_and_append_decode_only_delta_and_report_prefix_io() {
        let s = fixture();
        let initial = (0..2000)
            .map(|i| format!("{{\"id\":\"{}\",\"value\":{i}}}\n", i % 5))
            .collect::<String>();
        fs::write(s.root.join("test.jsonl"), &initial).unwrap();
        assert_eq!(read(&s).unwrap().len(), 5);
        assert_eq!(s.read_scan_metrics()[0].decoded_rows, 2000);
        read(&s).unwrap();
        assert_eq!(s.read_scan_metrics()[0].decoded_rows, 0);
        assert_eq!(s.read_scan_metrics()[0].bytes_read, 0);
        for i in 0..10 {
            writeln!(
                OpenOptions::new()
                    .append(true)
                    .open(s.root.join("test.jsonl"))
                    .unwrap(),
                "{{\"id\":\"0\",\"value\":{i}}}"
            )
            .unwrap();
            assert_eq!(read(&s).unwrap()["0"].value, i);
            let m = &s.read_scan_metrics()[0];
            assert_eq!(m.decoded_rows, 1);
            assert_eq!(m.reason, "append_delta");
            assert!(m.bytes_read >= initial.len() as u64);
        }
        fs::remove_dir_all(s.root()).unwrap();
    }
    #[test]
    fn replacement_truncate_same_size_and_growing_prefix_rewrite_rebuild() {
        let s = fixture();
        let path = s.root.join("test.jsonl");
        fs::write(&path, "{\"id\":\"a\",\"value\":1}\n").unwrap();
        read(&s).unwrap();
        fs::write(&path, "{\"id\":\"a\",\"value\":2}\n").unwrap();
        assert_eq!(read(&s).unwrap()["a"].value, 2);
        fs::write(
            &path,
            "{\"id\":\"a\",\"value\":3}\n{\"id\":\"b\",\"value\":4}\n",
        )
        .unwrap();
        assert_eq!(read(&s).unwrap()["a"].value, 3);
        assert_eq!(s.read_scan_metrics()[0].decoded_rows, 2);
        let next = s.root.join("next");
        fs::write(&next, "{\"id\":\"c\",\"value\":5}\n").unwrap();
        fs::rename(next, &path).unwrap();
        assert_eq!(read(&s).unwrap().keys().collect::<Vec<_>>(), vec!["c"]);
        fs::write(&path, "").unwrap();
        assert!(read(&s).unwrap().is_empty());
        fs::remove_dir_all(s.root()).unwrap();
    }
    #[test]
    fn separate_process_atomic_rewrites_rebuild_and_never_reuse_old_values() {
        let s = fixture();
        let path = s.root.join("test.jsonl");
        let source = s.root.join("source");
        let next = s.root.join("next");
        for version in 0..5 {
            let body = (0..500)
                .map(|i| format!("{{\"id\":\"{}\",\"value\":{version}}}\n", i % 3))
                .collect::<String>();
            fs::write(&source, body).unwrap();
            let status = std::process::Command::new("sh")
                .arg("-c")
                .arg("cp \"$1\" \"$2\" && mv \"$2\" \"$3\"")
                .arg("cache-test")
                .arg(&source)
                .arg(&next)
                .arg(&path)
                .status()
                .unwrap();
            assert!(status.success());
            assert!(read(&s).unwrap().values().all(|r| r.value == version));
            assert_eq!(s.read_scan_metrics()[0].decoded_rows, 500);
            read(&s).unwrap();
            assert_eq!(s.read_scan_metrics()[0].decoded_rows, 0);
        }
        fs::remove_dir_all(s.root()).unwrap();
    }

    #[test]
    fn missing_and_concurrent_rename_never_publish_a_mixed_snapshot() {
        let s = fixture();
        let path = s.root.join("test.jsonl");
        assert!(read(&s).unwrap().is_empty());
        let body = |v| {
            (0..100)
                .map(|i| format!("{{\"id\":\"{i}\",\"value\":{v}}}\n"))
                .collect::<String>()
        };
        fs::write(&path, body(0)).unwrap();
        read(&s).unwrap();
        let root = s.root.clone();
        let writer = thread::spawn(move || {
            for v in 1..100 {
                let next = root.join("next");
                fs::write(&next, body(v)).unwrap();
                fs::rename(&next, root.join("test.jsonl")).unwrap();
            }
        });
        for _ in 0..100 {
            let values = read(&s).unwrap();
            assert_eq!(values.len(), 100);
            let version = values.values().next().unwrap().value;
            assert!(values.values().all(|r| r.value == version));
        }
        writer.join().unwrap();
        assert!(read(&s).unwrap().values().all(|r| r.value == 99));
        fs::remove_file(&path).unwrap();
        assert!(read(&s).unwrap().is_empty());
        assert_eq!(s.read_scan_metrics()[0].reason, "missing");
        fs::remove_dir_all(s.root()).unwrap();
    }

    #[test]
    fn ordered_observation_keeps_last_append_position_across_delta() {
        let s = fixture();
        let path = s.root.join("test.jsonl");
        fs::write(
            &path,
            "{\"id\":\"b\",\"value\":1}\n{\"id\":\"a\",\"value\":2}\n",
        )
        .unwrap();
        let ordered = || {
            s.cached_latest_jsonl_in_append_order("test.jsonl", |r: &Row| r.id.clone(), |_| Ok(()))
                .unwrap()
                .into_iter()
                .map(|r| r.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ordered(), vec!["b", "a"]);
        writeln!(
            OpenOptions::new().append(true).open(&path).unwrap(),
            "{{\"id\":\"b\",\"value\":3}}"
        )
        .unwrap();
        assert_eq!(ordered(), vec!["a", "b"]);
        assert_eq!(ordered(), vec!["a", "b"]);
        fs::remove_dir_all(s.root()).unwrap();
    }

    #[test]
    fn scoped_lookup_clones_only_selected_rows_as_other_kinds_grow() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static CLONES: AtomicUsize = AtomicUsize::new(0);
        #[derive(Deserialize)]
        struct IndexedRow {
            kind: String,
            space: String,
            id: String,
        }
        impl Clone for IndexedRow {
            fn clone(&self) -> Self {
                CLONES.fetch_add(1, Ordering::Relaxed);
                Self {
                    kind: self.kind.clone(),
                    space: self.space.clone(),
                    id: self.id.clone(),
                }
            }
        }
        let key = |r: &IndexedRow| serde_json::to_string(&(&r.kind, &r.space, &r.id)).unwrap();
        for unrelated_rows in [100, 10_000] {
            let s = fixture();
            let mut body = (0..unrelated_rows)
                .map(|i| {
                    format!(
                        "{{\"kind\":\"runtime_command\",\"space\":\"space-a\",\"id\":\"{i}\"}}\n"
                    )
                })
                .collect::<String>();
            for space in ["space-a", "space-b"] {
                for id in ["session-1", "session-2"] {
                    body.push_str(&format!(
                        "{{\"kind\":\"agent_session\",\"space\":\"{space}\",\"id\":\"{id}\"}}\n"
                    ));
                }
            }
            fs::write(s.root.join("test.jsonl"), body).unwrap();
            let query = |selection| {
                s.cached_latest_jsonl_indexed("test.jsonl", key, |r| r.space.clone(), selection)
                    .unwrap()
            };
            query(CacheSelection::Groups); // Warm the same typed index.
            CLONES.store(0, Ordering::Relaxed);
            let started = Instant::now();
            for _ in 0..100 {
                let rows = query(CacheSelection::Prefix("[\"agent_session\",\"space-a\",")).0;
                assert_eq!(rows.len(), 2);
                assert!(rows
                    .iter()
                    .all(|r| r.kind == "agent_session" && r.space == "space-a"));
                let m = &s.read_scan_metrics()[0];
                assert_eq!(m.decoded_rows, 0);
                assert_eq!(m.bytes_read, 0);
                assert_eq!(m.cloned_rows, 2);
            }
            eprintln!(
                "unrelated_rows={unrelated_rows} lookups=100 cloned_rows={} elapsed_us={}",
                CLONES.load(Ordering::Relaxed),
                started.elapsed().as_micros()
            );
            assert_eq!(CLONES.load(Ordering::Relaxed), 200);
            CLONES.store(0, Ordering::Relaxed);
            assert_eq!(
                query(CacheSelection::Prefix("[\"agent_session\",")).0.len(),
                4
            );
            assert_eq!(CLONES.load(Ordering::Relaxed), 4);
            CLONES.store(0, Ordering::Relaxed);
            assert_eq!(query(CacheSelection::Groups).1, vec!["space-a", "space-b"]);
            assert_eq!(CLONES.load(Ordering::Relaxed), 0);
            fs::remove_dir_all(s.root()).unwrap();
        }
    }

    #[test]
    fn complete_corruption_is_never_hidden_by_cached_projection() {
        let s = fixture();
        let path = s.root.join("test.jsonl");
        fs::write(&path, "{\"id\":\"a\",\"value\":1}\n").unwrap();
        read(&s).unwrap();
        fs::write(&path, "malformed complete frame\n").unwrap();
        assert!(read(&s).is_err());
        assert!(read(&s).is_err());
        fs::remove_dir_all(s.root()).unwrap();
    }
}
