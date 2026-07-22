# Page matrix

| Priority | Page | State | Viewport | Baseline | Expected | Implemented | Review |
| --- | --- | --- | --- | --- | --- | --- | --- |
| P0 | Standing Agent focus | available + assigned work | 1536×1024 | V1 profile page | approved generated design | iteration 2 browser capture | pass with intentional deviations |
| P1 | Standing Agent focus | context open | 900×1180 | — | responsive transform declared in spec | iteration 2 browser capture | pass: right context becomes sheet |
| P1 | Standing Agent focus | context open | 390×844 | — | responsive transform declared in spec | iteration 2 browser capture | pass: bottom sheet retains page identity |

The P0 route is the durable fidelity contract. Tablet and mobile are browser
evidence of the declared responsive transformation; no synthetic Expected image
is treated as an approval contract for those viewports.
