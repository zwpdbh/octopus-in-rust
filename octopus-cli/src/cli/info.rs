use serde::Serialize;

use crate::constant;

#[derive(Serialize)]
struct InfoData {
    kimi_cli_version: String,
    agent_spec_versions: Vec<String>,
    wire_protocol_version: String,
}

pub fn run_info(json: bool) {
    let data = InfoData {
        kimi_cli_version: constant::get_version().to_string(),
        agent_spec_versions: vec!["1.0".to_string()],
        wire_protocol_version: "1.10".to_string(),
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&data).unwrap());
    } else {
        println!("kimi-cli version: {}", data.kimi_cli_version);
        println!(
            "agent spec versions: {}",
            data.agent_spec_versions.join(", ")
        );
        println!("wire protocol version: {}", data.wire_protocol_version);
    }
}
