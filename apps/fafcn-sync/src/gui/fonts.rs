//! CJK font loading for egui (its bundled fonts have no CJK glyphs).

use eframe::egui;

/// egui's bundled fonts have no CJK glyphs, so Chinese text renders as
/// boxes. Load the operating system's CJK font (Microsoft YaHei is present
/// on every Chinese Windows install) and register it as a fallback for both
/// font families — Latin text keeps using egui's default font.
pub(super) fn install_cjk_font(cc: &eframe::CreationContext<'_>) {
    let Some(bytes) = load_system_cjk_font() else {
        return;
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert("cjk".to_owned(), egui::FontData::from_owned(bytes).into());
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
