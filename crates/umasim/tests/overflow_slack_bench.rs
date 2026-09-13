//! 溢出摆烂 A/B 整局基准 v11 —— 17 臂：转正对照 × 留档哨兵 × EXP-006c 智力豁免剂量扫描
//!
//! 前置：CI 顺序应用 0001 + 0003 a-d + 0004 a-d + 0005（转正）+ 0006 + 0007 + 0008。
//! 臂 A = base（**0005 已转正：A=preset+rclamp 行为**；预期与 B 等值=转正自检）
//! 臂 B = rclamp（显式 token，转正后应与 A 完全一致——A/B 等值即 0005 生效证明）
//! 臂 C = vcurve（Y3=2.5） 臂 D = both 臂 E = vcurve30（Y3=3.0）——甲/乙判负，留档回归哨兵
//! 臂 F = rera（丙-2：r 分年 [40,20,10,0] 期权表，已判中性，留档哨兵）
//! 臂 G = capf25（0006：满位副属性折扣下限 0.25——收益侧零化修复）
//! 臂 H = restdamp（0007：休息价值分年折扣 [1.0,0.9,0.6,0.3]——恢复侧定价）
//! 臂 I = slack20（0008：status_gain 上限期权计价 +20——选择层有界前瞻）
//! 臂 M = recv18（fanfix 0010：训练正回体计价 1.8/点——无价收益通道补账，
//! 口径=ActionValue.vital 正部×影子价；权重=消耗侧 train_vital_value 对称值）
//! 臂 N = recv9（0010 半剂量敏感性：0.9/点，验证剂量-响应）
//! 臂 O = wisf32（EXP-006c 剂量①：仅豁免近零风险区 vital∈[32,45)——32=失败率体力阈值，
//! 出处：policy.rs 智力豁免注释「失败率体力阈值 ~32」；只放确定性安全区）
//! 臂 P = wisf20（EXP-006c 剂量②：下限放宽到 20，剂量-响应中档）
//! 臂 Q = wisf0（EXP-006c 剂量③：45 以下全豁免，最大剂量；0009 卷宗误杀案体力=29 在此档获救）
//! 计量：train/game、rests/game（守门标/anon/打分）、自选比赛、终局分。
//! 判读（一次一变量，各自独立对 A）：见 docs/bench-v10-hypotheses.md 的否决线；
//! 任何新臂终局分劣于 A 超过噪声带（≈0.15%）即判负留档，不进组合臂。

use umasim::bench::{load_player_builds, seeded_rngs, select_representatives, CardPickOpts};
use umasim::game::ramen::RamenGame;
use umasim::game::{Game, InheritInfo, Trainer};
use umasim::gamedata::init_global;
use umasim::trainer::local_ramen_trainer::RecommendedRamenTrainer;
use umasim::trainer::LoggingTrainer;
use umasim::utils::get_workspace_root;

const UMA: u32 = 102601;
const FRIEND: u32 = 303054;
const BASE_SEED: u64 = 61444;
const RUNS: u64 = 10;

fn parse_breakdown(bd: &str) -> Vec<(f32, String)> {
    bd.split(" | #")
        .filter_map(|item| {
            let item = item.trim_start_matches('#');
            let (_idx, rest) = item.split_once(' ')?;
            let (score_s, reason) = rest.split_once('[')?;
            let score = score_s.trim().parse::<f32>().ok()?;
            Some((score, reason.trim_end_matches(']').to_string()))
        })
        .collect()
}

#[derive(Default)]
struct Agg {
    score: f64,
    games: usize,
    race_forced: usize,
    race_free: usize,
    rest_gate_marked: usize,
    rest_gate_anon: usize,
    rest_scoring: usize,
    train_turns: usize,
    wisdom_picks: usize,
}

