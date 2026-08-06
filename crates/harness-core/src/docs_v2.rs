//! AI-first Docs slice records (ADR 0054).
//!
//! Additive contracts for the AI-first Docs target: a closed block kind set
//! with per-kind content validation, whole-page [`DocumentRevision`]s, and
//! atomic [`DocumentChangeOperation`] rows. Existing [`crate::company_os`]
//! records remain canonical until the ADR 0054 migration completes; these
//! types never rewrite them.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::company_os::{ActorRef, CompanyOsValidationError, EntityRef, ValidateCompanyOs};

fn required(value: &str, field: &'static str) -> Result<(), CompanyOsValidationError> {
    if value.trim().is_empty() {
        Err(CompanyOsValidationError::Required { field })
    } else {
        Ok(())
    }
}

fn invalid(field: &'static str, reason: impl Into<String>) -> CompanyOsValidationError {
    CompanyOsValidationError::Invalid {
        field,
        reason: reason.into(),
    }
}

/// Closed block kind set for the AI-first Docs page model. Adding a kind is a
/// schema-level change, not an ad-hoc extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKindV2 {
    Paragraph,
    Heading,
    BulletList,
    OrderedList,
    Checklist,
    Quote,
    Callout,
    Code,
    Table,
    Divider,
    PageEmbed,
    EntityEmbed,
    Image,
    Attachment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    Card,
    Inline,
}

fn display_mode(value: &Value, field: &'static str) -> Result<(), CompanyOsValidationError> {
    match value.as_str() {
        Some("card") | Some("inline") => Ok(()),
        other => Err(invalid(
            field,
            format!("display must be card|inline, got {other:?}"),
        )),
    }
}

fn string_entry(content: &Value, key: &str, field: &'static str) -> Result<(), CompanyOsValidationError> {
    match content.get(key).and_then(Value::as_str) {
        Some(_) => Ok(()),
        None => Err(invalid(field, format!("content.{key} must be a string"))),
    }
}

fn validate_list_content(content: &Value, field: &'static str) -> Result<(), CompanyOsValidationError> {
    let items = content
        .get("items")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(field, "content.items must be an array"))?;
    for (index, item) in items.iter().enumerate() {
        if item.get("text").and_then(Value::as_str).is_none() {
            return Err(invalid(field, format!("content.items[{index}].text must be a string")));
        }
        if let Some(checked) = item.get("checked") {
            if !checked.is_boolean() {
                return Err(invalid(field, format!("content.items[{index}].checked must be a boolean")));
            }
        }
    }
    Ok(())
}

fn validate_table_content(content: &Value, field: &'static str) -> Result<(), CompanyOsValidationError> {
    let header = content
        .get("header")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(field, "content.header must be an array"))?;
    for cell in header {
        if !cell.is_string() {
            return Err(invalid(field, "content.header cells must be strings"));
        }
    }
    let rows = content
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid(field, "content.rows must be an array"))?;
    for (index, row) in rows.iter().enumerate() {
        let cells = row
            .as_array()
            .ok_or_else(|| invalid(field, format!("content.rows[{index}] must be an array")))?;
        if cells.iter().any(|cell| !cell.is_string()) {
            return Err(invalid(field, format!("content.rows[{index}] cells must be strings")));
        }
    }
    Ok(())
}

