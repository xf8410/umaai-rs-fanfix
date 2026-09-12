//! 溢出摆烂 A/B 整局基准 v5 —— 守门方向作废（用户判定：守门是对的），只测上限软化
//!
//! 臂 A = preset 原样（上限硬门限悬崖在位）
//! 臂 B = rclamp（patch 0001 v2：预留罚分钳制到剩余空间；CI 应用补丁后此臂激活，否则跳过）
//! 计量：train/game、rests/game（守门标/守门anon/打分）、自选比赛、终局分。

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

    let arm_defs: [(&str, &str); 2] = [("A-preset", ""), ("B-rclamp", "rclamp")];
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
                        println!("跳过臂 {name}: {e}");
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

    println!("\n===== 上限软化 A/B（{RUNS} 局/build）=====");
    for (name, a) in &aggs {
        if a.games == 0 {
            println!("{name}: 跳过（补丁未应用）");
            continue;
        }
        let g = a.games as f64;
        println!(
            "{name}: mean_score={:.1} | train/game={:.2} rests/game={:.2}(守门 {:.2}+打分 {:.2}+anon {:.2}) | 自选比赛/game={:.2}",
            a.score / g,
            a.train_turns as f64 / g,
            (a.rest_gate_marked + a.rest_gate_anon + a.rest_scoring) as f64 / g,
            a.rest_gate_marked as f64 / g,
            a.rest_scoring as f64 / g,
            a.rest_gate_anon as f64 / g,
            a.race_free as f64 / g
        );
    }
    Ok(())
}