#[test]
fn overflow_slack_bench_ab() -> Result<(), Box<dyn std::error::Error>> {
    std::env::set_current_dir(get_workspace_root()?)?;
    let _ = init_global();

    let builds = load_player_builds()?;
    let representatives = select_representatives(&CardPickOpts::default())?;
    let inherit = InheritInfo {
        blue_count: [15, 0, 0, 0, 3],
        extra_count: [10, 10, 20, 20, 20, 40],
    };

    let arm_defs: [(&str, &str); 17] = [
        ("A-preset(转正)", "base"),
        ("B-rclamp", "rclamp"),
        ("C-vcurve", "vcurve"),
        ("D-both", "rclamp-vcurve"),
        ("E-vcurve30", "vcurve30"),
        ("F-rera", "rera"),
        ("G-capf25", "capf25"),
        ("H-restdamp", "restdamp"),
        ("I-slack20", "slack20"),
        ("J-feelprice30", "feel30"),
        ("K-slack40", "slack40"),
        ("L-slack60", "slack60"),
        ("M-recv18", "recv18"),
        ("N-recv9", "recv9"),
        ("O-wisf32", "wisf32"),
        ("P-wisf20", "wisf20"),
        ("Q-wisf0", "wisf0"),
    ];
    let mut aggs: Vec<(&str, Agg)> = arm_defs.iter().map(|(n, _)| (*n, Agg::default())).collect();
    let mut skipped: Vec<&str> = Vec::new();

    for build in &builds {
        let deck_ids = build.build_deck(&representatives.picked, FRIEND)?;
        for i in 0..RUNS {
            let seed = BASE_SEED + i;
            for (name, tokens) in &arm_defs {
                if skipped.contains(name) {
                    continue;
                }
                let trainer = match RecommendedRamenTrainer::with_tokens(tokens) {
                    Ok(t) => t,
                    Err(e) => {
                        println!("跳过臂 {name}（token={tokens:?}）: {e}");
                        skipped.push(name);
                        continue;
                    }
                };
                let (mut rng, rule_master) = seeded_rngs(BASE_SEED, i);
                let mut game = RamenGame::newgame(UMA, &deck_ids, inherit.clone())?;
                game.set_rule_master(rule_master);
                let logged = LoggingTrainer::new(trainer, seed);
                game.run_full_game(&logged, &mut rng)?;
                let log = logged.take_records();

                let a = &mut aggs.iter_mut().find(|(n, _)| n == name).unwrap().1;
                a.games += 1;
                a.score += game.uma.calc_score() as f64;

                for row in &log.rows {
                    if row.stage != "Train" {
                        continue;
                    }
                    if row.action_desc.contains("比赛") {
                        if game.uma.is_race_turn(row.turn) {
                            a.race_forced += 1;
                        } else {
                            a.race_free += 1;
                        }
                    } else if row.action_desc.contains("训练") {
                        a.train_turns += 1;
                        if row.action_desc.contains("智") {
                            a.wisdom_picks += 1;
                        }
                    } else if row.action_desc.contains("休息") {
                        let bd = row.score_breakdown.clone().unwrap_or_default();
                        if bd.contains("守门") {
                            a.rest_gate_marked += 1;
                        } else {
                            let entries = parse_breakdown(&bd);
                            let rest = entries.iter().find(|(_, r)| r == "休息").map(|(s, _)| *s);
                            let best_train = entries
                                .iter()
                                .filter(|(_, r)| r.contains("训练"))
                                .map(|(s, _)| *s)
                                .fold(f32::NEG_INFINITY, f32::max);
                            match rest {
                                Some(rs) if rs >= best_train - 0.5 => a.rest_scoring += 1,
                                _ => a.rest_gate_anon += 1,
                            }
                        }
                    }
                }
            }
        }
    }

    println!("\n===== 转正对照 × 体力曲线 × 期权化 A/B（{RUNS} 局/build × 17 臂）=====");
    for (name, a) in &aggs {
        if a.games == 0 {
            println!("{name}: 跳过（构造失败，见上方日志）");
            continue;
        }
        let g = a.games as f64;
        println!(
            "{name}: mean_score={:.1} | train/game={:.2} rests/game={:.2}(守门 {:.2}+打分 {:.2}+anon {:.2}) | 自选比赛/game={:.2} | 智训率={:.1}%",
            a.score / g,
            a.train_turns as f64 / g,
            (a.rest_gate_marked + a.rest_gate_anon + a.rest_scoring) as f64 / g,
            a.rest_gate_marked as f64 / g,
            a.rest_scoring as f64 / g,
            a.rest_gate_anon as f64 / g,
            a.race_free as f64 / g,
            if a.train_turns > 0 { a.wisdom_picks as f64 / a.train_turns as f64 * 100.0 } else { 0.0 }
        );
    }
    Ok(())
}
