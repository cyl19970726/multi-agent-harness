//! AI-first Docs v2 page commands (ADR 0054).
//!
//! Implements the page-first CLI surface: Markdown <-> closed block set
//! serialization, and `page create/read/write/append` over the store's atomic
//! revision boundary. Scoped reads follow the lark-cli-inspired contract:
//! outline/section/range/keyword with honest fragment markers.

use serde_json::json;

use firm_core::company_os::{
    ActorRef, ActorType, Block as LegacyBlock, BlockKind as LegacyBlockKind, Document,
    DocumentKind, LifecycleStatus,
};
use firm_core::docs_v2::{BlockKindV2, BlockV2, ChangeMutation, ChangeMutationOp};
use firm_store::docs_v2::PageWriteRequest;
use firm_store::{HarnessStore, StoreError};

use crate::{
    docs_actor_kind, generated_id, now_string, print_json, required, value, CliError, CliResult,
};

const PAGE_USAGE: &str = "usage: harness company docs page create --title <title> --actor <actor-id> [--markdown <text>] [--markdown-file <path>] [--id <doc-id>] [--space <id>] [--parent <doc-id>] [--format json|text] | page read --doc <id> [--scope outline|section|range|keyword] [--detail simple|with-ids|full] [--keyword <a|b>] [--start-block-id <id>] [--end-block-id <id>] [--revision <n|-1>] [--format json|markdown] | page write --doc <id> --expected-revision <n> (--markdown <text> | --markdown-file <path>) [--title <title>] [--summary <s>] [--format json|text] | page append --doc <id> (--markdown <text> | --markdown-file <path>) [--after <block-id|-1|end|heading:text>] [--expected-revision <n>] [--summary <s>] [--format json|text] | page search --keyword <a|b> [--limit <n>] | page rename --doc <id> --title <title> [--expected-revision <n>] [--format json|text] | page move --doc <id> --parent <doc-id|-1|root> [--expected-revision <n>] [--format json|text] | page archive --doc <id> --confirm [--expected-revision <n>] [--format json|text]";

fn actor_ref_from_args(args: &[String]) -> CliResult<ActorRef> {
    let actor_id = required(args, "--actor")?;
    let kind = docs_actor_kind(args)?;
    let actor_type = match kind.as_str() {
        "human" => ActorType::Human,
        _ => ActorType::Agent,
    };
    Ok(ActorRef {
        actor_type,
        actor_id,
    })
}

fn markdown_input(args: &[String]) -> CliResult<Option<String>> {
    match (value(args, "--markdown"), value(args, "--markdown-file")) {
        (Some(text), None) => Ok(Some(text)),
        (None, Some(path)) => std::fs::read_to_string(&path).map(Some).map_err(|error| {
            CliError::Usage(format!("cannot read --markdown-file {path}: {error}"))
        }),
        (Some(_), Some(_)) => Err(CliError::Usage(
            "cannot combine --markdown and --markdown-file".into(),
        )),
        (None, None) => Ok(None),
    }
}

/// When the caller pins `--action-id`, retries must be reproducible: block ids
/// derive deterministically from the action id so a resent payload matches the
/// committed one and replays instead of tripping IDEMPOTENCY_CONFLICT. With an
/// auto-generated action id every invocation is a fresh command anyway.
fn block_id_generator(
    action_command_id: &str,
    action_id_is_explicit: bool,
) -> Box<dyn FnMut() -> String> {
    if action_id_is_explicit {
        let prefix = format!("{action_command_id}-blk");
        let mut counter: u64 = 0;
        Box::new(move || {
            counter += 1;
            format!("{prefix}-{counter}")
        })
    } else {
        Box::new(|| generated_id("blk-v2"))
    }
}

/// F1: a missing page_embed target never blocks a write (targets are often
/// created later), but the result envelope surfaces an honest warning so the
/// operator knows a broken-ref card will render until the target exists.
fn embed_warnings(store: &HarnessStore, blocks: &[BlockV2]) -> Vec<String> {
    let mut warnings = Vec::new();
    for block in blocks {
        if block.kind != BlockKindV2::PageEmbed {
            continue;
        }
        let target = block
            .content
            .get("target_document_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if target.is_empty() {
            continue;
        }
        let exists = store.read_document_page(target).ok().flatten().is_some();
        if !exists {
            warnings.push(format!(
                "page_embed target missing: {target} (renders broken-ref until created)"
            ));
        }
    }
    warnings
}

fn conflict_to_usage(error: StoreError) -> CliError {
    match error {
        StoreError::Conflict(message) => CliError::Usage(message),
        StoreError::CompanyOsValidation(message) => CliError::Usage(message),
        StoreError::CompanyOsMissingReference(message) => CliError::Usage(message),
        other => CliError::Store(other),
    }
}

// ---------------------------------------------------------------------------
// Markdown -> blocks (closed set subset; inline formatting stays as markdown
// text inside content.text, declared lossy boundary per ADR 0054)
// ---------------------------------------------------------------------------

fn new_block(
    id: String,
    document_id: &str,
    kind: BlockKindV2,
    content: serde_json::Value,
    now: &str,
    actor: &ActorRef,
) -> BlockV2 {
    BlockV2 {
        id,
        document_id: document_id.to_string(),
        kind,
        content,
        referenced_entities: vec![],
        created_by: actor.clone(),
        updated_by: actor.clone(),
        created_at: now.to_string(),
        updated_at: now.to_string(),
    }
}

fn is_table_separator(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|')
        && trimmed.len() >= 3
        && trimmed.trim_matches('|').split('|').all(|cell| {
            let cell = cell.trim();
            !cell.is_empty() && cell.chars().all(|c| c == '-' || c == ':')
        })
}

