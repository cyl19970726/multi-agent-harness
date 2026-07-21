import { mkdir, readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";

import { chromium } from "playwright";

const root = resolve(import.meta.dirname, "..");
const designRoot = join(root, "docs/design/execution-workbench-v3");

const cases = [
  {
    name: "running-needs-you--desktop-1440x1000",
    width: 1440,
    height: 1000,
  },
  {
    name: "running-needs-you--tablet-context-open-900x1180",
    width: 900,
    height: 1180,
  },
  {
    name: "running-needs-you--mobile-context-open-390x844",
    width: 390,
    height: 844,
  },
];

const browser = await chromium.launch({ headless: true });
try {
  for (const item of cases) {
    const expected = `data:image/png;base64,${(await readFile(join(designRoot, "expected/member-run-focus", `${item.name}.png`))).toString("base64")}`;
    const actual = `data:image/png;base64,${(await readFile(join(designRoot, "implemented/member-run-focus", `${item.name}.png`))).toString("base64")}`;
    const comparison = join(designRoot, "comparisons/member-run-focus", `${item.name}.png`);
    const overlay = join(designRoot, "overlays/member-run-focus", `${item.name}.png`);
    await mkdir(dirname(comparison), { recursive: true });
    await mkdir(dirname(overlay), { recursive: true });

    const comparisonPage = await browser.newPage({
      viewport: { width: item.width * 2 + 48, height: item.height + 64 },
      deviceScaleFactor: 1,
    });
    await comparisonPage.setContent(`<!doctype html><style>
      * { box-sizing: border-box } body { margin: 0; background: #e9edf2; font: 600 13px system-ui; color: #354052 }
      main { display: grid; grid-template-columns: ${item.width}px ${item.width}px; gap: 16px; padding: 16px }
      figure { margin: 0; display: grid; gap: 8px } figcaption { height: 24px; letter-spacing: .04em; text-transform: uppercase }
      img { display: block; width: ${item.width}px; height: ${item.height}px; object-fit: fill; box-shadow: 0 8px 24px rgb(15 23 42 / .12) }
    </style><main><figure><figcaption>Expected · V3 candidate</figcaption><img src="${expected}"></figure><figure><figcaption>Actual · browser implementation</figcaption><img src="${actual}"></figure></main>`);
    await comparisonPage.waitForFunction(() => [...document.images].every((image) => image.complete));
    await comparisonPage.screenshot({ path: comparison, fullPage: true });
    await comparisonPage.close();

    const overlayPage = await browser.newPage({
      viewport: { width: item.width, height: item.height },
      deviceScaleFactor: 1,
    });
    await overlayPage.setContent(`<!doctype html><style>
      html, body { margin: 0; width: ${item.width}px; height: ${item.height}px; overflow: hidden; background: white }
      img { position: absolute; inset: 0; width: ${item.width}px; height: ${item.height}px; object-fit: fill }
      img:last-child { opacity: .5 }
    </style><img src="${expected}"><img src="${actual}">`);
    await overlayPage.waitForFunction(() => [...document.images].every((image) => image.complete));
    await overlayPage.screenshot({ path: overlay });
    await overlayPage.close();
  }
} finally {
  await browser.close();
}

console.log(JSON.stringify({ status: "materialized", cases: cases.map(({ name }) => name) }, null, 2));
