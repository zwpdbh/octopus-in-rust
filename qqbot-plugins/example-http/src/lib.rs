use extism_pdk::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize)]
struct RequestArgs {
    url: String,
    #[serde(default = "default_method")]
    method: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    #[serde(default)]
    body: Option<String>,
}

fn default_method() -> String {
    "GET".to_string()
}

#[derive(Serialize)]
struct ResponseResult {
    status: u16,
    body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Execute an HTTP request.
///
/// Input JSON: { "url": "...", "method": "GET", "headers": {}, "body": "..." }
/// Output JSON: { "status": 200, "body": "..." }
#[plugin_fn]
pub fn execute(input: String) -> FnResult<String> {
    let args: RequestArgs = match serde_json::from_str(&input) {
        Ok(a) => a,
        Err(e) => {
            return Ok(serde_json::to_string(&ResponseResult {
                status: 0,
                body: String::new(),
                error: Some(format!("Invalid JSON input: {}", e)),
            })?);
        }
    };

    let method = match args.method.to_uppercase().as_str() {
        "POST" => "POST",
        "PUT" => "PUT",
        "DELETE" => "DELETE",
        "PATCH" => "PATCH",
        "HEAD" => "HEAD",
        "OPTIONS" => "OPTIONS",
        _ => "GET",
    };

    let mut req = HttpRequest::new(&args.url).with_method(method);
    for (key, value) in &args.headers {
        req = req.with_header(key, value);
    }

    let res = match http::request::<String>(&req, args.body) {
        Ok(r) => r,
        Err(e) => {
            return Ok(serde_json::to_string(&ResponseResult {
                status: 0,
                body: String::new(),
                error: Some(format!("HTTP request failed: {}", e)),
            })?);
        }
    };

    let body = String::from_utf8_lossy(&res.body()).to_string();
    let result = ResponseResult {
        status: res.status_code(),
        body,
        error: None,
    };

    Ok(serde_json::to_string(&result)?)
}

/// Return metadata about this tool.
#[plugin_fn]
pub fn tool_metadata() -> FnResult<String> {
    Ok(r#"{
        "name": "HttpRequest",
        "description": "Make HTTP requests to external APIs and websites. Returns status code and response body.",
        "schema": {
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to request"
                },
                "method": {
                    "type": "string",
                    "description": "HTTP method: GET, POST, PUT, DELETE, PATCH",
                    "default": "GET"
                },
                "headers": {
                    "type": "object",
                    "description": "Optional HTTP headers as key-value pairs",
                    "additionalProperties": { "type": "string" }
                },
                "body": {
                    "type": "string",
                    "description": "Optional request body"
                }
            },
            "required": ["url"]
        }
    }"#.to_string())
}
