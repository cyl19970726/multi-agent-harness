# 文档体系

第一版文档要少而准。不要因为未来会有很多模块，就提前拆出大量空目录。

## 最小目录

```text
docs/
  README.md          # 文档入口和阅读顺序
  prd.md             # 产品需求、核心场景流、验收目标
  architecture.md    # 核心概念、模块、数据流、边界
  operations.md      # 本地运行、排障、CLI、发布和维护
  schemas.md         # 对象契约、schema、示例 artifact
  decisions.md       # 关键设计决策，ADR 可以先作为小节
```

当前已有的较细文档可以保留为草稿，但稳定入口应该收敛到上面六篇。

## 拆分原则

只有满足至少一个条件才拆文档：

- 单篇稳定超过 500 行，并且继续增长会影响定位；
- 读者群明显不同，例如 operator 和 adapter author；
- 生命周期不同，例如 schema 契约频繁变，架构说明较稳定；
- 内容有独立审查责任，例如安全运行手册；
- CI 或工具需要单独消费，例如 schema reference、CLI help snapshot。

短文优先合并。不要按“概念 / 步骤 / 示例”机械拆碎。

## 必须写清楚的内容

- PRD：为什么存在，核心场景流是什么，怎样算有用。
- Architecture：最小类型、模块边界、信任模型。
- Operations：如何运行、如何排障、如何发布。
- Schemas：Rust 类型和 JSON schema 如何对应。
- Decisions：为什么选择 Rust、message-first task、file store before DB。

## 一致性规则

CI 至少检查：

- Markdown 本地链接；
- JSON 是否可解析；
- schema/example 是否可校验；
- 文档中出现的 CLI 命令是否仍存在；
- 单篇文档超过 500 行时给出 warning。

后续可以加：

- Rust type 与 schema coverage；
- CLI `--help` snapshot；
- example adapter fixture validation；
- dashboard build。
