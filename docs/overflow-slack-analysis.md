# 溢出属性训练摆烂 — 根因分析（v1，待 umasim 实测确认）

> 症状（小黑板已知问题文档）：「溢出属性训练摆烂：例如溢出智时不训练，选择休息。已知问题，建议自行决定。」
> 上游代码基线：xulai1001/umaai-rs @ e8f6f263（2026-09-11 master）

## 决策链（真机）

小黑板 → umaai watch（协议 v2 重建局面）→ RamenMctsTrainer（search_n=8192）
→ rollout/leaf = `RecommendedRamenTrainer::for_rollout()` → `LocalRamenTrainer::decide_train_cached`
→ `RamenPolicy::decide_train_cached`（守门 0-3 → 打分 argmax）

## 嫌疑排序（按纸面证据）

### S1（主嫌）`reserve_penalty` 未把增益钳制到剩余空间 — local_ramen_trainer.rs

```rust
let h = (limit - status).max(0);
let b = (r - h).max(0.);
let a = (r - (h - gain)).max(0.);   // ← gain 未 clamp 到 h
p += (a*a - b*b) / (2r);  // 最终 ×6
```

- 属性已满（h=0）时游戏截断增益、属性根本不动，但公式按原始 gain 记「预留空间被侵占」：
  罚分 = 6×(gain + gain²/2r)。turn=50（r≈13.7）、gain=60 → **≈ −1150 分**。
- 已满位仅剩的 PT 价值（pt_rate 16/64 × 7~28 ≈ 112~1792）被淹没 → 该位训练分可为负。
- **只在生产 preset 触发**：`status_reserve_max=40` 由 `RecommendedRamenTrainer::new()` 设置；
  `LocalRamenConfig::default()` 为 0（关闭）→ 模拟器默认路径测不出来，必须用 Recommended preset。
- 修法：`let eff = gain[i].min(h);` 后再算 a（h=0 时罚分应为 0——溢出浪费已由
  `status_gain` 截断计价一次，不应重复收费）。

### S2 `cap_discount_weight=1.0`（preset 开启，policy.rs 方案 E）

主属性 cap_left=0 → ratio=0 → **副属性差分全部清零**（注释明言 PT 不打折）。
与 S1 叠加：溢出位只剩 PT，再被 S1 打成负分。

### S3 `wisdom_vital_floor = i32::MAX`（EXP-006c 豁免默认关）

智力是唯一体力 +5 的训练位；智 build 溢出后失去「免费体力」位，
其余位消耗体力 → vital 更快跌破 40 → **守门 2 强制休息**（这才是"选择休息"的直接出口？）。

## 待实测判据（umasim 诊断测试，RecommendedRamenTrainer）

构造：智 build（3 智卡）+ 智已满 + 扫描 vital∈{35..80} × turn∈{25,50,65}：
1. 打印全部候选完整 breakdown → 确认「休息胜出」由守门 2 触发还是打分 argmax 触发；
2. 若是打分：确认 reserve_penalty/cap_discount 项的量级；
3. 若是守门：S3 升为主嫌（vital 经济问题，不是溢出问题）。

## 验证纪律（照上游规矩）

- 固定 base_seed=61444，同种子配对 ≥100 局，逐字段 CSV 对照；
- 改策略打分 = 改变决策轨迹 → **拉面基线作废**，须重抓快照并在 PR 注明（上游惯例）；
- 规则层/模拟数值零改动（只动 policy/local 评分层）。

## 仓库说明

- 本仓 = 上游 master 的一次性镜像（.github/workflows/bootstrap-mirror.yml，跑完即删）；
- 与 umaai-rs-fanskip（另一 AI 在做粉丝数达标跳过）完全隔离，互不冲突。
