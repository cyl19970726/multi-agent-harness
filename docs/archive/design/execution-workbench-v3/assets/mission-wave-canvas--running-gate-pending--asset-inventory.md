# Asset inventory · Mission Wave canvas fidelity v2

| Asset | Role | Source strategy | Required sizes/states | Status |
| --- | --- | --- | --- | --- |
| Company mark | Product identity | Reuse current code-native mark | 34×34, default | available |
| Navigation and context icons | Wayfinding | Reuse current Lucide-compatible SVG components | 14–18px, default/active | available |
| Agent portraits | Team identity | Reuse approved shared portrait sprites; do not use generated screenshot pixels | 34×34 and 38×38, online/blocked dots | available |
| Wave journey line and nodes | Ordered Mission structure | CSS line plus semantic node primitives | accepted/running/planned | implement in code |
| Pressure glyph | QA decision pressure | Code-native SVG inside coral tint | 16px, pending | available/refine |
| Gate progress | Readiness, not lifecycle progress | CSS progress primitive | 0–3 criteria, pending | available |

No new raster illustration is required. Portraits are the only raster identity assets; operational semantics remain crisp, themeable SVG/CSS.
