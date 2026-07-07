# 1. Burn Basics for RL

This chapter introduces the pieces of [Burn](https://github.com/tracel-ai/burn) we use to build, train, and run the build-order policy. You do not need to be a Burn expert to follow the rest of the tutorial; every later chapter references back to the concepts introduced here.

Burn is a Rust deep-learning framework. Its design is heavily typed: tensors carry their rank as a type parameter, models are plain Rust structs that derive `Module`, and gradients are computed through an `Autodiff` backend wrapper.

## Project setup

`faf-sim` depends on Burn with the `autodiff` feature enabled, and selects the compute backend through Cargo features:

```toml
# crates/faf-sim/Cargo.toml ~line 9 — backend features
[features]
default = ["cpu"]
cpu = ["burn/ndarray"]
cuda = ["burn/cuda", "burn/fusion", "burn/autotune"]
wgpu = ["burn/wgpu", "burn/fusion", "burn/autotune"]

[dependencies]
burn = { version = "0.21", default-features = false, features = ["std", "autodiff"] }
```

- `cpu` (default for the library) selects the CPU `NdArray` backend.
- `cuda` selects the NVIDIA `Cuda` backend.
- `wgpu` selects the cross-platform `Wgpu` backend.
- `autodiff` wraps the selected backend so we can call `.backward()` on tensors.

The crate aliases the training backend for convenience:

```rust
// crates/faf-sim/src/planner/policy/train/mod.rs ~line 23 — training backend aliases
#[cfg(feature = "cpu")]
pub type TrainBackend = Autodiff<NdArray>;
#[cfg(all(feature = "cuda", not(feature = "cpu")))]
pub type TrainBackend = Autodiff<Cuda>;
#[cfg(all(feature = "wgpu", not(any(feature = "cpu", feature = "cuda"))))]
pub type TrainBackend = Autodiff<Wgpu>;

pub type TrainDevice = burn::tensor::Device<TrainBackend>;
```

Every tensor and model we train uses `TrainBackend`. The same recorded weights can be loaded back onto the same backend for inference during simulation.

The `faf-sim-cli` package defaults to the CUDA backend, so training on a GPU does not require extra feature flags:

```text
cargo run --release -p faf-sim-cli -- train -e 5000 -m 10000 uef novaxcenter
```

To use a different backend, disable the default feature and enable the desired one:

```text
cargo run --release -p faf-sim-cli --no-default-features --features cpu -- train -e 5000 -m 10000 uef novaxcenter
```

When working with the `faf-sim` library directly (for example in tests), the default backend is CPU.

## Backend and Device

Burn separates the *compute backend* from the *device*.

- A `Backend` trait implementation knows how to store tensors and run ops. `NdArray` is a CPU backend; `Cuda` and `Wgpu` are GPU backends.
- A `Device<Backend>` identifies where the data lives. For `NdArray` there is usually one default CPU device; for GPU backends there may be several.

```rust
// docref: example
use burn::backend::NdArray;
use burn::tensor::Device;

let device: Device<NdArray> = Default::default();
```

Most model constructors in `faf-sim` take a `&B::Device` so they can allocate weights on the right device for the chosen backend `B`.

## Tensor and TensorData

A `Tensor<B, D>` has a backend `B` and a dimensionality `D`. A 1-D vector of logits is `Tensor<B, 1>`; a single feature batch is `Tensor<B, 2>` with shape `[1, feature_count]`.

Tensors are created from data and a device using `TensorData`:

```rust
// crates/faf-sim/src/planner/policy/train/math.rs ~line 8 — tensor1d_from_vec
pub(crate) fn tensor1d_from_vec(features: &[f32]) -> Tensor<TrainBackend, 2> {
    let device: TrainDevice = Default::default();
    let data = TensorData::new(features.to_vec(), [1, features.len()]);
    Tensor::<TrainBackend, 2>::from_data(data, &device)
}
```

`TensorData::new(values, shape)` bundles host data with its desired shape. `Tensor::from_data` moves it onto the device and returns a typed tensor. Notice that `tensor1d_from_vec` returns a 2-D tensor with batch dimension `1`. This matches what Burn's `Linear` layers expect: `[batch, features]`.

Common operations we use:

- `tensor.into_data().as_slice::<f32>()` to get the values back out.
- `log_softmax(tensor, 0)` to turn logits into log-probabilities.
- `.select(0, index_tensor)` to pick a single element from a 1-D log-prob tensor.
- `.backward()` (only on `Autodiff` backends) to compute gradients.

## Module

A model in Burn is just a Rust struct whose fields are Burn layers, plus a `#[derive(Module)]` attribute:

```rust
// crates/faf-sim/src/planner/policy/macro_net.rs ~line 49 — HierarchicalPolicyNet
#[derive(Module, Debug)]
pub struct HierarchicalPolicyNet<B: Backend> {
    backbone1: Linear<B>,
    backbone2: Linear<B>,
    activation: Relu,
    direction_head: Linear<B>,
}
```

`derive(Module)` gives the struct four superpowers:

1. **Forward methods are plain Rust.** You write `self.linear.forward(x)` just like any other method call.
2. **The struct can be moved to a device.** `module.to_device(&device)` returns a copy on the target device.
3. **The struct can be recorded to disk.** `module.record(...)` writes weights in a backend-independent format.
4. **The struct can be loaded from disk.** `HierarchicalPolicyNet::<B>::load(...)` restores weights.

Because `HierarchicalPolicyNet<B>` is generic over `B: Backend`, the same struct definition serves for training (`Autodiff<NdArray>`) and inference (`NdArray`). During planning we hide the concrete type behind a trait object so the rest of the crate does not need to know which backend is in use:

```rust
// crates/faf-sim/src/planner/policy/value_net.rs ~line 17 — ValueNet trait
pub trait ValueNet: std::fmt::Debug + Send + Sync {
    fn evaluate_direction(&self, features: Vec<f32>) -> Vec<f32>;
    fn clone_box(&self) -> Box<dyn ValueNet>;
}
```

## Building a layer

Burn layers are constructed with a config object. For example, a `Linear` layer is initialized from a `LinearConfig`:

```rust
// crates/faf-sim/src/planner/policy/macro_net.rs ~line 80 — constructing linear layers
backbone1: LinearConfig::new(backbone_input, backbone_hidden).init(device),
```

`LinearConfig::new(input_dim, output_dim).init(device)` creates the weight matrix and bias on the requested device.

## Forward pass

A forward method is ordinary Rust. The only Burn-specific part is the tensor operations:

```rust
// crates/faf-sim/src/planner/policy/macro_net.rs ~line 120 — latent backbone forward
pub(crate) fn latent(&self, features: Tensor<B, 2>) -> Tensor<B, 2> {
    let x = self.backbone1.forward(features);
    let x = self.activation.forward(x);
    let x = self.backbone2.forward(x);
    self.activation.forward(x)
}
```

The direction head consumes the latent vector and produces logits over the six strategic directions:

```rust
// crates/faf-sim/src/planner/policy/macro_net.rs ~line 137 — direction head
pub(crate) fn direction_logits(&self, latent: Tensor<B, 2>) -> Tensor<B, 2> {
    self.direction_head.forward(latent)
}
```

For inference we provide a helper that takes a host `Vec<f32>` and returns host `Vec<f32>`:

```rust
// crates/faf-sim/src/planner/policy/macro_net.rs ~line 155 — evaluate_direction
pub fn evaluate_direction(&self, features: Vec<f32>, device: &B::Device) -> Vec<f32> {
    // 1. Input vector as a batched tensor: [1, STATE_FEATURE_COUNT].
    let features = tensor_from_vec(&features, device);

    // 2. Backbone: matrix multiply → ReLU → matrix multiply → ReLU.
    let latent = self.latent(features);

    // 3. Direction head: final matrix multiply → [1, DIRECTION_COUNT].
    let logits = self.direction_logits(latent);

    // 4. Convert back to a host-side Vec<f32>.
    logits.into_data().as_slice::<f32>().unwrap().to_vec()
}
```

### What `evaluate_direction` is actually doing

If you are new to ML frameworks, it helps to think of a trained network as a fixed pipeline of math operations, not a black box. The network has already learned its weight matrices and biases during training; `evaluate_direction` just multiplies the input through them.

The high-level flow is:

```text
input vector → matrix multiply → ReLU → matrix multiply → ReLU → matrix multiply → output vector
```

Here is exactly how that maps to the code, with the tensor shape after each step:

| English | Code | Shape |
|---|---|---|
| input vector | `tensor_from_vec(&features, device)` | `[1, 11]` |
| matrix multiply | `self.backbone1.forward(features)` | `[1, 11]` → `[1, 128]` |
| ReLU | `self.activation.forward(x)` | `[1, 128]` |
| matrix multiply | `self.backbone2.forward(x)` | `[1, 128]` → `[1, 64]` |
| ReLU | `self.activation.forward(x)` | `[1, 64]` |
| matrix multiply | `self.direction_head.forward(latent)` | `[1, 64]` → `[1, 6]` |
| output vector | `logits.into_data().as_slice::<f32>().unwrap().to_vec()` | 6 floats |

A few things worth pointing out:

- **Why `[1, 11]`?** The first dimension is the batch size. During policy inference we evaluate one game state at a time, so the batch is `1`. The second dimension is `STATE_FEATURE_COUNT` (11 state features).
- **`Linear::forward` is matrix multiplication plus a bias.** When you see `self.backbone1.forward(x)`, Burn is computing `x @ W + b` using the weight matrix `W` and bias vector `b` that were created when the layer was initialized.
- **`self.activation.forward` is ReLU.** The `activation` field holds a `Relu`, so each call zeros out negative values but does not change the tensor's shape.
- **The final `.to_vec()` converts back to normal Rust data.** ML frameworks operate on tensors, but the rest of our planner works with ordinary `Vec<f32>`, so we pull the six logit values out of the tensor before returning.

So the full chain expands to:

```rust
// crates/faf-sim/src/planner/policy/macro_net.rs ~line 155 — evaluate_direction (expanded view)
pub fn evaluate_direction(&self, features: Vec<f32>, device: &B::Device) -> Vec<f32> {
    // 1. input vector [1, 11]
    let tensor = tensor_from_vec(&features, device);

    // 2. latent() = matrix multiply → ReLU → matrix multiply → ReLU
    //    produces [1, 64]
    let latent = self.latent(tensor);

    // 3. direction_logits() = final matrix multiply
    //    produces [1, 6]
    let logits = self.direction_logits(latent);

    // 4. output vector: 6 floats
    logits.into_data().as_slice::<f32>().unwrap().to_vec()
}
```

And `latent()` itself expands to the first four operations:

```rust
// crates/faf-sim/src/planner/policy/macro_net.rs ~line 120 — latent (operation-by-operation view)
pub(crate) fn latent(&self, features: Tensor<B, 2>) -> Tensor<B, 2> {
    let x = self.backbone1.forward(features);   // matrix multiply: [1, 11] → [1, 128]
    let x = self.activation.forward(x);          // ReLU: shape stays [1, 128]
    let x = self.backbone2.forward(x);           // matrix multiply: [1, 128] → [1, 64]
    self.activation.forward(x)                   // ReLU: shape stays [1, 64]
}
```

## Masking illegal actions

In RL the set of legal actions changes every step. We implement action masking by adding a large negative value to the logits of illegal directions before the softmax:

```rust
// crates/faf-sim/src/planner/policy/macro_net.rs ~line 30 — mask value
pub(crate) const MASK_VALUE: f32 = -1e9;
```

```rust
// crates/faf-sim/src/planner/policy/macro_net.rs ~line 163 — apply_mask
pub fn apply_mask(logits: &mut [f32], mask: &[bool]) {
    for (l, legal) in logits.iter_mut().zip(mask.iter()) {
        if !legal {
            *l = MASK_VALUE;
        }
    }
}
```

After masking, `softmax` assigns near-zero probability to illegal directions. During training we apply the same mask inside the loss computation, so gradients never push the network toward illegal choices.

## Autodiff and gradients

To train, we need gradients. Burn provides them through the `Autodiff<B>` backend wrapper. A tensor on `Autodiff<NdArray>` remembers how it was computed, so calling `.backward()` produces a gradient tape.

```rust
// crates/faf-sim/src/planner/policy/train/trainer/update.rs ~line 93 — backward pass
let grads = loss.backward();
let grads = burn::optim::GradientsParams::from_grads(grads, &self.model);
self.model = self
    .optimizer
    .step(self.config.learning_rate, self.model.clone(), grads);
```

The steps are:

1. Compute a scalar loss tensor.
2. Call `loss.backward()` to get a `Gradients` object.
3. Convert it to `GradientsParams`, which tells the optimizer which parameters to update.
4. Call `optimizer.step(lr, model, grads)` to return an updated model.

Burn's `Optimizer::step` takes the model by value and returns a new model. There is no in-place parameter mutation.

## Optimizer

Burn's `Adam` optimizer is configured with `AdamConfig` and initialized with gradient clipping if desired:

```rust
// crates/faf-sim/src/planner/policy/train/trainer/core.rs ~line 51 — Adam optimizer setup
let optimizer = {
    let adam = AdamConfig::new();
    let adam = if let Some(clip) = config.grad_clip {
        adam.with_grad_clipping(Some(GradientClippingConfig::Norm(clip)))
    } else {
        adam
    };
    adam.init()
};
```

The concrete optimizer type is an alias:

```rust
// crates/faf-sim/src/planner/policy/train/trainer/core.rs ~line 19 — optimizer type alias
pub type AdamOptimizer = OptimizerAdaptor<Adam, PolicyBundle<TrainBackend>, TrainBackend>;
```

At each update, the learning rate is passed as a `f64` (Burn converts it internally).

## Recording and loading models

Burn's `CompactRecorder` writes a model's weights to a `.mpk` file (MessagePack). Loading reconstructs the model from the same file:

```rust
// crates/faf-sim/src/planner/policy/train/policy_training.rs ~line 21 — save_policy
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
// crates/faf-sim/src/planner/policy/train/policy_training.rs ~line 35 — load_policy
pub fn load_policy(path: &std::path::Path) -> Result<PolicyBundle<TrainBackend>, String> {
    let device: TrainDevice = Default::default();
    let recorder = CompactRecorder::new();
    let record = recorder
        .load(path.to_path_buf(), &device)
        .map_err(|e| format!("failed to load model: {e}"))?;
    let model = PolicyBundle::new(&device).load_record(record);
    Ok(model)
}
```

`into_record` turns a `Module` into a serializable record of its weights. `load_record` restores those weights into a freshly constructed model. Because the record format is backend-agnostic, a model trained on `Autodiff<Cuda>` can be loaded onto `NdArray` for CPU inference (the weights are the same; only the compute backend changes).

## What's next

With these Burn pieces in place, the next chapter shows how we model the FAF environment so the network has something meaningful to consume.
