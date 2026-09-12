# 溢出属性训练摆烂 — 根因分析（v2，CI 实证更新）

> 症状（小黑板已知问题文档）：「溢出属性训练摆烂：例如溢出智时不训练，选择休息。已知问题，建议自行决定。」
> 基线：上游 xulai1001/umaai-rs master @ e8f6f263（本仓 master 为压缩镜像）

## 决策链（真机）

小黑板 → umaai watch（协议 v2 重建局面）→ RamenMctsTrainer（search_n=8192）
→ rollout/leaf = `RecommendedRamenTrainer::for_rollout()` → `LocalRamenTrainer::decide_train_cached`
→ `RamenPolicy::decide_train_cached`（守门 0-3 → 打分 argmax）

## S1 根因已实证（CI run 34661144116，红）

`LocalRamenTrainer::reserve_penalty` 对已满属性位按**原始增益**全额收「预留空间侵占」罚分：

- 实测：智已满、gain=60、turn=50 → 罚 **1149.23** 分（期望 0）；
- 对照：智剩 10 点 → 罚 886.15（模型本意，保留）；
- 溢出浪费已由 `status_gain` 截断计价一次 → 此处是**重复收费**；
- 只在生产 preset 触发（`status_reserve_max=40`；`LocalRamenConfig::default()`=0 关闭 → 模拟器默认路径测不出）。

修复 = 钳制 `eff = gain.min(h)`（一行），见 `patches/0001-reserve-penalty-clamp.patch`。
gain≤h 时逐位不变；只影响「训练会溢出」分支；负增益语义不变。

## S2 叠加因素：`cap_discount_weight=1.0`（preset）

主属性 cap_left=0 → ratio=0 → 副属性差分清零（PT 保留）。与 S1 叠加后溢出位只剩 PT，
再被 1149 罚分淹没 → 溢出位评分崩坏。

## S3 直接出口：体力守门（vital<40 强制休息）+ 智力豁免默认关

智 build 溢出后失去唯一体力+5 的训练位 → 其他位消耗体力 → vital 跌破 40 →
守门 2 强制休息（`wisdom_vital_floor=i32::MAX` 豁免关闭）。
合成局面单回合测不出（缺真实人头分布/体力经济），整局 bench 才是症状计量——
双臂 A/B（preset vs 预留关闭）见 CI bench job。

## 附带发现（本次 CI breakdown 实锤）：比赛分数校准漂移

`race_panel_discount=0.3` 的注释明言按 **pt_rate=8** 校准（"降到 0.3 后比赛只在平凡回合胜出"）。
生产 preset 把 pt_rate 提到 16/64，但折扣未重标：
G1 比赛 PT 80×64×0.3≈**1536** vs 训练总分 417~546 → **比赛恒胜出**。
实测（turn=50 合成局）：AI 选 Race 1564 分碾压一切训练。
这很可能是文档里「粉丝数满足后 AI 依然选择比赛」的另一半根因（与 fanskip 修的达标判定互补）。
等比修正方向：`race_panel_discount ≈ 0.3×8/64 ≈ 0.04`（或把比赛 PT 单独走低倍率）。
**属策略参数变更，影响大，留给上游/用户拍板，本仓不擅动。**

## 落地边界

- 修复只动策略评分层；规则层/模拟数值零改动；
- 改决策轨迹 → 拉面基线作废（上游惯例，PR 须注明+重抓快照）；
- 157KB 大文件无法经 API 安全整写 → patch 文件交付，由持有 git 通道的一方 `git apply`。
