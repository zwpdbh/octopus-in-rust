/// A supported authentication platform.
#[derive(Debug, Clone)]
pub struct Platform {
    pub id: &'static str,
    pub name: &'static str,
    pub base_url: &'static str,
    pub search_url: Option<&'static str>,
    pub fetch_url: Option<&'static str>,
    /// Allowed API key prefixes for this platform (e.g., `["kimi-k"]`).
    pub allowed_prefixes: Option<&'static [&'static str]>,
    /// Whether this platform uses managed OAuth (device flow).
    pub managed_oauth: bool,
}

/// Default OAuth host for Kimi Code (can be overridden via env).
fn default_oauth_host() -> String {
    std::env::var("KIMI_CODE_OAUTH_HOST")
        .or_else(|_| std::env::var("KIMI_OAUTH_HOST"))
        .unwrap_or_else(|_| "https://auth.kimi.com".to_string())
}

/// Kimi Code base URL (can be overridden via env).
fn kimi_code_base_url() -> String {
    std::env::var("KIMI_CODE_BASE_URL")
        .unwrap_or_else(|_| "https://api.kimi.com/coding/v1".to_string())
}

pub static PLATFORMS: &[Platform] = &[
    Platform {
        id: "kimi-code",
        name: "Kimi Code",
        base_url: "", // resolved at runtime via kimi_code_base_url()
        search_url: None,
        fetch_url: None,
        allowed_prefixes: None,
        managed_oauth: true,
    },
    Platform {
        id: "moonshot-cn",
        name: "Moonshot AI Open Platform (moonshot.cn)",
        base_url: "https://api.moonshot.cn/v1",
        search_url: None,
        fetch_url: None,
        allowed_prefixes: Some(&["kimi-k"]),
        managed_oauth: false,
    },
    Platform {
        id: "moonshot-ai",
        name: "Moonshot AI Open Platform (moonshot.ai)",
        base_url: "https://api.moonshot.ai/v1",
        search_url: None,
        fetch_url: None,
        allowed_prefixes: Some(&["kimi-k"]),
        managed_oauth: false,
    },
];

/// Find a platform by its ID.
pub fn resolve_platform(id: &str) -> Option<&'static Platform> {
    PLATFORMS.iter().find(|p| p.id == id)
}

/// Get the effective base URL for a platform.
pub fn platform_base_url(platform: &Platform) -> String {
    if platform.id == "kimi-code" {
        kimi_code_base_url()
    } else {
        platform.base_url.to_string()
    }
}

/// Get the OAuth host URL.
pub fn oauth_host() -> String {
    default_oauth_host()
}
