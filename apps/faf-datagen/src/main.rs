//! faf-datagen — synthetic training data for FAF strategic-icon detection.
//!
//! Phase 1 of the unit-detection project: instead of hand-labeling
//! screenshots, composite the game's own strategic-icon sprites onto crops of
//! real screenshots. Bounding boxes are known BY CONSTRUCTION, so every
//! generated image comes with perfect labels (YOLO format).
//!
//! Pipeline:
//!   sprites (DDS, 36×40 line art) → tint with a team color → scale to the
//!   on-screen size range → alpha-blend onto a random screenshot crop →
//!   record (class, x, y, w, h) per pasted sprite
//!
//! Output layout (under --out):
//!   classes.txt           one class name per line (line number = class id)
//!   images/000001.png     synthetic samples
//!   labels/000001.txt     YOLO labels: "<class_id> <cx> <cy> <w> <h>" (0..1)
//!   previews/000001.png   first --previews samples with boxes drawn (eyeball check)
//!
//! ⚠ Domain-gap reminder: after generating, open previews/ AND a real
//! screenshot side by side — the synthetic icons must match the real render
//! in SIZE, COLOR, and edge sharpness, or the model learns the wrong object.
//! And keep FUTURE screenshots held out as the real test set — never train
//! on the only real data you have.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use image::imageops::{crop_imm, overlay, resize, FilterType};
use image::{Rgba, RgbaImage};
use rand::seq::IndexedRandom;
use rand::RngExt; // rand 0.10: random_range/random_bool live on RngExt

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

#[derive(Parser)]
#[command(
    name = "faf-datagen",
    about = "Synthetic FAF icon-detection training data"
)]
struct Cli {
    /// Directory with the strategic-icon .dds files
    #[arg(long, default_value = "tmp/custom-strategic-icons")]
    icons: PathBuf,

    /// Directory with real game screenshots (used as background pool)
    #[arg(long, default_value = "/mnt/d/download/faf_units_screenshots")]
    screenshots: PathBuf,

    /// Output directory
    #[arg(long, default_value = "data/faf-detect")]
    out: PathBuf,

    /// Number of synthetic samples to generate
    #[arg(long, default_value_t = 200)]
    count: usize,

    /// Side length of the square crop each sample is generated on
    #[arg(long, default_value_t = 640)]
    size: u32,

    /// Max units pasted per sample (min is 1)
    #[arg(long, default_value_t = 25)]
    max_units: usize,

    /// Sprite scale range as a fraction of the 36×40 source (0.35 ≈ 13 px —
    /// the zoomed-out on-screen size; check against real screenshots!)
    #[arg(long, default_value_t = 0.35)]
    scale_min: f32,
    #[arg(long, default_value_t = 0.65)]
    scale_max: f32,

    /// Also render bounding boxes for the first N samples into previews/
    #[arg(long, default_value_t = 10)]
    previews: usize,

    /// RNG seed (fixed by default for reproducible datasets)
    #[arg(long, default_value_t = 42)]
    seed: u64,
}

/// One strategic-icon sprite: the `_rest` variant of one class.
struct Sprite {
    class_name: String,
    img: RgbaImage, // 36×40 with alpha
}

/// A placed unit: class id + pixel bounding box (absolute, on the sample).
struct Label {
    class_id: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut rng = rand::rng();

    let sprites = load_sprites(&cli.icons)?;
    anyhow::ensure!(!sprites.is_empty(), "no sprites found in {:?}", cli.icons);

    // Class ids: sorted class names → deterministic classes.txt.
    let mut classes: Vec<String> = sprites.iter().map(|s| s.class_name.clone()).collect();
    classes.sort();
    classes.dedup();
    let class_id = |name: &str| classes.iter().position(|c| c == name).unwrap();

    let backgrounds = load_backgrounds(&cli.screenshots, cli.size)?;
    anyhow::ensure!(
        !backgrounds.is_empty(),
        "no screenshots (>= {}×{}) found in {:?}",
        cli.size,
        cli.size,
        cli.screenshots
    );

    let (img_dir, lbl_dir, prev_dir) = (
        cli.out.join("images"),
        cli.out.join("labels"),
        cli.out.join("previews"),
    );
    fs::create_dir_all(&img_dir)?;
    fs::create_dir_all(&lbl_dir)?;
    fs::create_dir_all(&prev_dir)?;
    fs::write(cli.out.join("classes.txt"), classes.join("\n") + "\n")?;

    println!(
        "generating {} samples: {} classes, {} backgrounds, {}×{} crops",
        cli.count,
        classes.len(),
        backgrounds.len(),
        cli.size,
        cli.size
    );
    for i in 0..cli.count {
        let (img, labels) = generate_sample(&mut rng, &sprites, &class_id, &backgrounds, &cli);
        let stem = format!("{i:06}");
        img.save(img_dir.join(format!("{stem}.png")))?;
        fs::write(
            lbl_dir.join(format!("{stem}.txt")),
            labels_to_yolo(&labels, cli.size),
        )?;
        if i < cli.previews {
            draw_preview(&img, &labels).save(prev_dir.join(format!("{stem}.png")))?;
        }
    }

