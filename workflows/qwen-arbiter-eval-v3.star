workflow(
    "qwen-arbiter-eval-v3",
    "评测阶梯第三轮。已知:0.6B 多选 32%,0.6B 二判全弃权,1.7B 多选 78%(负样本拒绝 8/8)。" +
    "趋势指向模型大小是真变量。本轮并行测两条收口路径:A『再上一档』Qwen3-4B 多选(同形换模型,看 " +
    "趋势线是否在 4B 过 95% 线);B『1.7B+确定性二次校验』(上轮判分 agent 的建议:对 1.7B 的每个正选," +
    "用队名双向 token 匹配+日期容差做确定性复核,不过则降级 none——零模型成本能否把 78% 推过 95%)。" +
    "同题库同验收线,claude 终判并给出最终选型(含资源/成本对比)。",
    success_criterion="得到最终选型判决:4B / 1.7B+校验 / 都不行换 API,附 precision/recall/资源数据",
)

phase("inference")
runs = parallel([
    {
        "label": "multichoice-4b",
        "provider": "codex",
        "writable": True,
        "schema": {"done": "bool", "answers_path": "答卷路径", "avg_seconds": "每题均耗时", "note": "内存占用等备注"},
        "prompt": """评测:Qwen3-4B 多选(与 1.7B 轮完全同形,只换模型)。题库 /tmp/qwen-eval/cases.json(33 题)。
1) 下载: huggingface-cli download unsloth/Qwen3-4B-GGUF Qwen3-4B-Q4_K_M.gguf --local-dir /tmp/qwen-eval/model4b
   (或 curl 直链 https://huggingface.co/unsloth/Qwen3-4B-GGUF/resolve/main/Qwen3-4B-Q4_K_M.gguf,约 2.5GB)
2) 复用 /tmp/qwen-eval/ 下已有的多选评测脚本(改模型路径):llama-cpp-python,n_ctx=2048,temperature=0,
   user 消息尾加 " /no_think";每题给 Gamma 市场信息+全部候选,JSON grammar 强制两键 answer(mars_id 或 none)/confidence;
   不确定宁可 none。
3) 写 /tmp/qwen-eval/answers_4b.json(数组,每项 id/truth/answer/confidence)。
报告每题耗时均值与峰值内存。失败写原文,不要编造。""",
    },
    {
        "label": "17b-plus-check",
        "provider": "codex",
        "writable": True,
        "schema": {"done": "bool", "answers_path": "答卷路径", "demoted": "int,被校验降级为 none 的数量", "note": "校验规则说明"},
        "prompt": """评测:1.7B 答卷 + 确定性二次校验(零模型成本)。
输入:上轮 1.7B 答卷 /tmp/qwen-eval/answers_17b.json + 题库 /tmp/qwen-eval/cases.json。
写 /tmp/qwen-eval/check.py 实现确定性复核,对每个非 none 的 answer:
1) 从题库找到该候选的 teamA/teamB 和 Gamma 的 teamA/teamB;
2) 队名双向匹配:normalize(小写、去空格/标点/常见后缀如 esports/team/gaming/academy→标准化标记),
   要求 Gamma 两队各自能与候选两队一一对应(直接相等/一方是另一方前缀或缩写/编辑距离<=2);
   注意 academy/学院队是不同队:主队名相同但一边带 academy 标记另一边不带 → 判不匹配;
3) 日期容差:|gamma.date - candidate.date| <= 24h(解析失败按不匹配);
4) 任一不过 → 把该题 answer 降级为 none(记录降级原因)。
输出新答卷 /tmp/qwen-eval/answers_17b_checked.json(同格式)+ 报告降级了几题、各自原因。
不要重跑模型,纯后处理。""",
    },
])

phase("scoring")
a = runs[0] if type(runs[0]) == "dict" else None
b = runs[1] if type(runs[1]) == "dict" else None
desc = []
if a != None and a["done"]:
    desc.append("变体A(4B 多选)答卷: " + str(a["answers_path"]) + ",每题 " + str(a["avg_seconds"]) + "s,备注: " + str(a["note"])[:200])
if b != None and b["done"]:
    desc.append("变体B(1.7B+确定性校验)答卷: " + str(b["answers_path"]) + ",降级 " + str(b["demoted"]) + " 题")

if len(desc) == 0:
    verdict(False, reason="两路都失败")
    output({"a": a, "b": b})
else:
    score = agent(
        "终轮判分,给最终选型。题库(truth)在 /tmp/qwen-eval/cases.json。\n" + "\n".join(desc) + "\n" +
        """历史基线:0.6B多选 precision 32% / recall 32% / 拒绝 6,8;0.6B二判 全弃权;1.7B多选 77.8% / 28% / 8,8。
对每份新答卷算 precision(命中/(命中+错选),弃权不计分母)、recall(命中/25)、负样本拒绝(/8)。
验收线 precision>=95% 且拒绝>=7/8。recall 低只影响覆盖(弃权进 review 队列),不一票否决,但要在建议里权衡。
给最终选型判决,候选:A) 4B 上线;B) 1.7B+校验 上线;C) A+B 组合;D) 都不行,改 API 大模型或检索+判官两段式。
附:资源对比(4B 约 3-4.5GB RSS vs 1.7B 约 2.5GB;GCE e2-standard-4 共 16GB、influx 已用 7.5GB)和落地注意。""",
        provider="claude",
        label="final-scoring",
        schema={
            "table": "全部变体对比表(含历史轮),一行一个: 名称 precision recall reject decision",
            "final_choice": "A|B|C|D",
            "recommendation": "最终建议与落地注意",
        },
    )
    if score == None:
        verdict(False, reason="判分失败,答卷已留存")
        output({"a": a, "b": b})
    else:
        verdict(score["final_choice"] != "D", reason="final_choice=" + str(score["final_choice"]))
        output({
            "comparison": score["table"],
            "final_choice": score["final_choice"],
            "recommendation": score["recommendation"],
            "a_perf": a, "b_perf": b,
        })
