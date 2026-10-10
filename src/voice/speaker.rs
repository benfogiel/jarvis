use super::playback;
use bevy::log::error;
use std::env;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

const PROVIDER_VARIABLE: &str = "JARVIS_TTS_PROVIDER";
const ELEVENLABS_KEY_VARIABLE: &str = "ELEVENLABS_API_KEY";
const ELEVENLABS_VOICE_VARIABLE: &str = "JARVIS_ELEVENLABS_VOICE";
const ELEVENLABS_MODEL_VARIABLE: &str = "JARVIS_ELEVENLABS_MODEL";
const ELEVENLABS_DEFAULT_MODEL: &str = "eleven_flash_v2_5";
const ELEVENLABS_URL: &str = "https://api.elevenlabs.io/v1/text-to-speech";
/// Asked for as raw 16-bit mono so it can go straight to the output device without a decoder.
const ELEVENLABS_SAMPLE_RATE: u32 = 24_000;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

#[cfg(target_os = "macos")]
const SYSTEM_SYNTHESISER: (&str, &[&str]) = ("say", &[]);
#[cfg(target_os = "windows")]
const SYSTEM_SYNTHESISER: (&str, &[&str]) = (
    "powershell",
    &[
        "-NoProfile",
        "-Command",
        "Add-Type -AssemblyName System.Speech; \
         (New-Object System.Speech.Synthesis.SpeechSynthesizer).Speak([Console]::In.ReadToEnd())",
    ],
);
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const SYSTEM_SYNTHESISER: (&str, &[&str]) = ("espeak-ng", &["--stdin"]);

pub enum Speaker {
    System,
    ElevenLabs { api_key: String, voice: String, model: String },
}

fn setting(variable: &str) -> Option<String> {
    env::var(variable).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

impl Speaker {
    /// ElevenLabs is used when it is asked for or has a key; otherwise the operating system speaks.
    pub fn from_environment() -> Result<Self, String> {
        let api_key = setting(ELEVENLABS_KEY_VARIABLE);
        let provider = setting(PROVIDER_VARIABLE)
            .unwrap_or_else(|| if api_key.is_some() { "elevenlabs" } else { "system" }.to_string());
        match provider.to_ascii_lowercase().as_str() {
            "system" => Ok(Speaker::System),
            "elevenlabs" => Ok(Speaker::ElevenLabs {
                api_key: api_key.ok_or(format!("elevenlabs needs {ELEVENLABS_KEY_VARIABLE}"))?,
                voice: setting(ELEVENLABS_VOICE_VARIABLE)
                    .ok_or(format!("elevenlabs needs a voice ID in {ELEVENLABS_VOICE_VARIABLE}"))?,
                model: setting(ELEVENLABS_MODEL_VARIABLE).unwrap_or(ELEVENLABS_DEFAULT_MODEL.to_string()),
            }),
            other => Err(format!("unknown {PROVIDER_VARIABLE} \"{other}\"; expected system or elevenlabs")),
        }
    }

    /// Returns once the text has been spoken. A reply is never left unsaid: if the chosen voice
    /// fails, the operating system's voice is used instead.
    pub fn speak(&self, text: &str) -> Result<(), String> {
        if let Speaker::ElevenLabs { api_key, voice, model } = self {
            match synthesise_with_elevenlabs(api_key, voice, model, text) {
                Ok(samples) => return playback::play(samples, ELEVENLABS_SAMPLE_RATE),
                Err(error) => error!("{error}"),
            }
        }
        speak_with_system(text)
    }
}

fn synthesise_with_elevenlabs(api_key: &str, voice: &str, model: &str, text: &str) -> Result<Vec<f32>, String> {
    let failed = |error: ureq::Error| format!("ElevenLabs request failed: {error}");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .into();
    let mut response = agent
        .post(format!("{ELEVENLABS_URL}/{voice}?output_format=pcm_{ELEVENLABS_SAMPLE_RATE}"))
        .header("xi-api-key", api_key)
        .send_json(serde_json::json!({"text": text, "model_id": model}))
        .map_err(failed)?;
    let status = response.status();
    let body = response.body_mut().read_to_vec().map_err(failed)?;
    if !status.is_success() {
        return Err(format!("ElevenLabs answered {status}: {}", String::from_utf8_lossy(&body)));
    }
    Ok(body
        .chunks_exact(2)
        .map(|sample| i16::from_le_bytes([sample[0], sample[1]]) as f32 / -(i16::MIN as f32))
        .collect())
}

/// The text goes in on standard input so nothing in it can be mistaken for an option.
fn speak_with_system(text: &str) -> Result<(), String> {
    let (program, arguments) = SYSTEM_SYNTHESISER;
    let failed = |error: std::io::Error| format!("could not speak with {program}: {error}");
    let mut synthesiser =
        Command::new(program).args(arguments).stdin(Stdio::piped()).spawn().map_err(failed)?;
    if let Some(mut input) = synthesiser.stdin.take() {
        input.write_all(text.as_bytes()).map_err(failed)?;
    }
    synthesiser.wait().map_err(failed)?;
    Ok(())
}
