//! CJK font loading for egui (its bundled fonts have no CJK glyphs).

use eframe::egui;

/// Subsetted Noto Sans SC (ASCII + GB2312), embedded so the UI stays
/// readable on Windows installs without any CJK system font (e.g. an
/// English Windows 11 without the Chinese supplemental fonts). Subset of
/// Noto Sans CJK SC, SIL OFL 1.1 — see `assets/FONT-LICENSE.txt`.
const EMBEDDED_CJK_FONT: &[u8] = include_bytes!("../../assets/cjk-fallback.ttf");

/// egui's bundled fonts have no CJK glyphs, so Chinese text renders as
/// boxes. Prefer the operating system's CJK font (Microsoft YaHei is present
/// on every Chinese Windows install); when none exists, use the embedded
/// Noto Sans SC subset. Registered as a fallback in both font families —
/// Latin text keeps using egui's default font.
pub(super) fn install_cjk_font(cc: &eframe::CreationContext<'_>) {
    let data = match load_system_cjk_font() {
        Some(bytes) => egui::FontData::from_owned(bytes),
        None => egui::FontData::from_static(EMBEDDED_CJK_FONT),
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("cjk".to_owned(), data.into());
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("cjk".to_owned());
    }
    cc.egui_ctx.set_fonts(fonts);
}

/// Read the first available OS CJK font.
fn load_system_cjk_font() -> Option<Vec<u8>> {
    const CANDIDATES: &[&str] = &[
        // Windows: Microsoft YaHei / SimHei / SimSun (always present on zh systems).
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
        // Linux: Noto CJK / WenQuanYi (for development).
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        // macOS.
        "/System/Library/Fonts/PingFang.ttc",
    ];
    CANDIDATES.iter().find_map(|p| std::fs::read(p).ok())
}
