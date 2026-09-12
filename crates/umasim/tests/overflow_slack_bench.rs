//! 溢出摆烂 A/B 整局基准（确定性种子，双臂同 run 配对）
//!
//! 臂 A = RecommendedRamenTrainer::new()（生产 preset，status_reserve_max=40，含重复收费 bug）
//! 臂 B = 同 preset 但 status_reserve_max=0（整个预留模型关闭，bug 上界对照）
//! 口径：7 build × 10 seed（base 61444），统计终局分、Train 阶段休息/比赛次数。
//! 注：B 臂不是修复本身（修复=钳制 gain≤h，保留近上限软惩罚），
//! 它给出预留模型总影响的上界；修复版数值以 patch 落地后的 run 为准。

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
        [16.0, 64.0, 64.0], // pt_rates 分年（preset 值）
        0.5,                // gap_strength
        0.5,                // overflow_strength
        140.0,              // max_base_score_sacrifice
        0.10,               // ramen_window_weight
        reserve,            // status_reserve_max ← 唯一差异
        8.0,                // early_bond_value
        6.0,                // hint_bonus
        0.0,                // weakboost（查表自适应，同 preset）
        0.0,                // region_weak_cover_weight
        true,               // eat_requires_covered_train
    )
}

struct Agg {
    score: f64,
    rests: usize,
    races: usize,
    games: usize,
}
impl Agg {
    fn new() -> Self { Self { score: 0.0, rests: 0, races: 0, games: 0 } }
    fn add(&mut self, s: i32, r: usize, c: usize) { self.score += s as f64; self.rests += r; self.races += c; self.games += 1; }
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
            for (arm, trainer) in [("A-preset-reserve40", preset_with_reserve(40.0)), ("B-reserve-off", preset_with_reserve(0.0))] {
                let (mut rng, rule_master) = seeded_rngs(BASE_SEED, i);
                let mut game = RamenGame::newgame(UMA, &deck_ids, inherit.clone())?;
                game.set_rule_master(rule_master);
                let logged = LoggingTrainer::new(trainer, seed);
                game.run_full_game(&logged, &mut rng)?;
                let log = logged.take_records();
                let rests = log.rows.iter().filter(|r| r.stage == "Train" && r.action_desc.contains("休息")).count();
                let races = log.rows.iter().filter(|r| r.stage == "Train" && r.action_desc.contains("比赛")).count();
                let score = game.uma.calc_score();
                println!("{} {} seed={} score={} rests={} races={}", arm, build.name(), seed, score, rests, races);
                let a = agg.iter_mut().find(|(n, _)| *n == arm).unwrap();
                a.1.add(score, rests, races);
            }
        }
    }

    println!("\n===== 汇总（{} 局/build × 2 臂）=====", RUNS);
    for (name, a) in &agg {
        println!(
            "{name}: mean_score={:.1} rests/game={:.2} races/game={:.2} games={}",
            a.score / a.games as f64,
            a.rests as f64 / a.games as f64,
            a.races as f64 / a.games as f64,
            a.games
        );
    }
    assert!(agg[0].1.games > 0);
    Ok(())
}
