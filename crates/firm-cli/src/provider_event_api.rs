//! Provider-native persisted Session reads.
//!
//! Provider callbacks are transport-only wake hints owned by the NodeDaemon.
//! They never carry or decode transcript content. Every semantic Session row is
//! read from the provider-owned persisted source through this service.

#[path = "provider_event_persisted.rs"]
mod persisted_service;
pub(crate) use persisted_service::*;

pub(crate) const DEFAULT_SESSION_PAGE_SIZE: usize = 80;
pub(crate) const MAX_SESSION_PAGE_SIZE: usize = 200;
