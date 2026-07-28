# Store authoring patterns

Use this before writing durable project Docs.

## Object mapping

```text
Document
  durable page and navigation identity

Block
  local explanation, heading, callout, table, or embedded context

TypedRecord
  reusable business fact with identity, lifecycle, and fields

Relation
  explicit cross-object link; avoids copied state

View
  saved presentation/query over canonical records

CustomPageDefinition / Package
  governed presentation metadata; not a data store
```

## Recommended sequence

1. Query current truth:

```bash
harness company docs query --document <document-id>
harness company docs traverse --document <root-document-id> --depth 2
harness company docs health
```

2. Define the page contract as a TypedRecord or explicit Document section.
3. Update Blocks for human-readable narrative and local tables.
4. Create or update TypedRecords for reusable facts.
5. Link Relations between Document, records, Work, Org, Finance, and source
   records.
6. Create Views for tables, boards, timelines, and matrices over records.
7. Verify with CLI, then use Store-live UI as human review evidence.

## Common command shape

```bash
harness company docs block append \
  --definition <page-definition-id> \
  --document <document-id> \
  --kind table \
  --content-json '<json>' \
  --actor <actor-id>

harness company docs typed-record append \
  --definition <page-definition-id> \
  --module <module-id> \
  --source-document <document-id> \
  --record-type page_contract \
  --title "<page title> contract" \
  --fields-json '<json>' \
  --actor <actor-id>

harness company docs relation link \
  --definition <page-definition-id> \
  --from-document <document-id> \
  --to-record <typed-record-id> \
  --actor <actor-id>
```

Use a simple table Block only when the rows are document-local explanation. Use
TypedRecords plus Views when rows will be reused, filtered, assigned, approved,
or linked to money or actors.
