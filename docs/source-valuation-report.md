# 源仓库估值模型实现报告书

代码基线：`xulai1001/umaai-rs` master @ `e8f6f26`（2026-09-12 全文读码：policy.rs / local_ramen_trainer.rs / uma.rs / ramen_mcts_trainer.rs / ramen_terminal.rs / luck_score.rs / constants.json / scenario_ramen.json / ramen_memo_cn.md）

用户问题：①体力收益是否判高（尤其 Y3 友人+100%减失败率）②手写属性上限门限软化 ③PT 收益与真实评分分离 ④期望评分/运气分与 MCTS 实际积分是否耦合。

---

## 0. 五条核心结论

1. **「分离」已经天然存在**：MCTS（FlatSearch）叶值=真实 `calc_score` 七分量，手写评估只充当 rollout 基策；运气分（`luck_score.rs`）是 T(n) 期望分 baseline 的纯显示层追踪，不参与决策。**改手写估值参数（体力/PT/门限）不会污染搜索值，只会改变 rollout 行为——A/B 是干净的。**
2. **「为最后 50-100 属性放弃数百 PT」不是模型 bug，是真实评分的经济性**：属性表凸（边际 0.5→4 分/点随段位上升），1200 段 1 属性≈3 分，而 1 PT 真实价=2 分（`pt_score_rate=2.0`）。顶段属性确实比 PT 值钱。
3. **但 preset 早已大幅抬高 PT**：pt_rate Y1=16、Y2/Y3=64（真实价 2.0 的 8~32 倍）。「PT 判低了要拉高」在当前 master preset 上不成立；真正压制训练的是**上限附近的三重属性压制叠加 reserve_penalty 的第四重错误收费**。
4. **reserve_penalty 在 h=0 的重复收费悬崖是真的**（单元实证 −1149@gain60/turn50；gain200 → −3000+），**但 `max_base_score_sacrifice=140` 回退门挡住了它的极端后果**：悬崖把局部最优推向休息、而满位是基础层最优时（牺牲>140）→ 回退基础层最优=训练。**当前 master 不可能出现「满体力→睡觉」**——CI Y1 复现（AI 选 Train(Wisdom) 345）与代码推演双重证实。你昨天真机看到的睡觉，首要假设是**设备 SO 构建早于这套回退机制**（待 `uma_status` 版本核实）。
5. 悬崖的残余危害=**margin≤140 的静默误导**（把最优满位训练改派到次优位）+ 最多 140 分的评分失真。rclamp 仍是正确修复：policy 层注释明文「PT 不打折：为拿 PT 继续训练已满位是正当行为」，local 层的 reserve_penalty 与之**自相矛盾**。

---

## 1. 三层估值架构与数据流

```
RamenPolicy (policy.rs)                    ← 基础打分：守门 + 即时收益
  └─ LocalRamenTrainer (local_ramen_trainer.rs)  ← 长期调整层：+羁绊/Hint/预留罚/平衡/动态体力
       └─ RecommendedRamenTrainer = 三份年策略      ← preset：years[16/40/40, 64/40/40, 64/40/0]
            ├─ 生产直跑（bench、设备端）
            └─ RamenMctsTrainer (ramen_mcts_trainer.rs)
                 = FlatSearch：rollout 基策=REC::for_rollout()，叶值=真实 calc_score
```

- **policy 层打分**（`score_train_action_eval`）：`attr(表差分,limit截断) + SP×pt_rate − 消耗体力×1.8 + 彩圈×60 + fail_adj`；休息=`20+(55−v)×2.5`（满体力=20 平值）；比赛=面板差分×0.3 折扣+自选赛 urgency。
- **local 层加项**（`decide_train_cached` 尾段）：`+train_long_term(羁绊/Hint×phase) −reserve_penalty +dynamic_status_adjustment +dynamic_vital +expected_fail +coupling +weak_boost`，最后过 **140 回退门**。
- **事件选项不走搜索**（一律转发手写）；吃面合并搜索会位移 RNG 流（基线作废警告，与本仓 bench 无关——bench 用 REC 直跑）。

## 2. 真实评分（ground truth，`uma.rs` + `constants.json`）

```
calc_score = skill_score + floor(skill_pt + hints×6.5)×2.0 + Σ table(min(status, limit))
```

