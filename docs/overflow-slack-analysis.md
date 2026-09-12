# 溢出属性训练摆烂 — 根因分析（v3，bench A/B 已出）

> 症状：「溢出属性训练摆烂：例如溢出智时不训练，选择休息」
> 基线：上游 master @ e8f6f263（本仓 master 为压缩镜像）

## 结论速览（CI run 34661489669，7 build × 10 seed × 2 臂 = 140 局）

| 臂 | mean_score | rests/game | races/game |
|---|---|---|---|
| A 生产 preset（reserve=40，含 bug） | **64338.3** | 4.10 | 12.33 |
| B 预留模型全关（上界代理） | 64271.5 | 4.10 | 12.29 |

1. **S1（reserve_penalty 重复收费）是真 bug 但整局聚合影响 ≈ 0**（−66.8 分/局，
   rests 完全相同；多数配对种子轨迹一致）。修复仍值得做（语义正确性+单回合崩坏），
   但它**不是摆烂症状的主因**。
2. **races/game ≈ 12.3** —— 生产 preset 每局打 ~12 场自选比赛，实锤
   `race_panel_discount=0.3`（pt_rate=8 时代校准）在 pt_rate=64 下严重失配，
   比赛分恒压训练。这更可能是「AI 总想比赛」类抱怨的主因。
3. rests/game=4.1 的来源待归因（守门 vs 打分）——归因版 bench 在跑
   （run @ 52aa580b，输出每次休息的回合号+breakdown 是否含「守门」）。

## 根因清单（更新）

### S1 `reserve_penalty` 重复收费 —— 已实证（单测红：1149.23 vs 期望 0）
- 修复 = 钳制 `eff = gain.min(h)`，见 `patches/0001-reserve-penalty-clamp.patch`；
- 语义：h=0→罚 0；gain≤h 逐位不变；负增益不变；
- 整局影响小（本表），但单回合决策质量修复正确（溢出位不再被 -1149 打成深负）。

### S3 体力守门（vital<40 强制休息）+ 智力豁免关闭 —— 归因中
智 build 溢出后失去唯一体力+5 位 → 体力经济恶化 → 守门 2 触发休息。
若归因显示 rests 多为「守门」，则摆烂主因是**体力经济**而非评分层，
修法方向 = 启用 `wisdom_vital_floor`（EXP-006c 已备好开关，preset 未开）
或按溢出位动态调 rest 阈值。

### 附带发现：比赛校准漂移（12.3 场/局实锤）
等比修正方向 `race_panel_discount ≈ 0.04`，属策略参数变更，留上游/用户拍板。

## 落地边界
- 修复只动策略评分层；规则层零改动；改决策轨迹 → 拉面基线作废（上游惯例）；
- 157KB 大文件无法经 API 安全整写 → patch 交付，由持 git 通道方 `git apply`；
- 生产真机路径 = MCTS(search_n=8192) + 本手写策略 rollout；本仓 bench 只测手写层，
  MCTS 层若有额外摆烂机制需下一步单独测（`ramen_mcts_trainer` + 单测构造根局面）。

## 仓库状态
- master = 上游镜像（bootstrap 已退役为惰性占位）；
- workbench/overflow-repro = 复现测试（1红2绿）+ 归因 bench + patch + 本文档；
- 与 umaai-rs-fanskip（另一 AI）零冲突。
