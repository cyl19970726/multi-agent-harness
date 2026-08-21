workflow(
    "cs2-latency-recorder",
    "套利 thesis 生死实验的录制器:在 GCE(co-located 执行点,Mars IP 白名单内)同时录三条流——" +
    "Mars /live WS(我们的信息源)、Polymarket Sports WS(官方比分,PandaScore 系)、market WSS 盘口" +
    "(reprice 终点,自带 custom_feature_enabled:true 顺带验证 best_bid_ask)。每条消息记录统一的本机" +
    "epoch+monotonic 接收时戳,NDJSON 落盘。先用当前任意 live CS2 场试录 10 分钟验证三流齐活," +
    "再用 cron 预约 06-14 11:50 UTC 起的 Major 窗口自动录 7 小时。对齐分析留给数据齐后的下一个 workflow。",
    success_criterion="试录三流均有消息且时戳可对齐;06-14 Major 窗口的自动录制任务已挂上 crontab",
)

SSH = "gcloud compute ssh strategy-cs2 --project unique-nebula-483513-g3 --zone europe-west4-a --tunnel-through-iap --command "

phase("build")
build = agent(
    "在 GCE 服务器上建三流录制器。远程执行模板: " + SSH + "'<命令>'(也可以把本地写好的脚本经 tar over ssh 传上去)。\n" +
    """要求:
1) 脚本 ~/latency-lab/recorder.py(python3,服务器上有 python3.11;websockets 库没有就 pip3 install --user websockets)。
   功能:并发维持三个 WS 连接,每条收到的消息写一行 NDJSON 到 ~/latency-lab/out/<run_id>/<stream>.ndjson:
   {"t_epoch_ms":..., "t_mono_ns":..., "stream":"mars|sports|book", "raw":<原始消息或截断到2KB>}
   a. mars: wss://ws.marzdata.cn/esport/ws/v3/live,鉴权 query 参数 app_id/app_secret
      (从 ~/earning-engine-merge/.env 读 CS2_MARS_APP_ID/CS2_MARS_APP_SECRET,绝不打印值);
      订阅 sport_id=2 全部频道(参考 ~/earning-engine-merge/packages/mars-sdk/src/ws-client.ts 的握手/订阅格式);
      mars 消息额外抽字段:channel、push_time_millis、match_id(若有)。
   b. sports: wss://sports-api.polymarket.com/ws,无鉴权;服务端 5s ping 需 10s 内回 pong;
      只落 leagueAbbreviation 含 cs2 的消息 + 每分钟记一条心跳统计(总消息数/league 分布)。
   c. book: wss://ws-subscriptions-clob.polymarket.com/ws/market,每 10s 发 PING;
      订阅消息 {"type":"market","assets_ids":[...],"custom_feature_enabled":true,"initial_dump":true};
      tokenIds 由启动逻辑解析:本机(服务器)localhost:8181 是 InfluxDB——
      先查 Mars REST 当前 live 比赛的 match_id 列表,再查 polymarket 库 match_mapping(confidence>=0.9)
      拿这些比赛的 conditionId,再调 Gamma /markets?condition_ids= 拿 clobTokenIds(JSON 字符串要再 parse);
      若当前没有任何 live 已映射比赛,fallback:订阅 Gamma 上 active CS2 events 的前 10 个 token。
   命令行参数:--minutes N --run-id NAME。三个流任一断线自动重连并在 NDJSON 里记 reconnect 事件。
2) 先跑通语法(python3 -m py_compile)。
返回脚本路径与关键设计点。失败如实报告。""",
    provider="codex",
    writable=True,
    label="build-recorder",
    schema={"done": "bool", "script_path": "服务器上的脚本路径", "note": "设计要点"},
)

if build == None or str(build.get("done")) != "true":
    verdict(False, reason="录制器构建失败")
    output({"build": build if build != None else "None"})
else:
    phase("dryrun")
    dry = agent(
        "试录验证。远程执行模板: " + SSH + "'<命令>'。\n" +
        """1) 服务器上后台跑: cd ~/latency-lab && nohup python3 recorder.py --minutes 10 --run-id dryrun-$(date +%H%M) > dryrun.log 2>&1 &
2) 等 11 分钟(sleep),然后检查 ~/latency-lab/out/<run_id>/ 三个 ndjson:
   - 每个流的消息条数;mars 是否有 cs2 消息(当前应有 live 比赛);sports 是否有任何消息(cs2 可能没有——如实记录,这本身是 sports-ws 覆盖疑云的证据);book 是否收到 book/price_change,以及**是否出现 best_bid_ask 事件**(custom flag 验证);
   - 抽 3 条消息验证 t_epoch_ms 与 t_mono_ns 同时存在且合理;
   - dryrun.log 里有无未处理异常。
返回各流条数与发现。""",
        provider="codex",
        writable=True,
        label="dryrun",
        schema={"mars_msgs": "条数", "sports_msgs": "条数(及其中 cs2 条数)", "book_msgs": "条数(及 best_bid_ask 条数)", "ok": "bool,三流是否都活", "note": "异常与发现"},
    )

    phase("schedule")
    sched = agent(
        "预约 Major 窗口自动录制。远程执行模板: " + SSH + "'<命令>'。\n" +
        """在服务器 crontab(crontab -l 先看现有,追加不覆盖)加一条:
50 11 14 6 * cd ~/latency-lab && nohup python3 recorder.py --minutes 420 --run-id major-0614 > major-0614.log 2>&1
(06-14 11:50 UTC 起录 7 小时,覆盖 12:00/14:30/17:00 三场 Major;服务器时区先用 timedatectl 确认是 UTC,不是就换算)。
crontab -l 回显确认。返回最终 crontab 相关行。""",
        provider="codex",
        writable=True,
        label="schedule-major",
        schema={"installed": "bool", "cron_line": "最终 cron 行", "tz": "服务器时区"},
    )

    dry_ok = dry != None and str(dry.get("ok")) == "true"
    sched_ok = sched != None and str(sched.get("installed")) == "true"
    verdict(dry_ok and sched_ok, reason="dryrun=" + str(dry_ok) + " schedule=" + str(sched_ok))
    output({
        "build": build,
        "dryrun": dry if dry != None else "None",
        "schedule": sched if sched != None else "None",
    })
