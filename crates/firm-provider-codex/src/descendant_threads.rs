//! Process-local classification for Codex app-server child-thread frames.
//!
//! `thread/started` is the protocol authority for ancestry. The app-server V2
//! schema exposes the spawned thread at `params.thread.id` and its direct
//! ancestor at `params.thread.parentThreadId`.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameThreadScope {
    Owned,
    Descendant,
    Pending,
    Unscoped,
}

#[derive(Debug)]
pub(crate) struct FrameThreadObservation {
    pub(crate) scope: FrameThreadScope,
    pub(crate) newly_registered: Vec<String>,
}

#[derive(Debug)]
pub(crate) struct DescendantThreadRegistry {
    owned_thread_id: String,
    descendants: BTreeSet<String>,
    pending_parents: BTreeMap<String, String>,
    pending_frames: BTreeMap<String, String>,
}

impl DescendantThreadRegistry {
    pub(crate) fn new(owned_thread_id: impl Into<String>) -> Self {
        Self {
            owned_thread_id: owned_thread_id.into(),
            descendants: BTreeSet::new(),
            pending_parents: BTreeMap::new(),
            pending_frames: BTreeMap::new(),
        }
    }

    pub(crate) fn observe(&mut self, frame: &Value) -> Result<FrameThreadObservation, String> {
        let method = frame
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        if method == "thread/started" {
            return self.observe_thread_started(frame);
        }

        let Some(thread_id) = frame.pointer("/params/threadId").and_then(Value::as_str) else {
            return Ok(FrameThreadObservation {
                scope: FrameThreadScope::Unscoped,
                newly_registered: Vec::new(),
            });
        };
        if thread_id == self.owned_thread_id {
            return Ok(FrameThreadObservation {
                scope: FrameThreadScope::Owned,
                newly_registered: Vec::new(),
            });
        }
        if self.descendants.contains(thread_id) {
            return Ok(FrameThreadObservation {
                scope: FrameThreadScope::Descendant,
                newly_registered: Vec::new(),
            });
        }

        self.pending_frames
            .entry(thread_id.to_string())
            .or_insert_with(|| method.to_string());
        Ok(FrameThreadObservation {
            scope: FrameThreadScope::Pending,
            newly_registered: Vec::new(),
        })
    }

    pub(crate) fn ensure_no_pending(&self) -> Result<(), String> {
        let Some((thread_id, method)) = self.pending_frames.iter().next() else {
            return Ok(());
        };
        Err(self.violation(thread_id, method))
    }

    fn observe_thread_started(&mut self, frame: &Value) -> Result<FrameThreadObservation, String> {
        let thread_id = frame
            .pointer("/params/thread/id")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                "CODEX_RUNTIME_POSTCONDITION_UNKNOWN: thread/started omitted thread.id".to_string()
            })?;
        if thread_id == self.owned_thread_id {
            return Ok(FrameThreadObservation {
                scope: FrameThreadScope::Owned,
                newly_registered: Vec::new(),
            });
        }
        let Some(parent_id) = frame
            .pointer("/params/thread/parentThreadId")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        else {
            return Err(self.violation(thread_id, "thread/started"));
        };

        self.pending_parents
            .insert(thread_id.to_string(), parent_id.to_string());
        let newly_registered = self.resolve_descendants();
        let scope = if self.descendants.contains(thread_id) {
            FrameThreadScope::Descendant
        } else {
            self.pending_frames
                .entry(thread_id.to_string())
                .or_insert_with(|| "thread/started".to_string());
            FrameThreadScope::Pending
        };
        Ok(FrameThreadObservation {
            scope,
            newly_registered,
        })
    }

    fn resolve_descendants(&mut self) -> Vec<String> {
        let mut registered = Vec::new();
        loop {
            let ready = self
                .pending_parents
                .iter()
                .filter(|(_, parent_id)| {
                    parent_id.as_str() == self.owned_thread_id
                        || self.descendants.contains(parent_id.as_str())
                })
                .map(|(thread_id, _)| thread_id.clone())
                .collect::<Vec<_>>();
            if ready.is_empty() {
                break;
            }
            for thread_id in ready {
                self.pending_parents.remove(&thread_id);
                self.pending_frames.remove(&thread_id);
                if self.descendants.insert(thread_id.clone()) {
                    registered.push(thread_id);
                }
            }
        }
        registered
    }

    fn violation(&self, observed_thread: &str, method: &str) -> String {
        format!(
            "CODEX_ONE_DRIVER_VIOLATION: {method} for thread {observed_thread} has no ancestry linking it to owned thread {}",
            self.owned_thread_id
        )
    }
}
