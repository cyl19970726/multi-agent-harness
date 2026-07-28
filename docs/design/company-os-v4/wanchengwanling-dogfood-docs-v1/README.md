# Wanchengwanling Dogfood Docs v1

```text
status: Store-live frontend evidence
project_id: new-day-wanchengwanling
store: /Users/hhh0x/.harness/projects/new-day-wanchengwanling
scope: Wanchengwanling 02/03 Docs foundation frontend verification
captured_at: 2026-07-28
```

This artifact records browser evidence for the Wanchengwanling commercial
dogfood Docs foundation. It is not the source of truth. The source of truth is
the Company OS Store, queried through `harness company docs ...`.

## Actual Store-live screenshots

| Page | Route | Screenshot | Frontend assertions |
| --- | --- | --- | --- |
| 02 Bracelet & Product | `/?api=.&project=new-day-wanchengwanling&surface=docs&document=document-wcw-bracelet-product` | [desktop 1536×1024](actual/docs-02-bracelet-product--desktop-1536x1024.png) | `02 Bracelet & Product`, `实体 NFC 手环`, `商家 ¥10`, `Agent 可操作的结构化事实` |
| 03 Route & AR Experience | `/?api=.&project=new-day-wanchengwanling&surface=docs&document=document-wcw-route-ar-experience` | [desktop 1536×1024](actual/docs-03-route-ar-experience--desktop-1536x1024.png) | `03 Route & AR Experience`, `十二印章路线`, `叩城印`, `12 点` |

## CLI verification

```bash
target/debug/harness --project /Users/hhh0x/new-day/wanchengwanling \
  company docs query --document document-wcw-bracelet-product --json

target/debug/harness --project /Users/hhh0x/new-day/wanchengwanling \
  company docs query --document document-wcw-route-ar-experience --json
```

Accepted results:

| Page | Records | Views | Relations | Health |
| --- | ---: | ---: | ---: | ---: |
| `document-wcw-bracelet-product` | 11 | 4 | 12 | 0 |
| `document-wcw-route-ar-experience` | 19 | 4 | 19 | 0 |

