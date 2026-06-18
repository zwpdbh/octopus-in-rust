use extism_pdk::*;
use serde::{Deserialize, Serialize};

/// Tool definition returned by `register_tools`.
#[derive(Debug, Clone, Serialize)]
struct ToolDef {
    name: String,
    description: String,
    prompt_fragment: Option<String>,
    parameters: serde_json::Value,
}

#[plugin_fn]
pub fn register_tools(_input: String) -> FnResult<String> {
    let tools = vec![
        ToolDef {
            name: "faf_party_parse_intent".to_string(),
            description: "Detect whether a message expresses intent to join, leave, or ignore a FAF party.".to_string(),
            prompt_fragment: None,
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string", "description": "The user's raw message." }
                },
                "required": ["message"]
            }),
        },
        ToolDef {
            name: "faf_party_parse_time".to_string(),
            description: "Parse a Chinese time expression into a structured start/end window. Returns unknown if the expression cannot be parsed.".to_string(),
            prompt_fragment: None,
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "expression": { "type": "string", "description": "The time expression to parse, e.g. '晚上8点以后' or '半小时后'." },
                    "now": { "type": "string", "description": "Current time as RFC3339, e.g. 2026-06-18T13:00:00+08:00." }
                },
                "required": ["expression", "now"]
            }),
        },
    ];

    Ok(serde_json::to_string(&tools)?)
}

#[derive(Debug, Clone, Deserialize)]
struct ExecuteInput {
    tool: String,
    arguments: serde_json::Value,
}

