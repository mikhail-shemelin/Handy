use crate::audio_toolkit::encode_wav_bytes;
use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, REFERER, USER_AGENT};
use reqwest::Url;
use serde::Deserialize;
use std::net::IpAddr;
use std::time::Duration;

const MAX_UPLOAD_BYTES: usize = 25 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_ERROR_BODY_BYTES: usize = 16 * 1024;

pub struct OpenAiTranscriptionConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub language: Option<String>,
    pub prompt: Option<String>,
    pub translate_to_english: bool,
    pub include_logprobs: bool,
    pub chunking_enabled: bool,
}

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
    logprobs: Option<Vec<OpenAiTranscriptionLogprob>>,
}

#[derive(Debug)]
pub struct OpenAiTranscriptionResult {
    pub text: String,
    pub logprobs: Option<Vec<OpenAiTranscriptionLogprob>>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAiTranscriptionLogprob {
    pub logprob: f64,
}

pub async fn transcribe_samples(
    samples: &[f32],
    config: OpenAiTranscriptionConfig,
) -> Result<OpenAiTranscriptionResult> {
    transcribe_samples_with_timeouts(samples, config, REQUEST_TIMEOUT, CONNECT_TIMEOUT).await
}

async fn transcribe_samples_with_timeouts(
    samples: &[f32],
    config: OpenAiTranscriptionConfig,
    request_timeout: Duration,
    connect_timeout: Duration,
) -> Result<OpenAiTranscriptionResult> {
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
        .connect_timeout(connect_timeout)
        .timeout(request_timeout)
        .build()?;

    let file_part = reqwest::multipart::Part::bytes(wav_bytes)
        .file_name("recording.wav")
        .mime_str("audio/wav")?;

    let include_logprobs =
        config.include_logprobs && !config.translate_to_english && supports_logprobs(&model);

    let mut form = reqwest::multipart::Form::new()
        .part("file", file_part)
        .text("model", model.clone())
        .text("response_format", "json");

    if config.chunking_enabled && supports_chunking_strategy(&model) && !config.translate_to_english
    {
        form = form.text("chunking_strategy", "auto");
    }

    if !config.translate_to_english {
        if let Some(language) = config.language.filter(|value| !value.trim().is_empty()) {
            form = form.text("language", language);
        }
    }

    if let Some(prompt) = config.prompt.filter(|value| !value.trim().is_empty()) {
        form = form.text("prompt", prompt);
    }

    if include_logprobs {
        form = form.text("include[]", "logprobs");
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
        let error_text = read_error_body_limited(response)
            .await
            .replace(api_key, "[REDACTED]");
        return Err(anyhow!(
            "OpenAI transcription failed with status {}: {}",
            status,
            error_text
        ));
    }

    let transcription: TranscriptionResponse = response.json().await?;
    Ok(OpenAiTranscriptionResult {
        text: transcription.text,
        logprobs: transcription.logprobs,
    })
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

    if !url.username().is_empty() || url.password().is_some() {
        return Err(anyhow!(
            "OpenAI transcription endpoint must not include embedded credentials"
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

async fn read_error_body_limited(response: reqwest::Response) -> String {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    let mut truncated = false;

    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else {
            return "Failed to read error response".to_string();
        };
        let remaining = MAX_ERROR_BODY_BYTES.saturating_sub(body.len());
        if chunk.len() > remaining {
            body.extend_from_slice(&chunk[..remaining]);
            truncated = true;
            break;
        }
        body.extend_from_slice(&chunk);
        if body.len() == MAX_ERROR_BODY_BYTES {
            truncated = true;
            break;
        }
    }

    let mut text = String::from_utf8_lossy(&body)
        .chars()
        .map(|character| {
            if character.is_control() && !matches!(character, '\n' | '\r' | '\t') {
                '\u{FFFD}'
            } else {
                character
            }
        })
        .collect::<String>();
    if truncated {
        text.push_str("… [truncated]");
    }
    if text.trim().is_empty() {
        "OpenAI-compatible endpoint returned an empty error response".to_string()
    } else {
        text
    }
}

fn model_for_request(config: &OpenAiTranscriptionConfig) -> String {
    if config.translate_to_english {
        "whisper-1".to_string()
    } else {
        config.model.trim().to_string()
    }
}

fn supports_logprobs(model: &str) -> bool {
    matches!(model, "gpt-4o-transcribe" | "gpt-4o-mini-transcribe")
}

fn supports_chunking_strategy(model: &str) -> bool {
    matches!(
        model,
        "gpt-4o-transcribe" | "gpt-4o-mini-transcribe" | "gpt-4o-transcribe-diarize"
    )
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
        HeaderValue::from_static("https://github.com/mikhail-shemelin/Handy"),
    );
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("Handy-Hybrid/1.0 (+https://github.com/mikhail-shemelin/Handy)"),
    );
    headers.insert("X-Title", HeaderValue::from_static("Handy Hybrid"));
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
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    fn mock_server(
        status: &str,
        body: String,
        delay: Duration,
    ) -> (String, mpsc::Receiver<Vec<u8>>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (request_tx, request_rx) = mpsc::channel();
        let status = status.to_string();
        let handle = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            let header_end = loop {
                let count = socket.read(&mut buffer).unwrap_or(0);
                if count == 0 {
                    return;
                }
                request.extend_from_slice(&buffer[..count]);
                if let Some(position) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    break position + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())
                        .flatten()
                })
                .unwrap_or(0);
            while request.len() < header_end + content_length {
                let count = socket.read(&mut buffer).unwrap_or(0);
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..count]);
            }
            let _ = request_tx.send(request);
            thread::sleep(delay);
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes());
        });
        (format!("http://{address}/v1"), request_rx, handle)
    }

    fn mock_config(base_url: String) -> OpenAiTranscriptionConfig {
        OpenAiTranscriptionConfig {
            base_url,
            api_key: "sk-test-only".to_string(),
            model: "gpt-4o-transcribe".to_string(),
            language: Some("de".to_string()),
            prompt: Some("Names: Ada".to_string()),
            translate_to_english: false,
            include_logprobs: true,
            chunking_enabled: true,
        }
    }

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
        assert!(normalize_base_url("https://user:secret@example.com/v1").is_err());
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
            include_logprobs: false,
            chunking_enabled: true,
        };

        assert_eq!(model_for_request(&config), "whisper-1");
    }

    #[test]
    fn enables_auto_chunking_for_gpt_transcription_models_only() {
        assert!(supports_chunking_strategy("gpt-4o-transcribe"));
        assert!(supports_chunking_strategy("gpt-4o-mini-transcribe"));
        assert!(supports_chunking_strategy("gpt-4o-transcribe-diarize"));
        assert!(!supports_chunking_strategy("whisper-1"));
    }

    #[test]
    fn sends_completed_wav_as_non_streaming_multipart_request() {
        let (base_url, requests, server) = mock_server(
            "200 OK",
            r#"{"text":"hello","logprobs":[{"logprob":-0.25}]}"#.to_string(),
            Duration::ZERO,
        );
        let result = tauri::async_runtime::block_on(transcribe_samples(
            &[0.0, 0.25, -0.25],
            mock_config(base_url),
        ))
        .unwrap();
        assert_eq!(result.text, "hello");

        let request = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        server.join().unwrap();
        let request_text = String::from_utf8_lossy(&request);
        assert!(request_text.starts_with("POST /v1/audio/transcriptions HTTP/1.1"));
        assert!(request_text
            .to_ascii_lowercase()
            .contains("authorization: bearer sk-test-only"));
        assert!(request.windows(4).any(|part| part == b"RIFF"));
        assert!(request_text.contains("name=\"model\""));
        assert!(request_text.contains("gpt-4o-transcribe"));
        assert!(request_text.contains("name=\"language\""));
        assert!(request_text.contains("name=\"prompt\""));
        assert!(request_text.contains("name=\"chunking_strategy\""));
        assert!(request_text.contains("name=\"include[]\""));
        assert!(!request_text.contains("name=\"stream\""));
    }

    #[test]
    fn translation_uses_whisper_endpoint_without_transcription_only_fields() {
        let (base_url, requests, server) =
            mock_server("200 OK", r#"{"text":"hello"}"#.to_string(), Duration::ZERO);
        let mut config = mock_config(base_url);
        config.translate_to_english = true;
        tauri::async_runtime::block_on(transcribe_samples(&[0.0], config)).unwrap();

        let request = requests.recv_timeout(Duration::from_secs(2)).unwrap();
        server.join().unwrap();
        let request_text = String::from_utf8_lossy(&request);
        assert!(request_text.starts_with("POST /v1/audio/translations HTTP/1.1"));
        assert!(request_text.contains("whisper-1"));
        assert!(!request_text.contains("name=\"language\""));
        assert!(!request_text.contains("name=\"chunking_strategy\""));
        assert!(!request_text.contains("name=\"include[]\""));
    }

    #[test]
    fn error_body_is_bounded_and_redacts_the_api_key() {
        let secret = "sk-test-only";
        let body = format!(
            "server echoed {secret} {}",
            "x".repeat(MAX_ERROR_BODY_BYTES + 100)
        );
        let (base_url, _requests, server) = mock_server("401 Unauthorized", body, Duration::ZERO);
        let error =
            tauri::async_runtime::block_on(transcribe_samples(&[0.0], mock_config(base_url)))
                .unwrap_err()
                .to_string();
        server.join().unwrap();
        assert!(!error.contains(secret));
        assert!(error.contains("[REDACTED]"));
        assert!(error.contains("[truncated]"));
    }

    #[test]
    fn malformed_success_response_is_rejected() {
        let (base_url, _requests, server) =
            mock_server("200 OK", "not-json".to_string(), Duration::ZERO);
        let result =
            tauri::async_runtime::block_on(transcribe_samples(&[0.0], mock_config(base_url)));
        server.join().unwrap();
        assert!(result.is_err());
    }

    #[test]
    fn delayed_response_respects_request_timeout() {
        let (base_url, _requests, server) = mock_server(
            "200 OK",
            r#"{"text":"late"}"#.to_string(),
            Duration::from_millis(200),
        );
        let error = tauri::async_runtime::block_on(transcribe_samples_with_timeouts(
            &[0.0],
            mock_config(base_url),
            Duration::from_millis(40),
            Duration::from_millis(40),
        ))
        .unwrap_err()
        .to_string();
        server.join().unwrap();
        assert!(error.contains("timed out"));
    }

    #[test]
    fn cancelling_request_drops_in_flight_mock_upload() {
        let (base_url, requests, server) = mock_server(
            "200 OK",
            r#"{"text":"too late"}"#.to_string(),
            Duration::from_millis(200),
        );
        let task =
            tauri::async_runtime::spawn(transcribe_samples(&[0.0; 4096], mock_config(base_url)));

        // Prove the completed-audio request reached the isolated mock before
        // cancelling the task, rather than cancelling prior to any I/O.
        requests.recv_timeout(Duration::from_secs(2)).unwrap();
        task.abort();
        assert!(tauri::async_runtime::block_on(task).is_err());
        server.join().unwrap();
    }
}
