//! 溢出属性训练摆烂 — 复现测试（基线 = 上游 xulai1001/umaai-rs master 镜像）
//!
//! 用户定调（09-12 原话）：体力硬守门是对的、与本问题无关——真机实观是
//! **满体力**下「不练智力双彩、选择睡觉」→ 病灶在打分层的属性上限硬门限：
//! (1) reserve_penalty 在 h=0 处按原始增益全额收 quadratic 预留罚（重复收费悬崖，
//!     单元级已实证：gain=60 → 罚 1149.23，run 34664275996）；
//! (2) 属性分凸、PT 分平 → 模型天生靠属性维，上限一满该维清零、无第二维说话；
//! (3) 超上限的训练收益没有合理建模（本仓先修 (1)，(3) 待机制确认后另 patch）。
//!
//! 场景标定记录：默认代表卡 raw gain≈60 时 PT+彩圈(≈1564)还能扛住 −1149 悬崖
//! （AI 选 Train(Wisdom) 345，run 34664275996 实测）；真机高面板卡+面倍率
//! gain 150~400 → 悬崖 −7000~−20000 → 睡觉。故场景加 current_ramen 倍率把
//! gain 推入真机量级。

use umasim::bench::seeded_rngs;
use umasim::game::ramen::policy::RamenPolicyConfig;
use umasim::game::ramen::{Operation, RamenGame, RamenStage};
use umasim::game::{Game, InheritInfo, PersonType, Trainer};
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

/// 真机场景复刻：满体力、智已满（其余四维 90%）、全卡集中智位满绊（双彩以上）、
/// 且处于吃面倍率窗口（把 raw gain 推入真机高面板量级，见文件头标定记录）
fn make_capped_shining_wisdom() -> Result<RamenGame, Box<dyn std::error::Error>> {
    let mut game = setup()?;
    game.base.turn = 20; // 第一年（pt_rate=16，属性维崩塌后 PT 更撑不起来）
    game.stage = RamenStage::Train;
    game.base.train_level_count = [0, 0, 0, 0, 16]; // 智位等级5
    game.base.distribution = vec![Vec::new(); 5];
    for (i, person) in game.persons.iter_mut().enumerate() {
        if person.person_type == PersonType::Card {
            person.friendship = 100;
            game.base.distribution[4].push(i as i32);
        }
    }
    for card in game.base.deck.iter_mut() {
        card.friendship = 100;
    }
    for idx in 0..4 {
        game.uma.five_status[idx] = game.uma.five_status_limit[idx] * 9 / 10;
    }
    game.uma.five_status[4] = game.uma.five_status_limit[4]; // 智溢出
    game.uma.vital = 100; // 满体力——与守门无关
    game.uma.motivation = 5;
    game.ramen.current_ramen = Some(2); // 面倍率窗口：真机溢出睡觉多发生于此
    Ok(game)
}

fn chosen_operation(
    game: &RamenGame,
    trainer: &RecommendedRamenTrainer,
) -> Result<Operation, Box<dyn std::error::Error>> {
    let (mut rng, _rm) = seeded_rngs(61444, 0);
    let actions = game.list_actions()?;
    let idx = trainer.select_action(game, &actions, &mut rng)?;
    println!(
        "选择: {:?} | breakdown: {:?}",
        actions[idx].operation,
        trainer.last_breakdown()
    );
    Ok(actions[idx].operation.clone())
}

/// 复现主测试：满体力+智溢出+全彩圈+面倍率，当前 preset 不得选择睡觉（修复前红）
#[test]
fn full_vital_capped_shining_wisdom_must_not_sleep() -> R {
    let game = make_capped_shining_wisdom()?;
    let trainer = RecommendedRamenTrainer::new();
    let chosen = chosen_operation(&game, &trainer)?;
    assert!(
        !matches!(chosen, Operation::Rest),
        "复现：满体力、智溢出但双彩高价值回合，AI 仍选择睡觉（上限硬门限悬崖所致）"
    );
    Ok(())
}

/// 解药测试：同局面 + rclamp（补丁 0001 v2 应用后可用）→ 必须恢复训练
#[test]
fn rclamp_full_vital_capped_shining_wisdom_trains() -> R {
    let trainer = match RecommendedRamenTrainer::with_tokens("rclamp") {
        Ok(t) => t,
        Err(e) => {
            println!("跳过：补丁未应用（{e}）");
            return Ok(());
        }
    };
    let game = make_capped_shining_wisdom()?;
    let chosen = chosen_operation(&game, &trainer)?;
    assert!(
        matches!(chosen, Operation::Train(_)),
        "rclamp 后应恢复训练（智位 PT+彩圈价值不再被悬崖淹没），实际 {chosen:?}"
    );
    Ok(())
}

/// 单元级：reserve_penalty 在 h=0 的重复收费（修复前红，已实证 1149.23）
#[test]
fn reserve_penalty_zero_for_capped_slot() -> R {
    let mut game = setup()?;
    game.base.turn = 50;
    game.uma.five_status[4] = game.uma.five_status_limit[4];

    let mut local = LocalRamenConfig::default();
    local.status_reserve_max = 40.0; // 生产 preset 值
    let trainer = LocalRamenTrainer::with_configs(RamenPolicyConfig::default(), local);

    let gain = [0i32, 0, 0, 0, 60, 0];
    let p_capped = trainer.reserve_penalty(&game, &gain);
    game.uma.five_status[4] = game.uma.five_status_limit[4] - 10;
    let p_near = trainer.reserve_penalty(&game, &gain);
    println!("已满罚分={p_capped} 剩10点罚分={p_near}");

    assert!(p_near > 0.0, "近上限应有软惩罚（模型本意）");
    assert!(
        p_capped.abs() < 1e-6,
        "复现：已满位预留罚分应为 0，实际 {p_capped}（重复收费悬崖）"
    );
    Ok(())
}
