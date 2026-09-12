# 溢出属性训练摆烂 — 根因分析（v3，撤回比赛结论 + 归因方法修正）

> 症状（小黑板已知问题文档）：「溢出属性训练摆烂：例如溢出智时不训练，选择休息。已知问题，建议自行决定。」
> 基线：上游 xulai1001/umaai-rs master @ e8f6f263（本仓 master 为压缩镜像）

## 决策链（真机）

小黑板 → umaai watch（协议 v2 重建局面）→ RamenMctsTrainer（search_n=8192）
→ rollout/leaf = `RecommendedRamenTrainer::for_rollout()` → `LocalRamenTrainer::decide_train_cached`
→ `RamenPolicy::decide_train_cached`（守门 0-3 → 打分 argmax）

## S1 根因已实证（CI 红）：reserve_penalty 对已满位重复收费

- 实测：智已满、gain=60、turn=50 → 罚 **1149.23** 分（期望 0）；对照剩 10 点 → 罚 886（模型本意，保留）；
- 只在生产 preset 触发（`status_reserve_max=40`；default=0 关闭 → 模拟器默认路径测不出）；
- 修复 = 钳制 `eff = gain.min(h)`（patches/0001-reserve-penalty-clamp.patch）；
- 但整局 A/B（7 build × 10 seed）：A=64338.3 vs B=64271.5，**聚合影响 ≈ −67 分（噪声级）**——
  它是真代码缺陷，但**不是摆烂症状的主因**。诚实降级。

## ❌ 已撤回：「比赛校准漂移 → 每局 12 场是策略崩坏」

**用户纠正（09-12 原话，来源级①）**：「有的马目标比赛有 12 个甚至更多，你不能根据错误结果推理
正确结果，然后拿来当答案，在模拟器里面设置目标赛程是必要的，不然和育成对不上，这也是模拟器有
这个 bug 的原因。」

实查确认：模拟器**有**目标赛程建模——`UmaData.races: Vec<i32>`（umaDB.json「比赛回合」）→
`zip_races()` → `Uma::career_races` 位图 → `is_race_turn()` 强制比赛（73/75/77 生涯决赛另短路）。
102601 一局 12 场 ≈ 目标赛程本身。**我此前把强制比赛和策略自选比赛混在一个计数里，
「12.3 races/game」不构成任何策略结论。** 合成单回合的 Race=1564 观察只是评分公式的事实，
不证明真实对局过度比赛。策略自选比赛率由 bench v2 单独计量（CI 内解析，见下）。

## 休息归因（bench v2 方法修正后重测中）

v1 数据：rests/game=4.10 = 守门 1.81 + 「打分」2.29——但 gate=false 行的 breakdown 在日志里
被截断到 ~200 字符，休息项分数不可见，**「打分选休息」未经证实**。
v2 改为 CI 内解析分类：守门 / 打分胜出（休息分≥最佳训练分）/ **ANOMALOUS**（训练分更高却休息
→ 存在未知路径，逐条打印样本）。比赛同样拆 强制/自选。

## 当前嫌疑排序（v2 数据回来后定谳）

1. **体力经济**（S3）：智 build 溢出后失去唯一体力+5 位 → vital<40 守门（已占 1.81/game）；
   `wisdom_vital_floor` 豁免默认关（EXP-006c 开关现成，可 A/B）。
2. **ANOMALOUS 路径**（若 v2 计数>0）：休息但训练分更高 → 决策路径与日志不一致，另查。
3. S1 修复照常落地（真缺陷，聚合影响小）。

## 落地边界

- 修复只动策略评分层；规则层/模拟数值零改动；
- 改决策轨迹 → 拉面基线作废（上游惯例，PR 须注明+重抓快照）；
- 157KB 大文件无法经 API 安全整写 → patch 文件交付，由持有 git 通道的一方 `git apply`；
- libtests 上游既有基线红（issues.md 记 4 failed pre-existing），待 master 基线分支复核。