| 属性段位 | 100 | 300 | 500 | 800 | 1000 | 1200 | 1600 | 2000 |
|---|---|---|---|---|---|---|---|---|
| 表值 | 51 | 305 | 510 | 811 | 1002 | 1205 | 1613 | 2016 |
| 边际分/点 | ~1 | ~1.5 | ~2 | ~2.5 | ~3 | ~3 | ~3.5 | ~4 |

- **溢出=真 0**：`add_value` 当场 `.min(limit)` 截断；`ramen_terminal.rs` 文档明确「终局不可能溢出」，其"溢出浪费"维度因恒零被删除。
- **上限会动**（headroom 的期权价值来源）：Y2 basic +20、Y3 +40（status_limit）；超级拉面 `training_limit_options` +100（四选一维）+ pt_limit +100；事件/技能给属性。→ reserve 模型本意正当，错在收费口径（见 §5）。

## 3. 体力估值链（用户判断①：Y3 体力判高——成立）

| 项 | 现值 | 问题 |
|---|---|---|
| train_vital_value | 1.8/点（消耗） | 固定价 |
| dynamic_vital `vital_factor(t)` | **3.5 + t/72×2**（t<72），URA=0.25 | **方向反了**：turn60≈5.2/点，越到后期体力越贵 |
| rest_vital_value | 2.5/点（补到 55 为止） | 固定价 |
| 训练回体力 | **不计值**（`(−vital).max(0)`，智训+5 白给） | 智位体力正收益被吞 |

Y3 真实处境：吃面失败率−100%（必成）、友人外出 48-80 体力+完链、有马 +40、URA 每回合 +20、智训自身 +5——**体力影子价应低于 Y1/Y2，模型却给到全程最高（5+/点）**。`y3_recovery_horizon` 已在吃面门禁里做了"确定恢复前无待保护回合"的正确建模（turn≥70 体力归零不付费），**但没延伸到训练打分**。
→ 修法：`vital_factor` 分年递减（Y1 4.0 / Y2 3.5 / Y3 2.5 / URA 0.25），或把 recovery-horizon 折扣接入训练体力项。守门 40 不动（用户拍板，且守门在打分之前，与本项正交）。

## 4. PT 估值链（用户判断③：拉高 PT——当前 preset 已拉，别再拉）

- 真实价：2.0/pt（终局折算）；技能折算均值 ≈6.7/pt（1200 分技能 ÷ ~180pt）。
- preset 内部价：**16 / 64 / 64**——Y2/Y3 已是真实价的 32 倍。再抬会扭曲 PT vs 休息/外出的可比性。
- 已有的正确出口：`RamenSelection::Pt`（`calc_score_with_pt_favor`：skill_pt×2×pt_favor_rate，五维 favor [1.0,0.9,0.7,0.7,1.0]，×0.37 回尺度）——**MCTS 层现成旋钮**，想验证"PT 导向更强"应搜这个口径，而不是再抬 pt_rate。
- 用户观察的"模型天生倾向拉满属性"在**真实评分层**成立（凸表），在**当前 preset 估值层**已被 64 倍 PT 对冲；满位训练真正被压垮的原因是 §5 的悬崖，不是 PT 判低。

## 5. 上限处理链（用户判断②：软化硬门限——拆成"纠错"与"建模"两件事）

上限附近现有四层作用：
1. `status_gain` 截断（真实、正确，保留）；
2. `cap_discount`（方案E，weight=1.0）：主属性快满时**副属性**打折分流（PT 不打折）——合理；
3. `dynamic_status_balance`（gap 0.5 / over 0.5×卡数放大）：完成度>70% 起平方衰减属性边际——合理（防过度喂养，EXP-006d 实证）；
4. `reserve_penalty`：`r=40×(76−turn)/76`，罚 `6×[(r−h+gain)²−(r−h)²]/2r`。**h=0 时按原始 gain 全额收费 = 对不存在的 headroom 消耗计费**（溢出已被第 1 层截断计价一次）→ 重复收费，这是唯一要修的。

