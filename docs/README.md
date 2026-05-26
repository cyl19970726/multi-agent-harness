# Docs

| Topic | Doc |
| --- | --- |
| Product requirements | [prd.md](prd.md) |
| Architecture, modules, task flow, and package plan | [architecture.md](architecture.md) |
| Operations | [operations.md](operations.md) |
| Schemas and minimal object contracts | [schemas.md](schemas.md) |
| Design decisions | [decisions.md](decisions.md) |

Project-specific tool usage belongs in `examples/adapters/**` or in the
integrating project repository, not in the generic core docs.

## Split Rule

Keep docs merged until a file is stable above roughly 500 lines, has a clearly
different reader, or is consumed by CI/tooling.
