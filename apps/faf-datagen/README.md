# faf-datagen

Synthetic training-data generator for FAF strategic-icon detection — phase 1
of the unit-detection ML project.

Instead of hand-labeling screenshots, it pastes the game's own strategic-icon
sprites (`tmp/custom-strategic-icons`, DDS) onto random crops of real
screenshots. Bounding boxes are known by construction → perfect labels for
free, in YOLO format.

## Usage

```sh
cargo run -p faf-datagen -- --count 1000
# options (defaults shown):
#   --icons tmp/custom-strategic-icons          sprite source
#   --screenshots /mnt/d/download/faf_units_screenshots
#   --out data/faf-detect                       (gitignored)
#   --size 640            crop side length
#   --max-units 25        units per sample (1..=25, cluster-biased)
#   --scale-min 0.35 --scale-max 0.65   fraction of the 36×40 source
#   --previews 10         render boxes for the first N samples
#   --seed 42
```

Output: `classes.txt` (sorted, line no. = class id), `images/*.png`,
`labels/*.txt` (YOLO: `<class> <cx> <cy> <w> <h>` normalized),
`previews/*.png` (boxes drawn, for eyeballing).

## Known limitations / next steps

- **Background label noise**: screenshot crops may already contain REAL
  icons (e.g. the minimap inset) which are then unlabeled positives. Fix:
  take screenshots of EMPTY map regions for the background pool (zoom into
  unexplored/empty terrain), or mask out UI insets before cropping.
- **Domain gap**: verify pasted icons match the real render in size, team
  color, and anti-aliasing (compare previews/ with a real screenshot).
  `--scale-min/max` and `TEAM_COLORS` in src/main.rs are the knobs.
- **193 classes** is very fine-grained (`bomber1_directfire`,
  `bomber2_antinavy`, …). Consider grouping to ~10 coarse classes
  (bomber/fighter/tank/artillery/structure/…) before training — edit the
  class-name mapping in `load_sprites`.
- Sprites use only the `_rest` state; `selected`/`over` variants are skipped.
- Held-out rule: never composite on the screenshots you'll test with —
  keep NEW real screenshots as the test set.

## Dependencies

- `image` 0.25 + `image_dds` 0.7 (`default-features = false`, no encoder →
  avoids the ISPC/intel-tex build) for DDS decoding.
