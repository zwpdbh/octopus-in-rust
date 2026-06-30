# 1. Burn Basics for RL

This chapter introduces the pieces of [Burn](https://github.com/tracel-ai/burn) we use to build, train, and run the build-order policy. You do not need to be a Burn expert to follow the rest of the tutorial; every later chapter references back to the concepts introduced here.

Burn is a Rust deep-learning framework. Its design is heavily typed: tensors carry their rank as a type parameter, models are plain Rust structs that derive `Module`, and gradients are computed through an `Autodiff` backend wrapper.

## Project setup

`faf-sim` depends on Burn with the `autodiff` feature enabled, and selects the compute backend through Cargo features:

```toml
# crates/faf-sim/Cargo.toml ~line 9 — backend features
[features]
default = ["cuda"]
cpu = ["burn/ndarray"]
cuda = ["burn/cuda", "burn/fusion", "burn/autotune"]
wgpu = ["burn/wgpu", "burn/fusion", "burn/autotune"]

[dependencies]
burn = { version = "0.21", default-features = false, features = ["std", "autodiff"] }
```

- `cuda` (default) selects the NVIDIA `Cuda` backend.
- `cpu` selects the CPU `NdArray` backend.
- `wgpu` selects the cross-platform `Wgpu` backend.
- `autodiff` wraps the selected backend so we can call `.backward()` on tensors.

The crate aliases the training backend for convenience:

```rust
// crates/faf-sim/src/planner/mcts/train/mod.rs ~line 36 — training backend aliases
#[cfg(feature = "cuda")]
pub type TrainBackend = Autodiff<Cuda>;
#[cfg(all(feature = "wgpu", not(feature = "cuda")))]
pub type TrainBackend = Autodiff<Wgpu>;
#[cfg(all(feature = "cpu", not(any(feature = "cuda", feature = "wgpu"))))]
pub type TrainBackend = Autodiff<NdArray>;

pub type TrainDevice = burn::tensor::Device<TrainBackend>;
```

Every tensor and model we train uses `TrainBackend`. The same recorded weights can be loaded back onto the same backend for inference inside MCTS.

CUDA is the default backend, so training on your 3090 no longer requires any extra feature flags:

```text
cargo run --release -p faf-sim-cli -- train -e 5000 -m 10000 uef novaxcenter
```

To use a different backend, disable the default feature and enable the desired one:

```text
cargo run --release -p faf-sim-cli --no-default-features --features cpu -- train -e 5000 -m 10000 uef novaxcenter
```

## Backend and Device

Burn separates the *compute backend* from the *device*.

- A `Backend` trait implementation knows how to store tensors and run ops. `NdArray` is a CPU backend; `Wgpu` would be a GPU backend.
- A `Device<Backend>` identifies where the data lives. For `NdArray` there is usually one default CPU device.

```rust
// docref: example
use burn::backend::NdArray;
use burn::tensor::Device;

let device: Device<NdArray> = Default::default();
```

Most model constructors in `faf-sim` take a `&B::Device` so they can allocate weights on the right device for the chosen backend `B`.

## Tensor

A `Tensor<B, D>` has a backend `B` and a dimensionality `D`. A 1-D vector of logits is `Tensor<B, 1>`; a single feature batch is `Tensor<B, 2>` with shape `[1, feature_count]`.

Tensors are created from data and a device:

```rust
// crates/faf-sim/src/planner/mcts/train/math.rs ~line 8 — tensor1d_from_vec
pub fn tensor1d_from_vec<B: Backend>(values: &[f32]) -> Tensor<B, 2> {
    let data = TensorData::new(values.to_vec(), [1, values.len()]);
    Tensor::<B, 2>::from_data(data, &Default::default())
}
```

Notice that `tensor1d_from_vec` returns a 2-D tensor with batch dimension `1`. This matches what Burn's `Linear` layers expect: `[batch, features]`.

Common operations we use:

- `tensor.into_data().as_slice::<f32>()` to get the values back out.
- `Tensor::cat(vec![a, b], 1)` to concatenate along the feature axis.
- `log_softmax(tensor, 0)` to turn logits into log-probabilities.
- `.select(0, index_tensor)` to pick a single element from a 1-D log-prob tensor.
- `.backward()` (only on `Autodiff` backends) to compute gradients.

## Module

A model in Burn is just a Rust struct whose fields are Burn layers, plus a `#[derive(Module)]` attribute:

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 48 — HierarchicalPolicyNet
#[derive(Module, Debug)]
pub struct HierarchicalPolicyNet<B: Backend> {
    backbone1: Linear<B>,
    backbone2: Linear<B>,
    activation: Relu,
    direction_head: Linear<B>,
    action_hidden: Linear<B>,
    action_head: Linear<B>,
    power_hidden: Linear<B>,
    power_head: Linear<B>,
    squad_hidden: Linear<B>,
    squad_head: Linear<B>,
}
```

`derive(Module)` gives the struct four superpowers:

1. **Forward methods are plain Rust.** You write `self.linear.forward(x)` just like any other method call.
2. **The struct can be moved to a device.** `module.to_device(&device)` returns a copy on the target device.
3. **The struct can be recorded to disk.** `module.record(...)` writes weights in a backend-independent format.
4. **The struct can be loaded from disk.** `HierarchicalPolicyNet::<B>::load(...)` restores weights.

Because `HierarchicalPolicyNet<B>` is generic over `B: Backend`, the same struct definition serves for training (`Autodiff<NdArray>`) and inference (`NdArray`).

## Building a layer

Burn layers are constructed with a config object. For example, a `Linear` layer is initialized from a `LinearConfig`:

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 63 — constructing linear layers
backbone1: LinearConfig::new(backbone_input, backbone_hidden).init(device),
```

`LinearConfig::new(input_dim, output_dim).init(device)` creates the weight matrix and bias on the requested device.

## Forward pass

A forward method is ordinary Rust. The only Burn-specific part is the tensor operations:

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 83 — latent backbone forward
pub(crate) fn latent(&self, features: Tensor<B, 2>) -> Tensor<B, 2> {
    let x = self.backbone1.forward(features);
    let x = self.activation.forward(x);
    let x = self.backbone2.forward(x);
    self.activation.forward(x)
}
```

The head methods take the latent vector and optional conditioning inputs (a one-hot direction, a one-hot edge, or a scalar power) and produce logits or regression outputs:

```rust
// crates/faf-sim/src/planner/mcts/macro_net.rs ~line 96 — action head
pub(crate) fn action_logits(
    &self,
    latent: Tensor<B, 2>,
    direction_one_hot: Tensor<B, 2>,
) -> Tensor<B, 2> {
    let x = Tensor::cat(vec![latent, direction_one_hot], 1);
    let x = self.action_hidden.forward(x);
    let x = self.activation.forward(x);
    self.action_head.forward(x)
}
```

## Autodiff and gradients

To train, we need gradients. Burn provides them through the `Autodiff<B>` backend wrapper. A tensor on `Autodiff<NdArray>` remembers how it was computed, so calling `.backward()` produces a gradient tape.

```rust
// crates/faf-sim/src/planner/mcts/train/trainer.rs ~line 720 — backward pass inside an update
let grads = loss.backward();
let grads = burn::optim::GradientsParams::from_grads(grads, &self.model);
self.model = self.optimizer
    .step(self.config.learning_rate.into(), self.model.clone(), grads);
```

The steps are:

1. Compute a scalar loss tensor.
2. Call `loss.backward()` to get a `Gradients` object.
3. Convert it to `GradientsParams`, which tells the optimizer which parameters to update.
4. Call `optimizer.step(lr, model, grads)` to return an updated model.

Burn's `Optimizer::step` takes the model by value and returns a new model. There is no in-place parameter mutation.

## Optimizer

Burn's `Adam` optimizer is configured with `AdamConfig` and initialized with a model reference so it knows the parameter shapes:

```rust
// crates/faf-sim/src/planner/mcts/train/trainer.rs ~line 61 — Adam optimizer setup
let optimizer = AdamConfig::new().init();
```

The optimizer is stored alongside the model in the `Trainer`:

```rust
// crates/faf-sim/src/planner/mcts/train/trainer.rs ~line 38 — optimizer type alias
type AdamOptimizer = OptimizerAdaptor<Adam, PolicyBundle<TrainBackend>, TrainBackend>;
```

At each update, the learning rate is passed as a `LearningRate` value converted from `f64`.

## Recording and loading models

Burn's `CompactRecorder` writes a model's weights to a `.mpk` file (MessagePack). Loading reconstructs the model from the same file:

```rust
// crates/faf-sim/src/planner/mcts/train/policy.rs ~line 14 — save_policy
pub fn save_policy(
    model: &PolicyBundle<TrainBackend>,
    path: &std::path::Path,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("failed to create model dir: {e}"))?;
    }
    let recorder = CompactRecorder::new();
    recorder
        .record(model.clone().into_record(), path.to_path_buf())
        .map_err(|e| format!("failed to save model: {e}"))
}
```

```rust
// crates/faf-sim/src/planner/mcts/train/policy.rs ~line 28 — load_policy
pub fn load_policy(
    path: &std::path::Path,
    num_edges: usize,
) -> Result<PolicyBundle<TrainBackend>, String> {
    let device: TrainDevice = Default::default();
    let recorder = CompactRecorder::new();
    let record = recorder
        .load(path.to_path_buf(), &device)
        .map_err(|e| format!("failed to load model: {e}"))?;
    let model = PolicyBundle::new(&device, num_edges).load_record(record);

    if model.num_edges() != num_edges {
        return Err(format!(
            "action head output dimension mismatch: expected {num_edges}, got {}; retrain the model",
            model.num_edges()
        ));
    }

    Ok(model)
}
```

Notice that `load_policy` first creates a randomly-initialized model of the correct shape, then loads the saved record into it. This is the standard Burn pattern: shape comes from the struct definition; values come from the record.

## Key takeaways

- Burn models are generic Rust structs with `#[derive(Module)]`.
- Tensors are typed by backend and rank: `Tensor<B, D>`.
- Training uses `Autodiff<B>`; inference can use the simpler `B` backend.
- Gradients are explicit: `loss.backward()`, `GradientsParams::from_grads`, and `optimizer.step`.
- Models are saved/loaded through records and a `Recorder`.

With these ideas in place, we can look at the simulator state and how we featurize it.
