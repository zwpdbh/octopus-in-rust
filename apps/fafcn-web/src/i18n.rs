//! Hand-rolled English/Chinese i18n.
//!
//! Exactly two languages are supported, so translations live in a typed enum
//! table: the compiler (not a runtime lookup) guarantees every UI string has
//! both translations. Components call [`use_t`] and render `t.t(Text::…)`.
//!
//! The active language is a `Signal<Lang>` provided at the app root, so
//! toggling it re-renders every subscribed component. The choice persists in
//! `localStorage`; the initial value falls back to the browser language.

use dioxus::prelude::*;

/// Supported UI languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Zh,
}

impl Lang {
    /// The other language (used by the navbar toggle).
    pub fn toggled(self) -> Lang {
        match self {
            Lang::En => Lang::Zh,
            Lang::Zh => Lang::En,
        }
    }
}

const STORAGE_KEY: &str = "fafcn-lang";

/// Provide the language signal at the app root and persist changes.
///
/// Call once in the root component; use [`use_t`] everywhere else.
pub fn use_provide_lang() {
    let lang = use_context_provider(|| Signal::new(load_initial_lang()));
    use_effect(move || save_lang(*lang.read()));
}

/// Copyable translator handle; reading it subscribes the component to
/// language changes.
#[derive(Clone, Copy)]
pub struct T(pub Lang);

impl T {
    /// Translate one UI string into the active language.
    pub fn t(self, text: Text) -> &'static str {
        text.get(self.0)
    }
}

/// Access the translator inside a component.
pub fn use_t() -> T {
    T(*use_context::<Signal<Lang>>().read())
}

/// Access the raw language signal (e.g. for the navbar toggle).
pub fn use_lang_signal() -> Signal<Lang> {
    use_context::<Signal<Lang>>()
}

fn load_initial_lang() -> Lang {
    let Some(window) = web_sys::window() else {
        return Lang::En;
    };
    if let Some(storage) = window.local_storage().ok().flatten() {
        if let Ok(Some(v)) = storage.get_item(STORAGE_KEY) {
            return if v == "zh" { Lang::Zh } else { Lang::En };
        }
    }
    let browser = window.navigator().language().unwrap_or_default();
    if browser.to_lowercase().starts_with("zh") {
        Lang::Zh
    } else {
        Lang::En
    }
}

fn save_lang(lang: Lang) {
    let storage = web_sys::window().and_then(|w| w.local_storage().ok().flatten());
    if let Some(storage) = storage {
        let v = match lang {
            Lang::En => "en",
            Lang::Zh => "zh",
        };
        let _ = storage.set_item(STORAGE_KEY, v);
    }
}

/// Translate a server-provided unit category name (data-driven, so this is a
/// function on the raw string rather than a `Text` variant).
pub fn translate_category(category: &str, lang: Lang) -> String {
    if lang == Lang::En {
        return category.to_string();
    }
    match category {
        "Land" => "陆军",
        "Air" => "空军",
        "Naval" => "海军",
        "Structures - Factories" => "建筑 - 工厂",
        "Structures - Economy" => "建筑 - 经济",
        "Structures - Weapons" => "建筑 - 武器",
        "Structures - Support" => "建筑 - 支援",
        "Structures - Intelligence" => "建筑 - 情报",
        "Construction - Buildpower" => "建造 - 工程单位",
        "Experimental" => "实验级",
        other => return other.to_string(),
    }
    .to_string()
}

/// Translate a server-provided unit kind badge.
pub fn translate_kind(kind: &str, lang: Lang) -> String {
    if lang == Lang::En {
        return kind.to_string();
    }
    match kind {
        "Base" => "基地",
        "Land" => "陆军",
        "Air" => "空军",
        "Naval" => "海军",
        other => return other.to_string(),
    }
    .to_string()
}

