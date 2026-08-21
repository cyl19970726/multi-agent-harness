workflow(
    "qwen-arbiter-eval-v2",
    "0.6B 多选题评测 FAIL(precision 32% vs 95% 线)后的第二轮:分离两个失败假设并行重测。" +
    "假设A『任务形态不利』:同一 0.6B 改两两二判(对每个候选独立问 yes/no,小模型舒适区);" +
    "假设B『模型太小』:升级 Qwen3-1.7B 仍用多选形态。复用同一题库 /tmp/qwen-eval/cases.json " +
    "和同一验收线(precision>=95%,负样本拒绝>=7/8),两路并行因互相独立,最后 claude 对比判分。",
    success_criterion="两个变体各自的 precision/recall/拒绝率,和『哪条路能用/都不能用』的对比判决",
)

phase("inference")
runs = parallel([
    {
        "label": "pairwise-0.6b",
        "provider": "codex",
        "writable": True,
        "schema": {"done": "bool", "answers_path": "答卷路径", "avg_seconds": "每题(含全部二判)平均耗时", "note": "备注"},
        "prompt": """评测变体A:Qwen3-0.6B + 两两二判。题库 /tmp/qwen-eval/cases.json(33 题),模型已在 /tmp/qwen-eval/model/(Qwen3-0.6B-Q4_K_M.gguf,已下载;若缺再下)。
写 /tmp/qwen-eval/run_pairwise.py:
- llama-cpp-python,n_ctx=1024,temperature=0,user 消息尾加 " /no_think";
- 对每题的每个候选独立问一次:给出 Gamma 市场(title/teamA/teamB/date)和这一个 Mars 候选(teamA/teamB/date),
  问「这两条记录指的是同一场比赛吗?队名需语义等价(缩写/学院队/改名要当心),时间需吻合(同一天±12小时)」,
  JSON grammar 强制输出两键:same(true/false)、confidence(0-1);
- 汇总规则:恰好一个 same=true → 选它;没有 true → none;多个 true → 取 confidence 最高者,若最高两个差 <0.1 → none(歧义弃权);
- 写 /tmp/qwen-eval/answers_pairwise.json,数组,每项 id/truth/answer/confidence 四键。
跑完报告总耗时与每题均值。失败写原文,不要编造。""",
    },
    {
        "label": "multichoice-1.7b",
        "provider": "codex",
        "writable": True,
        "schema": {"done": "bool", "answers_path": "答卷路径", "avg_seconds": "每题平均耗时", "note": "备注(含模型内存占用)"},
        "prompt": """评测变体B:Qwen3-1.7B + 多选形态(与上轮 0.6B 完全同形,只换模型)。题库 /tmp/qwen-eval/cases.json。
1) 下载: huggingface-cli download unsloth/Qwen3-1.7B-GGUF Qwen3-1.7B-Q4_K_M.gguf --local-dir /tmp/qwen-eval/model17
   (或 curl 直链 https://huggingface.co/unsloth/Qwen3-1.7B-GGUF/resolve/main/Qwen3-1.7B-Q4_K_M.gguf)
2) 复用上轮脚本逻辑(/tmp/qwen-eval/run.py 若在,改模型路径即可):n_ctx=2048,temperature=0," /no_think",
   每题给 Gamma 市场信息 + 全部候选列表,JSON grammar 强制两键 answer(mars_id 或 none)/confidence;
   不确定宁可 none。
3) 写 /tmp/qwen-eval/answers_17b.json,数组,每项 id/truth/answer/confidence。
报告每题耗时均值和模型加载后内存占用。失败写原文,不要编造。""",
    },
])

phase("scoring")
pa = runs[0] if type(runs[0]) == "dict" else None
mb = runs[1] if type(runs[1]) == "dict" else None
desc = []
if pa != None and pa["done"]:
    desc.append("变体A(0.6B 两两二判)答卷: " + str(pa["answers_path"]) + ",每题均耗时 " + str(pa["avg_seconds"]))
if mb != None and mb["done"]:
    desc.append("变体B(1.7B 多选)答卷: " + str(mb["answers_path"]) + ",每题均耗时 " + str(mb["avg_seconds"]))

if len(desc) == 0:
    verdict(False, reason="两个变体推理都失败")
    output({"a": pa, "b": mb})
else:
    score = agent(
        "对比判分两个 mapping 仲裁变体。题库(truth)在 /tmp/qwen-eval/cases.json。\n" +
        "\n".join(desc) + "\n" +
        """对每份答卷计算:正样本命中/弃权/错选,负样本正确拒绝/误选,
precision = 命中/(命中+全部错选)[弃权不计分母]、recall、负样本拒绝率。
验收线:precision>=95% 且 拒绝>=7/8。
另外参考上轮基线:0.6B 多选 precision=32%, recall=32%, 拒绝 6/8。
输出对比表 + 每个变体 PASS/BORDERLINE/FAIL + 最终建议(用哪条路/都不用而改用什么)。""",
        provider="claude",
        label="scoring",
        schema={
            "table": "对比表,一行一个变体: 名称 precision recall reject decision",
            "best": "最优变体名或 none",
            "recommendation": "最终建议",
        },
    )
    if score == None:
        verdict(False, reason="判分失败,答卷已留存")
        output({"a": pa, "b": mb})
    else:
        verdict(score["best"] != "none", reason="best=" + str(score["best"]))
        output({
            "comparison": score["table"],
            "best": score["best"],
            "recommendation": score["recommendation"],
            "a_perf": pa, "b_perf": mb,
        })
