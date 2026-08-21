workflow(
    "qwen-arbiter-eval",
    "用真实生产数据评测 Qwen3-0.6B 能否胜任 CS2 mapping 仲裁(在写任何集成代码之前先证伪/证实)。" +
    "三步串行:① 从 InfluxDB 已验证的 0.95 映射构造 golden 题库(正样本=真比赛在候选列表里要选对," +
    "负样本=把真比赛从候选里抽掉要敢拒绝);② 本机下载 Qwen3-0.6B GGUF,用 llama.cpp + JSON grammar " +
    "+ temperature 0 逐题作答;③ 程序化判分 + claude 出最终判决。串行因 ② 依赖 ① 的题库,③ 依赖 ② 的答卷。" +
    "验收标准事先声明:precision >= 95%(选错映射是毒药),负样本拒绝率 >= 7/8,可弃权(弃权进 review 队列不算错)。",
    success_criterion="得到 Qwen3-0.6B 在 >=30 道真实题上的 precision/recall/拒绝率,和『能用/不能用/换更大模型』的明确判决",
)

phase("dataset")
ds = agent(
    """构造 CS2 mapping 仲裁的 golden 评测集。
快捷路径:若 /tmp/qwen-eval/cases.json 已存在且包含 >=30 题(用 python 验证 JSON 可解析、含 positive/negative 两类),
直接复用并报告其统计,跳过下面所有构造步骤。
否则:本机 localhost:18181 是 InfluxDB 3 tunnel:
curl -s -X POST http://localhost:18181/api/v3/query_sql -H 'Content-Type: application/json' -d '{"db":"polymarket","q":"<SQL>","format":"json"}'
(camelCase 标识符要双引号)

步骤:
1) 取已验证映射做真值:
   SELECT DISTINCT "marsMatchId","gammaTitle","teamA","teamB","matchDate" FROM match_mapping WHERE confidence>=0.95 AND "gammaTitle" IS NOT NULL LIMIT 120
2) 构造 25 道正样本题:每题取一条真值,候选列表 = 真比赛 + 从其他映射行里挑 5-8 个**时间相近**(matchDate 差 <48h 优先)的比赛作干扰项(用它们的 marsMatchId/teamA/teamB/matchDate)。把候选顺序打乱(用 marsMatchId 排序后取模换位等确定性方法,不要用随机)。
3) 构造 8 道负样本题:同上,但把真比赛从候选列表里**抽掉**,正确答案是 none。
4) 每题格式:
   {"id":"q1","type":"positive|negative","gamma":{"title":...,"teamA":...,"teamB":...,"date":...},
    "candidates":[{"mars_id":...,"teamA":...,"teamB":...,"date":...},...],
    "truth":"<marsMatchId 或 none>"}
写入 /tmp/qwen-eval/cases.json(数组)。输出题目总数和正负样本数。""",
    provider="codex",
    writable=True,
    label="dataset",
    schema={"total": "int,题目总数", "path": "题库文件路径", "note": "构造说明"},
)

if ds == None:
    verdict(False, reason="题库构造失败")
    output("dataset step failed")
else:
    phase("inference")
    inf = agent(
        "在本机(macOS,Apple Silicon)用 Qwen3-0.6B 跑 mapping 仲裁评测。题库在 " + str(ds["path"]) + "(共 " + str(ds["total"]) + " 题)。\n" +
        """
步骤:
1) 准备模型:优先用 python 的 llama-cpp-python(pip install llama-cpp-python,若已装跳过);
   模型下载: huggingface-cli download unsloth/Qwen3-0.6B-GGUF Qwen3-0.6B-Q4_K_M.gguf --local-dir /tmp/qwen-eval/model
   (若 huggingface-cli 不在,pip install -U huggingface_hub 或用 curl 直链 https://huggingface.co/unsloth/Qwen3-0.6B-GGUF/resolve/main/Qwen3-0.6B-Q4_K_M.gguf)
2) 写评测脚本 /tmp/qwen-eval/run.py:
   - 加载模型(n_ctx=2048, temperature=0);
   - Qwen3 默认开 thinking,评测要关:在 user 消息末尾加 " /no_think";
   - 每题 prompt:系统角色=电竞比赛匹配仲裁员;给出 Gamma 市场信息(title/teamA/teamB/date)和候选 Mars 比赛列表;
     要求:若某候选与 Gamma 是同一场比赛(队名语义等价+时间吻合)输出其 mars_id,否则输出 none;不确定时宁可 none;
   - 用 llama-cpp-python 的 response_format json_schema 或 grammar 强制输出 JSON(两个键:answer = mars_id 或 none,confidence = 0 到 1);
   - 逐题写结果到 /tmp/qwen-eval/answers.json,数组,每项含 id/truth/answer/confidence 四键。
3) 跑完输出:总题数、每题耗时均值、内存峰值(粗略即可,如 /usr/bin/time -l 或 psutil)。
注意:任何一步失败都把错误原文写进输出,不要编造结果。""",
        provider="codex",
        writable=True,
        label="inference",
        schema={"done": "bool,是否全部题目跑完", "avg_seconds": "每题平均耗时", "answers_path": "答卷路径", "note": "环境/性能备注"},
    )

    if inf == None or not inf["done"]:
        verdict(False, reason="推理步骤未完成")
        output({"dataset": ds, "inference": inf if inf != None else "None"})
    else:
        phase("scoring")
        score = agent(
            """判分。读 /tmp/qwen-eval/cases.json(truth)和 {answers}(模型答卷),程序化计算:
1) 正样本:命中数(answer==truth)、弃权数(answer==none)、错选数(answer!=truth 且 !=none);
2) 负样本:正确拒绝数(answer==none)、误选数;
3) precision = 命中 / (命中+错选+负样本误选)  [弃权不计入分母]
   recall = 命中 / 正样本总数
   拒绝率 = 正确拒绝 / 负样本总数
4) 列出每一道错题的详情(题目队名 vs 模型选择)。
对照验收线:precision>=95% 且 负样本拒绝>=7/8。
给出判决:PASS(直接集成)/ BORDERLINE(加 few-shot 或换 1.7B 重测)/ FAIL(0.6B 不胜任)。""".format(answers=inf["answers_path"]),
            provider="claude",
            label="scoring",
            schema={
                "precision": "数值或分数",
                "recall": "数值或分数",
                "reject_rate": "负样本拒绝率",
                "decision": "PASS|BORDERLINE|FAIL",
                "failures": "错题明细,一行一题",
                "summary": "最终判决与依据",
            },
        )
        if score == None:
            verdict(False, reason="判分失败,答卷在 /tmp/qwen-eval/answers.json")
            output({"inference": inf})
        else:
            verdict(score["decision"] == "PASS" or score["decision"] == "BORDERLINE", reason="decision=" + score["decision"])
            output({
                "decision": score["decision"],
                "precision": score["precision"],
                "recall": score["recall"],
                "reject_rate": score["reject_rate"],
                "avg_seconds_per_case": inf["avg_seconds"],
                "failures": score["failures"],
                "summary": score["summary"],
            })
