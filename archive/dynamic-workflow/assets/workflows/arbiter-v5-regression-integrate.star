workflow(
    "arbiter-v5-regression-integrate",
    "mapping 仲裁收尾闭环:①题库 33→100+ 扩容回归(防小样本侥幸,V4 的 LightGBM 100% 需要更宽的" +
    "评估面确认);②回归过线才允许写仓库集成代码(L1 规则+L2 LightGBM 移植为 TypeScript 模块," +
    "m2cgen 导出纯 TS 打分函数,零运行时依赖);③TS 实现必须与 python 版在全部题目上答案一致" +
    "(parity 测试)+ claude 终审 code review。串行 gate 设计:任何一关不过即停,绝不带病集成。",
    success_criterion="100+ 题回归数字 + 过线后 feat/match-arbiter 分支上有通过 parity 测试与 review 的 TS 模块",
)

phase("regression")
reg = agent(
    """题库扩容 + 分层管线回归。本机 localhost:18181 是 InfluxDB tunnel(POST /api/v3/query_sql,camelCase 加双引号)。
已有资产:/tmp/qwen-eval/cases.json(33 题)、/tmp/qwen-eval/ 下 V4 的 rules 评分器与 LightGBM 训练脚本。

1) 扩题库到 >=100 题(75 正 + 25 负):从 polymarket.match_mapping(confidence>=0.95, gammaTitle 非空,
   LIMIT 400)取真值,构造方法与原题库一致(正样本=真比赛混入 5-8 个时间相近干扰项;负样本=抽掉真比赛);
   确定性打乱;写 /tmp/qwen-eval/cases_100.json。旧 33 题的比赛照常可入新题库(它们对 LightGBM 训练集
   仍按防泄漏规则剔除)。
2) 重训 LightGBM:训练数据同 V4 方法,但剔除集合改为新题库涉及的全部 marsMatchId;报告训练对数与 5-fold AUC。
3) 跑**分层管线**(按生产形态):L1 rules-fuzzy(高置信阈值:分数>=0.85 且领先>=0.1 才接;否则下放)
   → L2 LightGBM(P>=0.9 且领先>=0.15 → 选;否则 none/弃权)。记录每题由哪层裁决。
4) 写 /tmp/qwen-eval/answers_layered_100.json(id/truth/answer/confidence/layer)+ 统计:
   precision(命中/(命中+错选))、recall(/75)、负样本拒绝(/25)、L1/L2 各自承担的题数。""",
    provider="codex",
    writable=True,
    label="regression-100",
    schema={
        "total": "int,题目总数",
        "precision": "数值或分数",
        "recall": "数值或分数",
        "reject": "负样本拒绝,如 24/25",
        "layer_split": "L1/L2 各裁决多少题",
        "auc": "重训 5-fold AUC",
        "note": "异常与错题摘要",
    },
)

if reg == None:
    verdict(False, reason="回归步骤失败")
    output("regression failed")
else:
    gate = agent(
        "回归 gate 判定。结果: " + json.encode(reg) + "\n" +
        """验收线:precision>=95% 且 负样本拒绝率>=88%(22/25)。recall 低于 90% 要在备注里写明影响但不否决。
另外检查合理性:AUC 是否 >=0.99;错题模式是否集中(集中=可修,弥散=能力问题)。
输出 proceed=true/false 和理由。""",
        provider="claude",
        label="gate",
        schema={"proceed": "bool", "reason": "判定理由"},
    )

    if gate == None or not gate["proceed"]:
        verdict(False, reason="回归未过线,不集成: " + (gate["reason"] if gate != None else "gate失败"))
        output({"regression": reg, "gate": gate if gate != None else "None"})
    else:
        phase("integrate")
        impl = agent(
            """把 L1+L2 仲裁器集成进 earning-engine 仓库(TypeScript)。
仓库:/Users/hhh0x/earning-engine-merge(直接 cd 过去操作,不要在当前 harness 仓库写业务代码)。
分支:git checkout main && git checkout -b feat/match-arbiter(若已存在则复用)。

交付物(目录 packages/data-platform/src/bridge/match-arbiter/):
1) features.ts —— 与 python 版**逐字节等价**的特征抽取:队名归一化(小写/去标点/剥后缀/academy 标记)、
   token_set_ratio、Jaro-Winkler、缩写命中、两队配对取优、日期差小时。参考 /tmp/qwen-eval/ 下的 python 实现,
   注意浮点行为一致。
2) model.ts —— 用 m2cgen 把 /tmp/qwen-eval/ 重训好的 LightGBM 导出为纯 TS 打分函数
   (pip install m2cgen;m2cgen 不支持时退而求其次:lightgbm dump_model JSON + 手写树遍历器,零依赖)。
3) arbiter.ts —— 分层入口 resolveMatch(gammaCtx, candidates):L1 规则(>=0.85 且领先>=0.1)→
   L2 模型(P>=0.9 且领先>=0.15)→ 返回 {matchedMarsId|null, confidence, layer, features} 结构;阈值做成常量可配。
4) parity 测试(vitest,放包内 __tests__):把 /tmp/qwen-eval/cases_100.json 和 python 答卷
   answers_layered_100.json 复制为测试 fixture(放 packages/data-platform/src/bridge/match-arbiter/fixtures/),
   断言 TS 仲裁器对全部题目的 answer 与 python 完全一致。
5) scripts/match-arbiter-retrain.py —— 重训脚本入仓(从 influx 拉映射→造对→训练→导出 model.ts/JSON),
   头部注释写清用法与防泄漏规则。
完成标准:pnpm 内 typecheck 通过 + parity 测试全绿 + conventional commit 提交到 feat/match-arbiter。
返回:文件清单、测试结果原文、commit hash。任何失败如实报告。""",
            provider="codex",
            writable=True,
            label="integrate-ts",
            schema={"done": "bool", "commit": "commit hash 或 none", "test_result": "parity 测试结果原文(截断)", "files": "文件清单,一行一个"},
        )

        if impl == None or not impl["done"]:
            verdict(False, reason="集成未完成")
            output({"regression": reg, "integration": impl if impl != None else "None"})
        else:
            review = agent(
                "终审 code review。仓库 /Users/hhh0x/earning-engine-merge 分支 feat/match-arbiter,commit " + str(impl["commit"]) + "\n" +
                """检查(git show/读文件):
1) parity 测试是真测试还是糊弄(fixture 是否真实、断言是否逐题);
2) features.ts 与 python 特征是否真等价(抽查 token_set_ratio/JaroWinkler 实现);
3) model.ts 是生成的树还是占位;阈值常量与回归用的一致;
4) 有没有 console.log 调试残留 / 类型 any 滥用 / 对仓库其他部分的意外改动(git diff main --stat 应只含新文件+scripts)。
输出 approve=true/false + 问题清单。""",
                provider="claude",
                label="review",
                schema={"approve": "bool", "issues": "问题清单,一行一个,没有写 none"},
            )
            ok = review != None and review["approve"]
            verdict(ok, reason=("review approved" if ok else "review 未通过或失败"))
            output({
                "regression": {"precision": reg["precision"], "recall": reg["recall"], "reject": reg["reject"], "auc": reg["auc"], "layer_split": reg["layer_split"]},
                "commit": impl["commit"],
                "files": impl["files"],
                "test_result": str(impl["test_result"])[:500],
                "review": review if review != None else "review step failed",
            })
