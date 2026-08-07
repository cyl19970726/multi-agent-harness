# Company OS Live PRD V1 visual contract

This package defines the visual direction for one living product report that
explains Company OS without forcing a reader to reconstruct the product from
many separate design reviews and architecture documents.

```text
Product UI Expected       -> how the core pages cooperate
Architecture Explainers   -> why the systems, organization, and execution boundary exist
Usage Example             -> how one trademark operation uses the product end to end
Implementation Truth      -> what is Actual, partial, Expected, or planned today
```

The first P0 artifact is an exact `1536×1024` Expected design for the report's
first viewport. Human Owner approved its direction on 2026-07-22:

- [Expected desktop design](expected/live-prd--product-map--desktop-1536x1024.png)
- [Expected Product Journey Explorer](expected/live-prd--product-journey--desktop-1536x1024.png)
- [Expected OS architecture](expected/live-prd--os-architecture--desktop-1536x1024.png)
- [Content and asset map](content-map.md)
- [Generation prompt](prompts/live-prd--product-map.md)
- [Page matrix](page-matrix.md)

The report embeds the three approved Product Map, Product Journey, and OS
Architecture images directly. HTML supplies navigation, accessible transcripts,
business-line exploration, and truth-labelled evidence; it does not redraw the
approved explainers. Only real product pages require browser-rendered Actual
screenshots, and the report never presents an Expected image as Actual browser
evidence.

The Product Journey Explorer is intentionally different from an execution
trace. Its stages are navigable Company OS pages; its graph documents routes,
canonical refs and Organization authority. The architecture page stops at the
`ExecutionRef` boundary and does not expand execution internals in V1.