/// Every translatable UI string, grouped by area.
///
/// Adding a variant without both translations is a compile error — that is
/// the point of the hand-rolled table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Text {
    // Navbar.
    NavHome,
    NavSimulate,
    NavQa,
    NavSync,

    // Common.
    Loading,
    Save,
    Clear,
    Copy,
    Copied,
    Builder,
    Target,
    HintBuildPower,
    HintDropAny,

    // Unit browser / selector.
    SearchUnits,
    NoUnitsMatch,
    SelectUnit,
    SelectUnitDetail,
    ClickToSelectUnit,
    LoadUnitsFailed,
    MassCost,
    EnergyCost,
    BuildTime,
    BuildPower,

    // Simulate: eco panel + stats.
    EcoSettings,
    MassProduction,
    EnergyProduction,
    MassStorage,
    EnergyStorage,
    MassIncome,
    MassDrain,
    EnergyIncome,
    EnergyDrain,

    // Simulate: plan queue.
    ConstructionPlan,
    NewItem,
    QueueItemPrefix,
    EmptyQueue,
    ShowCards,
    ShowJson,
    EditPlanJson,
    InvalidJson,

    // Simulate: controls.
    Start,
    Pause,
    Resume,
    Stop,
    Reset,
    Speed,
    Unlimited,

    // Simulate: chart + snapshot.
    EnergyBudget,
    MassBudget,
    Efficiency,
    MassSpent,
    EnergySpent,
    Income,
    Maintenance,
    Available,
    Drain,
    Net,
    GrossIncome,
    ScaledIncome,
    Current,
    Cap,
    MaintenanceThreshold,
    TotalMassSpent,
    TotalEnergySpent,
    Snapshot,
    Time,
    Production,
    Scaled,
    Storage,
    ScalingActive,

    // Q&A page.
    QaTitle,
    QaSubtitle,
    QaPlaceholder,
    QaSuggestionMonkeylord,
    QaSuggestionBuildOrder,
    QaSuggestionMex,

    // Sync page.
    SyncTitle,
    SyncIntro,
    MirrorStatus,
    LoadingStatus,
    LoadStatusFailed,
    MirrorEmpty,
    PatchVersion,
    LastUpdated,
    UploadedBy,
    FileCount,
    TotalSize,
    SyncClient,
    DownloadClient,
    SyncStepDownload,
    SyncStepFirstRun,
    SyncStepResync,
    SyncStepPlay,
    SyncClientNote,
}

