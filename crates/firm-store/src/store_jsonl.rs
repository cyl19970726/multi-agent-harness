use super::*;

impl HarnessStore {
    pub(super) fn append_jsonl<T: Serialize>(&self, file_name: &str, value: &T) -> StoreResult<()> {
        self.init()?;
        let _lock = self.acquire_write_lock()?;
        self.append_jsonl_unlocked(file_name, value)
    }

    pub(super) fn append_jsonl_unlocked<T: Serialize>(
        &self,
        file_name: &str,
        value: &T,
    ) -> StoreResult<()> {
        let mut row = Vec::new();
        serde_json::to_writer(&mut row, value)?;
        row.push(b'\n');
        let path = self.root.join(file_name);
        let creates_ledger = !path.exists();
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        file.write_all(&row)?;
        file.flush()?;
        // Durability: fsync the row to stable storage before returning. Without
        // this a crash immediately after a claim append (the Running session row
        // + the Acknowledged message row in `claim_queued_message_delivery`) can
        // lose those rows from the OS page cache; latest-wins projection would
        // then revert the message to Queued and double-deliver it. `flush()`
        // only drains the userspace buffer, not the kernel cache, so we must
        // `sync_all`. Always called under the global flock, so write ordering
        // across files is preserved.
        file.sync_all()?;
        if creates_ledger {
            // The first fence/operation append creates a directory entry.
            // Syncing only the inode is insufficient across a system crash;
            // persist that new name before reporting the append durable.
            File::open(&self.root)?.sync_all()?;
        }
        Ok(())
    }

    /// Read only the trailing `window` bytes of a JSONL file, dropping the first
    /// (possibly partial) line unless the window covers the whole file.
    ///
    /// Only valid for latest-wins projections keyed by a field, where the answer
    /// is the LAST matching row. Callers must fall back to `read_jsonl` when the
    /// key is absent from the tail — absence in the window proves nothing.
    ///
    /// Motivation: Supervisor lease heartbeats append ~1 row/s per live run and
    /// every renewal re-parsed the entire file under the global write lock,
    /// making heartbeat cost O(N) and cumulative cost O(N²). Measured on
    /// `star-harness-dogfood`: 71,524 rows / 23 MB, 15,101 renewals in 4.77 h
    /// for one run, with observed renewal drift (p50 1135 ms against a 1000 ms
    /// sleep) already showing the parse cost.
    pub(super) fn read_jsonl_tail<T: DeserializeOwned>(
        &self,
        file_name: &str,
        window: u64,
    ) -> StoreResult<Vec<T>> {
        let path = self.root.join(file_name);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let start = len.saturating_sub(window);
        // Whether the byte before `start` is a newline decides if `start`
        // already sits on a row boundary. Discarding unconditionally would drop
        // a COMPLETE row whenever the window happens to land there, which costs
        // a needless full-scan fallback (and would silently lose a row for any
        // future caller that does not have one).
        let starts_on_boundary = if start == 0 {
            true
        } else {
            file.seek(SeekFrom::Start(start - 1))?;
            let mut prev = [0u8; 1];
            std::io::Read::read_exact(&mut file, &mut prev)?;
            prev[0] == b'\n'
        };
        file.seek(SeekFrom::Start(start))?;
        let mut values = Vec::new();
        let mut lines = BufReader::new(file).lines();
        if !starts_on_boundary {
            // Discard the torn first line inside the window.
            let _ = lines.next();
        }
        for line in lines {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            values.push(serde_json::from_str(&line)?);
        }
        Ok(values)
    }

    /// Latest lease for one run: tail window first, full scan only on miss.
    pub(super) fn latest_lease_for_run_unlocked(
        &self,
        team_run_id: &str,
    ) -> StoreResult<Option<TeamSupervisorLease>> {
        const TAIL_WINDOW_BYTES: u64 = 256 * 1024;
        let tail = self.read_jsonl_tail::<TeamSupervisorLease>(
            "team_supervisor_leases.jsonl",
            TAIL_WINDOW_BYTES,
        )?;
        // rfind, not filter().next_back(): latest-wins means the LAST matching
        // row in the window, and rfind scans from the back so it stops at the
        // first hit instead of walking the whole window.
        if let Some(found) = tail
            .into_iter()
            .rfind(|lease| lease.team_run_id == team_run_id)
        {
            return Ok(Some(found));
        }
        Ok(latest_by_id(
            self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?,
            |lease| lease.team_run_id.clone(),
        )
        .remove(team_run_id))
    }

