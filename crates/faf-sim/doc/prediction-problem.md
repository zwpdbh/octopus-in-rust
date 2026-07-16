# Build-Time Prediction: Problem Summary

## Current approach

The predictor in `crates/faf-build-prediction` estimates how long a build plan takes to finish.

```rust
// crates/faf-build-prediction/src/model/predictor.rs ~line 15 — EcoPredictorConfig
#[derive(Config, Debug)]
pub struct EcoPredictorConfig {
    #[config(default = 128)]
    pub hidden_size: usize,
    #[config(default = 0.0)]
    pub dropout: f64,
}
```

- **Input**: a sequence of up to 10 `BuildTask`s, each described by a 27-dimensional feature vector.
- **Features**: initial economy snapshot, per-task aggregates (build power, costs, production, storage), and cumulative economy contributions from earlier tasks.
- **Target**: `log(completion_time)`.
- **Loss**: time-weighted MSE, `weight = raw_time^{-time_weight_power}`, so rare fast plans are not drowned out by slow plans.
- **Model**: single-layer LSTM with a linear output head.

The simulation provides exact labels by running each generated plan to completion, so label noise is not the issue.

## Symptom

Predictions are inaccurate, especially for the fast/practical plans that matter most. The model tends to over-predict completion time for feasible plans and does not reliably distinguish practical from non-practical build orders.

## Root cause: the training data does not look like real FAF plans

The biggest bottleneck is dataset generation in `crates/faf-build-prediction/src/data/generator.rs`.

```rust
// crates/faf-build-prediction/src/data/generator.rs ~line 140 — generate_sample
fn generate_sample<R: Rng>(&self, rng: &mut R) -> EcoPlanSample<Unsimulated> {
    let initial_eco = self.sample_initial_eco(rng);
    let task_count = rng.random_range(1..=self.config.max_tasks.max(1));
    let plan: Vec<BuildTask> = (0..task_count)
        .map(|id| self.sample_build_task(rng, id as u32))
        .collect();

    EcoPlanSample::new(initial_eco, plan)
}
```

Problems with this generator:

1. **Builders and targets are sampled independently.** A plan can contain a T3 engineer trying to build a Monkeylord right after the ACU, or a factory trying to assist an ACU upgrade. The model learns from physically or strategically invalid combinations.
2. **Initial economy is independent of the plan.** A plan that requires a T2 economy can be paired with a T0 starting income, and vice versa. The model sees label/time pairs that could never occur together in a real game.
3. **No tech progression or build-order structure.** Real FAF plans have stages: ACU → engineers → factory → units → tech → goal. Random sampling has no such structure.
4. **Severe class imbalance.** Roughly 94% of generated plans are "not practical" (slower than the 600-second threshold) and only ~6% are practical. Time-weighting helps, but the model still sees very few examples of good plans.
5. **Not-practical labels are capped.** Plans that exceed the time limit are labeled with a capped time, which creates a ceiling effect and gives the model ambiguous targets near the cap.

Because the data distribution is so different from real build orders, the model is learning to interpolate between random nonsense rather than to reason about FAF strategy.

## Why architecture and loss are secondary

The current choices are reasonable:

- Predicting `log(time)` is correct because completion times span orders of magnitude and relative error matters more than absolute seconds.
- Time-weighted MSE is a sensible way to handle imbalance.
- A 128-unit LSTM is a perfectly adequate baseline.

With realistic data these choices would likely work well. Without it, no architecture or loss function can invent the structure of real FAF play.

## Recommended next steps (in order of impact)

### 1. Fix dataset generation (highest impact)

Generate plans that resemble real build orders:

- **Start from a realistic initial economy** (ACU-level income and storage) instead of sampling independently.
- **Enforce progression.** Sample plans in stages: economy first, then production, then tech, then the goal. Allow randomness within each stage.
- **Validate builder-target compatibility.** Reject plans where the assigned builders cannot build the target or where the plan is impossible to start with the given initial economy.
- **Oversample practical plans.** Either reject slow samples or bias generation toward cheaper, faster build orders until the dataset is closer to balanced.

### 2. Measure properly (essential)

The training loop currently only reports `LossMetric`.

```rust
// crates/faf-build-prediction/src/train.rs ~line 84 — SupervisedTraining setup
let training = SupervisedTraining::new(artifact_dir, dataloader_train, dataloader_valid)
    .metrics((LossMetric::new(),))
```

Add validation metrics that reveal real prediction quality:

- MAE / median absolute error on raw predicted time.
- Relative error `|pred - true| / true`.
- Percentage of predictions within 10%, 25%, and 50% of the true time.
- Practical/not-practical classification accuracy, precision, and recall.
- Per-time-bucket error, so you know whether fast or slow plans are worse.

Also stratify the train/validation split by practical/not-practical and by time buckets, rather than splitting randomly.

### 3. Add a practical/not-practical classification head

The downstream decision is usually "is this plan feasible?" rather than "what is the exact completion time?". Add a second output:

- regression head: `log(time)`
- classification head: `Practical / NotPractical`

Train both heads with a combined loss. The shared LSTM representations will learn faster because the classification signal is much stronger than the exact-time signal.

### 4. Enrich features once the data is better

After fixing generation, consider adding:

- **Unit embeddings** or one-hot identifiers, because the difference between units is not fully captured by cost/ production numbers.
- **Ratio features** such as `mass_cost / mass_income`, `build_power / mass_cost`, and `energy_cost / energy_income`.
- **Builder-target compatibility flags** and explicit tech-level progression features.

### 5. Training refinements

- Use **Huber loss or MAE** instead of MSE to reduce the influence of outliers.
- Apply **label smoothing** to capped not-practical samples.
- Add **learning-rate scheduling** and early stopping based on validation relative error.

## Conclusion

The prediction problem is primarily a data problem. The most effective single action is to replace random plan generation with structured, realistic build-order generation and to add proper validation metrics. Model and loss improvements should come after the dataset contains the patterns the model is meant to learn.
