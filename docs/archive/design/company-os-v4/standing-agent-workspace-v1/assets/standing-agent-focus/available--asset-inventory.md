# Asset inventory · standing-agent-focus

| Asset | Role | Source strategy | Required sizes/states | Status |
| --- | --- | --- | --- | --- |
| Standing Agent portrait | Durable actor identity | deterministic `ActorAvatar` identity seed; generated portrait in Expected is directional only | 48px header, 32px compact actor rows; available ring | implemented |
| Product and record icons | Navigation, Work, permissions, tools, Docs | Lucide SVG components with semantic color tokens | 14–20px, light theme | implemented |
| Status marks | Availability and WorkItem state | CSS token + icon, never color alone | available, in progress, completed, blocked | implemented |
| Expected composition | Visual target | built-in image generation from approved Company OS art direction | 1536×1024 | durable |

No raster asset from the Expected image is shipped into the product UI. The
portrait is an identity aid, not evidence of actor state or authority.
