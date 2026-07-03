use crate::planner::core::Goal;
use crate::planner::mcts::features::{SHORTFALL_FEATURE_COUNT, STATE_FEATURE_COUNT};
use crate::planner::mcts::train::{load_policy, save_policy, TrainConfig, TrainDevice, Trainer};
use crate::units::{TechLevel, Units};

fn load_units() -> Units {
    let json = include_str!("../../../../../../plugins/faf-units/data/faf_units.json");
    Units::new(serde_json::from_str(json).expect("embedded index should parse"))
}

#[test]
fn trainer_runs_episodes_without_panic() {
    let units = load_units();
    let goal = Goal {
        tech_level: TechLevel::T4,
        mass_cost: 28_000.0,
        energy_cost: 340_000.0,
        build_time: 46_250.0,
    };
    let mut trainer = Trainer::new(TrainConfig {
        episodes: 3,
        max_steps: 50,
        ..Default::default()
    });

    let stats = trainer.train(&units, &goal);

    assert_eq!(stats.episode_lengths.len(), 3);
}

#[test]
fn save_and_load_policy_round_trip() {
    let units = load_units();
    let goal = Goal {
        tech_level: TechLevel::T4,
        mass_cost: 28_000.0,
        energy_cost: 340_000.0,
        build_time: 46_250.0,
    };
    let mut trainer = Trainer::new(TrainConfig {
        episodes: 2,
        max_steps: 20,
        ..Default::default()
    });
    trainer.train(&units, &goal);
    let model = trainer.into_model();

    let dir = std::env::temp_dir().join("faf-sim-train-test");
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("test-policy");

    save_policy(&model, &path).expect("save should succeed");
    let loaded = load_policy(&path).expect("load should succeed");

    let device: TrainDevice = Default::default();
    let dummy = vec![0.0f32; STATE_FEATURE_COUNT + SHORTFALL_FEATURE_COUNT];
    let before = model.evaluate_direction(dummy.clone(), &device);
    let after = loaded.evaluate_direction(dummy, &device);
    assert_eq!(before.len(), after.len());
    for (a, b) in before.iter().zip(after.iter()) {
        assert!((a - b).abs() < 1e-3, "loaded model outputs should match");
    }
}
