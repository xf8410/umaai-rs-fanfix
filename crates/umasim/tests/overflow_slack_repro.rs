//! 溢出属性训练摆烂 — 复现测试（基线 = 上游 xulai1001/umaai-rs master 镜像）
//!
//! 症状（小黑板已知问题）：属性溢出时 AI 不训练、选择休息。
//! 主嫌：`LocalRamenTrainer::reserve_penalty` 未把增益钳制到剩余空间——
//! 属性已满（h=0）时游戏会截断增益、属性根本不动，但公式按原始 gain 记
//! 「预留空间被侵占」，罚分 = 6×(gain + gain²/2r)，把已满位训练打成深度负分。
//! 溢出浪费已由 `status_gain` 截断计价一次，此处属重复收费。
//!
//! 本文件在修复前预期为红（复现），修复后应全绿。
//! 注：集成测试 crate 只能引用 umasim 公开面（不直接引 anyhow/rand），
//! RNG 经 `umasim::bench::seeded_rngs` 取得。

use umasim::bench::seeded_rngs;
use umasim::game::ramen::policy::RamenPolicyConfig;
use umasim::game::ramen::{Operation, RamenGame, RamenStage};
use umasim::game::{Game, InheritInfo, Trainer};
use umasim::gamedata::init_global;
use umasim::trainer::local_ramen_trainer::{
    LocalRamenConfig, LocalRamenTrainer, RecommendedRamenTrainer,
};
use umasim::utils::get_workspace_root;

type R = Result<(), Box<dyn std::error::Error>>;

const DECK: [u32; 6] = [302424, 302894, 303044, 302924, 303024, 303054];

fn setup() -> Result<RamenGame, Box<dyn std::error::Error>> {
    std::env::set_current_dir(get_workspace_root()?)?;
    let _ = init_global();
    let inherit = InheritInfo {
        blue_count: [15, 3, 0, 0, 0],
        extra_count: [0, 30, 0, 0, 30, 30],
    };
    Ok(RamenGame::newgame(102601, &DECK, inherit)?)
}

/// 守门测试：属性已满的训练位，预留罚分必须为 0（增益被游戏截断，无空间可侵占）
#[test]
fn reserve_penalty_zero_for_capped_slot() -> R {
    let mut game = setup()?;
    game.base.turn = 50;
    game.uma.five_status[4] = game.uma.five_status_limit[4]; // 智已满

    let mut local = LocalRamenConfig::default();
    local.status_reserve_max = 40.0; // 生产 preset 值（LocalRamenConfig::default 为 0=关闭）
    let trainer = LocalRamenTrainer::with_configs(RamenPolicyConfig::default(), local);

    let gain = [0i32, 0, 0, 0, 60, 0];
    let p_capped = trainer.reserve_penalty(&game, &gain);
    println!("智已满 reserve_penalty(raw gain=60) = {p_capped}");

    // 对照：未满但在预留区内，罚分应为正（模型本意）
    game.uma.five_status[4] = game.uma.five_status_limit[4] - 10;
    let p_near = trainer.reserve_penalty(&game, &gain);
    println!("智剩10点 reserve_penalty = {p_near}");

    assert!(p_near > 0.0, "近上限应有软惩罚（模型本意）");
    assert!(
        p_capped.abs() < 1e-6,
        "复现：属性已满时增益被截断，预留罚分应为 0，实际 {p_capped}（重复收费）"
    );
    Ok(())
}

/// 症状测试：智已满、其余四维均在 50%（远未溢出）、体力充足时，
/// 生产 preset 不得选择休息——应继续训练（其他位或吃 PT）。
#[test]
fn capped_wisdom_with_healthy_vital_must_not_rest() -> R {
    let mut game = setup()?;
    game.base.turn = 50; // 第三年
    game.stage = RamenStage::Train;
    for i in 0..5 {
        game.uma.five_status[i] = game.uma.five_status_limit[i] / 2;
    }
    game.uma.five_status[4] = game.uma.five_status_limit[4]; // 智溢出
    game.uma.motivation = 5;

    let trainer = RecommendedRamenTrainer::new();
    let (mut rng, _rule_master) = seeded_rngs(61444, 0);
    let mut failures = Vec::new();

    for vital in [80, 70, 60, 50, 45, 42] {
        game.uma.vital = vital;
        let actions = game.list_actions()?;
        let idx = trainer.select_action(&game, &actions, &mut rng)?;
        let chosen = actions[idx].operation.clone();
        let breakdown = trainer.last_breakdown().unwrap_or_default();
        println!("vital={vital} 选择={chosen:?}\n  breakdown: {breakdown}");
        if matches!(chosen, Operation::Rest) {
            failures.push(vital);
        }
    }

    assert!(
        failures.is_empty(),
        "复现：智溢出但其他位健康、体力充足时仍选择休息的 vital 档: {failures:?}"
    );
    Ok(())
}

/// 对照测试：智未溢出时同样局面必须训练（证明症状场景不是天然休息局）
#[test]
fn uncapped_wisdom_baseline_trains() -> R {
    let mut game = setup()?;
    game.base.turn = 50;
    game.stage = RamenStage::Train;
    for i in 0..5 {
        game.uma.five_status[i] = game.uma.five_status_limit[i] / 2;
    }
    game.uma.motivation = 5;
    game.uma.vital = 70;

    let trainer = RecommendedRamenTrainer::new();
    let (mut rng, _rule_master) = seeded_rngs(61444, 0);
    let actions = game.list_actions()?;
    let idx = trainer.select_action(&game, &actions, &mut rng)?;
    let chosen = actions[idx].operation.clone();
    println!("对照（智未溢出）选择: {chosen:?}");
    assert!(matches!(chosen, Operation::Train(_)), "对照组应训练，实际 {chosen:?}");
    Ok(())
}
