use std::sync::OnceLock;

pub const NAME: &str = "Kimi Code CLI";

pub fn get_version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        option_env!("CARGO_PKG_VERSION")
            .unwrap_or("0.0.0")
            .to_string()
    })
}

pub fn get_user_agent() -> &'static str {
    static USER_AGENT: OnceLock<String> = OnceLock::new();
    USER_AGENT.get_or_init(|| format!("KimiCLI/{}", get_version()))
}
