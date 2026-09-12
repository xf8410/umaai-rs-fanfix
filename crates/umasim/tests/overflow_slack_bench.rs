//! 溢出摆烂 A/B 整局基准（确定性种子，双臂同 run 配对）+ 休息归因诊断
//!
//! 臂 A = RecommendedRamenTrainer preset（生产参数，status_reserve_max=40）
//! 臂 B = 同 preset 但 status_reserve_max=0（预留模型关闭，影响上界代理）
//! 休息归因：breakdown 含「守门」= 体力/心情/生病硬守门（S3 体力经济）；
//! 不含「守门」= 正常打分 argmax 输给休息（评分层问题）。
//! 同时打印每次休息的回合号与局面摘要，检验是否聚集在属性溢出后的回合。

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

struct Agg {
    score: f64,
    rests: usize,
    races: usize,
    games: usize,
    gate_rests: usize,   // breakdown 含「守门」的休息
    score_rests: usize, // 纯打分输给的休息
}
impl Agg {
    fn new() -> Self {
        Self { score: 0.0, rests: 0, races: 0, games: 0, gate_rests: 0, score_rests: 0 }
    }
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

    let mut agg = [("A-preset-reserve40", Agg::new()), ("B-reserve-off", Agg::new())];

    for build in &builds {
        let deck_ids = build.build_deck(&representatives.picked, FRIEND)?;
        for i in 0..RUNS {
            let seed = BASE_SEED + i;
            for (arm_name, trainer) in
                [("A-preset-reserve40", preset_with_reserve(40.0)), ("B-reserve-off", preset_with_reserve(0.0))]
            {
                let (mut rng, rule_master) = seeded_rngs(BASE_SEED, i);
                let mut game = RamenGame::newgame(UMA, &deck_ids, inherit.clone())?;
                game.set_rule_master(rule_master);
                let logged = LoggingTrainer::new(trainer, seed);
                game.run_full_game(&logged, &mut rng)?;
                let log = logged.take_records();
                let mut rests = 0;
                let mut gate = 0;
                let mut pure = 0;
                for r in &log.rows {
                    if r.stage != "Train" {
                        continue;
                    }
                    if r.action_desc.contains("比赛") {
                        rests += 0; // races counted below
                    }
                    if r.action_desc.contains("休息") {
                        rests += 1;
                        let bd = r.score_breakdown.clone().unwrap_or_default();
                        if bd.contains("守门") {
                            gate += 1;
                        } else {
                            pure += 1;
                        }
                        println!(
                            "REST {arm_name} {} seed={} turn={} gate={} bd={}",
                            build.name(),
                            seed,
                            r.turn,
                            bd.contains("守门"),
                            &bd.chars().take(160).collect::<String>()
                        );
                    }
                }
                let races = log.rows.iter().filter(|r| r.stage == "Train" && r.action_desc.contains("比赛")).count();
                let score = game.uma.calc_score();
                println!("{arm_name} {} seed={seed} score={score} rests={rests} races={races}", build.name());
                let a = &mut agg.iter_mut().find(|(n, _)| *n == arm_name).unwrap().1;
                a.score += score as f64;
                a.rests += rests;
                a.races += races;
                a.games += 1;
                a.gate_rests += gate;
                a.score_rests += pure;
            }
        }
    }

    println!("\n===== 汇总（{RUNS} 局/build × 2 臂）=====");
    for (name, a) in &agg {
        println!(
            "{name}: mean_score={:.1} rests/game={:.2} (守门 {:.2} + 打分 {:.2}) races/game={:.2} games={}",
            a.score / a.games as f64,
            a.rests as f64 / a.games as f64,
            a.gate_rests as f64 / a.games as f64,
            a.score_rests as f64 / a.games as f64,
            a.races as f64 / a.games as f64,
            a.games
        );
    }
    Ok(())
}
