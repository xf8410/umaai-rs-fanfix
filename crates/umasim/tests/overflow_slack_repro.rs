//! 溢出属性训练摆烂 — 复现报告（v2，上游 AGENTS.md 合规：println 报红绿，不用 assert 宏，release 跑）
//!
//! 红绿证据以 "VERDICT <id> RED|GREEN|SKIP ..." 行打印；CI 步骤 tee 存报告上传 artifact，
//! 不再拿 panic 当门（红证据由报告文字呈现，文档落盘引用 run id）。
//!
//! 背景定调（用户 09-12 原话）：体力硬守门是对的、与本问题无关——真机实观是
//! **满体力**下「不练智力双彩、选择睡觉」→ 病灶在打分层的属性上限硬门限：
//! (1) reserve_penalty 在 h=0 处按原始增益全额收 quadratic 预留罚（重复收费悬崖，
//!     单元级已实证：gain=60 → 罚 1149.23，run 34664275996）；
//! (2) 属性分凸、PT 分平 → 模型天生靠属性维，上限一满该维清零、无第二维说话；
//! (3) 超上限的训练收益没有合理建模（先修 (1)，(3) 由 0004 期权化承接）。
//!
//! 场景标定记录：默认卡 gain≈60 太温和；真机高面板+面倍率 gain 150~400 → 加
//! current_ramen=Some(2)；turn=20 撞比赛回合会假绿 → 用非赛期 turn=21。

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
/// 吃面倍率窗口（gain 推入真机量级）、非赛期回合（排除比赛吸收效应）
fn make_capped_shining_wisdom() -> Result<RamenGame, Box<dyn std::error::Error>> {
    let mut game = setup()?;
    game.base.turn = 21; // 第一年非赛期回合（pt_rate=16，属性维崩塌后 PT 撑不起来）
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

/// 主场景：满体力+智溢出+全彩圈+面倍率。睡觉=RED（悬崖吃掉最优位）；训练=GREEN。
/// 注：当前 master 有 140 分回退门兜底，预期 GREEN——这不是无罪，是危害被回退门
/// 吸走后只剩 margin≤140 区的位次误导（见 cliff_readings 与 ab-results 文档）。
#[test]
fn full_vital_capped_shining_wisdom_must_not_sleep() -> R {
    let game = make_capped_shining_wisdom()?;
    let trainer = RecommendedRamenTrainer::new();
    let chosen = chosen_operation(&game, &trainer)?;
    if matches!(chosen, Operation::Rest) {
        println!("VERDICT must_not_sleep RED 满体力智溢出仍选睡觉（上限硬门限悬崖所致）");
    } else {
        println!("VERDICT must_not_sleep GREEN 当前 preset 未睡（140 回退门兜底，位次误导见 cliff_readings）");
    }
    Ok(())
}

/// 解药场景：同局面 + rclamp（0001 v2 应用后可用）。恢复训练=GREEN；补丁未应用=SKIP。
#[test]
fn rclamp_full_vital_capped_shining_wisdom_trains() -> R {
    let trainer = match RecommendedRamenTrainer::with_tokens("rclamp") {
        Ok(t) => t,
        Err(e) => {
            println!("VERDICT rclamp_trains SKIP 补丁未应用（{e}）");
            return Ok(());
        }
    };
    let game = make_capped_shining_wisdom()?;
    let chosen = chosen_operation(&game, &trainer)?;
    if matches!(chosen, Operation::Train(_)) {
        println!("VERDICT rclamp_trains GREEN rclamp 后恢复训练（PT+彩圈价值不再被悬崖淹没）");
    } else {
        println!("VERDICT rclamp_trains RED rclamp 后仍未训练，实际 {chosen:?}");
    }
    Ok(())
}

/// 单元读数：reserve_penalty 在 h=0 的重复收费悬崖。p_capped≈0=GREEN（已修/已关），
/// 显著为正=RED（悬崖在位）。近上限罚分 p_near 应恒为正（模型本意，打印供核对）。
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
    println!("读数: 已满罚分={p_capped} 剩10点罚分={p_near}（近上限罚分为正=模型本意）");
    if p_capped.abs() < 1e-6 {
        println!("VERDICT cliff_readings GREEN 已满位预留罚分=0（钳制在位或门控关闭且已默认修）");
    } else {
        println!("VERDICT cliff_readings RED 已满位预留罚分={p_capped}≠0（重复收费悬崖在位，默认关=preset 含 bug）");
    }
    Ok(())
}
