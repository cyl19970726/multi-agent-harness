workflow(
    "cs2-four-probes",
    "并行验证四个互相独立的关键未知,全部来自官方 API 调研后的行动清单:A) Polymarket Sports WS " +
    "是否真实推送 CS2 比分(它是官方比赛状态锚点);B) market WSS 带 custom_feature_enabled 后 " +
    "best_bid_ask 是否真的推送(解锁最干净底价源的一行修复验证);C) InfluxDB 覆盖漏斗口径定案 " +
    "(119 vs 355 之谜);D) 小模型仲裁能救回多少被 resolver 丢弃的映射覆盖率(operator 刚拍板的 " +
    "①.5 层的可行性预演)。四者无依赖故并行,最后一个 claude 节点做关键综合。",
    success_criterion="四个探针至少三个产出明确结论,且综合报告给出每个探针的下一步动作",
)

phase("probes")
probes = parallel([
    {
        "label": "sports-ws",
        "provider": "codex",
        "writable": True,
        "schema": {
            "works": "bool,是否成功收到 CS2 消息",
            "summary": "消息 schema、收到的 gameId 样例、推送频率、ping/pong 行为",
        },
        "prompt": """探测 Polymarket 官方 Sports WS。写一个临时 node 或 python 脚本连接
wss://sports-api.polymarket.com/ws (无需鉴权),采集 3 分钟消息。
注意:服务端每 5 秒发 ping,需在 10 秒内回 pong,否则会被断开。
过滤 leagueAbbreviation 含 cs2 的消息(若 3 分钟内没有 cs2,也记录收到的其他 league 样例证明通道活着)。
报告:1) 连接是否成功、消息总数;2) 一条完整消息的字段结构;3) cs2 消息的 gameId/score/period/status 样例;
4) 推送频率观感。临时文件写在 /tmp 下。""",
    },
    {
        "label": "bba-flag",
        "provider": "codex",
        "writable": True,
        "schema": {
            "works": "bool,带 flag 订阅后是否收到 best_bid_ask 事件",
            "summary": "订阅消息原文、收到的事件类型分布、best_bid_ask 样例或未收到的证据",
        },
        "prompt": """验证 Polymarket market WSS 的 custom_feature_enabled 标志。步骤:
1) 先取一个活跃 CS2 市场的 token:
   curl -s -H 'User-Agent: Mozilla/5.0' 'https://gamma-api.polymarket.com/events?tag_slug=counter-strike-2&active=true&closed=false&limit=20'
   从结果里找一个 markets[].clobTokenIds 非空的市场(clobTokenIds 是 JSON 字符串要再 parse),取第一个 tokenId。
   若 CS2 没有活跃市场,fallback 用任意活跃体育市场(改 tag_slug=nba 之类)。
2) 写临时脚本连 wss://ws-subscriptions-clob.polymarket.com/ws/market,发订阅:
   {"type":"market","assets_ids":["<tokenId>"],"custom_feature_enabled":true,"initial_dump":true}
   每 10 秒发一次 PING。采集 3 分钟。
3) 报告:收到的事件 type 分布(book/price_change/best_bid_ask/...),best_bid_ask 是否出现及其样例 payload;
   若没出现,说明书面证据(收到了什么)。临时文件写 /tmp。""",
    },
    {
        "label": "coverage-audit",
        "provider": "codex",
        "writable": True,
        "schema": {
            "funnel": "漏斗各级数字,一行一级",
            "verdict_119_355": "119 vs 355 的口径定案结论",
        },
        "prompt": """InfluxDB 覆盖漏斗审计。本机 localhost:18181 是 InfluxDB 3 tunnel,查询:
curl -s -X POST http://localhost:18181/api/v3/query_sql -H 'Content-Type: application/json' -d '{"db":"<库>","q":"<SQL>","format":"json"}'
SQL 中 camelCase 标识符要双引号。依次查并输出漏斗:
1) polymarket 库 match_mapping: count(DISTINCT "conditionId") 和 count(DISTINCT "marsMatchId"),分 confidence>=0.9 与全部两档;
2) augmentation 库 cs2_event: count(DISTINCT match_id);cs2_round: count(DISTINCT match_id);cs2_match: count(DISTINCT match_id);
3) cs2_match 里 status 分布(GROUP BY status),以及有 cs2_event 的 match 与 cs2_match 全集的差;
4) 用 3 的结果判断:355(mapping 认识的 marsMatchId)与 119(有事件的)差距,主要是 collector 没采 live 数据的比赛,
   还是 mapping 把不存在的比赛也映了?给出口径定案。
输出漏斗数字表 + 一段结论。""",
    },
    {
        "label": "llm-arbiter-dryrun",
        "provider": "codex",
        "writable": True,
        "schema": {
            "recoverable": "int,抽样中可救回的场数",
            "sampled": "int,抽样总数",
            "summary": "每场一行:mars 队名 -> 找到/没找到 Polymarket 市场及原因",
        },
        "prompt": """预演"小模型仲裁能救回多少映射覆盖率"。步骤:
1) 经 localhost:18181 tunnel 查 InfluxDB(POST /api/v3/query_sql,camelCase 加双引号):
   a. augmentation 库: SELECT DISTINCT match_id FROM cs2_event (有事件的比赛全集)
   b. polymarket 库: SELECT DISTINCT "marsMatchId" FROM match_mapping WHERE confidence>=0.9
   求差集 = 有事件但没映射的比赛。从中取 8 场,再查 cs2_match 拿每场的 team_a_name/team_b_name/start_time。
2) 对每场,用队名查 Polymarket 是否其实存在对应市场:
   curl -s -H 'User-Agent: Mozilla/5.0' 'https://gamma-api.polymarket.com/public-search?q=<队名>'
   或 events 搜索。每次请求 sleep 4 秒防 403。
3) 你自己扮演仲裁模型:对每场判断「Polymarket 有市场但 resolver 漏了(可救回)」还是「Polymarket 根本没开盘(救不回)」,
   给出判断依据(队名对应/时间吻合)。
输出:可救回场数/抽样数 + 每场一行的明细。""",
    },
])

phase("synthesize")
names = ["sports-ws", "bba-flag", "coverage-audit", "llm-arbiter-dryrun"]
parts = []
for i in range(len(probes)):
    body = json.encode(probes[i]) if type(probes[i]) == "dict" else str(probes[i])[:2000]
    parts.append("### " + names[i] + "\n" + body)

syn = agent(
    """四个并行探针的结果如下,请做关键综合(这是给 operator 的决策材料):

{body}

要求:
1. 每个探针一段:结论(成/败/部分)+ 它解锁或封死了哪条路;
2. 综合判断:对「CS2 事件驱动 maker 策略」的下一步,哪个动作现在最值得做;
3. 凡探针失败的,说明失败原因与重试建议。
中文,精炼,不超过 600 字。""".format(body="\n\n".join(parts)),
    provider="claude",
    label="synthesize",
    schema={"report": "综合报告全文", "next_action": "最值得立即做的一个动作"},
)

ok_count = len([p for p in probes if type(p) == "dict" and p.get("works", True)])
if syn == None:
    verdict(False, "综合步骤失败,但探针数据已留存")
    output({"probes": parts})
else:
    verdict(ok_count >= 3, str(ok_count) + "/4 探针有效")
    output({
        "report": syn["report"],
        "next_action": syn["next_action"],
        "probes_raw": parts,
    })
