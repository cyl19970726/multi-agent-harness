workflow(
    "arbiter-eval-v4-nonllm",
    "mapping 仲裁的非 LLM 路线评测(operator 提问『不一定非得是 LLM』)。任务本质是 entity resolution," +
    "经典领域非 LLM 是主流。同一题库(33 题)同一验收线(precision>=95% 拒绝>=7/8)测三个变体:" +
    "① 纯规则+模糊评分器(rapidfuzz,零模型);② LightGBM 分类器(355 条已验证映射造训练对," +
    "严格排除评测题涉及的比赛防泄漏);③ Qwen3-Reranker-0.6B(任务形状契合,方向交规则)。" +
    "三者独立并行。已知基线:Qwen3-4B 多选 100/100/8-8。若非 LLM 达标,4B 降级为长尾兜底,内存压力消失。",
    success_criterion="三个非 LLM 变体的 precision/recall/拒绝率对比 4B 基线,和分层架构的最终建议",
)

phase("inference")
runs = parallel([
    {
        "label": "rules-fuzzy",
        "provider": "codex",
        "writable": True,
        "schema": {"done": "bool", "answers_path": "答卷路径", "note": "评分公式说明"},
        "prompt": """变体①:纯规则+模糊评分器(零模型)。题库 /tmp/qwen-eval/cases.json(33 题)。
写 /tmp/qwen-eval/run_rules.py(pip install rapidfuzz):
- 队名归一化:小写、去标点/空格、剥常见后缀(esports/team/gaming/club),academy/youth/学院 提取为独立布尔标记;
- 单队相似度 = max(token_set_ratio, JaroWinkler, 缩写命中(首字母串==对方/一方是另一方子序列且长度<=5));
  academy 标记不一致 → 该队相似度直接置 0;
- 两队配对取两种排列的较优(匈牙利式),场相似度 = 两队相似度均值;日期差 >24h → 置 0,<=24h 线性衰减系数;
- 每题:对全部候选打分,最高分 >= 0.85 且比第二名高 >= 0.1 → 选之;否则 none;
- 写 /tmp/qwen-eval/answers_rules.json(id/truth/answer/confidence=分数)。
输出答卷路径和评分公式要点。""",
    },
    {
        "label": "lightgbm",
        "provider": "codex",
        "writable": True,
        "schema": {"done": "bool", "answers_path": "答卷路径", "train_size": "训练对数量", "note": "特征列表与防泄漏说明"},
        "prompt": """变体②:LightGBM 对子分类器。题库 /tmp/qwen-eval/cases.json。
训练数据:本机 localhost:18181 是 InfluxDB tunnel,
curl -s -X POST http://localhost:18181/api/v3/query_sql -H 'Content-Type: application/json' -d '{"db":"polymarket","q":"SELECT DISTINCT \\"marsMatchId\\",\\"gammaTitle\\",\\"teamA\\",\\"teamB\\",\\"matchDate\\" FROM match_mapping WHERE confidence>=0.95 AND \\"gammaTitle\\" IS NOT NULL LIMIT 400","format":"json"}'
步骤(pip install lightgbm rapidfuzz scikit-learn):
1) **防泄漏**:先读题库,把题库中出现过的全部 marsMatchId(候选+真值)从训练数据剔除;
2) 正对 = (gammaTitle 解析出的两队, 该行 teamA/teamB, 日期差0);负对 = 同池内错配组合(相近日期的其他比赛,1:3 正负比);
3) 特征(对子级):两队配对相似度(token_set/JaroWinkler/缩写,两种排列取优)、单队最小相似度、日期差小时、
   academy 标记一致性、队名长度差、首字母命中;
4) 训练 LightGBM 二分类,5-fold 看 AUC;
5) 评测:对题库每题每候选算 P(same),最高概率 >=0.9 且领先第二名 >=0.15 → 选之,否则 none;
6) 写 /tmp/qwen-eval/answers_lgbm.json(id/truth/answer/confidence=概率)。
输出训练对数、AUC、特征重要性 top3。""",
    },
    {
        "label": "reranker-0.6b",
        "provider": "codex",
        "writable": True,
        "schema": {"done": "bool", "answers_path": "答卷路径", "avg_seconds": "每题均耗时", "note": "用法备注"},
        "prompt": """变体③:Qwen3-Reranker-0.6B 选场(方向不归它管)。题库 /tmp/qwen-eval/cases.json。
1) 模型: huggingface-cli download ggml-org/Qwen3-Reranker-0.6B-Q8_0-GGUF qwen3-reranker-0.6b-q8_0.gguf --local-dir /tmp/qwen-eval/model-rr
   (或 curl 直链 https://huggingface.co/ggml-org/Qwen3-Reranker-0.6B-Q8_0-GGUF/resolve/main/qwen3-reranker-0.6b-q8_0.gguf)
2) Qwen3-Reranker 用法(llama-cpp-python 或 llama.cpp 的 embedding/rerank 模式):
   它是 yes/no logit 打分:prompt 格式为 instruct + query + doc,取 "yes" token 的概率作为相关度;
   query = "Gamma 市场: <title> | <teamA> vs <teamB> | <date>";doc = 每个候选 "<teamA> vs <teamB> | <date>";
   instruction = "判断这两条记录是否同一场 CS2 比赛(队名语义等价且时间吻合)"。
   若 llama.cpp rerank 模式难走通,fallback:用 llama-cpp-python 对该 GGUF 按上述模板取 yes/no logprob。
3) 每题:候选最高分归一化后 >=0.8 且领先 >=0.1 → 选之,否则 none;
4) 写 /tmp/qwen-eval/answers_rr.json(id/truth/answer/confidence)。
若模型用法彻底走不通,如实报告 done=false 和原因,不要编造。""",
    },
])

phase("scoring")
labels = ["rules-fuzzy", "lightgbm", "reranker-0.6b"]
desc = []
for i in range(len(runs)):
    r = runs[i]
    if type(r) == "dict" and r.get("done"):
        desc.append(labels[i] + " 答卷: " + str(r["answers_path"]) + " | 备注: " + str(r.get("note"))[:300])
    else:
        desc.append(labels[i] + " 失败/未完成: " + (json.encode(r) if type(r) == "dict" else str(r)[:300]))

score = agent(
    "终判:非 LLM 路线 vs 4B 基线。题库(truth)/tmp/qwen-eval/cases.json。\n" + "\n".join(desc) + "\n" +
    """对每份存在的答卷算 precision(命中/(命中+错选),弃权不计分母)、recall(/25)、负样本拒绝(/8)。
基线:Qwen3-4B 多选 precision 100% recall 100% 拒绝 8/8(峰值内存 5.7GB);1.7B+校验 100%/28%/8-8(2.9GB)。
验收线 precision>=95% 且拒绝>=7/8。
给最终架构建议:分层(词典→哪个非LLM层→LLM兜底?)还是 4B 单干?考虑:内存、可审计性、
维护成本、以及『LightGBM 可随词典增长持续重训』的飞轮属性。""",
    provider="claude",
    label="final-scoring",
    schema={
        "table": "对比表含基线,一行一个: 名称 precision recall reject 内存 decision",
        "architecture": "最终分层架构一句话",
        "recommendation": "完整建议",
    },
)
if score == None:
    verdict(False, reason="判分失败,答卷已留存")
    output({"raw": desc})
else:
    verdict(True, reason="对比完成")
    output({
        "comparison": score["table"],
        "architecture": score["architecture"],
        "recommendation": score["recommendation"],
    })
