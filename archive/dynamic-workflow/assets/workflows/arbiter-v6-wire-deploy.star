workflow(
    "arbiter-v6-wire-deploy",
    "match-arbiter 真集成:把已验证的 L1+L2 仲裁器(feat/match-arbiter,100 题 100% precision)接入 " +
    "dynamic-match-resolver 的丢弃分支(ambiguous_teams/SKIP/低置信 miss——46% 覆盖缺口的来源)," +
    "加特性开关与来源标记,测试+review 过线后合 main 并部署到 GCE reconciler(服务器 git 凭证已失效," +
    "故用 gcloud scp 直传变更文件+服务器端构建+pm2 重启)。串行 gate:集成→验证→review→部署,任一不过即停。",
    success_criterion="resolver 丢弃分支接通 arbiter(开关可控,行 confidence=0.85 source=arbiter),测试全绿,review 通过,GCE reconciler 跑新代码",
)

phase("wire")
impl = agent(
    """把 match-arbiter 接入 resolver 的丢弃分支。仓库 /Users/hhh0x/earning-engine-merge,分支 feat/match-arbiter(已含 arbiter 模块,直接在其上工作)。

改动目标 packages/data-platform/src/bridge/dynamic-match-resolver.ts:
1) 在两个 miss 路径上加 arbiter fallback:
   a. findBestMatch 返回 null(ambiguous_teams)时;
   b. alignDecision 全 SKIP 导致候选为空时(若代码路径如此);
   两处都改为:把 Mars 候选列表(已在作用域内)转成 arbiter 的 candidates 形状,调
   packages/data-platform/src/bridge/match-arbiter 的 resolveMatch(gammaCtx, candidates);
   命中 → 返回正常 ResolverOutput,但 confidence=0.85、evidence 增加 {source:'arbiter', layer, score};
   未命中 → 维持原 miss 行为完全不变。
2) 特性开关:环境变量 CS2_ARBITER_ENABLED(默认 '1';设 '0' 时行为与改动前逐字节一致)。
3) 每次 arbiter 裁决(无论命中)console.log 一行结构化 JSON(prefix [arbiter])供生产观测。
4) **绝不动 0.95 高置信 happy path 的任何逻辑**;diff 范围只允许:resolver 的 miss 分支 + import + 开关。
5) 单测:在 match-arbiter __tests__ 加 resolver-fallback 测试——构造一个 ambiguous 场景(两个候选名字相近),
   断言开关开时走 arbiter 且 evidence.source='arbiter',开关关时返回原 miss。
完成标准:包内 typecheck 通过 + 全部测试绿(含原 parity)+ conventional commit。
返回 files/commit/test 输出。失败如实报告。""",
    provider="codex",
    writable=True,
    label="wire-resolver",
    schema={"done": "bool", "commit": "hash 或 none", "test_result": "测试输出尾部", "files": "改动文件清单"},
)

if impl == None or str(impl.get("done")) != "true":
    verdict(False, reason="集成步骤未完成")
    output({"wire": impl if impl != None else "None"})
else:
    phase("review")
    rev = agent(
        "Review 集成 commit " + str(impl["commit"]) + "(仓库 /Users/hhh0x/earning-engine-merge 分支 feat/match-arbiter)。\n" +
        """重点(git show + 读上下文):
1) git diff 范围是否真的只有 miss 分支+import+开关(任何对高置信路径/打分逻辑的改动 = 否决);
2) 开关关闭时是否逐字节恢复原行为(看代码路径,不是看注释);
3) arbiter 命中写出的 ResolverOutput 形状是否与正常产出兼容(下游 reconciler 写 match_mapping 不会炸);
4) fallback 测试是真断言还是空壳;
5) 候选转换(MarsMatch → arbiter candidates)字段映射是否正确(teamA/teamB/日期单位)。
输出 approve true/false + 问题清单。""",
        provider="claude",
        label="review",
        schema={"approve": "bool", "issues": "问题清单或 none"},
    )

    if rev == None or str(rev.get("approve")) != "true":
        verdict(False, reason="review 未通过: " + (str(rev.get("issues"))[:200] if rev != None else "review 失败"))
        output({"wire": impl, "review": rev if rev != None else "None"})
    else:
        phase("deploy")
        dep = agent(
            """部署到 GCE。本机仓库 /Users/hhh0x/earning-engine-merge 分支 feat/match-arbiter(已 review 通过)。
1) 本机:git checkout main && git merge --no-ff feat/match-arbiter -m 'merge: match-arbiter L1+L2 wired into resolver' && git push origin main
   (本机 gh 已认证;push 失败则报告并继续后续 scp 部署,不阻塞)
2) 服务器 git 凭证已失效,用文件直传:找出 main 相对服务器代码的变更文件清单(本次 = packages/data-platform/src/bridge/match-arbiter/ 全目录 + dynamic-match-resolver.ts + scripts/match-arbiter-retrain.py):
   gcloud compute scp --recurse <本地文件/目录> hhh0x@strategy-cs2:~/earning-engine-merge/<对应路径> --project unique-nebula-483513-g3 --zone europe-west4-a --tunnel-through-iap
3) 服务器构建+重启(经 gcloud compute ssh 同 project/zone/IAP):
   cd ~/earning-engine-merge/packages/data-platform && export PATH=「fnm node bin 路径,ls ~/.local/share/fnm/node-versions/*/installation/bin 取第一个」:$PATH && npx tsc -p . (或包内 build script)
   然后 data-workers/cs2-mapping-reconciler 若依赖 data-platform dist 需一并构建;pm2 restart cs2-mapping-reconciler
4) 验证:pm2 logs cs2-mapping-reconciler --lines 40 --nostream,确认进程 online、无新错误;grep '[arbiter]' 看是否已有裁决日志(没有也正常,下个 reconcile 周期才会有)。
返回:push 结果、scp 清单、构建输出尾部、pm2 状态、是否见到 [arbiter] 日志。任何失败如实报告。""",
            provider="codex",
            writable=True,
            label="deploy-gce",
            schema={"deployed": "bool", "pushed": "bool 或说明", "pm2_status": "reconciler 状态", "note": "构建/日志摘要"},
        )
        ok = dep != None and str(dep.get("deployed")) == "true"
        verdict(ok, reason=("deployed" if ok else "部署未完成"))
        output({
            "commit": impl["commit"],
            "review": "approved",
            "deploy": dep if dep != None else "None",
        })