fn split_table_row(line: &str) -> Vec<String> {
    let trimmed = line.trim().trim_matches('|');
    trimmed
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

fn flush_paragraph(
    blocks: &mut Vec<BlockV2>,
    paragraph: &mut Vec<String>,
    document_id: &str,
    now: &str,
    actor: &ActorRef,
    next_id: &mut dyn FnMut() -> String,
) {
    if paragraph.is_empty() {
        return;
    }
    let text = paragraph.join("\n");
    paragraph.clear();
    blocks.push(new_block(
        next_id(),
        document_id,
        BlockKindV2::Paragraph,
        json!({ "text": text }),
        now,
        actor,
    ));
}

fn flush_list(
    blocks: &mut Vec<BlockV2>,
    list_kind: &mut Option<BlockKindV2>,
    list_items: &mut Vec<serde_json::Value>,
    document_id: &str,
    now: &str,
    actor: &ActorRef,
    next_id: &mut dyn FnMut() -> String,
) {
    if let Some(kind) = list_kind.take() {
        let items = std::mem::take(list_items);
        blocks.push(new_block(
            next_id(),
            document_id,
            kind,
            json!({ "items": items }),
            now,
            actor,
        ));
    }
}

/// Parse a GFM subset into the closed block set. One pass, line-oriented.
pub fn parse_markdown_blocks(
    markdown: &str,
    document_id: &str,
    now: &str,
    actor: &ActorRef,
    mut next_id: impl FnMut() -> String,
) -> Vec<BlockV2> {
    let mut blocks: Vec<BlockV2> = Vec::new();
    let mut paragraph: Vec<String> = Vec::new();
    let mut list_kind: Option<BlockKindV2> = None;
    let mut list_items: Vec<serde_json::Value> = Vec::new();

    let lines: Vec<&str> = markdown.lines().collect();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();

        if trimmed.is_empty() {
            flush_paragraph(
                &mut blocks,
                &mut paragraph,
                document_id,
                now,
                actor,
                &mut next_id,
            );
            flush_list(
                &mut blocks,
                &mut list_kind,
                &mut list_items,
                document_id,
                now,
                actor,
                &mut next_id,
            );
            index += 1;
            continue;
        }

        // Fenced code
        if let Some(language) = trimmed.strip_prefix("```") {
            flush_paragraph(
                &mut blocks,
                &mut paragraph,
                document_id,
                now,
                actor,
                &mut next_id,
            );
            flush_list(
                &mut blocks,
                &mut list_kind,
                &mut list_items,
                document_id,
                now,
                actor,
                &mut next_id,
            );
            let language = language.trim().to_string();
            let mut body = Vec::new();
            index += 1;
            while index < lines.len() && !lines[index].trim().starts_with("```") {
                body.push(lines[index].to_string());
                index += 1;
            }
            index += 1; // closing fence (or end of input)
            let mut content = json!({ "text": body.join("\n") });
            if !language.is_empty() {
                content["language"] = json!(language);
            }
            blocks.push(new_block(
                next_id(),
                document_id,
                BlockKindV2::Code,
                content,
                now,
                actor,
            ));
            continue;
        }

        // Heading
        if trimmed.starts_with('#') {
            let level = trimmed.chars().take_while(|c| *c == '#').count();
            if let Some(text) = trimmed
                .get(level..)
                .map(str::trim)
                .filter(|t| !t.is_empty())
            {
                if level <= 6 {
                    flush_paragraph(
                        &mut blocks,
                        &mut paragraph,
                        document_id,
                        now,
                        actor,
                        &mut next_id,
                    );
                    flush_list(
                        &mut blocks,
                        &mut list_kind,
                        &mut list_items,
                        document_id,
                        now,
                        actor,
                        &mut next_id,
                    );
                    blocks.push(new_block(
                        next_id(),
                        document_id,
                        BlockKindV2::Heading,
                        json!({ "level": level, "text": text }),
                        now,
                        actor,
                    ));
                    index += 1;
                    continue;
                }
            }
        }

        // Divider
        if (trimmed.chars().all(|c| c == '-')
            || trimmed.chars().all(|c| c == '*')
            || trimmed.chars().all(|c| c == '_'))
            && trimmed.len() >= 3
        {
            flush_paragraph(
                &mut blocks,
                &mut paragraph,
                document_id,
                now,
                actor,
                &mut next_id,
            );
            flush_list(
                &mut blocks,
                &mut list_kind,
                &mut list_items,
                document_id,
                now,
                actor,
                &mut next_id,
            );
            blocks.push(new_block(
                next_id(),
                document_id,
                BlockKindV2::Divider,
                json!({}),
                now,
                actor,
            ));
            index += 1;
            continue;
        }

        // Embed marker: ![[page:<id> display=card]] or ![[view:<id>]]
        if trimmed.starts_with("![[") && trimmed.ends_with("]]") {
            let inner = &trimmed[3..trimmed.len() - 2];
            if let Some((kind_part, rest)) = inner.split_once(':') {
                let mut rest_parts = rest.split_whitespace();
                let target_id = rest_parts.next().unwrap_or("").to_string();
                let mut display = "card".to_string();
                for part in rest_parts {
                    if let Some(mode) = part.strip_prefix("display=") {
                        if matches!(mode, "card" | "inline") {
                            display = mode.to_string();
                        }
                    }
                }
                if !target_id.is_empty() {
                    flush_paragraph(
                        &mut blocks,
                        &mut paragraph,
                        document_id,
                        now,
                        actor,
                        &mut next_id,
                    );
                    flush_list(
                        &mut blocks,
                        &mut list_kind,
                        &mut list_items,
                        document_id,
                        now,
                        actor,
                        &mut next_id,
                    );
                    let (kind, content) = if kind_part == "page" {
                        (
                            BlockKindV2::PageEmbed,
                            json!({ "target_document_id": target_id, "display": display }),
                        )
                    } else if matches!(
                        kind_part,
                        "view" | "typed_record" | "work_item" | "relation"
                    ) {
                        (
                            BlockKindV2::EntityEmbed,
                            json!({ "target": { "kind": kind_part, "id": target_id }, "display": display }),
                        )
                    } else {
                        (BlockKindV2::Paragraph, json!({ "text": trimmed }))
                    };
                    blocks.push(new_block(next_id(), document_id, kind, content, now, actor));
                    index += 1;
                    continue;
                }
            }
        }

        // Callout: > [!note] optional title
        if let Some(marker) = trimmed.strip_prefix("> [!") {
            if let Some(close) = marker.find(']') {
                let tone_raw = marker[..close].to_lowercase();
                if matches!(
                    tone_raw.as_str(),
                    "note" | "tip" | "warning" | "danger" | "info"
                ) {
                    flush_paragraph(
                        &mut blocks,
                        &mut paragraph,
                        document_id,
                        now,
                        actor,
                        &mut next_id,
                    );
                    flush_list(
                        &mut blocks,
                        &mut list_kind,
                        &mut list_items,
                        document_id,
                        now,
                        actor,
                        &mut next_id,
                    );
                    let title = marker[close + 1..].trim().to_string();
                    let mut body = Vec::new();
                    index += 1;
                    while index < lines.len() && lines[index].trim().starts_with('>') {
                        body.push(
                            lines[index]
                                .trim()
                                .trim_start_matches('>')
                                .trim()
                                .to_string(),
                        );
                        index += 1;
                    }
                    let mut content = json!({ "tone": tone_raw, "text": body.join("\n") });
                    if !title.is_empty() {
                        content["title"] = json!(title);
                    }
                    blocks.push(new_block(
                        next_id(),
                        document_id,
                        BlockKindV2::Callout,
                        content,
                        now,
                        actor,
                    ));
                    continue;
                }
            }
        }

        // Quote
        if trimmed.starts_with('>') {
            flush_paragraph(
                &mut blocks,
                &mut paragraph,
                document_id,
                now,
                actor,
                &mut next_id,
            );
            flush_list(
                &mut blocks,
                &mut list_kind,
                &mut list_items,
                document_id,
                now,
                actor,
                &mut next_id,
            );
            let mut body = Vec::new();
            while index < lines.len() && lines[index].trim().starts_with('>') {
                body.push(
                    lines[index]
                        .trim()
                        .trim_start_matches('>')
                        .trim()
                        .to_string(),
                );
                index += 1;
            }
            blocks.push(new_block(
                next_id(),
                document_id,
                BlockKindV2::Quote,
                json!({ "text": body.join("\n") }),
                now,
                actor,
            ));
            continue;
        }

        // Table: header row + separator row
        if trimmed.starts_with('|')
            && index + 1 < lines.len()
            && is_table_separator(lines[index + 1])
        {
            flush_paragraph(
                &mut blocks,
                &mut paragraph,
                document_id,
                now,
                actor,
                &mut next_id,
            );
            flush_list(
                &mut blocks,
                &mut list_kind,
                &mut list_items,
                document_id,
                now,
                actor,
                &mut next_id,
            );
            let header = split_table_row(lines[index]);
            index += 2;
            let mut rows = Vec::new();
            while index < lines.len() && lines[index].trim().starts_with('|') {
                rows.push(split_table_row(lines[index]));
                index += 1;
            }
            blocks.push(new_block(
                next_id(),
                document_id,
                BlockKindV2::Table,
                json!({ "header": header, "rows": rows }),
                now,
                actor,
            ));
            continue;
        }

        // Checklist / bullet / ordered list items
        let checklist = trimmed
            .strip_prefix("- [")
            .or_else(|| trimmed.strip_prefix("* ["));
        if let Some(rest) = checklist {
            if let Some(checked_char) = rest.chars().next() {
                if matches!(checked_char, ' ' | 'x' | 'X') && rest.chars().nth(1) == Some(']') {
                    let text = rest.get(2..).unwrap_or("").trim().to_string();
                    if list_kind != Some(BlockKindV2::Checklist) {
                        flush_list(
                            &mut blocks,
                            &mut list_kind,
                            &mut list_items,
                            document_id,
                            now,
                            actor,
                            &mut next_id,
                        );
                        list_kind = Some(BlockKindV2::Checklist);
                    }
                    list_items.push(json!({ "text": text, "checked": checked_char != ' ' }));
                    flush_paragraph(
                        &mut blocks,
                        &mut paragraph,
                        document_id,
                        now,
                        actor,
                        &mut next_id,
                    );
                    index += 1;
                    continue;
                }
            }
        }
        if let Some(text) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            if list_kind != Some(BlockKindV2::BulletList) {
                flush_list(
                    &mut blocks,
                    &mut list_kind,
                    &mut list_items,
                    document_id,
                    now,
                    actor,
                    &mut next_id,
                );
                list_kind = Some(BlockKindV2::BulletList);
            }
            list_items.push(json!({ "text": text.trim() }));
            flush_paragraph(
                &mut blocks,
                &mut paragraph,
                document_id,
                now,
                actor,
                &mut next_id,
            );
            index += 1;
            continue;
        }
        let mut ordered_text: Option<&str> = None;
        for (pos, ch) in trimmed.char_indices() {
            if ch.is_ascii_digit() {
                continue;
            }
            if (ch == '.' || ch == ')') && pos > 0 {
                ordered_text = trimmed.get(pos + 1..).map(str::trim);
            }
            break;
        }
        if let Some(text) = ordered_text {
            if list_kind != Some(BlockKindV2::OrderedList) {
                flush_list(
                    &mut blocks,
                    &mut list_kind,
                    &mut list_items,
                    document_id,
                    now,
                    actor,
                    &mut next_id,
                );
                list_kind = Some(BlockKindV2::OrderedList);
            }
            list_items.push(json!({ "text": text }));
            flush_paragraph(
                &mut blocks,
                &mut paragraph,
                document_id,
                now,
                actor,
                &mut next_id,
            );
            index += 1;
            continue;
        }

        paragraph.push(line.trim_end().to_string());
        index += 1;
    }
    flush_paragraph(
        &mut blocks,
        &mut paragraph,
        document_id,
        now,
        actor,
        &mut next_id,
    );
    flush_list(
        &mut blocks,
        &mut list_kind,
        &mut list_items,
        document_id,
        now,
        actor,
        &mut next_id,
    );
    blocks
}

