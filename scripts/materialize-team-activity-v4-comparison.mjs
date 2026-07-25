import { copyFile, mkdir, readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";

import { chromium } from "playwright";

const root = resolve(import.meta.dirname, "..");
const designRoot = join(root, "docs/design/execution-workbench-v3");
const expectedPath = join(designRoot, "expected/team-war-room/team-activity-mailboxes-group-chat-v3-1536x1024.png");
const capturedPath = join(root, ".visual-evidence/execution-workbench-v3/team-mailboxes-v4-iteration-3/team-war-room--running-needs-you--desktop-1536x1024.png");
const actualPath = join(designRoot, "implemented/team-war-room/team-activity-mailboxes-group-chat-v4-1536x1024.png");
const comparisonPath = join(designRoot, "comparisons/team-war-room/team-activity-mailboxes-group-chat-v4-1536x1024.png");
const overlayPath = join(designRoot, "overlays/team-war-room/team-activity-mailboxes-group-chat-v4-1536x1024.png");

await Promise.all([
  mkdir(dirname(actualPath), { recursive: true }),
  mkdir(dirname(comparisonPath), { recursive: true }),
  mkdir(dirname(overlayPath), { recursive: true }),
]);
await copyFile(capturedPath, actualPath);

const [expected, actual] = await Promise.all([
  readFile(expectedPath).then((value) => `data:image/png;base64,${value.toString("base64")}`),
  readFile(actualPath).then((value) => `data:image/png;base64,${value.toString("base64")}`),
]);

const browser = await chromium.launch({ headless: true });
try {
  const comparison = await browser.newPage({ viewport: { width: 3120, height: 1088 }, deviceScaleFactor: 1 });
  await comparison.setContent(`<!doctype html><style>
    * { box-sizing: border-box } body { margin: 0; background: #e9edf2; font: 600 13px system-ui; color: #354052 }
    main { display: grid; grid-template-columns: 1536px 1536px; gap: 16px; padding: 16px }
    figure { margin: 0; display: grid; gap: 8px } figcaption { height: 24px; letter-spacing: .04em; text-transform: uppercase }
    img { display: block; width: 1536px; height: 1024px; box-shadow: 0 8px 24px rgb(15 23 42 / .12) }
  </style><main><figure><figcaption>Expected · approved direction</figcaption><img src="${expected}"></figure><figure><figcaption>Actual · V4 candidate</figcaption><img src="${actual}"></figure></main>`);
  await comparison.waitForFunction(() => [...document.images].every((image) => image.complete));
  await comparison.screenshot({ path: comparisonPath, fullPage: true });
  await comparison.close();

  const overlay = await browser.newPage({ viewport: { width: 1536, height: 1024 }, deviceScaleFactor: 1 });
  await overlay.setContent(`<!doctype html><style>
    html, body { margin: 0; width: 1536px; height: 1024px; overflow: hidden; background: white }
    img { position: absolute; inset: 0; width: 1536px; height: 1024px }
    img:last-child { opacity: .5 }
  </style><img src="${expected}"><img src="${actual}">`);
  await overlay.waitForFunction(() => [...document.images].every((image) => image.complete));
  await overlay.screenshot({ path: overlayPath });
  await overlay.close();
} finally {
  await browser.close();
}

console.log(JSON.stringify({
  status: "materialized",
  expected: expectedPath,
  actual: actualPath,
  comparison: comparisonPath,
  overlay: overlayPath,
}, null, 2));
