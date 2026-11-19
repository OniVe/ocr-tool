use anyhow::{anyhow, bail, Result};
use base64::Engine;
use serde_json::{json, Value};

use ocr_core::OCR_LAYOUT_PROMPT;

/// Build the OpenAI-compatible chat-completions request understood by
/// `llama-server`. `temperature: 0` keeps OCR deterministic; the official
/// Chandra layout prompt is always used.
pub fn build_request(model: &str, jpeg: &[u8], max_tokens: u32) -> Value {
    let image = base64::engine::general_purpose::STANDARD.encode(jpeg);
    json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": [
                { "type": "text", "text": OCR_LAYOUT_PROMPT },
                { "type": "image_url", "image_url": { "url": format!("data:image/jpeg;base64,{image}") } }
            ]
        }],
        "temperature": 0,
        "max_tokens": max_tokens
    })
}

/// Extract the answer from a llama-server response.
///
/// With `--reasoning off` the HTML is in `message.content`. Some builds or
/// older flags place it in `message.reasoning_content`; support both, as well
/// as llama.cpp's native `/completion` shape (`content`) as a defensive extra.
pub fn parse_chat_response(body: &str) -> Result<String> {
    let value: Value =
        serde_json::from_str(body).map_err(|e| anyhow!("invalid JSON from llama-server: {e}"))?;

    let choice = value
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|c| c.first());

    if let Some(choice) = choice {
        if let Some(message) = choice.get("message") {
            if let Some(content) = non_empty_str(message.get("content")) {
                return Ok(content.to_string());
            }
            if let Some(reasoning) = non_empty_str(message.get("reasoning_content")) {
                return Ok(reasoning.to_string());
            }
        }
        if let Some(text) = non_empty_str(choice.get("text")) {
            return Ok(text.to_string());
        }
    }

    if let Some(text) = non_empty_str(value.get("content")) {
        return Ok(text.to_string());
    }

    if let Some(error) = value.get("error") {
        bail!("llama-server returned an error: {error}");
    }

    bail!(
        "llama-server response contains no OCR text (finish_reason={:?})",
        choice
            .and_then(|c| c.get("finish_reason"))
            .and_then(|v| v.as_str())
    )
}

fn non_empty_str(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_uses_official_prompt_temperature_zero_and_base64_image() {
        let request = build_request("chandra", &[0xff, 0xd8, 0xff, 0xd9], 321);
        assert_eq!(request["model"], "chandra");
        assert_eq!(request["temperature"], 0);
        assert_eq!(request["max_tokens"], 321);
        let content = &request["messages"][0]["content"];
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], OCR_LAYOUT_PROMPT);
        let url = content[1]["image_url"]["url"].as_str().unwrap();
        assert!(url.starts_with("data:image/jpeg;base64,"));
        assert!(url.ends_with("/2Q==")); // JPEG SOI/EOI bytes
    }

    #[test]
    fn parses_content() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"<div>Привет</div>"},"finish_reason":"stop"}]}"#;
        assert_eq!(parse_chat_response(body).unwrap(), "<div>Привет</div>");
    }

    #[test]
    fn falls_back_to_reasoning_content() {
        let body =
            r#"{"choices":[{"message":{"content":"","reasoning_content":"<div>Счет</div>"}}]}"#;
        assert_eq!(parse_chat_response(body).unwrap(), "<div>Счет</div>");
    }

    #[test]
    fn prefers_content_over_reasoning() {
        let body =
            r#"{"choices":[{"message":{"content":"<p>ответ</p>","reasoning_content":"думаю"}}]}"#;
        assert_eq!(parse_chat_response(body).unwrap(), "<p>ответ</p>");
    }

    #[test]
    fn parses_native_completion_shape() {
        let body = r#"{"content":"<p>текст</p>","stop":true}"#;
        assert_eq!(parse_chat_response(body).unwrap(), "<p>текст</p>");
    }

    #[test]
    fn empty_or_invalid_responses_are_errors() {
        assert!(parse_chat_response("not json").is_err());
        assert!(parse_chat_response(r#"{"choices":[{"message":{"content":"   "}}]}"#).is_err());
        assert!(parse_chat_response(r#"{"error":{"message":"boom"}}"#).is_err());
    }
}
