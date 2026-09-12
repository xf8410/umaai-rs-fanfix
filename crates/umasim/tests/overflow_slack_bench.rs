//! 溢出摆烂 A/B 整局基准 v2 —— 归因在 CI 内完成，不靠人眼读截断日志
//!
//! 比赛归因：career_races 位图（umaDB races 目标赛程）→ is_race_turn 判强制；
//!           非强制回合选比赛 = 策略自选（free），这才是策略层计量。
//! 休息归因：breakdown 含「守门」= 体力守门；否则解析各候选分数——
//!           休息分 ≥ 最佳训练分 = 打分胜出；训练分更高却休息 = ANOMALOUS（未知路径，逐条打印）。
//! 臂 A = preset(reserve40, 含 S1 重复收费 bug)；臂 B = 同 preset 但预留模型关闭（上界对照）。

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

fn preset_with_reserve(reserve: f32) -> RecommendedRamenTrainer {
    RecommendedRamenTrainer::with_experiment_overrides(
        [16.0, 64.0, 64.0], 0.5, 0.5, 140.0, 0.10, reserve, 8.0, 6.0, 0.0, 0.0, true,
    )
}

/// 解析 "#0 520[速训练 ...] | #6 20[休息]" 形式的 breakdown → (score, reason)
fn parse_breakdown(bd: &str) -> Vec<(f32, String)> {
    bd.split(" | #")
        .filter_map(|item| {
            let item = item.trim_start_matches('#').trim_start_matches(|c: char| c.is_ascii_digit()).trim_start();
            let (num, rest) = item.split_once(' ')?;
            let _ = num;
            let score = rest.split('[').next()?.trim().parse::<f32>().ok()?;
            let reason = rest.split_once('[').map(|(_, r)| r.trim_end_matches(']').to_string())?;
            Some((score, reason))
        })
        .collect()
}

#[derive(Default)]
struct Agg {
    score: f64,
    games: usize,
    race_forced: usize,
    race_free: usize,
    rest_gate: usize,
    rest_scoring: usize,
    rest_anomalous: usize,
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

    let mut arms = [("A-preset-reserve40", Agg::default()), ("B-reserve-off", Agg::default())];
    let mut anom_samples: Vec<String> = Vec::new();

    for build in &builds {
        let deck_ids = build.build_deck(&representatives.picked, FRIEND)?;
        for i in 0..RUNS {
            let seed = BASE_SEED + i;
            for (arm_name, trainer) in [
                ("A-preset-reserve40", preset_with_reserve(40.0)),
                ("B-reserve-off", preset_with_reserve(0.0)),
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
                    } else if row.action_desc.contains("休息") {
                        let bd = row.score_breakdown.clone().unwrap_or_default();
                        if bd.contains("守门") {
                            a.rest_gate += 1;
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
                                _ => {
                                    a.rest_anomalous += 1;
                                    if anom_samples.len() < 12 {
                                        anom_samples.push(format!(
                                            "{arm_name} {} seed={seed} turn={} best_train={:.0} bd={}",
                                            build.name(), row.turn, best_train,
                                            &bd[..bd.len().min(400)]
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    println!("\n===== 归因汇总（{} 局/build × 2 臂）=====", RUNS);
    for (name, a) in &arms {
        let g = a.games.max(1) as f64;
        println!(
            "{name}: mean_score={:.1} | races/game 强制={:.2} 自选={:.2} | rests/game 守门={:.2} 打分={:.2} 异常={:.2}",
            a.score / g,
            a.race_forced as f64 / g,
            a.race_free as f64 / g,
            a.rest_gate as f64 / g,
            a.rest_scoring as f64 / g,
            a.rest_anomalous as f64 / g
        );
    }
    println!("\n===== ANOMALOUS 样本（休息但训练分更高 → 未知路径）=====");
    for s in &anom_samples {
        println!("{s}");
    }
    assert!(arms[0].1.games > 0);
    Ok(())
}