// ---------------------------------------------------------------------------
// Blocks -> Markdown
// ---------------------------------------------------------------------------

fn block_markdown(block: &BlockV2) -> String {
    let content = &block.content;
    match block.kind {
        BlockKindV2::Paragraph => content
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        BlockKindV2::Heading => {
            let level = content
                .get("level")
                .and_then(|v| v.as_u64())
                .unwrap_or(1)
                .min(6) as usize;
            let text = content.get("text").and_then(|v| v.as_str()).unwrap_or("");
            format!("{} {}", "#".repeat(level.max(1)), text)
        }
        BlockKindV2::BulletList | BlockKindV2::OrderedList | BlockKindV2::Checklist => {
            let items = content
                .get("items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let mut out = Vec::new();
            for (i, item) in items.iter().enumerate() {
                let text = item.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let prefix = match block.kind {
                    BlockKindV2::BulletList => "-".to_string(),
                    BlockKindV2::OrderedList => format!("{}.", i + 1),
                    _ => {
                        let checked = item
                            .get("checked")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        if checked {
                            "- [x]".to_string()
                        } else {
                            "- [ ]".to_string()
                        }
                    }
                };
                out.push(format!("{prefix} {text}"));
            }
            out.join("\n")
        }
        BlockKindV2::Quote => {
            let text = content.get("text").and_then(|v| v.as_str()).unwrap_or("");
            text.lines()
                .map(|line| format!("> {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        }
        BlockKindV2::Callout => {
            let tone = content
                .get("tone")
                .and_then(|v| v.as_str())
                .unwrap_or("note")
                .to_uppercase();
            let title = content.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let text = content.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let mut out = vec![format!("> [!{tone}] {title}").trim_end().to_string()];
            for line in text.lines() {
                out.push(format!("> {line}"));
            }
            out.join("\n")
        }
        BlockKindV2::Code => {
            let language = content
                .get("language")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let text = content.get("text").and_then(|v| v.as_str()).unwrap_or("");
            format!("```{language}\n{text}\n```")
        }
        BlockKindV2::Table => {
            let header = content
                .get("header")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let rows = content
                .get("rows")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let header_cells: Vec<String> = header
                .iter()
                .map(|cell| cell.as_str().unwrap_or("").to_string())
                .collect();
            let mut out = vec![format!("| {} |", header_cells.join(" | "))];
            out.push(format!(
                "| {} |",
                header_cells
                    .iter()
                    .map(|_| "---")
                    .collect::<Vec<_>>()
                    .join(" | ")
            ));
            for row in rows.iter().filter_map(|row| row.as_array()) {
                let cells: Vec<String> = row
                    .iter()
                    .map(|cell| cell.as_str().unwrap_or("").to_string())
                    .collect();
                out.push(format!("| {} |", cells.join(" | ")));
            }
            out.join("\n")
        }
        BlockKindV2::Divider => "---".to_string(),
        BlockKindV2::PageEmbed => {
            let target = content
                .get("target_document_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let display = content
                .get("display")
                .and_then(|v| v.as_str())
                .unwrap_or("card");
            format!("![[page:{target} display={display}]]")
        }
        BlockKindV2::EntityEmbed => {
            let target = content.get("target").cloned().unwrap_or(json!({}));
            let kind = target.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let id = target.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let display = content
                .get("display")
                .and_then(|v| v.as_str())
                .unwrap_or("card");
            format!("![[{kind}:{id} display={display}]]")
        }
        BlockKindV2::Image => {
            let blob = content
                .get("blob_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let alt = content.get("alt").and_then(|v| v.as_str()).unwrap_or("");
            format!("![{alt}](blob:{blob})")
        }
        BlockKindV2::Attachment => {
            let blob = content
                .get("blob_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let name = content.get("name").and_then(|v| v.as_str()).unwrap_or(blob);
            format!("[{name}](blob:{blob})")
        }
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Core page operations shared by the CLI and the serve API
// ---------------------------------------------------------------------------

/// R2 read compatibility: legacy (Block-era) blocks projected read-only into
/// the v2 block shape. Content mapping is best-effort and lossy by design;
/// unmapped payload is stringified into text so nothing silently disappears.
/// This path is read-only: v2 writes never flow through it.
fn legacy_block_to_v2(block: &LegacyBlock) -> BlockV2 {
    let content = &block.content;
    let text_fallback = || -> String {
        content
            .get("text")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| serde_json::to_string(content).unwrap_or_default())
    };
    let items_fallback = || -> serde_json::Value {
        content
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .map(serde_json::Value::Array)
            .unwrap_or_else(|| json!([{ "text": text_fallback() }]))
    };
    let entity_embed = |kind: &str| -> serde_json::Value {
        let id = content
            .get("id")
            .or_else(|| content.get(format!("{kind}_id").as_str()))
            .or_else(|| content.get("ref"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        json!({ "target": { "kind": kind, "id": id }, "display": "card" })
    };
    let (kind, mapped_content) = match block.kind {
        LegacyBlockKind::RichText => (BlockKindV2::Paragraph, json!({ "text": text_fallback() })),
        LegacyBlockKind::Heading => {
            let level = content
                .get("level")
                .and_then(|v| v.as_u64())
                .unwrap_or(2)
                .clamp(1, 6);
            (
                BlockKindV2::Heading,
                json!({ "level": level, "text": text_fallback() }),
            )
        }
        LegacyBlockKind::List => {
            let ordered = content
                .get("ordered")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let kind = if ordered {
                BlockKindV2::OrderedList
            } else {
                BlockKindV2::BulletList
            };
            (kind, json!({ "items": items_fallback() }))
        }
        LegacyBlockKind::Checklist => {
            (BlockKindV2::Checklist, json!({ "items": items_fallback() }))
        }
        LegacyBlockKind::Callout => {
            let tone = content
                .get("tone")
                .and_then(|v| v.as_str())
                .unwrap_or("note")
                .to_string();
            let mut mapped = json!({ "tone": tone, "text": text_fallback() });
            if let Some(title) = content.get("title").and_then(|v| v.as_str()) {
                mapped["title"] = json!(title);
            }
            (BlockKindV2::Callout, mapped)
        }
        LegacyBlockKind::Code => {
            let mut mapped = json!({ "text": text_fallback() });
            if let Some(language) = content.get("language").and_then(|v| v.as_str()) {
                mapped["language"] = json!(language);
            }
            (BlockKindV2::Code, mapped)
        }
        LegacyBlockKind::Media => (BlockKindV2::Image, content.clone()),
        LegacyBlockKind::Attachment => (BlockKindV2::Attachment, content.clone()),
        LegacyBlockKind::SimpleTable => {
            let header = content
                .get("header")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let rows = content
                .get("rows")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            (
                BlockKindV2::Table,
                json!({ "header": header, "rows": rows }),
            )
        }
        LegacyBlockKind::Comment => (
            BlockKindV2::Callout,
            json!({ "tone": "note", "title": "legacy comment", "text": text_fallback() }),
        ),
        LegacyBlockKind::Mention => (BlockKindV2::Paragraph, json!({ "text": text_fallback() })),
        LegacyBlockKind::EmbeddedView => (BlockKindV2::EntityEmbed, entity_embed("view")),
        LegacyBlockKind::Metric => (BlockKindV2::EntityEmbed, entity_embed("typed_record")),
        LegacyBlockKind::Decision => (BlockKindV2::EntityEmbed, entity_embed("typed_record")),
        LegacyBlockKind::WorkItem => (BlockKindV2::EntityEmbed, entity_embed("work_item")),
        LegacyBlockKind::RelationSummary => (
            BlockKindV2::Callout,
            json!({ "tone": "info", "title": "legacy relation summary", "text": text_fallback() }),
        ),
    };
    BlockV2 {
        id: block.id.clone(),
        document_id: block.document_id.clone(),
        kind,
        content: mapped_content,
        referenced_entities: block.referenced_entities.clone(),
        created_by: block.created_by.clone(),
        updated_by: block.updated_by.clone(),
        created_at: block.created_at.clone(),
        updated_at: block.updated_at.clone(),
    }
}

/// Block resolution for reads: prefer v2 block rows; when a document has none,
/// project its legacy blocks read-only (R2 retirement compatibility). The bool
/// reports whether the legacy projection was used.
fn page_blocks_with_legacy_fallback(
    store: &HarnessStore,
    document: &Document,
    v2_blocks: Vec<BlockV2>,
) -> CliResult<(Vec<BlockV2>, bool)> {
    let has_v2 = v2_blocks.iter().any(|b| b.document_id == document.id);
    if has_v2 || document.block_ids.is_empty() {
        // A document with v2 rows uses them; a document with no block ids at
        // all is simply empty (legacy rows, if any, are orphans and hidden).
        if has_v2 {
            return Ok((v2_blocks, false));
        }
    }
    let legacy = store.latest_blocks().map_err(conflict_to_usage)?;
    let doc_legacy: Vec<&LegacyBlock> = legacy
        .iter()
        .filter(|b| b.document_id == document.id)
        .collect();
    if doc_legacy.is_empty() {
        return Ok((v2_blocks, false));
    }
    let mapped: std::collections::BTreeMap<String, BlockV2> = doc_legacy
        .iter()
        .map(|b| (b.id.clone(), legacy_block_to_v2(b)))
        .collect();
    let mut ordered: Vec<BlockV2> = Vec::new();
    for id in &document.block_ids {
        if let Some(block) = mapped.get(id) {
            ordered.push(block.clone());
        }
    }
    let mut rest: Vec<&&LegacyBlock> = doc_legacy
        .iter()
        .filter(|b| !document.block_ids.contains(&b.id))
        .collect();
    rest.sort_by_key(|b| b.position);
    for block in rest {
        if let Some(mapped_block) = mapped.get(&block.id) {
            ordered.push(mapped_block.clone());
        }
    }
    Ok((ordered, true))
}

fn resolve_page(
    store: &HarnessStore,
    document_id: &str,
) -> CliResult<firm_store::docs_v2::DocumentPageState> {
    store
        .read_document_page(document_id)
        .map_err(conflict_to_usage)?
        .ok_or_else(|| CliError::Usage(format!("document not found: {document_id}")))
}

#[derive(Debug, Clone, Default)]
pub struct PageReadOptions {
    pub scope: String,
    pub detail: String,
    pub revision: Option<u64>,
    pub keyword: Option<String>,
    pub start_block_id: Option<String>,
    pub end_block_id: Option<String>,
    pub context_before: usize,
    pub context_after: usize,
}

pub fn read_page_value(
    store: &HarnessStore,
    document_id: &str,
    options: &PageReadOptions,
) -> CliResult<serde_json::Value> {
    let scope = if options.scope.is_empty() {
        "full".to_string()
    } else {
        options.scope.clone()
    };
    let detail = if options.detail.is_empty() {
        "simple".to_string()
    } else {
        options.detail.clone()
    };
    if !matches!(detail.as_str(), "simple" | "with-ids" | "full") {
        return Err(CliError::Usage(
            "--detail must be simple|with-ids|full".into(),
        ));
    }

    let mut legacy_projection = false;
    let (document, blocks, revision): (
        Document,
        Vec<BlockV2>,
        Option<firm_core::docs_v2::DocumentRevision>,
    ) = match options.revision {
        Some(number) => {
            let revision = store
                .document_revision_history(document_id)
                .map_err(conflict_to_usage)?
                .into_iter()
                .find(|r| r.revision_number == number)
                .ok_or_else(|| {
                    CliError::Usage(format!(
                        "revision {number} not found for document {document_id}"
                    ))
                })?;
            let snapshot = &revision.content_snapshot;
            let document: Document = serde_json::from_value(snapshot["document"].clone())
                .map_err(|e| CliError::Usage(format!("revision snapshot document invalid: {e}")))?;
            let blocks: Vec<BlockV2> = serde_json::from_value(snapshot["blocks"].clone())
                .map_err(|e| CliError::Usage(format!("revision snapshot blocks invalid: {e}")))?;
            (document, blocks, Some(revision))
        }
        None => {
            let state = resolve_page(store, document_id)?;
            let (blocks, legacy) =
                page_blocks_with_legacy_fallback(store, &state.document, state.blocks)?;
            legacy_projection = legacy;
            (state.document, blocks, state.revision)
        }
    };

    let mut fragment = false;
    let mut excerpts: Vec<String> = Vec::new();
    let mut scope_info = json!({ "mode": scope });
    let selected: Vec<BlockV2> = match scope.as_str() {
        "full" => blocks.clone(),
        "outline" => {
            fragment = true;
            blocks
                .iter()
                .filter(|b| b.kind == BlockKindV2::Heading)
                .cloned()
                .collect()
        }
        "keyword" => {
            let keyword = options.keyword.clone().ok_or_else(|| {
                CliError::Usage("--scope keyword requires --keyword <a|b>".into())
            })?;
            let needles: Vec<String> = keyword
                .split('|')
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
            if needles.is_empty() {
                return Err(CliError::Usage(
                    "--keyword must contain at least one term".into(),
                ));
            }
            let mut matched: Vec<usize> = Vec::new();
            for (i, block) in blocks.iter().enumerate() {
                let haystack = block_markdown(block).to_lowercase();
                if needles
                    .iter()
                    .any(|needle| haystack.contains(needle.as_str()))
                {
                    matched.push(i);
                }
            }
            let mut picked: Vec<usize> = Vec::new();
            for i in matched {
                let start = i.saturating_sub(options.context_before);
                let end = (i + 1 + options.context_after).min(blocks.len());
                for j in start..end {
                    if !picked.contains(&j) {
                        picked.push(j);
                    }
                }
                excerpts.push(blocks[i].id.clone());
            }
            fragment = true;
            scope_info["keyword"] = json!(keyword);
            picked.into_iter().map(|i| blocks[i].clone()).collect()
        }
        "section" => {
            let start_id = options.start_block_id.clone().ok_or_else(|| {
                CliError::Usage("--scope section requires --start-block-id".into())
            })?;
            let start = blocks
                .iter()
                .position(|b| b.id == start_id)
                .ok_or_else(|| CliError::Usage(format!("block not found: {start_id}")))?;
            let level = heading_level(&blocks[start]);
            let mut end = blocks.len();
            if level > 0 {
                for (offset, block) in blocks.iter().enumerate().skip(start + 1) {
                    if block.kind == BlockKindV2::Heading {
                        if let Some(l) = block.content.get("level").and_then(|v| v.as_u64()) {
                            if l <= level {
                                end = offset;
                                break;
                            }
                        }
                    }
                }
            }
            fragment = true;
            blocks[start..end].to_vec()
        }
        "range" => {
            let start_id = options.start_block_id.clone();
            let end_id = options.end_block_id.clone();
            if start_id.is_none() && end_id.is_none() {
                return Err(CliError::Usage(
                    "--scope range requires --start-block-id and/or --end-block-id".into(),
                ));
            }
            let start = match &start_id {
                Some(id) => blocks
                    .iter()
                    .position(|b| b.id == *id)
                    .ok_or_else(|| CliError::Usage(format!("block not found: {id}")))?,
                None => 0,
            };
            let end = match &end_id {
                Some(id) if id != "-1" => blocks
                    .iter()
                    .position(|b| b.id == *id)
                    .map(|p| p + 1)
                    .ok_or_else(|| CliError::Usage(format!("block not found: {id}")))?,
                _ => blocks.len(),
            };
            if end < start {
                return Err(CliError::Usage("range end precedes range start".into()));
            }
            fragment = true;
            blocks[start..end].to_vec()
        }
        other => {
            return Err(CliError::Usage(format!(
                "--scope must be outline|section|range|keyword (or omitted for full), got {other}"
            )))
        }
    };
    scope_info["fragment"] = json!(fragment);
    scope_info["excerpts"] = json!(excerpts);

    let with_ids = detail != "simple";
    let include_content = detail == "full";
    let rendered: Vec<serde_json::Value> = selected
        .iter()
        .map(|block| {
            let mut entry = json!({
                "kind": serde_json::to_value(block.kind).unwrap_or(json!(null)),
                "markdown": block_markdown(block),
            });
            if with_ids {
                entry["id"] = json!(block.id);
            }
            if include_content {
                entry["content"] = block.content.clone();
            }
            entry
        })
        .collect();
    Ok(json!({
        "document_id": document.id,
        "title": document.title,
        "parent_document_id": document.parent_document_id,
        "legacy_projection": legacy_projection,
        "lifecycle_status": document.lifecycle_status,
        "revision_id": revision.as_ref().map(|r| r.id.clone()),
        "revision_number": revision.as_ref().map(|r| r.revision_number).unwrap_or(0),
        "content_digest": revision.as_ref().map(|r| r.content_digest.clone()),
        "scope": scope_info,
        "blocks": rendered,
    }))
}

/// Serialize a `read_page_value` result back to plain Markdown.
pub fn page_value_markdown(value: &serde_json::Value) -> String {
    value["blocks"]
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .map(|block| block["markdown"].as_str().unwrap_or("").to_string())
                .collect::<Vec<_>>()
                .join("\n\n")
        })
        .unwrap_or_default()
}

#[allow(clippy::too_many_arguments)]
pub fn create_page_value(
    store: &HarnessStore,
    document_id: &str,
    title: &str,
    markdown: &str,
    space_id: &str,
    parent_document_id: Option<&str>,
    actor: ActorRef,
    summary: &str,
    action_id: Option<String>,
) -> CliResult<serde_json::Value> {
    let now = now_string();
    if store
        .read_document_page(document_id)
        .map_err(conflict_to_usage)?
        .is_some()
    {
        return Err(CliError::Usage(format!(
            "document already exists: {document_id}"
        )));
    }
    let explicit = action_id.is_some();
    let action_command_id = action_id.unwrap_or_else(|| generated_id("action-cli-docs-page"));
    let blocks = parse_markdown_blocks(
        markdown,
        document_id,
        &now,
        &actor,
        block_id_generator(&action_command_id, explicit),
    );
    let document = Document {
        id: document_id.to_string(),
        space_id: space_id.to_string(),
        parent_document_id: parent_document_id.map(str::to_string),
        title: title.to_string(),
        kind: DocumentKind::Page,
        lifecycle_status: LifecycleStatus::Active,
        block_ids: blocks.iter().map(|b| b.id.clone()).collect(),
        template_ref: None,
        permission_policy_refs: vec!["company.records.write".to_string()],
        reference_refs: vec![],
        created_by: actor.clone(),
        updated_by: actor.clone(),
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    let outcome = store
        .write_document_page_atomic(&PageWriteRequest {
            document,
            block_rows: blocks.clone(),
            mutations: vec![ChangeMutation {
                op: ChangeMutationOp::DocumentMetaUpdate,
                anchor_block_id: None,
                target_block_id: None,
                source_block_ids: vec![],
                block: None,
            }],
            expected_revision: 0,
            change_summary: summary.to_string(),
            authored_by: actor,
            execution_ref: None,
            action_command_id,
            created_at: now,
        })
        .map_err(conflict_to_usage)?;
    Ok(json!({
        "result": "success",
        "document_id": document_id,
        "revision_id": outcome.revision_id,
        "revision_number": outcome.revision_number,
        "content_digest": outcome.content_digest,
        "blocks": blocks.len(),
        "warnings": embed_warnings(store, &blocks),
    }))
}

#[allow(clippy::too_many_arguments)]
pub fn write_page_value(
    store: &HarnessStore,
    document_id: &str,
    markdown: &str,
    expected_revision: u64,
    new_title: Option<&str>,
    actor: ActorRef,
    summary: &str,
    action_id: Option<String>,
) -> CliResult<serde_json::Value> {
    let now = now_string();
    let current = resolve_page(store, document_id)?;
    let explicit = action_id.is_some();
    let action_command_id = action_id.unwrap_or_else(|| generated_id("action-cli-docs-page"));
    let blocks = parse_markdown_blocks(
        markdown,
        document_id,
        &now,
        &actor,
        block_id_generator(&action_command_id, explicit),
    );
    let mut document = current.document.clone();
    if let Some(title) = new_title {
        document.title = title.to_string();
    }
    document.block_ids = blocks.iter().map(|b| b.id.clone()).collect();
    document.updated_by = actor.clone();
    document.updated_at = now.clone();

    let outcome = store
        .write_document_page_atomic(&PageWriteRequest {
            document,
            block_rows: blocks.clone(),
            mutations: vec![ChangeMutation {
                op: ChangeMutationOp::BlockReplace,
                anchor_block_id: None,
                target_block_id: None,
                source_block_ids: vec![],
                block: None,
            }],
            expected_revision,
            change_summary: summary.to_string(),
            authored_by: actor,
            execution_ref: None,
            action_command_id,
            created_at: now,
        })
        .map_err(conflict_to_usage)?;
    Ok(json!({
        "result": "success",
        "document_id": document_id,
        "revision_id": outcome.revision_id,
        "revision_number": outcome.revision_number,
        "content_digest": outcome.content_digest,
        "replayed": outcome.replayed,
        "blocks": blocks.len(),
        "warnings": embed_warnings(store, &blocks),
    }))
}

#[allow(clippy::too_many_arguments)]
pub fn append_page_value(
    store: &HarnessStore,
    document_id: &str,
    markdown: &str,
    after_block_id: Option<&str>,
    expected_revision: Option<u64>,
    actor: ActorRef,
    summary: &str,
    action_id: Option<String>,
) -> CliResult<serde_json::Value> {
    let now = now_string();
    let current = resolve_page(store, document_id)?;
    let expected = match expected_revision {
        Some(value) => value,
        None => current
            .revision
            .as_ref()
            .map(|r| r.revision_number)
            .unwrap_or(0),
    };
    if let Some(anchor_id) = after_block_id {
        if !current.document.block_ids.contains(&anchor_id.to_string()) {
            return Err(CliError::Usage(format!(
                "anchor block not found in document {document_id}: {anchor_id}"
            )));
        }
    }
    let explicit = action_id.is_some();
    let action_command_id = action_id.unwrap_or_else(|| generated_id("action-cli-docs-page"));
    let new_blocks = parse_markdown_blocks(
        markdown,
        document_id,
        &now,
        &actor,
        block_id_generator(&action_command_id, explicit),
    );
    let mut block_ids = current.document.block_ids.clone();
    match after_block_id {
        Some(anchor_id) => {
            let position = block_ids
                .iter()
                .position(|id| id == anchor_id)
                .unwrap_or(block_ids.len());
            let mut tail = block_ids.split_off(position + 1);
            block_ids.extend(new_blocks.iter().map(|b| b.id.clone()));
            block_ids.append(&mut tail);
        }
        None => block_ids.extend(new_blocks.iter().map(|b| b.id.clone())),
    }
    let mut document = current.document.clone();
    document.block_ids = block_ids;
    document.updated_by = actor.clone();
    document.updated_at = now.clone();

    let outcome = store
        .write_document_page_atomic(&PageWriteRequest {
            document,
            block_rows: new_blocks.clone(),
            mutations: vec![ChangeMutation {
                op: ChangeMutationOp::BlockAppend,
                anchor_block_id: after_block_id.map(str::to_string),
                target_block_id: None,
                source_block_ids: vec![],
                block: None,
            }],
            expected_revision: expected,
            change_summary: summary.to_string(),
            authored_by: actor,
            execution_ref: None,
            action_command_id,
            created_at: now,
        })
        .map_err(conflict_to_usage)?;
    Ok(json!({
        "result": "success",
        "document_id": document_id,
        "revision_id": outcome.revision_id,
        "revision_number": outcome.revision_number,
        "content_digest": outcome.content_digest,
        "appended_blocks": new_blocks.len(),
        "warnings": embed_warnings(store, &new_blocks),
    }))
}

// ---------------------------------------------------------------------------
// CLI command wrappers (argument parsing only; logic lives above)
// ---------------------------------------------------------------------------

pub fn company_docs_page_v2_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    crate::require_subcommand(
        args,
        "company docs page create|read|write|append|search|rename|move|archive|scaffold|verify|publish",
    )?;
    match args[0].as_str() {
        "create" => page_create_command(store, &args[1..]),
        "read" => page_read_command(store, &args[1..]),
        "write" => page_write_command(store, &args[1..]),
        "append" => page_append_command(store, &args[1..]),
        "search" => page_search_command(store, &args[1..]),
        "rename" => page_rename_command(store, &args[1..]),
        "move" => page_move_command(store, &args[1..]),
        "archive" => page_archive_command(store, &args[1..]),
        other => Err(CliError::Usage(format!(
            "not a docs-v2 page verb: {other}; {PAGE_USAGE}"
        ))),
    }
}

fn parse_expected_revision(args: &[String], required_flag: bool) -> CliResult<Option<u64>> {
    match value(args, "--expected-revision") {
        Some(raw) => Ok(Some(raw.parse::<u64>().map_err(|_| {
            CliError::Usage("--expected-revision must be an integer".into())
        })?)),
        None if required_flag => Err(CliError::Usage(format!(
            "page write requires --expected-revision; {PAGE_USAGE}"
        ))),
        None => Ok(None),
    }
}

fn page_create_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let title = required(args, "--title")?;
    let actor = actor_ref_from_args(args)?;
    let space_id = value(args, "--space").unwrap_or_else(|| "company".to_string());
    let parent = value(args, "--parent");
    let slug = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let document_id = value(args, "--id").unwrap_or_else(|| {
        format!(
            "document-cli-{}",
            if slug.is_empty() {
                generated_id("page")
            } else {
                slug
            }
        )
    });
    let markdown = markdown_input(args)?.unwrap_or_default();
    let summary = value(args, "--summary").unwrap_or_else(|| "page create".to_string());
    let result = create_page_value(
        store,
        &document_id,
        &title,
        &markdown,
        &space_id,
        parent.as_deref(),
        actor,
        &summary,
        value(args, "--action-id"),
    )?;
    if value(args, "--format").as_deref() == Some("text") {
        let digest = result["content_digest"].as_str().unwrap_or("");
        println!(
            "ok {} r{} sha256:{}… ({} blocks)",
            result["document_id"],
            result["revision_number"],
            &digest[..digest.len().min(12)],
            result["blocks"]
        );
        for warning in result["warnings"].as_array().unwrap_or(&vec![]) {
            println!("warning: {}", warning.as_str().unwrap_or(""));
        }
        return Ok(());
    }
    print_json(&result)
}

fn page_write_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let document_id = required(args, "--doc")?;
    let expected = parse_expected_revision(args, true)?.unwrap();
    let markdown = markdown_input(args)?.ok_or_else(|| {
        CliError::Usage(format!(
            "page write requires --markdown or --markdown-file; {PAGE_USAGE}"
        ))
    })?;
    let actor = actor_ref_from_args(args)?;
    let summary = value(args, "--summary").unwrap_or_else(|| "page write".to_string());
    let result = write_page_value(
        store,
        &document_id,
        &markdown,
        expected,
        value(args, "--title").as_deref(),
        actor,
        &summary,
        value(args, "--action-id"),
    )?;
    if value(args, "--format").as_deref() == Some("text") {
        let digest = result["content_digest"].as_str().unwrap_or("");
        println!(
            "ok {} r{} sha256:{}… ({} blocks{})",
            result["document_id"],
            result["revision_number"],
            &digest[..digest.len().min(12)],
            result["blocks"],
            if result["replayed"].as_bool().unwrap_or(false) {
                ", replayed"
            } else {
                ""
            }
        );
        for warning in result["warnings"].as_array().unwrap_or(&vec![]) {
            println!("warning: {}", warning.as_str().unwrap_or(""));
        }
        return Ok(());
    }
    print_json(&result)
}

/// F2: anchor addressing for append. Accepts a raw block id, `-1`/`end` for
/// the document end, or `heading:<text>` for a unique case-insensitive
/// heading match. Ambiguous heading matches are rejected with guidance.
fn resolve_after_anchor(
    store: &HarnessStore,
    document_id: &str,
    after: &str,
) -> CliResult<Option<String>> {
    if after == "-1" || after == "end" {
        return Ok(None);
    }
    if let Some(needle) = after.strip_prefix("heading:") {
        let needle = needle.trim().to_lowercase();
        if needle.is_empty() {
            return Err(CliError::Usage(
                "heading:<text> requires a non-empty heading match".into(),
            ));
        }
        let state = resolve_page(store, document_id)?;
        let heading_matches: Vec<&BlockV2> = state
            .blocks
            .iter()
            .filter(|b| {
                b.kind == BlockKindV2::Heading
                    && b.content
                        .get("text")
                        .and_then(|v| v.as_str())
                        .map(|t| t.to_lowercase().contains(&needle))
                        .unwrap_or(false)
            })
            .collect();
        return match heading_matches.len() {
            0 => Err(CliError::Usage(format!(
                "no heading matches '{needle}' in document {document_id}"
            ))),
            1 => Ok(Some(heading_matches[0].id.clone())),
            n => Err(CliError::Usage(format!(
                "{n} headings match '{needle}' in document {document_id}; use a block id to disambiguate"
            ))),
        };
    }
    Ok(Some(after.to_string()))
}

fn page_append_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let document_id = required(args, "--doc")?;
    let markdown = markdown_input(args)?.ok_or_else(|| {
        CliError::Usage(format!(
            "page append requires --markdown or --markdown-file; {PAGE_USAGE}"
        ))
    })?;
    let actor = actor_ref_from_args(args)?;
    let anchor_id = match value(args, "--after").as_deref() {
        Some(after) => resolve_after_anchor(store, &document_id, after)?,
        None => None,
    };
    let summary = value(args, "--summary").unwrap_or_else(|| "page append".to_string());
    let result = append_page_value(
        store,
        &document_id,
        &markdown,
        anchor_id.as_deref(),
        parse_expected_revision(args, false)?,
        actor,
        &summary,
        value(args, "--action-id"),
    )?;
    if value(args, "--format").as_deref() == Some("text") {
        let digest = result["content_digest"].as_str().unwrap_or("");
        println!(
            "ok {} r{} sha256:{}… (+{} blocks)",
            result["document_id"],
            result["revision_number"],
            &digest[..digest.len().min(12)],
            result["appended_blocks"]
        );
        for warning in result["warnings"].as_array().unwrap_or(&vec![]) {
            println!("warning: {}", warning.as_str().unwrap_or(""));
        }
        return Ok(());
    }
    print_json(&result)
}

fn page_read_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let document_id = required(args, "--doc")?;
    let format = value(args, "--format").unwrap_or_else(|| "json".to_string());
    if !matches!(format.as_str(), "json" | "markdown") {
        return Err(CliError::Usage("--format must be json|markdown".into()));
    }
    let options = PageReadOptions {
        scope: value(args, "--scope").unwrap_or_else(|| "full".to_string()),
        detail: value(args, "--detail").unwrap_or_else(|| "simple".to_string()),
        revision: match value(args, "--revision") {
            Some(raw) if raw != "-1" => Some(
                raw.parse::<u64>()
                    .map_err(|_| CliError::Usage("--revision must be an integer or -1".into()))?,
            ),
            _ => None,
        },
        keyword: value(args, "--keyword"),
        start_block_id: value(args, "--start-block-id"),
        end_block_id: value(args, "--end-block-id"),
        context_before: value(args, "--context-before")
            .map(|v| v.parse::<usize>().unwrap_or(0))
            .unwrap_or(0),
        context_after: value(args, "--context-after")
            .map(|v| v.parse::<usize>().unwrap_or(0))
            .unwrap_or(0),
    };
    let result = read_page_value(store, &document_id, &options)?;
    if format == "markdown" {
        println!("{}", page_value_markdown(&result));
        return Ok(());
    }
    print_json(&result)
}

/// R1: metadata-only page change (rename/move/archive) committed through the
/// same revision mechanism: one document-row update, no block-row appends,
/// one new revision whose snapshot carries the updated document metadata.
#[allow(clippy::too_many_arguments)]
pub fn update_page_meta_value(
    store: &HarnessStore,
    document_id: &str,
    new_title: Option<&str>,
    new_parent: Option<Option<String>>,
    new_lifecycle: Option<firm_core::company_os::LifecycleStatus>,
    expected_revision: Option<u64>,
    actor: ActorRef,
    summary: &str,
    action_id: Option<String>,
) -> CliResult<serde_json::Value> {
    let now = now_string();
    let current = resolve_page(store, document_id)?;
    let expected = match expected_revision {
        Some(value) => value,
        None => current
            .revision
            .as_ref()
            .map(|r| r.revision_number)
            .unwrap_or(0),
    };
    let mut document = current.document.clone();
    if let Some(title) = new_title {
        if title.trim().is_empty() {
            return Err(CliError::Usage("--title must be non-empty".into()));
        }
        document.title = title.trim().to_string();
    }
    if let Some(parent) = new_parent {
        document.parent_document_id = parent;
    }
    if let Some(lifecycle) = new_lifecycle {
        document.lifecycle_status = lifecycle;
    }
    document.updated_by = actor.clone();
    document.updated_at = now.clone();

    let outcome = store
        .write_document_page_atomic(&PageWriteRequest {
            document,
            block_rows: vec![],
            mutations: vec![ChangeMutation {
                op: ChangeMutationOp::DocumentMetaUpdate,
                anchor_block_id: None,
                target_block_id: None,
                source_block_ids: vec![],
                block: None,
            }],
            expected_revision: expected,
            change_summary: summary.to_string(),
            authored_by: actor,
            execution_ref: None,
            action_command_id: action_id.unwrap_or_else(|| generated_id("action-cli-docs-page")),
            created_at: now,
        })
        .map_err(conflict_to_usage)?;
    Ok(json!({
        "result": "success",
        "document_id": document_id,
        "revision_id": outcome.revision_id,
        "revision_number": outcome.revision_number,
        "content_digest": outcome.content_digest,
    }))
}

/// F3 (interim): cross-document search over latest page projections. This is
/// an honest substring scan, not an FTS index; the derived SQLite layer (spec
/// Phase 3) supersedes it.
pub fn search_pages_value(
    store: &HarnessStore,
    keyword: &str,
    limit: usize,
) -> CliResult<serde_json::Value> {
    let needles: Vec<String> = keyword
        .split('|')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if needles.is_empty() {
        return Err(CliError::Usage(
            "--keyword must contain at least one term".into(),
        ));
    }
    let documents = store.latest_documents().map_err(conflict_to_usage)?;
    let all_blocks = store.latest_blocks_v2().map_err(conflict_to_usage)?;
    let mut blocks_by_doc: std::collections::BTreeMap<String, Vec<BlockV2>> =
        std::collections::BTreeMap::new();
    for block in all_blocks {
        blocks_by_doc
            .entry(block.document_id.clone())
            .or_default()
            .push(block);
    }
    let mut hits: Vec<serde_json::Value> = Vec::new();
    'outer: for document in documents.iter().filter(|d| d.kind == DocumentKind::Page) {
        if needles
            .iter()
            .any(|n| document.title.to_lowercase().contains(n.as_str()))
        {
            hits.push(json!({
                "document_id": document.id,
                "title": document.title,
                "hit": "title",
                "block_id": null,
                "snippet": document.title,
            }));
            if hits.len() >= limit {
                break;
            }
        }
        let doc_blocks = blocks_by_doc.get(&document.id).cloned().unwrap_or_default();
        for block in &doc_blocks {
            let markdown = block_markdown(block);
            if needles
                .iter()
                .any(|n| markdown.to_lowercase().contains(n.as_str()))
            {
                let snippet: String = markdown.chars().take(160).collect();
                hits.push(json!({
                    "document_id": document.id,
                    "title": document.title,
                    "hit": "block",
                    "block_id": block.id,
                    "kind": serde_json::to_value(block.kind).unwrap_or(json!(null)),
                    "snippet": snippet,
                }));
                if hits.len() >= limit {
                    break 'outer;
                }
            }
        }
    }
    Ok(json!({
        "index": "projection-scan (not FTS; SQLite FTS arrives in Phase 3)",
        "keyword": keyword,
        "count": hits.len(),
        "matches": hits,
    }))
}

fn page_rename_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let document_id = required(args, "--doc")?;
    let title = required(args, "--title")?;
    let actor = actor_ref_from_args(args)?;
    let summary = value(args, "--summary").unwrap_or_else(|| "page rename".to_string());
    let result = update_page_meta_value(
        store,
        &document_id,
        Some(&title),
        None,
        None,
        parse_expected_revision(args, false)?,
        actor,
        &summary,
        value(args, "--action-id"),
    )?;
    emit_meta_result(args, &result, "renamed");
    Ok(())
}

fn page_move_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let document_id = required(args, "--doc")?;
    let parent_raw = required(args, "--parent")?;
    let actor = actor_ref_from_args(args)?;
    let new_parent: Option<String> = if parent_raw == "-1" || parent_raw == "root" {
        None
    } else {
        // Parent must exist and must not be the document itself or one of its
        // descendants (cycle check over the latest document projection).
        let documents = store.latest_documents().map_err(conflict_to_usage)?;
        if !documents.iter().any(|d| d.id == parent_raw) {
            return Err(CliError::Usage(format!(
                "parent document not found: {parent_raw}"
            )));
        }
        let mut cursor: Option<String> = Some(parent_raw.clone());
        while let Some(id) = cursor {
            if id == document_id {
                return Err(CliError::Usage(format!(
                    "move rejected: {parent_raw} is {document_id} itself or one of its descendants (parent cycle)"
                )));
            }
            cursor = documents
                .iter()
                .find(|d| d.id == id)
                .and_then(|d| d.parent_document_id.clone());
        }
        Some(parent_raw)
    };
    let summary = value(args, "--summary").unwrap_or_else(|| "page move".to_string());
    let result = update_page_meta_value(
        store,
        &document_id,
        None,
        Some(new_parent),
        None,
        parse_expected_revision(args, false)?,
        actor,
        &summary,
        value(args, "--action-id"),
    )?;
    emit_meta_result(args, &result, "moved");
    Ok(())
}

fn page_archive_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let document_id = required(args, "--doc")?;
    let actor = actor_ref_from_args(args)?;
    if !crate::has_flag(args, "--confirm") {
        let current = resolve_page(store, &document_id)?;
        print_json(&json!({
            "result": "dry_run",
            "document_id": document_id,
            "title": current.document.title,
            "revision_number": current.revision.as_ref().map(|r| r.revision_number).unwrap_or(0),
            "note": "page archive requires --confirm to commit; nothing was written",
        }))?;
        return Ok(());
    }
    let summary = value(args, "--summary").unwrap_or_else(|| "page archive".to_string());
    let result = update_page_meta_value(
        store,
        &document_id,
        None,
        None,
        Some(firm_core::company_os::LifecycleStatus::Archived),
        parse_expected_revision(args, false)?,
        actor,
        &summary,
        value(args, "--action-id"),
    )?;
    emit_meta_result(args, &result, "archived");
    Ok(())
}

fn emit_meta_result(args: &[String], result: &serde_json::Value, verb: &str) {
    if value(args, "--format").as_deref() == Some("text") {
        let digest = result["content_digest"].as_str().unwrap_or("");
        println!(
            "ok {} r{} sha256:{}… ({verb})",
            result["document_id"],
            result["revision_number"],
            &digest[..digest.len().min(12)],
        );
        return;
    }
    let _ = print_json(result);
}

fn page_search_command(store: &HarnessStore, args: &[String]) -> CliResult<()> {
    let keyword = required(args, "--keyword")?;
    let limit = value(args, "--limit")
        .map(|v| v.parse::<usize>().unwrap_or(50))
        .unwrap_or(50);
    print_json(&search_pages_value(store, &keyword, limit)?)
}

fn heading_level(block: &BlockV2) -> u64 {
    if block.kind != BlockKindV2::Heading {
        return 0;
    }
    block
        .content
        .get("level")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
}