fn validate_block_content(kind: BlockKindV2, content: &Value) -> Result<(), CompanyOsValidationError> {
    let field = "BlockV2.content";
    if !content.is_object() {
        return Err(invalid(field, "content must be an object"));
    }
    match kind {
        BlockKindV2::Paragraph | BlockKindV2::Quote | BlockKindV2::Code => {
            string_entry(content, "text", field)
        }
        BlockKindV2::Heading => {
            match content.get("level").and_then(Value::as_u64) {
                Some(level) if (1..=6).contains(&level) => {}
                other => {
                    return Err(invalid(field, format!("content.level must be 1..6, got {other:?}")))
                }
            }
            string_entry(content, "text", field)
        }
        BlockKindV2::BulletList | BlockKindV2::OrderedList | BlockKindV2::Checklist => {
            validate_list_content(content, field)
        }
        BlockKindV2::Callout => {
            let tone = content.get("tone").and_then(Value::as_str).unwrap_or_default();
            if !matches!(tone, "note" | "tip" | "warning" | "danger" | "info") {
                return Err(invalid(
                    field,
                    format!("content.tone must be note|tip|warning|danger|info, got {tone:?}"),
                ));
            }
            string_entry(content, "text", field)
        }
        BlockKindV2::Table => validate_table_content(content, field),
        BlockKindV2::Divider => Ok(()),
        BlockKindV2::PageEmbed => {
            required(
                content.get("target_document_id").and_then(Value::as_str).unwrap_or(""),
                "BlockV2.content.target_document_id",
            )?;
            display_mode(content.get("display").unwrap_or(&Value::Null), "BlockV2.content.display")
        }
        BlockKindV2::EntityEmbed => {
            let target = content
                .get("target")
                .ok_or_else(|| invalid(field, "content.target must be an entity ref object"))?;
            required(
                target.get("id").and_then(Value::as_str).unwrap_or(""),
                "BlockV2.content.target.id",
            )?;
            required(
                target.get("kind").and_then(Value::as_str).unwrap_or(""),
                "BlockV2.content.target.kind",
            )?;
            display_mode(content.get("display").unwrap_or(&Value::Null), "BlockV2.content.display")
        }
        BlockKindV2::Image | BlockKindV2::Attachment => required(
            content.get("blob_id").and_then(Value::as_str).unwrap_or(""),
            "BlockV2.content.blob_id",
        ),
    }
}

/// A block in the AI-first Docs page model. Block ids are stable anchors by
/// contract: replacement and reorder preserve identity. Ordering lives in the
/// owning `Document.block_ids`, not on the block row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockV2 {
    pub id: String,
    pub document_id: String,
    pub kind: BlockKindV2,
    pub content: Value,
    pub referenced_entities: Vec<EntityRef>,
    pub created_by: ActorRef,
    pub updated_by: ActorRef,
    pub created_at: String,
    pub updated_at: String,
}

impl ValidateCompanyOs for BlockV2 {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "BlockV2.id")?;
        required(&self.document_id, "BlockV2.document_id")?;
        validate_block_content(self.kind, &self.content)?;
        for reference in &self.referenced_entities {
            reference.validate()?;
        }
        self.created_by.validate()?;
        self.updated_by.validate()?;
        required(&self.created_at, "BlockV2.created_at")?;
        required(&self.updated_at, "BlockV2.updated_at")
    }
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Immutable whole-page revision. The snapshot reconstructs the document at
/// this revision; the digest is the sha256 of the canonical snapshot
/// serialization (serde_json default key ordering).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentRevision {
    pub id: String,
    pub document_id: String,
    pub revision_number: u64,
    pub parent_revision_id: Option<String>,
    pub content_snapshot: Value,
    pub content_digest: String,
    pub change_summary: String,
    pub authored_by: ActorRef,
    pub execution_ref: Option<EntityRef>,
    pub action_command_id: String,
    pub created_at: String,
}

