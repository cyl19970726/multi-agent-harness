# Page contract reference

Use this when a real business page must be created, reorganized, or prepared
for a custom page.

## Boundary

Repository design files are construction plans. The live Company OS truth must
exist in the project Store as Documents, Blocks, TypedRecords, Relations,
Views, BusinessModules, Work records, Actors, Approvals, and FinancialRecords.

For example:

```text
repo design/.../wanchengwanling-docs-ia-v1.md
  -> design contract for future agents and developers

Store Document + page_contract TypedRecord + Relations + Views
  -> actual company memory and Agent-operable truth
```

Do not treat a repository markdown design as the product's live business
document. If humans or Agents are expected to operate it, write the required
page contract and business facts into the Store through governed commands.

## Required fields

A `page_contract` record should name:

```text
page_id
document_id
module_id
primary_question
human_audience
agent_audience
required_sections
required_typed_records
required_views
required_relations
left_navigation
center_content
right_rail_context
work_links
org_links
finance_links
software_source_links
custom_page_candidate
custom_page_reason
fallback_view_id
expected_visual_artifact
acceptance_checks
implementation_state
```

Use `implementation_state` values consistently:

```text
design-only | partial | implemented | verified
```

## Front-end shape

A page contract should describe the intended UI shape even before a custom
page exists:

```text
left: document tree / module navigation
center: document Blocks + standard Views + important tables
right rail: related Work, responsible Actors, Finance effects, source drift,
            related assets, next actions
```

For a custom page candidate, record:

```text
layout shell
primary cards / tables / boards
declared queries
declared actions
fallback standard View
expected design image path when approved
actual screenshot path when implemented
```

The page code may present this shape, but it must not own business facts.

## Minimal acceptance

Before handing off a page:

1. `harness company docs query --document <id>` returns the updated page.
2. Stable reusable facts exist as TypedRecords or Views when needed.
3. Relations connect the page to source records and cross-system objects.
4. The Store-live UI renders the page and document tree.
5. The page contract says whether custom page work is planned, implemented, or
   not needed.