#[plugin_fn]
pub fn execute(input: String) -> FnResult<String> {
    if input.is_empty() {
        return Ok(serde_json::to_string(
            &serde_json::json!({"error":"empty input"}),
        )?);
    }

    let parsed: ExecuteInput = match serde_json::from_str(&input) {
        Ok(i) => i,
        Err(e) => {
            return Ok(serde_json::to_string(
                &serde_json::json!({"error": format!("invalid input: {e}") }),
            )?);
        }
    };

    let result = match parsed.tool.as_str() {
        "faf_party_parse_intent" => parse_intent_tool(parsed.arguments),
        "faf_party_parse_time" => parse_time_tool(parsed.arguments),
        _ => serde_json::json!({"error": format!("Unknown tool: {}", parsed.tool) }),
    };

    Ok(serde_json::to_string(&result)?)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Availability {
    start: String,
    end: String,
    description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Intent {
    Join,
    Leave,
    Unknown,
}

fn parse_intent_tool(args: serde_json::Value) -> serde_json::Value {
    let args: ParseIntentArgs = match serde_json::from_value(args) {
        Ok(a) => a,
        Err(e) => return serde_json::json!({"error": format!("invalid arguments: {e}") }),
    };

    let (intent, time_expression) = parse_intent(&args.message);
    serde_json::json!({
        "intent": match intent {
            Intent::Join => "join",
            Intent::Leave => "leave",
            Intent::Unknown => "unknown",
        },
        "time_expression": time_expression,
    })
}

#[derive(Debug, Clone, Deserialize)]
struct ParseIntentArgs {
    message: String,
}

fn parse_intent(message: &str) -> (Intent, Option<String>) {
    let normalized = message.to_lowercase();

    let leave_signals = [
        "不玩了",
        "cancel",
        "退出",
        "leave",
        "/faf leave",
        "不打了",
        "不来",
        "算了",
    ];
    if leave_signals.iter().any(|s| normalized.contains(s)) {
        return (Intent::Leave, None);
    }

    let join_signals = [
        "玩", "打faf", "打 faf", "来", "加", "+1", "可以", "能玩", "有空", "行", "好", "ok", "yes",
    ];
    let looks_like_join = join_signals.iter().any(|s| normalized.contains(s));
    if !looks_like_join {
        return (Intent::Unknown, None);
    }

    // Extract the time-expression part. For now, return the whole message;
    // the time parser will ignore non-time words.
    (Intent::Join, Some(message.to_string()))
}

fn parse_time_tool(args: serde_json::Value) -> serde_json::Value {
    let args: ParseTimeArgs = match serde_json::from_value(args) {
        Ok(a) => a,
        Err(e) => return serde_json::json!({"error": format!("invalid arguments: {e}") }),
    };

    let now = match chrono::DateTime::parse_from_rfc3339(&args.now) {
        Ok(dt) => dt.with_timezone(&chrono::FixedOffset::east_opt(0).unwrap()),
        Err(e) => return serde_json::json!({"error": format!("invalid now: {e}") }),
    };

    match parse_availability(&args.expression, now) {
        Some(avail) => serde_json::json!({
            "unknown": false,
            "start": avail.start,
            "end": avail.end,
            "description": avail.description,
        }),
        None => serde_json::json!({"unknown": true}),
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ParseTimeArgs {
    expression: String,
    now: String,
}

/// Parse a Chinese availability expression into a start/end window.
/// Default end is 22:00. Past times roll forward to tomorrow.
fn parse_availability(
    message: &str,
    now: chrono::DateTime<chrono::FixedOffset>,
) -> Option<Availability> {
    use chrono::{Duration, NaiveTime, TimeZone};
    use regex::Regex;

    let today = now.date_naive();
    let tomorrow = today.succ_opt()?;
    let default_end_time = NaiveTime::from_hms_opt(22, 0, 0).unwrap();
    let evening_start = NaiveTime::from_hms_opt(18, 0, 0).unwrap();

    let at_time = |date: chrono::NaiveDate, time: NaiveTime| {
        let dt = now
            .timezone()
            .from_local_datetime(&date.and_time(time))
            .single()?;
        if dt <= now {
            now.timezone()
                .from_local_datetime(&tomorrow.and_time(time))
                .single()
        } else {
            Some(dt)
        }
    };

    let normalized = message.to_lowercase();

    // Explicit ranges like "9点到10点", "21:00-22:00".
    let range_re = Regex::new(r"(?:(?:(\d{1,2}))\s*点?\s*(?::(\d{2}))?\s*)?(?:到|[-~])\s*(?:(\d{1,2}))\s*点?\s*(?::(\d{2}))?").unwrap();
    if let Some(caps) = range_re.captures(&normalized) {
        let start_hour: i32 = caps.get(1)?.as_str().parse().ok()?;
        let start_minute: i32 = caps
            .get(2)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0);
        let end_hour: i32 = caps.get(3)?.as_str().parse().ok()?;
        let end_minute: i32 = caps
            .get(4)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0);
        let start_time = NaiveTime::from_hms_opt(start_hour as u32, start_minute as u32, 0)?;
        let end_time = NaiveTime::from_hms_opt(end_hour as u32, end_minute as u32, 0)?;

        let start = at_time(today, start_time)?;
        let mut end = at_time(today, end_time)?;
        if end <= start {
            end = now
                .timezone()
                .from_local_datetime(&tomorrow.and_time(end_time))
                .single()?;
        }
        return Some(Availability {
            start: start.to_rfc3339(),
            end: end.to_rfc3339(),
            description: format!(
                "{}:{}到{}:{}",
                start_hour, start_minute, end_hour, end_minute
            ),
        });
    }

    // Relative time expressions.
    let minutes = parse_relative_minutes(&normalized);
    if minutes > 0 {
        let start = now + Duration::minutes(minutes);
        let end = start.date_naive().and_time(default_end_time);
        let end_dt = now.timezone().from_local_datetime(&end).single()?;
        let desc = if minutes < 60 {
            format!("{}分钟后到晚上10点", minutes)
        } else {
            format!("{}小时后到晚上10点", minutes / 60)
        };
        return Some(Availability {
            start: start.to_rfc3339(),
            end: end_dt.to_rfc3339(),
            description: desc,
        });
    }

    // "今晚", "今天晚上", "明天晚上".
    if normalized.contains("今晚") || normalized.contains("今天晚上") {
        let start = at_time(today, evening_start)?;
        let end = at_time(today, default_end_time)?;
        return Some(Availability {
            start: start.to_rfc3339(),
            end: end.to_rfc3339(),
            description: "今晚6点到晚上10点".to_string(),
        });
    }
    if normalized.contains("明天晚上") {
        let start = now
            .timezone()
            .from_local_datetime(&tomorrow.and_time(evening_start))
            .single()?;
        let end = now
            .timezone()
            .from_local_datetime(&tomorrow.and_time(default_end_time))
            .single()?;
        return Some(Availability {
            start: start.to_rfc3339(),
            end: end.to_rfc3339(),
            description: "明天晚上6点到晚上10点".to_string(),
        });
    }

    // Absolute hour like "8点", "20点", "晚上8点".
    let hour_re =
        Regex::new(r"(?:晚上|傍晚|下午)?\s*(\d{1,2})\s*点\s*(?:(\d{1,2})\s*分?)?").unwrap();
    if let Some(caps) = hour_re.captures(&normalized) {
        let mut hour: i32 = caps.get(1)?.as_str().parse().ok()?;
        if normalized.contains("晚上") || normalized.contains("傍晚") {
            if hour < 12 {
                hour += 12;
            }
        } else if normalized.contains("下午") && hour < 12 {
            hour += 12;
        }
        let minute: i32 = caps
            .get(2)
            .and_then(|m| m.as_str().parse().ok())
            .unwrap_or(0);
        let time = NaiveTime::from_hms_opt(hour as u32, minute as u32, 0)?;
        let start = at_time(today, time)?;
        let end = at_time(today, default_end_time)?;
        return Some(Availability {
            start: start.to_rfc3339(),
            end: end.to_rfc3339(),
            description: format!("{}点{}分后到晚上10点", hour, minute),
        });
    }

    // "现在", "马上".
    if normalized.contains("现在") || normalized.contains("马上") {
        let start = now;
        let end = at_time(today, default_end_time)?;
        return Some(Availability {
            start: start.to_rfc3339(),
            end: end.to_rfc3339(),
            description: "现在到晚上10点".to_string(),
        });
    }

    None
}

fn parse_relative_minutes(text: &str) -> i64 {
    use regex::Regex;

    let normalized = text.to_lowercase();

    if Regex::new(r"半\s*个?\s*小时\s*后")
        .unwrap()
        .is_match(&normalized)
    {
        return 30;
    }
    if Regex::new(r"一\s*个?\s*小时\s*后")
        .unwrap()
        .is_match(&normalized)
    {
        return 60;
    }
    if Regex::new(r"两\s*个?\s*小时\s*后|二\s*个?\s*小时\s*后")
        .unwrap()
        .is_match(&normalized)
    {
        return 120;
    }
    if Regex::new(r"三\s*个?\s*小时\s*后")
        .unwrap()
        .is_match(&normalized)
    {
        return 180;
    }

    let min_re = Regex::new(r"(\d+)\s*分\s*钟?\s*后").unwrap();
    if let Some(caps) = min_re.captures(&normalized) {
        if let Ok(n) = caps[1].parse::<i64>() {
            return n;
        }
    }

    let hour_re = Regex::new(r"(\d+)\s*个?\s*半?\s*小时\s*后").unwrap();
    if let Some(caps) = hour_re.captures(&normalized) {
        if let Ok(n) = caps[1].parse::<i64>() {
            let half = if normalized.contains("半") { 30 } else { 0 };
            return n * 60 + half;
        }
    }

    0
}
