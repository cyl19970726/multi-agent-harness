//! Canonical filesystem ownership for Remote Fabric durable state.
//!
//! `firm-fabric` implements the transaction protocol; `firm-store` owns where
//! those journals live. Control Plane state is Company-scoped. Inbox/outbox
//! state is machine-scoped and durably binds itself to one Company + Node.

#![allow(clippy::result_large_err)]

use firm_fabric::{FabricError, FabricErrorCode, FabricStore, NodeLocalFabricStore};
use std::fs;
use std::path::{Path, PathBuf};

pub struct RemoteFabricStoreLayout {
    firm_home: PathBuf,
}

impl RemoteFabricStoreLayout {
    pub fn open(firm_home: impl AsRef<Path>) -> Result<Self, FabricError> {
        let firm_home = firm_home.as_ref();
        fs::create_dir_all(firm_home).map_err(layout_error)?;
        if fs::symlink_metadata(firm_home)
            .map_err(layout_error)?
            .file_type()
            .is_symlink()
        {
            return Err(FabricError::none(
                FabricErrorCode::StoreUnavailable,
                "FIRM_HOME may not be a symlink for Remote Fabric journals",
            ));
        }
        Ok(Self {
            firm_home: fs::canonicalize(firm_home).map_err(layout_error)?,
        })
    }

    pub fn firm_home(&self) -> &Path {
        &self.firm_home
    }

    pub fn control_plane_root(&self, company_id: &str) -> Result<PathBuf, FabricError> {
        validate_id(company_id, "Company")?;
        Ok(self
            .firm_home
            .join("companies")
            .join(company_id)
            .join("remote-fabric"))
    }

    pub fn node_local_root(&self, company_id: &str, node_id: &str) -> Result<PathBuf, FabricError> {
        validate_id(company_id, "Company")?;
        validate_id(node_id, "Node")?;
        Ok(self
            .firm_home
            .join("nodes")
            .join(node_id)
            .join("remote-fabric")
            .join(company_id))
    }

    /// Company-scoped Wave 6 business registry. This is deliberately adjacent
    /// to, but not inside, the FabricStore: route journals and collaboration
    /// relationships have distinct authorities and recovery lifecycles.
    pub fn collaboration_root(&self, company_id: &str) -> Result<PathBuf, FabricError> {
        validate_id(company_id, "Company")?;
        Ok(self
            .firm_home
            .join("companies")
            .join(company_id)
            .join("collaboration-v1"))
    }

    pub fn open_collaboration_store(
        &self,
        company_id: &str,
    ) -> Result<crate::HarnessStore, FabricError> {
        let store = crate::HarnessStore::new(self.collaboration_root(company_id)?);
        store.init().map_err(|error| {
            FabricError::none(
                FabricErrorCode::StoreUnavailable,
                format!("Company collaboration Store failed: {error}"),
            )
        })?;
        Ok(store)
    }

    pub fn open_control_plane(&self, company_id: &str) -> Result<FabricStore, FabricError> {
        FabricStore::open(self.control_plane_root(company_id)?)
    }

    pub fn open_node_local(
        &self,
        company_id: &str,
        node_id: &str,
    ) -> Result<NodeLocalFabricStore, FabricError> {
        NodeLocalFabricStore::open(
            self.node_local_root(company_id, node_id)?,
            company_id,
            node_id,
        )
    }
}

fn validate_id(value: &str, label: &str) -> Result<(), FabricError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
        || matches!(value, "." | "..")
    {
        return Err(FabricError::none(
            FabricErrorCode::InvalidPayload,
            format!("{label} id is not a safe canonical path component"),
        ));
    }
    Ok(())
}

fn layout_error(error: std::io::Error) -> FabricError {
    FabricError::none(
        FabricErrorCode::StoreUnavailable,
        format!("Remote Fabric Store layout failed: {error}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_root() -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let root = std::env::temp_dir().join(format!(
            "agentfirm-store-fabric-layout-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&root).expect("create isolated root");
        root
    }

    #[test]
    fn firm_store_owns_distinct_company_and_node_fabric_roots() {
        let root = temp_root();
        let layout = RemoteFabricStoreLayout::open(&root).expect("layout");
        let control = layout
            .open_control_plane("company-a")
            .expect("control store");
        let node = layout
            .open_node_local("company-a", "node-a")
            .expect("node store");
        let collaboration = layout
            .open_collaboration_store("company-a")
            .expect("collaboration store");
        assert!(control.root().starts_with(layout.firm_home()));
        assert!(node.root().starts_with(layout.firm_home()));
        assert_ne!(control.root(), node.root());
        assert_ne!(control.root(), collaboration.root());
        assert_eq!(
            layout
                .open_node_local("company-b", "node-a")
                .expect("separate Company root")
                .snapshot()
                .expect("snapshot")
                .revision,
            0
        );
        assert_eq!(
            layout
                .node_local_root("../escape", "node-a")
                .expect_err("path traversal must fail")
                .code,
            FabricErrorCode::InvalidPayload
        );
        fs::remove_dir_all(root).expect("remove isolated root");
    }
}