    /// Collapse the lease file to one row per run (latest wins).
    ///
    /// Called on acquisition, which is rare (one per Supervisor generation),
    /// while heartbeats are frequent. Bounds the file at ~#runs rows so the
    /// tail window above always hits and the file stops growing without bound.
    /// Generation fencing is unaffected: the retained row is exactly the row a
    /// full-scan latest-wins projection would have produced.
    pub(super) fn compact_supervisor_leases_unlocked(&self) -> StoreResult<()> {
        let path = self.root.join("team_supervisor_leases.jsonl");
        if !path.exists() {
            return Ok(());
        }
        let all = self.read_jsonl::<TeamSupervisorLease>("team_supervisor_leases.jsonl")?;
        let latest = latest_by_id(all, |lease| lease.team_run_id.clone());
        let temp = self.root.join("team_supervisor_leases.jsonl.compact");
        {
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&temp)?;
            for lease in latest.values() {
                let mut row = Vec::new();
                serde_json::to_writer(&mut row, lease)?;
                row.push(b'\n');
                file.write_all(&row)?;
            }
            file.flush()?;
            file.sync_all()?;
        }
        fs::rename(&temp, &path)?;
        // fsync the PARENT DIRECTORY, not just the temp inode. POSIX allows a
        // crash to recover either the old or the new directory entry after a
        // rename; only syncing the directory makes the replacement durable.
        // Without it a system crash can resurrect the pre-compaction file and
        // with it an already-issued generation, violating the monotonic
        // higher-generation contract in ADR 0044.
        if let Ok(dir) = File::open(&self.root) {
            let _ = dir.sync_all();
        }
        Ok(())
    }

    pub(super) fn acquire_write_lock(&self) -> StoreResult<StoreWriteLock> {
        let (timeout, poll_interval) = store_write_lock_policy();
        self.acquire_write_lock_with_policy(timeout, poll_interval)
    }

    pub(super) fn acquire_write_lock_with_policy(
        &self,
        timeout: Duration,
        poll_interval: Duration,
    ) -> StoreResult<StoreWriteLock> {
        let lock_path = self.root.join(".store.lock");
        let deadline = Instant::now() + timeout;
        loop {
            if self
                .process_write_lock
                .compare_exchange(
                    false,
                    true,
                    AtomicOrdering::Acquire,
                    AtomicOrdering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
            if Instant::now() >= deadline {
                return Err(StoreError::LockTimeout(lock_path.display().to_string()));
            }
            thread::sleep(poll_interval.min(deadline.saturating_duration_since(Instant::now())));
        }
        let process_write_lock = self.process_write_lock.clone();
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .inspect_err(|_| process_write_lock.store(false, AtomicOrdering::Release))?;
        loop {
            match lock_file_exclusive(&file) {
                Ok(()) => {
                    return Ok(StoreWriteLock {
                        file,
                        process_write_lock,
                    })
                }
                Err(error) if would_block_lock(&error) => {
                    if Instant::now() >= deadline {
                        process_write_lock.store(false, AtomicOrdering::Release);
                        return Err(StoreError::LockTimeout(lock_path.display().to_string()));
                    }
                    thread::sleep(
                        poll_interval.min(deadline.saturating_duration_since(Instant::now())),
                    );
                }
                Err(error) => {
                    process_write_lock.store(false, AtomicOrdering::Release);
                    return Err(StoreError::Io(error));
                }
            }
        }
    }

    pub(super) fn read_jsonl<T: DeserializeOwned>(&self, file_name: &str) -> StoreResult<Vec<T>> {
        let path = self.root.join(file_name);
        if !path.exists() {
            return Ok(Vec::new());
        }

        // A writer holds the store flock, but ordinary projections deliberately
        // do not: Dashboard/API reads must remain concurrent with one another.
        // `write_all` still may expose a short prefix before the trailing
        // newline becomes visible, so take a byte snapshot and retry only that
        // unmistakably incomplete final-row state. A complete snapshot is
        // immutable in memory even if another append starts immediately after.
        // The bounded retry preserves honest corruption reporting for a file
        // that remains truncated instead of silently dropping its final row.
        const INCOMPLETE_ROW_RETRY: Duration = Duration::from_secs(1);
        const INCOMPLETE_ROW_POLL: Duration = Duration::from_millis(5);
        let deadline = Instant::now() + INCOMPLETE_ROW_RETRY;
        let snapshot = loop {
            let bytes = fs::read(&path)?;
            if bytes.is_empty() || bytes.ends_with(b"\n") || Instant::now() >= deadline {
                break bytes;
            }
            thread::sleep(INCOMPLETE_ROW_POLL);
        };

        let mut values = Vec::new();
        for line in snapshot.split(|byte| *byte == b'\n') {
            if line.iter().all(|byte| byte.is_ascii_whitespace()) {
                continue;
            }
            values.push(serde_json::from_slice(line)?);
        }
        Ok(values)
    }
}
