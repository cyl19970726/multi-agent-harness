workflow(
    "mars-id-pattern-probe",
    "比对同一场 CS2 比赛在 Mars / PandaScore / GRID 三个体系里的全套 ID(match/series/league/team)," +
    "用多场已验证映射的比赛找出 ID 是否一致或存在换算规律。分三步:A 从 InfluxDB(经本机 tunnel)取 " +
    "Mars 侧 ID 表;B 用礼貌节流(UA+sleep)逐个查 Gamma 拿 PandaScore/GRID ID,避免上次的 403;" +
    "C 对并排表做数值规律分析。串行因 B 依赖 A 的 conditionId,C 依赖两者输出。",
    success_criterion="拿到 >=5 场同场比赛的双侧 ID 并排表,并给出'是否一致/有无规律'的明确结论",
)

phase("influx")
influx = agent(
    """你在一台 Mac 上,本机 localhost:18181 是到 InfluxDB 3 的 tunnel(无鉴权)。
查询方式: curl -s -X POST http://localhost:18181/api/v3/query_sql -H 'Content-Type: application/json' -d '{"db":"<库名>","q":"<SQL>","format":"json"}'
注意 SQL 里 camelCase 标识符要双引号,如 "marsMatchId"。

对这 8 场比赛: 451718,451672,451668,451035,450230,451659,451709,451711

1) 从 polymarket 库 match_mapping 表取每场一个 conditionId:
   SELECT DISTINCT "marsMatchId","conditionId","marketType" FROM match_mapping WHERE "marsMatchId" IN ('451718',...) AND confidence>=0.9 AND "marketType" IN ('moneyline','match_winner')
   每个 marsMatchId 只保留一条(优先 moneyline)。
2) 从 augmentation 库 cs2_match 表取 Mars 侧 ID:
   SELECT team_a_id, team_b_id, series_id, league_id FROM cs2_match WHERE match_id='<id>' ORDER BY time DESC LIMIT 1

输出:每场一行 CSV,格式
marsMatchId,conditionId,team_a_id,team_b_id,series_id,league_id
字段缺失写 null。不要输出其他内容。""",
    provider="codex",
    writable=True,
    label="influx-ids",
    schema={"rows": "CSV 行,每场一行,一行一个"},
)

if influx == None:
    verdict(False, "influx 侧取数失败")
    output("influx worker 未返回有效数据")
else:
    phase("gamma")
    gamma = agent(
        """下面是若干场 CS2 比赛的 Mars 侧 ID(CSV: marsMatchId,conditionId,team_a_id,team_b_id,series_id,league_id):

{rows}

对每一行的 conditionId,查 Polymarket Gamma API 拿对应 event 的第三方 ID:
curl -s -H 'User-Agent: Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36' \
  'https://gamma-api.polymarket.com/markets?condition_ids=<conditionId>&closed=true'
重要:每次请求之间 sleep 4 秒(上次连续请求被 403 限流);若仍 403,再 sleep 20 秒重试一次。
从响应 markets[0].events[0] 提取: gameId,以及 eventMetadata(可能是 JSON 字符串,要再 parse)里的
pandascoreMatchId、gridSeriesId,顺带 league、leagueTier、tournament。

输出:每场一行 CSV,格式
marsMatchId,gameId,pandascoreMatchId,gridSeriesId,league,leagueTier
取不到的字段写 null。不要输出其他内容。""".format(rows=influx["rows"]),
        provider="codex",
        writable=True,
        label="gamma-ids",
        schema={"rows": "CSV 行,每场一行,一行一个"},
    )

    if gamma == None:
        verdict(False, "gamma 侧取数失败")
        output("influx 侧数据已取得,但 gamma worker 未返回有效数据:\n" + influx["rows"])
    else:
        phase("analyze")
        ana = agent(
            """同一批 CS2 比赛在两个体系里的 ID 并排数据:

Mars 侧 (marsMatchId,conditionId,team_a_id,team_b_id,series_id,league_id):
{mars}

Polymarket/PandaScore/GRID 侧 (marsMatchId,gameId,pandascoreMatchId,gridSeriesId,league,leagueTier):
{gamma}

请做严格的数值规律分析:
1. gameId 与 pandascoreMatchId 是否逐场相等?
2. marsMatchId 与 pandascoreMatchId:计算每场差值,差值是否恒定?比值是否恒定?有无任何线性关系?
3. mars series_id / league_id / team_id 与 gridSeriesId 或其他 ID 之间有没有相等、恒定偏移或数量级规律?
4. 各 ID 的号段范围分别是什么(说明它们是不是独立自增空间)?
结论要诚实:没有规律就明说没有规律。""".format(mars=influx["rows"], gamma=gamma["rows"]),
            provider="codex",
            label="analyze",
            schema={
                "pattern_found": "bool,是否发现任何可用的换算规律",
                "game_eq_ps": "bool,gameId 是否处处等于 pandascoreMatchId",
                "summary": "中文结论,含每场差值列表与号段范围",
            },
        )

        if ana == None:
            verdict(False, "分析步骤失败")
            output("数据已齐但分析失败。Mars:\n" + influx["rows"] + "\nGamma:\n" + gamma["rows"])
        else:
            n_rows = len([x for x in gamma["rows"].splitlines() if x.strip()])
            verdict(n_rows >= 5, reason="拿到 " + str(n_rows) + " 场双侧并排数据")
            output({
                "mars_side_csv": influx["rows"],
                "gamma_side_csv": gamma["rows"],
                "pattern_found": ana["pattern_found"],
                "gameId_equals_pandascore": ana["game_eq_ps"],
                "conclusion": ana["summary"],
            })