impl Text {
    /// The string for one language.
    pub fn get(self, lang: Lang) -> &'static str {
        match (self, lang) {
            // Navbar.
            (Text::NavHome, Lang::En) => "Home",
            (Text::NavHome, Lang::Zh) => "首页",
            (Text::NavSimulate, Lang::En) => "Simulate",
            (Text::NavSimulate, Lang::Zh) => "建造模拟",
            (Text::NavQa, Lang::En) => "Q&A",
            (Text::NavQa, Lang::Zh) => "问答",
            (Text::NavSync, Lang::En) => "Sync",
            (Text::NavSync, Lang::Zh) => "补丁同步",

            // Common.
            (Text::Loading, Lang::En) => "Loading...",
            (Text::Loading, Lang::Zh) => "正在加载…",
            (Text::Save, Lang::En) => "Save",
            (Text::Save, Lang::Zh) => "保存",
            (Text::Clear, Lang::En) => "Clear",
            (Text::Clear, Lang::Zh) => "清空",
            (Text::Copy, Lang::En) => "Copy",
            (Text::Copy, Lang::Zh) => "复制",
            (Text::Copied, Lang::En) => "Copied!",
            (Text::Copied, Lang::Zh) => "已复制!",
            (Text::Builder, Lang::En) => "Builder",
            (Text::Builder, Lang::Zh) => "建造者",
            (Text::Target, Lang::En) => "Target",
            (Text::Target, Lang::Zh) => "目标",
            (Text::HintBuildPower, Lang::En) => "Requires build power",
            (Text::HintBuildPower, Lang::Zh) => "需要建造力",
            (Text::HintDropAny, Lang::En) => "Drop any unit",
            (Text::HintDropAny, Lang::Zh) => "任意单位",

            // Unit browser / selector.
            (Text::SearchUnits, Lang::En) => "Search units...",
            (Text::SearchUnits, Lang::Zh) => "搜索单位…",
            (Text::NoUnitsMatch, Lang::En) => "No units match the current filters.",
            (Text::NoUnitsMatch, Lang::Zh) => "没有符合筛选条件的单位。",
            (Text::SelectUnit, Lang::En) => "Select Unit",
            (Text::SelectUnit, Lang::Zh) => "选择单位",
            (Text::SelectUnitDetail, Lang::En) => "Select a unit to view details.",
            (Text::SelectUnitDetail, Lang::Zh) => "选择一个单位查看详情。",
            (Text::ClickToSelectUnit, Lang::En) => "Click to select a unit",
            (Text::ClickToSelectUnit, Lang::Zh) => "点击选择单位",
            (Text::LoadUnitsFailed, Lang::En) => "Failed to load units: ",
            (Text::LoadUnitsFailed, Lang::Zh) => "加载单位失败:",
            (Text::MassCost, Lang::En) => "Mass",
            (Text::MassCost, Lang::Zh) => "质量",
            (Text::EnergyCost, Lang::En) => "Energy",
            (Text::EnergyCost, Lang::Zh) => "能量",
            (Text::BuildTime, Lang::En) => "Build Time",
            (Text::BuildTime, Lang::Zh) => "建造时间",
            (Text::BuildPower, Lang::En) => "Build Power",
            (Text::BuildPower, Lang::Zh) => "建造力",

            // Simulate: eco panel + stats.
            (Text::EcoSettings, Lang::En) => "Eco Settings",
            (Text::EcoSettings, Lang::Zh) => "经济设置",
            (Text::MassProduction, Lang::En) => "Mass production",
            (Text::MassProduction, Lang::Zh) => "质量产量",
            (Text::EnergyProduction, Lang::En) => "Energy production",
            (Text::EnergyProduction, Lang::Zh) => "能量产量",
            (Text::MassStorage, Lang::En) => "Mass storage",
            (Text::MassStorage, Lang::Zh) => "质量存储",
            (Text::EnergyStorage, Lang::En) => "Energy storage",
            (Text::EnergyStorage, Lang::Zh) => "能量存储",
            (Text::MassIncome, Lang::En) => "Mass Income",
            (Text::MassIncome, Lang::Zh) => "质量收入",
            (Text::MassDrain, Lang::En) => "Mass Drain",
            (Text::MassDrain, Lang::Zh) => "质量消耗",
            (Text::EnergyIncome, Lang::En) => "Energy Income",
            (Text::EnergyIncome, Lang::Zh) => "能量收入",
            (Text::EnergyDrain, Lang::En) => "Energy Drain",
            (Text::EnergyDrain, Lang::Zh) => "能量消耗",

            // Simulate: plan queue.
            (Text::ConstructionPlan, Lang::En) => "Construction Plan",
            (Text::ConstructionPlan, Lang::Zh) => "建造计划",
            (Text::NewItem, Lang::En) => "New Item",
            (Text::NewItem, Lang::Zh) => "新建项目",
            (Text::QueueItemPrefix, Lang::En) => "Queue Item #",
            (Text::QueueItemPrefix, Lang::Zh) => "队列项 #",
            (Text::EmptyQueue, Lang::En) => {
                "No items in the queue yet. Use the New Item panel on the left to add one."
            }
            (Text::EmptyQueue, Lang::Zh) => "队列为空,请使用左侧的“新建项目”面板添加。",
            (Text::ShowCards, Lang::En) => "Show cards",
            (Text::ShowCards, Lang::Zh) => "卡片视图",
            (Text::ShowJson, Lang::En) => "Show JSON",
            (Text::ShowJson, Lang::Zh) => "JSON 视图",
            (Text::EditPlanJson, Lang::En) => "Edit the plan JSON below.",
            (Text::EditPlanJson, Lang::Zh) => "在下方编辑计划 JSON。",
            (Text::InvalidJson, Lang::En) => "Invalid JSON: ",
            (Text::InvalidJson, Lang::Zh) => "JSON 无效:",

            // Simulate: controls.
            (Text::Start, Lang::En) => "Start",
            (Text::Start, Lang::Zh) => "开始",
            (Text::Pause, Lang::En) => "Pause",
            (Text::Pause, Lang::Zh) => "暂停",
            (Text::Resume, Lang::En) => "Resume",
            (Text::Resume, Lang::Zh) => "继续",
            (Text::Stop, Lang::En) => "Stop",
            (Text::Stop, Lang::Zh) => "停止",
            (Text::Reset, Lang::En) => "Reset",
            (Text::Reset, Lang::Zh) => "重置",
            (Text::Speed, Lang::En) => "Speed",
            (Text::Speed, Lang::Zh) => "速度",
            (Text::Unlimited, Lang::En) => "Unlimited",
            (Text::Unlimited, Lang::Zh) => "不限速",

            // Simulate: chart + snapshot.
            (Text::EnergyBudget, Lang::En) => "Energy budget",
            (Text::EnergyBudget, Lang::Zh) => "能量预算",
            (Text::MassBudget, Lang::En) => "Mass budget",
            (Text::MassBudget, Lang::Zh) => "质量预算",
            (Text::Efficiency, Lang::En) => "Efficiency",
            (Text::Efficiency, Lang::Zh) => "效率",
            (Text::MassSpent, Lang::En) => "Mass spent",
            (Text::MassSpent, Lang::Zh) => "累计质量消耗",
            (Text::EnergySpent, Lang::En) => "Energy spent",
            (Text::EnergySpent, Lang::Zh) => "累计能量消耗",
            (Text::Income, Lang::En) => "Income",
            (Text::Income, Lang::Zh) => "收入",
            (Text::Maintenance, Lang::En) => "Maintenance",
            (Text::Maintenance, Lang::Zh) => "维护",
            (Text::Available, Lang::En) => "Available",
            (Text::Available, Lang::Zh) => "可用",
            (Text::Drain, Lang::En) => "Drain",
            (Text::Drain, Lang::Zh) => "消耗",
            (Text::Net, Lang::En) => "Net",
            (Text::Net, Lang::Zh) => "净变化",
            (Text::GrossIncome, Lang::En) => "Gross income",
            (Text::GrossIncome, Lang::Zh) => "总收入",
            (Text::ScaledIncome, Lang::En) => "Scaled income",
            (Text::ScaledIncome, Lang::Zh) => "折算收入",
            (Text::Current, Lang::En) => "Current",
            (Text::Current, Lang::Zh) => "当前",
            (Text::Cap, Lang::En) => "Cap",
            (Text::Cap, Lang::Zh) => "上限",
            (Text::MaintenanceThreshold, Lang::En) => "Maintenance threshold",
            (Text::MaintenanceThreshold, Lang::Zh) => "维护阈值",
            (Text::TotalMassSpent, Lang::En) => "Total mass spent",
            (Text::TotalMassSpent, Lang::Zh) => "累计质量消耗",
            (Text::TotalEnergySpent, Lang::En) => "Total energy spent",
            (Text::TotalEnergySpent, Lang::Zh) => "累计能量消耗",
            (Text::Snapshot, Lang::En) => "Snapshot",
            (Text::Snapshot, Lang::Zh) => "快照",
            (Text::Time, Lang::En) => "Time",
            (Text::Time, Lang::Zh) => "时间",
            (Text::Production, Lang::En) => "Production",
            (Text::Production, Lang::Zh) => "产量",
            (Text::Scaled, Lang::En) => "Scaled",
            (Text::Scaled, Lang::Zh) => "折算后",
            (Text::Storage, Lang::En) => "Storage",
            (Text::Storage, Lang::Zh) => "存储",
            (Text::ScalingActive, Lang::En) => " (scaling active)",
            (Text::ScalingActive, Lang::Zh) => "(折算生效中)",

            // Q&A page.
            (Text::QaTitle, Lang::En) => "FAF Q&A",
            (Text::QaTitle, Lang::Zh) => "FAF 问答",
            (Text::QaSubtitle, Lang::En) => {
                "Ask anything about Forged Alliance Forever units and economy."
            }
            (Text::QaSubtitle, Lang::Zh) => "询问任何关于 FAF 单位与经济的问题。",
            (Text::QaPlaceholder, Lang::En) => "Ask about a unit, build order, or economy...",
            (Text::QaPlaceholder, Lang::Zh) => "询问单位、建造顺序或经济问题…",
            (Text::QaSuggestionMonkeylord, Lang::En) => "Explain the Cybran Monkeylord",
            (Text::QaSuggestionMonkeylord, Lang::Zh) => "讲解一下赛布兰的猴王(Monkeylord)",
            (Text::QaSuggestionBuildOrder, Lang::En) => "What is a good build order for UEF?",
            (Text::QaSuggestionBuildOrder, Lang::Zh) => "UEF 有什么好的建造顺序?",
            (Text::QaSuggestionMex, Lang::En) => "How do mass extractors work?",
            (Text::QaSuggestionMex, Lang::Zh) => "质量萃取器是如何工作的?",

            // Sync page.
            (Text::SyncTitle, Lang::En) => "Gamedata Sync",
            (Text::SyncTitle, Lang::Zh) => "游戏数据同步",
            (Text::SyncIntro, Lang::En) => {
                "Chinese players can fetch the hard-to-download FAF gamedata patch files \
                 from this community mirror instead of passing them through QQ. \
                 Download the sync client below and run it — it figures out which files \
                 you need by itself."
            }
            (Text::SyncIntro, Lang::Zh) => {
                "国内玩家无需再经过 QQ 传文件,可直接从本社区镜像下载难以获取的 FAF \
                 gamedata 补丁文件。下载下方的同步客户端并运行即可——它会自动判断你需要哪些文件。"
            }
            (Text::MirrorStatus, Lang::En) => "Mirror status",
            (Text::MirrorStatus, Lang::Zh) => "镜像状态",
            (Text::LoadingStatus, Lang::En) => "Loading mirror status...",
            (Text::LoadingStatus, Lang::Zh) => "正在加载镜像状态…",
            (Text::LoadStatusFailed, Lang::En) => "Failed to load status: ",
            (Text::LoadStatusFailed, Lang::Zh) => "加载状态失败:",
            (Text::MirrorEmpty, Lang::En) => {
                "The mirror is empty — waiting for a player with VPN access \
                 to upload the latest patch."
            }
            (Text::MirrorEmpty, Lang::Zh) => "镜像为空——请有 VPN 的玩家先上传最新补丁。",
            (Text::PatchVersion, Lang::En) => "Patch version",
            (Text::PatchVersion, Lang::Zh) => "补丁版本",
            (Text::LastUpdated, Lang::En) => "Last updated",
            (Text::LastUpdated, Lang::Zh) => "更新时间",
            (Text::UploadedBy, Lang::En) => "Uploaded by",
            (Text::UploadedBy, Lang::Zh) => "上传者",
            (Text::FileCount, Lang::En) => "Files",
            (Text::FileCount, Lang::Zh) => "文件数",
            (Text::TotalSize, Lang::En) => "Total size",
            (Text::TotalSize, Lang::Zh) => "总大小",
            (Text::SyncClient, Lang::En) => "Sync client",
            (Text::SyncClient, Lang::Zh) => "同步客户端",
            (Text::DownloadClient, Lang::En) => "Download fafcn-sync (Windows)",
            (Text::DownloadClient, Lang::Zh) => "下载 fafcn-sync(Windows)",
            (Text::SyncStepDownload, Lang::En) => {
                "Download the client and put it anywhere (e.g. your Desktop), then double-click it."
            }
            (Text::SyncStepDownload, Lang::Zh) => "下载客户端,放到任意位置(如桌面),然后双击运行。",
            (Text::SyncStepFirstRun, Lang::En) => {
                "The mirror address is already embedded in your download — it fills in automatically, \
                 no setup needed."
            }
            (Text::SyncStepFirstRun, Lang::Zh) => {
                "镜像地址已随下载自动配置好,打开即可使用,无需手动填写。"
            }
            (Text::SyncStepResync, Lang::En) => {
                "The gamedata folder is detected automatically; if not, point the app to your \
                 FAForever\\gamedata folder once. It remembers both fields."
            }
            (Text::SyncStepResync, Lang::Zh) => {
                "gamedata 目录会自动检测;如果没有,手动选择一次 FAForever\\gamedata 目录即可。两项设置都会被记住。"
            }
            (Text::SyncStepPlay, Lang::En) => {
                "Before playing, open the app and click \"Sync now\". Then start the FAF client — nothing else to do."
            }
            (Text::SyncStepPlay, Lang::Zh) => {
                "每次玩之前打开它点一下“开始同步”,然后启动 FAF 客户端即可,无需其他操作。"
            }
            (Text::SyncClientNote, Lang::En) => {
                "The client only downloads files whose hash differs from the mirror \
                 manifest; it verifies every download and never deletes your files. \
                 If the mirror looks stale (check \"Last updated\" above), ask a \
                 VPN-having player to upload the newest patch."
            }
            (Text::SyncClientNote, Lang::Zh) => {
                "客户端只会下载与镜像清单哈希不一致的文件;每次下载都会校验,且绝不会删除你的文件。\
                 如果镜像看起来过时(查看上方“更新时间”),请提醒有 VPN 的玩家上传最新补丁。"
            }
        }
    }
}