impl ValidateCompanyOs for DocumentRevision {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.id, "DocumentRevision.id")?;
        required(&self.document_id, "DocumentRevision.document_id")?;
        if self.revision_number < 1 {
            return Err(invalid("DocumentRevision.revision_number", "must be >= 1"));
        }
        if !self.content_snapshot.is_object() {
            return Err(invalid("DocumentRevision.content_snapshot", "must be an object"));
        }
        if !is_sha256_hex(&self.content_digest) {
            return Err(invalid(
                "DocumentRevision.content_digest",
                "must be 64 lowercase hex chars (sha256)",
            ));
        }
        self.authored_by.validate()?;
        if let Some(execution) = &self.execution_ref {
            execution.validate()?;
        }
        required(&self.action_command_id, "DocumentRevision.action_command_id")?;
        required(&self.created_at, "DocumentRevision.created_at")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeMutationOp {
    BlockAppend,
    BlockInsertAfter,
    BlockReplace,
    BlockDelete,
    BlockMoveAfter,
    DocumentMetaUpdate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChangeMutation {
    pub op: ChangeMutationOp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_block_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_block_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_block_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<BlockV2>,
}

/// One atomic page change. `action_command_id` is the canonical idempotency
/// key; `expected_revision` is the revision the change was prepared against.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentChangeOperation {
    pub action_command_id: String,
    pub document_id: String,
    pub expected_revision: u64,
    pub mutations: Vec<ChangeMutation>,
    pub resulting_document_id: String,
    pub resulting_block_ids: Vec<String>,
    pub document_revision_id: String,
    pub created_at: String,
}

impl ValidateCompanyOs for DocumentChangeOperation {
    fn validate(&self) -> Result<(), CompanyOsValidationError> {
        required(&self.action_command_id, "DocumentChangeOperation.action_command_id")?;
        required(&self.document_id, "DocumentChangeOperation.document_id")?;
        for mutation in &self.mutations {
            if let Some(block) = &mutation.block {
                block.validate()?;
            }
        }
        required(
            &self.resulting_document_id,
            "DocumentChangeOperation.resulting_document_id",
        )?;
        required(
            &self.document_revision_id,
            "DocumentChangeOperation.document_revision_id",
        )?;
        required(&self.created_at, "DocumentChangeOperation.created_at")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::company_os::ActorType;
    use serde_json::json;

    fn actor() -> ActorRef {
        ActorRef {
            actor_type: ActorType::Agent,
            actor_id: "agent-test".to_string(),
        }
    }

    fn paragraph(id: &str) -> BlockV2 {
        BlockV2 {
            id: id.to_string(),
            document_id: "doc-1".to_string(),
            kind: BlockKindV2::Paragraph,
            content: json!({ "text": "hello" }),
            referenced_entities: vec![],
            created_by: actor(),
            updated_by: actor(),
            created_at: "2026-08-05T00:00:00Z".to_string(),
            updated_at: "2026-08-05T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn block_v2_validates_per_kind_content() {
        assert!(paragraph("blk-1").validate().is_ok());

        let mut heading = paragraph("blk-2");
        heading.kind = BlockKindV2::Heading;
        heading.content = json!({ "level": 2, "text": "Title" });
        assert!(heading.validate().is_ok());
        heading.content = json!({ "text": "no level" });
        assert!(heading.validate().is_err());

        let mut embed = paragraph("blk-3");
        embed.kind = BlockKindV2::PageEmbed;
        embed.content = json!({ "target_document_id": "doc-2", "display": "card" });
        assert!(embed.validate().is_ok());
        embed.content = json!({ "target_document_id": "doc-2", "display": "modal" });
        assert!(embed.validate().is_err());

        let mut legacy = paragraph("blk-4");
        legacy.kind = BlockKindV2::Paragraph;
        legacy.content = json!({ "items": [] });
        assert!(legacy.validate().is_err());
    }

    #[test]
    fn revision_requires_sha256_digest() {
        let revision = DocumentRevision {
            id: "rev-1".to_string(),
            document_id: "doc-1".to_string(),
            revision_number: 1,
            parent_revision_id: None,
            content_snapshot: json!({}),
            content_digest: "x".repeat(64),
            change_summary: "init".to_string(),
            authored_by: actor(),
            execution_ref: None,
            action_command_id: "act-1".to_string(),
            created_at: "2026-08-05T00:00:00Z".to_string(),
        };
        assert!(revision.validate().is_err());

        let mut ok = revision.clone();
        ok.content_digest = "a".repeat(64);
        assert!(ok.validate().is_ok());

        let mut zero = ok.clone();
        zero.revision_number = 0;
        assert!(zero.validate().is_err());
    }
}
