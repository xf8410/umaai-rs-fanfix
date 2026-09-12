//! 溢出摆烂 A/B 整局基准 v3 —— 归因已定谳（休息=体力硬守门），本轮测解药
//!
//! 臂 A = preset 对照（vital_rest=40 硬守门，wisdom_vital_floor=MAX 豁免关）
//! 臂 C = preset + wisf25（vital≥25 时智力训练豁免硬守门——智力位失败率阈值~32
//!        远低于其他位、且体力+5，正是"按别的指标放行"的现成开关）
//! 臂 D = preset + wisf35（更激进：vital≥35 即豁免，几乎把守门让位给打分）
//! 计量：rests/game（守门/打分/异常=守门重算覆盖）、自选比赛、终局分。
//! 异常类在 v2 已证明=守门+recovery_guard 覆盖 breakdown，本轮把异常并回守门统计口径。

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

    let mut arms = [
        ("A-preset", Agg::default()),
        ("C-wisf25", Agg::default()),
        ("D-wisf35", Agg::default()),
    ];

    for build in &builds {
        let deck_ids = build.build_deck(&representatives.picked, FRIEND)?;
        for i in 0..RUNS {
            let seed = BASE_SEED + i;
            for (arm_name, trainer) in [
                ("A-preset", RecommendedRamenTrainer::new()),
                ("C-wisf25", RecommendedRamenTrainer::with_tokens("wisf25")?),
                ("D-wisf35", RecommendedRamenTrainer::with_tokens("wisf35")?),
            ] {
                let (mut rng, rule_master) = seeded_rngs(BASE_SEED, i);
                let mut game = RamenGame::newgame(UMA, &deck_ids, inherit.clone())?;
                game.set_rule_master(rule_master);
                let logged = LoggingTrainer::new(trainer, seed);
                game.run_full_game(&logged, &mut rng)?;
                let log = logged.take_records();

                let a = &mut arms.iter_mut().find(|(n, _)| *n == arm_name).unwrap().1;
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
                                // 休息分不是最高 → recovery_guard 覆盖 breakdown 的守门路径
                                _ => a.rest_gate_anon += 1,
                            }
                        }
                    }
                }
            }
        }
    }

    println!("\n===== 解药 A/B（{RUNS} 局/build × 3 臂）=====");
    for (name, a) in &arms {
        let g = a.games.max(1) as f64;
        println!(
            "{name}: mean_score={:.1} | train/game={:.2} rests/game={:.2}(守门标 {:.2}+守门anon {:.2}+打分 {:.2}) | races/game 强制={:.2} 自选={:.2}",
            a.score / g,
            a.train_turns as f64 / g,
            (a.rest_gate_marked + a.rest_gate_anon + a.rest_scoring) as f64 / g,
            a.rest_gate_marked as f64 / g,
            a.rest_gate_anon as f64 / g,
            a.rest_scoring as f64 / g,
            a.race_forced as f64 / g,
            a.race_free as f64 / g
        );
    }
    assert!(arms[0].1.games > 0);
    Ok(())
}
