workflow(
    "cs2-ops-check",
    "运维体检:回答 operator 的问题「CS2 worker 启动没有、机器资源够吗」。三个独立探针并行:" +
    "A) GCE 系统资源(磁盘——已知之前 99% 满、内存、CPU、关键进程);B) worker 进程现状" +
    "(之前 pm2 在非交互 shell 下找不到,需要多途径找:ps/systemd/pm2 各种 PATH);" +
    "C) 数据新鲜度(InfluxDB 各表 max(time)——数据还在写就是 worker 活着的铁证,比进程表更可信)。" +
    "最后 claude 综合成「能否安全重启 worker」的运维判决,因为磁盘 99% 时重启采集器会写爆盘。",
    success_criterion="给出每个 worker 的明确状态(跑/停)+ 磁盘/内存数字 + 能否安全重启的判决",
)

SSH = "gcloud compute ssh strategy-cs2 --project unique-nebula-483513-g3 --zone europe-west4-a --tunnel-through-iap --command "

phase("probes")
probes = parallel([
    {
        "label": "gce-system",
        "provider": "codex",
        "writable": True,
        "schema": {
            "disk_pct": "根分区使用百分比数字",
            "mem": "内存总量/已用/可用",
            "summary": "磁盘各大目录、CPU 负载、influxdb3/docker 进程状态",
        },
        "prompt": """检查 GCE 服务器 strategy-cs2 的系统资源。用这个命令模板执行远程命令(本机 gcloud 已认证):
{ssh}'<远程命令>'
依次查:
1) df -h /  (注意:之前是 99% 满,1.9G 剩余 —— 确认现状)
2) sudo du -xh --max-depth=1 /var/lib/influxdb3/data 2>/dev/null | sort -rh | head -5  (influx 还在涨吗)
3) free -h ; uptime ; nproc
4) ps -eo pid,pcpu,pmem,args --sort=-pmem | head -12
5) docker ps --format 'table {{{{.Names}}}}\t{{{{.Status}}}}' 2>/dev/null || sudo docker ps --format 'table {{{{.Names}}}}\t{{{{.Status}}}}'
输出关键数字 + 简评。""".format(ssh=SSH),
    },
    {
        "label": "workers",
        "provider": "codex",
        "writable": True,
        "schema": {
            "running": "正在跑的 worker 名单,一行一个,没有写 none",
            "stopped": "存在定义但没在跑的 worker 名单,一行一个",
            "summary": "worker 管理方式(pm2/systemd/裸进程)与各自状态明细",
        },
        "prompt": """查 GCE 服务器 strategy-cs2 上 CS2 相关 worker 的进程现状。命令模板:
{ssh}'<远程命令>'
注意:之前直接跑 pm2 报 "no pm2"(非交互 shell PATH 问题)。多途径找:
1) {ssh}'export PATH=$PATH:$HOME/.local/share/fnm/aliases/default/bin:$HOME/.npm-global/bin:/usr/local/bin; which pm2 && pm2 jlist | head -c 3000 || echo NO_PM2'
2) {ssh}'ps -eo pid,etime,args | grep -iE "cs2|market-data|collector|supervisor|mm-|ops-aggregator|ee " | grep -v grep'
3) {ssh}'systemctl list-units --type=service --state=running 2>/dev/null | grep -iE "cs2|market|collector|trading" ; ls ~/earning-engine-merge/ecosystem*.cjs'
4) {ssh}'ls -la ~/.pm2/pids/ ~/.pm2/dump.pm2 2>/dev/null | head -20'
对照仓库已知的 worker 定义(cs2-collector, cs2-mapping-reconciler, market-data, mm-eth-15m, ops-aggregator,
cs2 做市 ecosystem),逐个给出 跑/停 判断。""".format(ssh=SSH),
    },
    {
        "label": "data-freshness",
        "provider": "codex",
        "writable": True,
        "schema": {
            "fresh": "正在持续写入的表,一行一个 '表名 最后写入时间 距今分钟'",
            "stale": "已停写的表,同格式",
            "summary": "数据面结论:哪些采集链路活着",
        },
        "prompt": """用数据新鲜度判断采集 worker 是否真的在工作(比进程表更可信)。
本机 localhost:18181 是 GCE InfluxDB 3 的 tunnel:
curl -s -X POST http://localhost:18181/api/v3/query_sql -H 'Content-Type: application/json' -d '{"db":"<库>","q":"<SQL>","format":"json"}'
对下列表各查 SELECT max(time) FROM <表>:
- augmentation 库: cs2_event, cs2_round, cs2_match, cs2_trend, binance_depth, crypto_prices
- polymarket 库: orderbook_events, last_trades, match_mapping, market_metadata
- trading 库: execution_events
把 max(time) 与当前 UTC 时间比,算出距今多少分钟。距今 <10 分钟算"在写",否则算"停写"。
输出两组清单 + 一句话结论。""",
    },
])

phase("synthesize")
names = ["gce-system", "workers", "data-freshness"]
parts = []
for i in range(len(probes)):
    body = json.encode(probes[i]) if type(probes[i]) == "dict" else str(probes[i])[:2000]
    parts.append("### " + names[i] + "\n" + body)

syn = agent(
    """这是 CS2 交易系统 GCE 服务器的三路运维体检结果:

{body}

背景:① 这台机器之前磁盘 99% 满(118G 盘只剩 1.9G),InfluxDB 占 71G,清理一直没执行;
② worker 之前是 operator 主动叫停的;③ orderbook 采集器若在跑,orderbook_price_changes 表
(3.4B 行)还会继续膨胀。

给出运维判决:
1. 每个 worker 的状态汇总表(跑/停/未知);
2. 机器资源够不够:磁盘还能撑多久(按数据新鲜度推断当前写入速率),内存/CPU 有没有压力;
3. **现在重启全部 worker 安全吗?** 若不安全,先做什么(精确到动作:删哪个表/设多少天保留期/扩多大盘);
4. 一个最优先动作。
中文,精炼。""".format(body="\n\n".join(parts)),
    provider="claude",
    label="ops-verdict",
    schema={
        "safe_to_restart": "bool,现在重启全部 worker 是否安全",
        "verdict": "完整运维判决",
        "top_action": "最优先的一个动作",
    },
)

ok_count = len([p for p in probes if type(p) == "dict"])
if syn == None:
    verdict(False, reason="综合失败,探针数据已留存")
    output({"probes": parts})
else:
    verdict(ok_count >= 2, reason=str(ok_count) + "/3 探针有效")
    output({
        "safe_to_restart": syn["safe_to_restart"],
        "verdict": syn["verdict"],
        "top_action": syn["top_action"],
        "probes_raw": parts,
    })
