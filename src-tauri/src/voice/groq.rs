//! Groq's hosted Whisper, over the OpenAI-shaped transcription endpoint.
//!
//! Groq rather than OpenAI itself for the fallback: the same `whisper-large-v3`
//! weights cost $0.111 an hour there against $0.36, the turbo checkpoint costs
//! $0.04, and the endpoint is the one OpenAI documents — so a user who would
//! rather pay OpenAI, or run a compatible server of their own, only changes the
//! base URL.

use serde::Deserialize;

use super::engine::SttEngine;
use super::{capture::SAMPLE_RATE, wav};

const ENDPOINT: &str = "https://api.groq.com/openai/v1/audio/transcriptions";

/// Long enough for a slow upload on a poor connection, short enough that a
/// dictation which is never coming back says so while the user still remembers
/// what they said.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Fixed, and deliberately not derived from anything in the payload: the body
/// is audio and cannot contain it, so there is nothing to escape.
const BOUNDARY: &str = "----hangar-ai-voice-boundary";

pub struct GroqEngine {
    model: String,
    api_key: String,
}

impl GroqEngine {
    pub fn new(model: &str, api_key: &str) -> Result<Self, String> {
        if api_key.trim().is_empty() {
            return Err("no Groq API key: add one in Settings → Voice".into());
        }
        Ok(Self {
            model: model.to_string(),
            api_key: api_key.trim().to_string(),
        })
    }
}

#[derive(Deserialize)]
struct Transcription {
    text: String,
}

impl SttEngine for GroqEngine {
    fn transcribe(&mut self, samples: &[f32], language: Option<&str>) -> Result<String, String> {
        let audio = wav::encode(samples, SAMPLE_RATE);
        let body = multipart(&audio, &self.model, language);

        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .build()
            .new_agent();

        let mut response = agent
            .post(ENDPOINT)
            .header("authorization", &format!("Bearer {}", self.api_key))
            .header(
                "content-type",
                &format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .send(&body[..])
            .map_err(explain)?;

        let parsed: Transcription = response
            .body_mut()
            .read_json()
            .map_err(|err| format!("Groq sent something unreadable back: {err}"))?;
        Ok(parsed.text.trim().to_string())
    }
}

/// Turns the transport error into something worth showing in a toast.
///
/// ureq reports an HTTP failure as a status code and nothing else, and the two
/// that actually happen here — a key that is wrong, and an account out of
/// credit — are indistinguishable from "it did not work" unless they are named.
fn explain(err: ureq::Error) -> String {
    match err {
        ureq::Error::StatusCode(401) => "Groq rejected the API key".into(),
        ureq::Error::StatusCode(429) => "Groq rate limit reached — try again in a moment".into(),
        ureq::Error::StatusCode(413) => "the recording was too long for Groq to accept".into(),
        other => format!("Groq request failed: {other}"),
    }
}

fn multipart(audio: &[u8], model: &str, language: Option<&str>) -> Vec<u8> {
    let mut body = Vec::with_capacity(audio.len() + 512);

    let mut field = |name: &str, value: &str| {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    };

    field("model", model);
    // `json` rather than `verbose_json`: segments, timestamps and per-token
    // probabilities are all things dictation throws away.
    field("response_format", "json");
    if let Some(code) = language {
        field("language", code);
    }

    body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"speech.wav\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: audio/wav\r\n\r\n");
    body.extend_from_slice(audio);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());

    body
}