    println!("done → {:?} ({} classes)", cli.out, classes.len());
    println!("\nNEXT: the domain-gap check — open previews/ next to a REAL screenshot.");
    println!("Icons must match in size/color/sharpness. And keep future real");
    println!("screenshots held out as the test set — never composite on them.");
    Ok(())
}

// ── sprites ─────────────────────────────────────────────────────────────────

/// Loads every `icon_*_rest.dds` (plus suffix-less strategic icons), decoding
/// DDS → RGBA8. Class name = filename minus `icon_` prefix and state suffix:
/// `icon_bomber1_directfire_rest.dds` → `bomber1_directfire`.
fn load_sprites(dir: &Path) -> Result<Vec<Sprite>> {
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

// ── backgrounds ─────────────────────────────────────────────────────────────

/// Loads background images from `dir`.
///
/// Two modes:
/// - plain directory of PNGs (e.g. a manual `data/faf-backgrounds/` folder):
///   every PNG is used.
/// - the faf-ml platform's screenshot store (`data/faf-ml/screenshots`,
///   recognized by its `index.json`): only screenshots whose kind is
///   `background` are used — battle screenshots must never become canvases,
///   because their real units would end up as unlabeled ghosts in the
///   synthetic data.
fn load_backgrounds(dir: &Path, min_size: u32) -> Result<Vec<RgbaImage>> {
    let allowed: Option<Vec<String>> = match std::fs::read_to_string(dir.join("index.json")) {
        Ok(raw) => {
            let metas: Vec<faf_ml_core::ScreenshotMeta> = serde_json::from_str(&raw)
                .with_context(|| format!("parsing {:?}", dir.join("index.json")))?;
            let ids = metas
                .iter()
                .filter(|m| m.kind == faf_ml_core::ScreenshotKind::Background)
                .map(|m| format!("{}.png", m.id))
                .collect::<Vec<_>>();
            println!(
                "platform store detected: {} of {} screenshots marked as background",
                ids.len(),
                metas.len()
            );
            Some(ids)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };

    let mut out = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {dir:?}"))? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("png") {
            continue;
        }
        if let Some(allowed) = &allowed {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !allowed.iter().any(|a| a == name) {
                continue;
            }
        }
        let img = image::open(&path)
            .with_context(|| format!("opening {path:?}"))?
            .to_rgba8();
        if img.width() >= min_size && img.height() >= min_size {
            out.push(img);
        }
    }
    Ok(out)
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
    class_id: &dyn Fn(&str) -> usize,
    backgrounds: &[RgbaImage],
    cli: &Cli,
) -> (RgbaImage, Vec<Label>) {
    let bg = backgrounds.choose(rng).expect("non-empty background pool");
    let max_x = bg.width() - cli.size;
    let max_y = bg.height() - cli.size;
    let crop_x = rng.random_range(0..=max_x);
    let crop_y = rng.random_range(0..=max_y);
    let mut canvas = crop_imm(bg, crop_x, crop_y, cli.size, cli.size).to_image();

    let n_units = rng.random_range(1..=cli.max_units);
    let mut labels = Vec::with_capacity(n_units);
    // Cluster center for clumping (real games: armies move in blobs).
    let mut cluster: Option<(i64, i64)> = None;

    for _ in 0..n_units {
        let sprite = sprites.choose(rng).expect("non-empty sprite pool");
        let scale = rng.random_range(cli.scale_min..cli.scale_max);
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
                (cx + rng.random_range(-40..=40)).clamp(0, cli.size as i64 - w as i64),
                (cy + rng.random_range(-40..=40)).clamp(0, cli.size as i64 - h as i64),
            ),
            _ => (
                rng.random_range(0..=(cli.size - w) as i64),
                rng.random_range(0..=(cli.size - h) as i64),
            ),
        };
        cluster = Some((x, y));

        overlay(&mut canvas, &icon, x, y);
        labels.push(Label {
            class_id: class_id(&sprite.class_name),
            x: x as u32,
            y: y as u32,
            w,
            h,
        });
    }
    (canvas, labels)
}

// ── labels & previews ───────────────────────────────────────────────────────

/// YOLO format: `<class_id> <cx> <cy> <w> <h>`, all normalized to 0..1.
fn labels_to_yolo(labels: &[Label], size: u32) -> String {
    let s = size as f32;
    labels
        .iter()
        .map(|l| {
            format!(
                "{} {:.6} {:.6} {:.6} {:.6}",
                l.class_id,
                (l.x as f32 + l.w as f32 / 2.0) / s,
                (l.y as f32 + l.h as f32 / 2.0) / s,
                l.w as f32 / s,
                l.h as f32 / s
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// Renders red bounding boxes for the eyeball/domain-gap check.
fn draw_preview(img: &RgbaImage, labels: &[Label]) -> RgbaImage {
    let mut out = img.clone();
    let red = Rgba([255, 0, 0, 255]);
    for l in labels {
        for dx in 0..l.w {
            for &yy in &[l.y, (l.y + l.h - 1).min(out.height() - 1)] {
                if l.x + dx < out.width() {
                    out.put_pixel(l.x + dx, yy, red);
                }
            }
        }
        for dy in 0..l.h {
            for &xx in &[l.x, (l.x + l.w - 1).min(out.width() - 1)] {
                if l.y + dy < out.height() {
                    out.put_pixel(xx, l.y + dy, red);
                }
            }
        }
    }
    out
}
