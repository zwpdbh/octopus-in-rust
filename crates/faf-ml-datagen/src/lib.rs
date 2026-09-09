//! faf-ml-datagen — synthetic training data for FAF strategic-icon detection.
//!
//! Instead of hand-labeling screenshots, composite the game's own
//! strategic-icon sprites onto crops of real (empty-terrain) screenshots.
//! Bounding boxes are known BY CONSTRUCTION, so every generated image comes
//! with perfect labels.
//!
//! Pipeline:
//!   sprites (DDS, 36×40 line art) → tint with a team color → scale to the
//!   on-screen size range → alpha-blend onto a random screenshot crop →
//!   record (class, x, y, w, h) per pasted sprite
//!
//! This crate is pure generation logic; it used to be the `faf-datagen` CLI.
//! Now `faf-ml-server` drives it: `POST /api/datagen` runs [`generate`] in a
//! background job, and the `on_sample` callback streams each sample into the
//! platform store (PNG + `LabeledBox` JSON — the store's JSON labels are the
//! single label format; the old YOLO text output was dropped with the CLI).
//!
//! ⚠ Domain-gap reminder: after generating, open a synthetic sample AND a
//! real screenshot side by side — the synthetic icons must match the real
//! render in SIZE, COLOR, and edge sharpness, or the model learns the wrong
//! object. And keep FUTURE screenshots held out as the real test set — never
//! train on the only real data you have.

use std::fs;
use std::io::Cursor;
use std::path::Path;

use anyhow::{Context, Result};
use image::imageops::{crop_imm, overlay, resize, FilterType};
use image::{Rgba, RgbaImage};
use rand::rngs::StdRng;
use rand::seq::IndexedRandom;
use rand::{RngExt, SeedableRng}; // rand 0.10: random_range/random_bool live on RngExt

pub use faf_ml_core::DatagenConfig;

/// Team colors seen on the strategic map (approximate — tune against real
/// screenshots during the domain-gap check).
const TEAM_COLORS: [Rgba<u8>; 6] = [
    Rgba([240, 240, 240, 255]), // white / own
    Rgba([80, 220, 80, 255]),   // green (ally)
    Rgba([190, 80, 220, 255]),  // purple (enemy)
    Rgba([80, 200, 220, 255]),  // cyan
    Rgba([230, 210, 60, 255]),  // yellow
    Rgba([200, 200, 200, 255]), // grey (neutral)
];

/// One strategic-icon sprite: the `_rest` variant of one class.
pub struct Sprite {
    pub class_name: String,
    pub img: RgbaImage, // 36×40 with alpha
}

/// A placed unit: class NAME + pixel bounding box (absolute, on the sample).
/// The server maps these directly to platform `LabeledBox`es — no YOLO
/// roundtrip.
pub struct GenBox {
    pub class_name: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Loads every `icon_*_rest.dds` (plus suffix-less strategic icons), decoding
/// DDS → RGBA8. Class name = filename minus `icon_` prefix and state suffix:
/// `icon_bomber1_directfire_rest.dds` → `bomber1_directfire`.
pub fn load_sprites(dir: &Path) -> Result<Vec<Sprite>> {
    let mut sprites = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {dir:?}"))? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.starts_with("icon_") || !name.ends_with(".dds") {
            continue;
        }
        let base = &name["icon_".len()..name.len() - ".dds".len()];
        let class_name = ["_selectedover", "_selected", "_over", "_rest"] // longest first
            .iter()
            .find_map(|suffix| base.strip_suffix(suffix))
            .unwrap_or(base);
        // Keep only the resting state (plus suffix-less strategic icons) —
        // the over/selected variants add UI markers and would double classes.
        if base != class_name && !name.ends_with("_rest.dds") {
            continue;
        }
        let class_name = class_name.to_string();

        let bytes = fs::read(&path)?;
        let dds = image_dds::ddsfile::Dds::read(&mut Cursor::new(bytes))
            .with_context(|| format!("parsing {name}"))?;
        let img = image_dds::image_from_dds(&dds, 0).with_context(|| format!("decoding {name}"))?;
        sprites.push(Sprite { class_name, img });
    }
    Ok(sprites)
}

