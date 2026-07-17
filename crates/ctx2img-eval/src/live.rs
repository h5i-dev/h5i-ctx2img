//! Optional live probing against the Anthropic API. Uses a `curl`
//! subprocess deliberately: no TLS/HTTP dependency tree for a feature most
//! builds never exercise, and trivially auditable. Requires
//! ANTHROPIC_API_KEY; degrades to offline-bundle mode without it.

use anyhow::{bail, Context, Result};
use serde_json::json;
use std::process::Command;

pub const DEFAULT_MODEL: &str = "claude-sonnet-5";

/// Ask one question about a PNG image. Returns the model's text answer.
pub fn ask_about_image(png: &[u8], legend: &str, question: &str, model: &str) -> Result<String> {
    let key = std::env::var("ANTHROPIC_API_KEY").context("ANTHROPIC_API_KEY not set")?;
    let b64 = base64(png);
    let payload = json!({
        "model": model,
        "max_tokens": 200,
        "messages": [{
            "role": "user",
            "content": [
                {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": b64}},
                {"type": "text", "text": format!("{legend}\n\n{question}")}
            ]
        }]
    });
    let tmp = std::env::temp_dir().join(format!("ctx2img-probe-{}.json", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec(&payload)?)?;
    let out = Command::new("curl")
        .args([
            "-s",
            "-S",
            "--max-time",
            "120",
            "https://api.anthropic.com/v1/messages",
        ])
        .args(["-H", &format!("x-api-key: {key}")])
        .args(["-H", "anthropic-version: 2023-06-01"])
        .args(["-H", "content-type: application/json"])
        .args(["-d", &format!("@{}", tmp.display())])
        .output()
        .context("running curl (required for --live)")?;
    let _ = std::fs::remove_file(&tmp);
    if !out.status.success() {
        bail!("curl failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).context("parse API response")?;
    if let Some(err) = v.get("error") {
        bail!("API error: {err}");
    }
    let text = v["content"]
        .as_array()
        .and_then(|blocks| {
            blocks
                .iter()
                .find_map(|b| b["text"].as_str().map(|s| s.to_string()))
        })
        .context("no text block in response")?;
    Ok(text)
}

/// Text-only variant (for the legend-only benchmark arm).
pub fn ask_text(prompt: &str, model: &str) -> Result<String> {
    let key = std::env::var("ANTHROPIC_API_KEY").context("ANTHROPIC_API_KEY not set")?;
    let payload = json!({
        "model": model,
        "max_tokens": 200,
        "messages": [{"role": "user", "content": prompt}]
    });
    let tmp = std::env::temp_dir().join(format!("ctx2img-probe-t-{}.json", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec(&payload)?)?;
    let out = Command::new("curl")
        .args([
            "-s",
            "-S",
            "--max-time",
            "120",
            "https://api.anthropic.com/v1/messages",
        ])
        .args(["-H", &format!("x-api-key: {key}")])
        .args(["-H", "anthropic-version: 2023-06-01"])
        .args(["-H", "content-type: application/json"])
        .args(["-d", &format!("@{}", tmp.display())])
        .output()
        .context("running curl (required for --live)")?;
    let _ = std::fs::remove_file(&tmp);
    if !out.status.success() {
        bail!("curl failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).context("parse API response")?;
    if let Some(err) = v.get("error") {
        bail!("API error: {err}");
    }
    v["content"]
        .as_array()
        .and_then(|blocks| {
            blocks
                .iter()
                .find_map(|b| b["text"].as_str().map(|s| s.to_string()))
        })
        .context("no text block in response")
}

pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(TABLE[(n >> 18 & 63) as usize] as char);
        out.push(TABLE[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_matches_reference() {
        assert_eq!(super::base64(b"Man"), "TWFu");
        assert_eq!(super::base64(b"Ma"), "TWE=");
        assert_eq!(super::base64(b"M"), "TQ==");
        assert_eq!(super::base64(b"hello world!"), "aGVsbG8gd29ybGQh");
    }
}
