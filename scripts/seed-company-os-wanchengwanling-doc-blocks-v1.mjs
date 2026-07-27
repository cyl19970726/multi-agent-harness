#!/usr/bin/env node

/**
 * Add the first human-readable content Blocks to Wanchengwanling Company OS
 * Documents through the governed Docs CLI.
 *
 * This is intentionally a repair/enrichment script over the existing native
 * Docs substrate. It does not create Work, Approval, Finance, Organization, or
 * execution records.
 */

import { execFileSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const harness = join(repoRoot, "target", "debug", "harness");
const token = process.env.HARNESS_COMPANY_OS_TOKEN || "wanchengwanling-docs-v0-token";
const actor = process.env.WCW_DOCS_ACTOR || "agent-wcw-docs-governance";

function argument(name, fallback = "") {
  const index = process.argv.indexOf(name);
  return index === -1 ? fallback : process.argv[index + 1];
}

const project = argument("--project", process.env.HARNESS_PROJECT || "");
if (!project) {
  throw new Error("Pass --project <id|path> or set HARNESS_PROJECT to the Wanchengwanling Company OS project.");
}

const docs = [
  {
    document: "document-wcw-project-home",
    definition: "page-wcw-project-home",
    kind: "heading",
    text: "项目一句话",
  },
  {
    document: "document-wcw-project-home",
    definition: "page-wcw-project-home",
    text: "万城万灵是一个 AR 文旅商业项目：用手环身份、AR 景点打卡、实体文创、商家合作和内容传播，把古城游玩变成可复制的数字文旅体验。",
  },
  {
    document: "document-wcw-project-home",
    definition: "page-wcw-project-home",
    kind: "heading",
    text: "MVP 用户闭环",
  },
  {
    document: "document-wcw-project-home",
    definition: "page-wcw-project-home",
    text: "MVP 主链路：购买实体或虚拟手环 → 12 个点位路线打卡 → 满 8 点兑换 AR 冰箱贴 → 满 12 点参与抽奖 → 到合作商家消费、兑换或享受权益 → 内容分享带来增长。",
  },
  {
    document: "document-wcw-project-home",
    definition: "page-wcw-project-home",
    kind: "heading",
    text: "Company OS 模块地图",
  },
  {
    document: "document-wcw-project-home",
    definition: "page-wcw-project-home",
    text: "00 Project Home 是商业项目总入口；01 Business Model 定义卖什么、钱怎么分、商家为什么加入和复制模型；02 Bracelet & Product 管实体/虚拟手环与产品权益；03 Route & AR Experience 管 12 点位、8/12 规则和 AR 体验；04 Merchant Network 管寄卖、兑换、权益和采购商家；05 Rewards, Procurement & Inventory 管冰箱贴、拍立得、小吃券、采购、物流和库存；06 Content Growth 管自媒体内容增长；07 Creator Outreach 管博主合作；08 Launch Readiness 管上线 gate；09 IP & Product Design 管 IP、手环、冰箱贴和 AR 素材；10 Software Product Sources 管 GitHub dev PRD 映射。",
  },
  {
    document: "document-wcw-project-home",
    definition: "page-wcw-project-home",
    kind: "heading",
    text: "默认页面与 custom page",
  },
  {
    document: "document-wcw-project-home",
    definition: "page-wcw-project-home",
    text: "默认 Document 页面负责展示本文档 Blocks、子页面、模板、关系和低风险 governed Block 操作；默认 Module 页面负责展示该模块的 TypedRecords、Views、Relations 和标准 Action 入口；Command Center 是 Wanchengwanling 的代码声明 custom page，只做跨模块运营呈现，所有事实仍来自 Docs / Work / Org / Finance 原生 Store。",
  },
  {
    document: "document-wcw-project-home",
    definition: "page-wcw-project-home",
    kind: "heading",
    text: "当前最重要的下一步",
  },
  {
    document: "document-wcw-project-home",
    definition: "page-wcw-project-home",
    text: "把商家清单、奖品采购、AR 点位验收、自媒体排期、博主合作、上线 readiness 和软件 PRD 差异都转成源文档明确、负责人明确、结果回写明确的 WorkItems；涉及花钱的采购和奖品支出必须进入 Finance Commitment / Payment 路径，不从文档文字直接推断付款或授权。",
  },
  {
    document: "document-wcw-business-model",
    definition: "page-wcw-business-model",
    kind: "heading",
    text: "卖什么",
  },
  {
    document: "document-wcw-business-model",
    definition: "page-wcw-business-model",
    text: "商业模式核心：实体 NFC 手环 30 元，虚拟手环 20 元；实体手环采用商家寄卖，商家分 10 元，公司留 20 元。",
  },
  {
    document: "document-wcw-business-model",
    definition: "page-wcw-business-model",
    text: "实体手环出现在古城合作店铺和小程序店铺列表里，承担线下购买、NFC 身份触发、路线参与和实体纪念属性；虚拟手环降低购买门槛，主要承载小程序内身份、AR 打卡资格和权益领取。",
  },
  {
    document: "document-wcw-business-model",
    definition: "page-wcw-business-model",
    kind: "heading",
    text: "商家为什么加入",
  },
  {
    document: "document-wcw-business-model",
    definition: "page-wcw-business-model",
    text: "寄卖与网点商家获得每个实体手环 10 元分成，并通过兑换冰箱贴、兑换小吃券或承接手环权益获得二次消费；权益合作商家获得小程序曝光、路线导流和可被内容传播引用的线下场景。",
  },
  {
    document: "document-wcw-business-model",
    definition: "page-wcw-business-model",
    kind: "heading",
    text: "成本、奖品与财务边界",
  },
  {
    document: "document-wcw-business-model",
    definition: "page-wcw-business-model",
    text: "主要成本包括手环制作、AR 内容制作、冰箱贴/后续手办、2 台拍立得、本地小吃券采购、物流、包装、内容投放和商家运营。采购、付款、预算占用和报销必须落到 Finance 记录；本页只解释商业逻辑，不直接授权花钱。",
  },
  {
    document: "document-wcw-business-model",
    definition: "page-wcw-business-model",
    kind: "heading",
    text: "复制到新城市 / 景区 / 商圈",
  },
  {
    document: "document-wcw-business-model",
    definition: "page-wcw-business-model",
    text: "复制模型：小程序能力模块化，换城市、景区或商圈时主要替换点位配置、AR 资产、商家网络、奖品库存、内容计划和上线清单。",
  },
  {
    document: "document-wcw-business-model",
    definition: "page-wcw-business-model",
    text: "复制不是复制页面文案，而是复制一套 Company OS 模块结构：路线与 AR 配置、商家网络、奖品采购库存、内容增长、博主合作、上线 gate、软件 PRD 映射、Finance 控制和 WorkItem 交付路径。",
  },
  {
    document: "document-wcw-business-model",
    definition: "page-wcw-business-model",
    kind: "heading",
    text: "MVP 验证指标",
  },
  {
    document: "document-wcw-business-model",
    definition: "page-wcw-business-model",
    text: "MVP 优先看：实体/虚拟手环销量、实体寄卖店铺转化、8 点位冰箱贴兑换率、12 点位抽奖完成率、奖品核销率、商家二次消费反馈、内容播放/点赞/分享、博主合作转化、单用户获客成本、奖品和物流成本、合作商家续约意愿。",
  },
  {
    document: "document-wcw-business-model",
    definition: "page-wcw-business-model",
    kind: "heading",
    text: "本页边界",
  },
  {
    document: "document-wcw-business-model",
    definition: "page-wcw-business-model",
    text: "01 Business Model 不存具体商家沟通流水、不存采购付款凭证、不替代任务看板。具体执行进入 WorkItem；具体钱进入 Finance；具体商家进入 Merchant Network；具体资产进入 IP & Product Design；软件需求变化由 Software Product Sources 观察后再回到商业判断。",
  },
  {
    document: "document-wcw-bracelet-product",
    definition: "page-wcw-bracelet-product",
    text: "产品售卖：实体 NFC 手环用于线下寄卖和身份触发，虚拟手环用于小程序内低门槛购买；两者都承载 AR 打卡、奖励资格和商家权益。",
  },
  {
    document: "document-wcw-route-ar-experience",
    definition: "page-wcw-route-ar-experience",
    text: "路线体验：MVP 规划 12 个古城点位。用户满 8 个景点可兑换 AR 冰箱贴，满 12 个景点可参与抽奖。",
  },
  {
    document: "document-wcw-merchant-network",
    definition: "page-wcw-merchant-network",
    text: "商家网络包含多种角色：手环寄卖点、冰箱贴寄售或兑换点、奖品采购来源、奖品兑换点、手环权益合作商家，以及小程序古城店铺展示商家。",
  },
  {
    document: "document-wcw-rewards-procurement-inventory",
    definition: "page-wcw-rewards-procurement-inventory",
    text: "奖品与库存：AR 冰箱贴是 8 点位完成奖励；12 点位抽奖包含 2 台拍立得和本地小吃券。采购、物流、库存、兑换点和财务承诺必须保持关联。",
  },
  {
    document: "document-wcw-content-growth",
    definition: "page-wcw-content-growth",
    text: "内容增长：依靠高品质 AR 动画、古城路线体验、实体文创和奖品机制制造可分享素材，并用播放量、点赞、转化和商家反馈调整下一阶段计划。",
  },
  {
    document: "document-wcw-creator-outreach",
    definition: "page-wcw-creator-outreach",
    text: "博主合作：维护本地探店、文旅、亲子、摄影等创作者线索，记录沟通、报价、交付物、发布时间和效果指标。",
  },
  {
    document: "document-wcw-launch-readiness",
    definition: "page-wcw-launch-readiness",
    text: "上线准备：软件 PRD/实现、AR 点位验收、手环库存、冰箱贴与抽奖奖品、商家列表、内容计划、财务授权和现场运营都需要进入同一个 readiness gate。",
  },
  {
    document: "document-wcw-ip-product-design",
    definition: "page-wcw-ip-product-design",
    text: "IP 与产品设计：统一管理主 IP、手环视觉、冰箱贴设计、AR 触发图、包装、说明卡、制造规格和素材使用场景。",
  },
  {
    document: "document-wcw-software-product-sources",
    definition: "page-wcw-software-product-sources",
    text: "软件产品源：GitHub dev 分支 PRD、架构、设计与交付文档通过 source sync 作为外部软件事实被观察；商业事实仍以 Company OS Docs 为准。",
  },
];

function run(args) {
  return execFileSync(harness, ["--project", project, ...args], {
    cwd: repoRoot,
    env: { ...process.env, HARNESS_COMPANY_OS_TOKEN: token },
    encoding: "utf8",
  });
}

function query(document) {
  return JSON.parse(run(["company", "docs", "query", "--document", document]));
}

function hasText(documentQuery, text) {
  return (documentQuery.blocks ?? []).some((block) => block?.content?.text === text);
}

let appended = 0;
let skipped = 0;

for (const entry of docs) {
  const current = query(entry.document);
  if (hasText(current, entry.text)) {
    skipped += 1;
    continue;
  }
  run([
    "company", "docs", "block", "append",
    "--definition", entry.definition,
    "--document", entry.document,
    ...(entry.kind ? ["--kind", entry.kind] : []),
    "--text", entry.text,
    "--actor", actor,
  ]);
  appended += 1;
}

console.log(JSON.stringify({
  ok: true,
  project,
  actor,
  appended,
  skipped,
  documents_touched: [...new Set(docs.map((entry) => entry.document))],
  side_effect_boundary: "Docs-only block.append + document.append",
}, null, 2));