/// Sorted, deduplicated class names of a sprite pool.
pub fn class_names(sprites: &[Sprite]) -> Vec<String> {
    let mut classes: Vec<String> = sprites.iter().map(|s| s.class_name.clone()).collect();
    classes.sort();
    classes.dedup();
    classes
}

/// Generate `config.count` samples, invoking `on_sample` per sample so the
/// caller can stream them into its store. Seeded by `config.seed` for
/// reproducibility. Empty sprite/background pools generate nothing — the
/// caller is expected to validate them first.
pub fn generate(
    config: &DatagenConfig,
    sprites: &[Sprite],
    backgrounds: &[RgbaImage],
    on_sample: &mut dyn FnMut(RgbaImage, Vec<GenBox>),
) {
    if sprites.is_empty() || backgrounds.is_empty() {
        return;
    }
    let mut rng = StdRng::seed_from_u64(config.seed);
    for _ in 0..config.count {
        let (img, boxes) = generate_sample(&mut rng, sprites, backgrounds, config);
        on_sample(img, boxes);
    }
}

// ── compositing ─────────────────────────────────────────────────────────────

/// Recolors a sprite to a team color: keep the alpha, scale the team color by
/// the source luminance (the sprites are grayscale line art the game tints).
fn tint(sprite: &RgbaImage, color: Rgba<u8>) -> RgbaImage {
    let mut out = sprite.clone();
    for Rgba([r, g, b, a]) in out.pixels_mut() {
        if *a == 0 {
            continue;
        }
        let lum = (*r as u32 + *g as u32 + *b as u32) as f32 / (3.0 * 255.0);
        *r = (color.0[0] as f32 * lum) as u8;
        *g = (color.0[1] as f32 * lum) as u8;
        *b = (color.0[2] as f32 * lum) as u8;
    }
    out
}

/// Generates one synthetic sample: random background crop + N tinted, scaled
/// sprites (with a clustering bias — real strategic views are clumpy).
fn generate_sample(
    rng: &mut impl RngExt,
    sprites: &[Sprite],
    backgrounds: &[RgbaImage],
    config: &DatagenConfig,
) -> (RgbaImage, Vec<GenBox>) {
    let bg = backgrounds.choose(rng).expect("non-empty background pool");
    let max_x = bg.width() - config.size;
    let max_y = bg.height() - config.size;
    let crop_x = rng.random_range(0..=max_x);
    let crop_y = rng.random_range(0..=max_y);
    let mut canvas = crop_imm(bg, crop_x, crop_y, config.size, config.size).to_image();

    let n_units = rng.random_range(1..=config.max_units);
    let mut boxes = Vec::with_capacity(n_units);
    // Cluster center for clumping (real games: armies move in blobs).
    let mut cluster: Option<(i64, i64)> = None;

    for _ in 0..n_units {
        let sprite = sprites.choose(rng).expect("non-empty sprite pool");
        let scale = rng.random_range(config.scale_min..config.scale_max);
        let w = ((sprite.img.width() as f32 * scale).round() as u32).max(2);
        let h = ((sprite.img.height() as f32 * scale).round() as u32).max(2);
        let icon = resize(
            &tint(&sprite.img, *TEAM_COLORS.choose(rng).unwrap()),
            w,
            h,
            FilterType::Lanczos3,
        );

        // 40% chance: place near the previous unit (cluster); else uniform.
        let (x, y) = match cluster {
            Some((cx, cy)) if rng.random_bool(0.4) => (
                (cx + rng.random_range(-40..=40)).clamp(0, config.size as i64 - w as i64),
                (cy + rng.random_range(-40..=40)).clamp(0, config.size as i64 - h as i64),
            ),
            _ => (
                rng.random_range(0..=(config.size - w) as i64),
                rng.random_range(0..=(config.size - h) as i64),
            ),
        };
        cluster = Some((x, y));

        overlay(&mut canvas, &icon, x, y);
        boxes.push(GenBox {
            class_name: sprite.class_name.clone(),
            x: x as u32,
            y: y as u32,
            w,
            h,
        });
    }
    (canvas, boxes)
}
