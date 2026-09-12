/// 对照测试：智未溢出时同样局面不得选择休息（证明症状场景不是天然休息局）
///
/// 注：合成局面（空人头分布）下训练 PT 偏低，比赛面板分可能胜出——
/// 本对照只钉「不得休息」，比赛/训练之争另案（race_panel_discount 校准漂移）。
#[test]
fn uncapped_wisdom_baseline_not_rest() -> R {
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
    assert!(!matches!(chosen, Operation::Rest), "对照组不得休息，实际 {chosen:?}");
    Ok(())
}
