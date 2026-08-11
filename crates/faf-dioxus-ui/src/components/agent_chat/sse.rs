//! Low-level SSE client for the agent chat wire protocol.

use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, Request, RequestInit, Response};

use super::events::AgentStreamEvent;

/// POST a question to an agent chat SSE endpoint and invoke `on_event` for
/// each parsed [`AgentStreamEvent`] as it arrives.
///
/// The endpoint must accept a JSON body `{"question": "..."}` and reply with
/// `text/event-stream` frames whose `data:` lines are JSON-serialized events.
pub async fn stream_agent_events(
    url: &str,
    question: &str,
    on_event: &mut impl FnMut(AgentStreamEvent),
) -> Result<(), String> {
    let window = web_sys::window().ok_or("no window")?;

    let body = serde_json::json!({ "question": question }).to_string();

    let headers = Headers::new().map_err(|e| format!("headers: {e:?}"))?;
    headers
        .append("Content-Type", "application/json")
        .map_err(|e| format!("headers: {e:?}"))?;

    let init = RequestInit::new();
    init.set_method("POST");
    init.set_body(&JsValue::from_str(&body));
    init.set_headers(&headers);

    let request =
        Request::new_with_str_and_init(url, &init).map_err(|e| format!("request: {e:?}"))?;

    let response_value = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("fetch: {e:?}"))?;
    let response: Response = response_value
        .dyn_into()
        .map_err(|e| format!("response cast: {e:?}"))?;

    if !response.ok() {
        let status = response.status();
        return Err(format!("HTTP {status}"));
    }

    let stream = response.body().ok_or("response has no body")?;
    let reader = stream
        .get_reader()
        .dyn_into::<web_sys::ReadableStreamDefaultReader>()
        .map_err(|e| format!("reader cast: {e:?}"))?;

    let mut buffer = String::new();
    loop {
        let result = JsFuture::from(reader.read())
            .await
            .map_err(|e| format!("read: {e:?}"))?;

        let done = js_sys::Reflect::get(&result, &JsValue::from_str("done"))
            .map_err(|e| format!("reflect done: {e:?}"))?
            .as_bool()
            .unwrap_or(true);

        if done {
            break;
        }

        let value = js_sys::Reflect::get(&result, &JsValue::from_str("value"))
            .map_err(|e| format!("reflect value: {e:?}"))?;
        let chunk = js_sys::Uint8Array::from(value).to_vec();
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some((event_text, rest)) = buffer.split_once("\n\n") {
            parse_sse_event(event_text, on_event);
            buffer = rest.to_string();
        }
    }

    if !buffer.is_empty() {
        parse_sse_event(&buffer, on_event);
    }

    Ok(())
}

fn parse_sse_event(text: &str, on_event: &mut impl FnMut(AgentStreamEvent)) {
    for line in text.lines() {
        let line = line.trim_start();
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim_start();
            if data.is_empty() {
                continue;
            }
            match serde_json::from_str::<AgentStreamEvent>(data) {
                Ok(event) => on_event(event),
                Err(e) => {
                    web_sys::console::error_1(&JsValue::from_str(&format!(
                        "failed to parse SSE event: {e}: {data}"
                    )));
                }
            }
        }
    }
}
