//! 溢出摆烂 A/B 整局基准（确定性种子，修复前后各跑一次对照）
//!
//! 口径：7 build × 10 seed（base 61444），RecommendedRamenTrainer 手写策略整局，
//! 统计终局分、Train 阶段休息次数（action_desc 含「休息」）、比赛次数。
//! 本文件不断言数值，只打印表格；A/B 结论由两次 CI run 的表格对比得出。
//! 上游验证纪律：同种子配对、逐字段可复现；改策略打分 → 决策轨迹变 → 基线作废。

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

    let mut totals: std::collections::BTreeMap<String, (f64, usize, usize, usize)> =
        std::collections::BTreeMap::new();

    for build in &builds {
        let deck_ids = build.build_deck(&representatives.picked, FRIEND)?;
        for i in 0..RUNS {
            let seed = BASE_SEED + i;
            let (mut rng, rule_master) = seeded_rngs(BASE_SEED, i);
            let mut game = RamenGame::newgame(UMA, &deck_ids, inherit.clone())?;
            game.set_rule_master(rule_master);
            let trainer = RecommendedRamenTrainer::new();
            let logged = LoggingTrainer::new(trainer, seed);
            game.run_full_game(&logged, &mut rng)?;
            let log = logged.take_records();
            let rests = log
                .rows
                .iter()
                .filter(|r| r.stage == "Train" && r.action_desc.contains("休息"))
                .count();
            let races = log
                .rows
                .iter()
                .filter(|r| r.stage == "Train" && r.action_desc.contains("比赛"))
                .count();
            let score = game.uma.calc_score();
            println!("{} seed={seed} score={score} rests={rests} races={races}", build.name());
            let e = totals.entry(build.name().to_string()).or_insert((0.0, 0, 0, 0));
            e.0 += score as f64;
            e.1 += rests;
            e.2 += races;
            e.3 += 1;
        }
    }

    println!("\n===== 汇总（{RUNS} 局/build）=====");
    let mut g_score = 0.0;
    let mut g_rest = 0usize;
    let mut g_race = 0usize;
    for (name, (s, r, c, n)) in &totals {
        println!("{name}: mean_score={:.0} rests_total={r} races_total={c}", s / *n as f64);
        g_score += s;
        g_rest += r;
        g_race += c;
    }
    let games: f64 = totals.values().map(|(_, _, _, n)| *n as f64).sum();
    println!(
        "TOTAL: mean_score={:.1} rests_per_game={:.2} races_per_game={:.2} games={}",
        g_score / games,
        g_rest as f64 / games,
        g_race as f64 / games,
        games as u64
    );
    assert!(games > 0.0);
    Ok(())
}