- **rclamp（0001 v2，已在 CI）**：`eff = min(gain, h)`——h=0 罚 0；gain≤h 逐位不变；负增益语义不变。= 纯纠错。
- **用户"允许溢出一点"的正确形态（0004 候选）**：不是给溢出真实分数（游戏会截断，给了就是谎），而是**把上限抬升的期权价值建模进 r**：r 分年/维（Y1 事件密集+两次抬升→40 合理；Y3 抬升将尽→10；URA 上限已定→0），或等价地给 `status_gain` 加 `cap_slack`（≤20，对应 Y2/Y3 basic 抬升幅度）。A/B 决定。
- 140 回退门（§0.4）是上游已有的第四道保险——它解释了为什么悬崖在整局 bench 里伤害有限，也解释了为什么你的睡觉案例需要"旧构建"假设。

## 6. MCTS / 运气分与「分离」问题（用户判断④）

- 搜索取分口径 `RamenSelection::Score` = 终局真实 `calc_score`；`Pt` = pt_favor 口径。**两者都不是手写评估分** → 手写参数失真不会顺着叶值进搜索。
- 手写评估在搜索中的角色 = rollout 策略（`for_rollout`，关日志、决策逐位同普通实例——有测试钉死）。rollout 质量影响搜索效率，不影响价值定义。
- 运气分 = `T(n+1)−T(n)`，T(n)=MCTS candidate_scores 按局数加权 + `(78−turn)×mcts_turn_bonus` 显示换算——纯观测/展示（AIRedirector 用），零决策权重。
- 结论：**你担心的耦合不存在于当前架构**；「期望评分」和「实际积分」已经是分开的两套。

## 7. 真机睡觉案例归因（待验证）

| 假设 | 依据 | 验证动作 |
|---|---|---|
| A. 设备 SO 早于 140 回退门/现 preset | 当前 master 推演+CI 双重排除"新代码能睡" | `uma_status` 查 SO 版本 vs master 提交日期（本次设备离线，待重试） |
| B. 当时实际体力<40 走守门 | 守门在打分前，回退门管不到 | 下次睡觉场景抓 breakdown（future_reason 通道） |
| C. 新代码悬崖误导（非睡觉） | margin≤140 时改位次优 | bench A/B 的 train/game 列会显形 |

## 8. 修复路线图（顺序即优先级）

1. **0001 rclamp**（CI 在跑）：纠错，默认关；bench A/B 正 → preset 默认开。
2. **0003 vital_year_scale**：`vital_factor` 分年递减（Y3 5.2→~2.5）+ 智训回体力计值；A/B。
3. **0004 reserve 期权化**：r 分年/维（40→10→0）或 cap_slack≤20；A/B。
4. **设备版本核实**：决定要不要给上游提"睡觉已在 master 修复、设备端需升级"的说明。
5. **不动**：守门 40、pt_rate 64、cap_discount、dynamic_status_balance、MCTS 叶值口径。

## 附录 A：preset 关键参数速查

| 参数 | Y1 | Y2 | Y3 | 备注 |
|---|---|---|---|---|
| pt_rate | 16 | 64 | 64 | 真实价 2.0 |
| vital_rest / eating | 40/40 | 40/40 | 40/**0** | Y3 吃面回合免守门（必成） |
| status_reserve_max | 40 | 40 | 40 | r 随 turn 线性缩 |
| cap_discount_weight | 1.0 | 1.0 | 1.0 | 副属性分流 |
| gap/over | 0.5/0.5 | 同 | 同 | 短板追赶/近上限衰减 |
| max_base_score_sacrifice | 140 | 140 | 140 | **回退门** |
| dynamic_vital | on | on | on | 3.5+t/72×2 |
| eat_requires_training / covered | on | on | on | 吃面事务门 |
| y3 门禁 | — | — | pre25/hard15/soft0.5/horizon | 吃面侧 |
| 友人配额 | [0,2,5] 累计 | | | 休息替代 |

## 附录 B：本仓验证资产

- 复现测试：`crates/umasim/tests/overflow_slack_repro.rs`（Y1 场景已证实当前 preset 不睡觉=回退门工作；单元测试钉死 h=0 罚 1149.23）。
- 补丁：`patches/0001-reserve-penalty-clamp.patch`（rclamp，配置门控，默认逐位不变）。
- CI：`.github/workflows/umasim-test.yml`（repro / repro-patched / bench 140 局 A/B / libtests）。
- 上游对照快照：REC seed42 整局 64336；MCTS gate-off 65741（`test_combined_gate_off_full_game`）。
