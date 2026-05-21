use crate::audio_toolkit::encode_wav_bytes;
use anyhow::{anyhow, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, REFERER, USER_AGENT};
use reqwest::Url;
use serde::Deserialize;
use std::net::IpAddr;
use std::time::Duration;

const MAX_UPLOAD_BYTES: usize = 25 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub struct OpenAiTranscriptionConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub language: Option<String>,
    pub prompt: Option<String>,
    pub translate_to_english: bool,
}

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
}

pub async fn transcribe_samples(
    samples: &[f32],
    config: OpenAiTranscriptionConfig,
) -> Result<String> {
    let api_key = config.api_key.trim();
    if api_key.is_empty() {
        return Err(anyhow!("OpenAI transcription API key is required"));
    }

    let model = model_for_request(&config);
    if model.is_empty() {
        return Err(anyhow!("OpenAI transcription model is required"));
    }

    let wav_bytes = encode_wav_bytes(samples)?;
    if wav_bytes.len() > MAX_UPLOAD_BYTES {
        return Err(anyhow!(
            "Recording is too large for OpenAI transcription ({} MB limit)",
            MAX_UPLOAD_BYTES / 1024 / 1024
        ));
    }

    let base_url = normalize_base_url(&config.base_url)?;
    let path = if config.translate_to_english {
        "translations"
    } else {
        "transcriptions"
    };
    let url = format!("{base_url}/audio/{path}");
    let client = reqwest::Client::builder()
        .default_headers(build_headers(api_key)?)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()?;

    let file_part = reqwest::multipart::Part::bytes(wav_bytes)
        .file_name("recording.wav")
        .mime_str("audio/wav")?;

    let mut form = reqwest::multipart::Form::new()
        .part("file", file_part)
        .text("model", model)
        .text("response_format", "json");

    if !config.translate_to_english {
        if let Some(language) = config.language.filter(|value| !value.trim().is_empty()) {
            form = form.text("language", language);
        }
    }

    if let Some(prompt) = config.prompt.filter(|value| !value.trim().is_empty()) {
        form = form.text("prompt", prompt);
    }

    let response = client
        .post(url)
        .multipart(form)
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                anyhow!("OpenAI transcription request timed out")
            } else {
                anyhow!("OpenAI transcription request failed: {}", error)
            }
        })?;
    let status = response.status();
    if !status.is_success() {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Failed to read error response".to_string());
        return Err(anyhow!(
            "OpenAI transcription failed with status {}: {}",
            status,
            error_text
        ));
    }

    let transcription: TranscriptionResponse = response.json().await?;
    Ok(transcription.text)
}

pub fn normalize_base_url(base_url: &str) -> Result<String> {
    let base_url = base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Err(anyhow!("OpenAI transcription endpoint is required"));
    }

    let url = Url::parse(base_url)
        .map_err(|_| anyhow!("OpenAI transcription endpoint must be a valid URL"))?;

    if url.query().is_some() || url.fragment().is_some() {
        return Err(anyhow!(
            "OpenAI transcription endpoint must not include query parameters or fragments"
        ));
    }

    match url.scheme() {
        "https" => Ok(base_url.to_string()),
        "http" if is_loopback_url(&url) => Ok(base_url.to_string()),
        "http" => Err(anyhow!(
            "OpenAI transcription endpoint must use HTTPS unless it points to localhost"
        )),
        _ => Err(anyhow!(
            "OpenAI transcription endpoint must use HTTPS, or HTTP for localhost"
        )),
    }
}

fn model_for_request(config: &OpenAiTranscriptionConfig) -> String {
    if config.translate_to_english {
        "whisper-1".to_string()
    } else {
        config.model.trim().to_string()
    }
}

fn is_loopback_url(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };

    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }

    host.trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
        .map(|address| address.is_loopback())
        .unwrap_or(false)
}

fn build_headers(api_key: &str) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", api_key))?,
    );
    headers.insert(
        REFERER,
        HeaderValue::from_static("https://github.com/cjpais/Handy"),
    );
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("Handy/1.0 (+https://github.com/cjpais/Handy)"),
    );
    headers.insert("X-Title", HeaderValue::from_static("Handy"));
    Ok(headers)
}

pub fn normalize_language(language: &str) -> Option<String> {
    match language {
        "auto" | "" => None,
        "zh-Hans" | "zh-Hant" => Some("zh".to_string()),
        other => Some(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_openai_language_codes() {
        assert_eq!(normalize_language("auto"), None);
        assert_eq!(normalize_language("zh-Hans").as_deref(), Some("zh"));
        assert_eq!(normalize_language("zh-Hant").as_deref(), Some("zh"));
        assert_eq!(normalize_language("en").as_deref(), Some("en"));
    }

    #[test]
    fn normalizes_secure_openai_base_urls() {
        assert_eq!(
            normalize_base_url(" https://api.openai.com/v1/ ").unwrap(),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            normalize_base_url("http://localhost:11434/v1/").unwrap(),
            "http://localhost:11434/v1"
        );
        assert_eq!(
            normalize_base_url("http://127.0.0.1:11434/v1").unwrap(),
            "http://127.0.0.1:11434/v1"
        );
        assert_eq!(
            normalize_base_url("http://[::1]:11434/v1").unwrap(),
            "http://[::1]:11434/v1"
        );
    }

    #[test]
    fn rejects_insecure_remote_openai_base_urls() {
        assert!(normalize_base_url("http://example.com/v1").is_err());
        assert!(normalize_base_url("ftp://example.com/v1").is_err());
        assert!(normalize_base_url("https://example.com/v1?token=value").is_err());
    }

    #[test]
    fn uses_whisper_for_openai_translation_endpoint() {
        let config = OpenAiTranscriptionConfig {
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: "sk-test".to_string(),
            model: "gpt-4o-transcribe".to_string(),
            language: Some("de".to_string()),
            prompt: None,
            translate_to_english: true,
        };

        assert_eq!(model_for_request(&config), "whisper-1");
    }
}
