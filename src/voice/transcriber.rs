use bevy::log::info;
use std::path::{Path, PathBuf};
use std::{env, fs, io};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub const SAMPLE_RATE: u32 = 16_000;

const MODEL_VARIABLE: &str = "JARVIS_WHISPER_MODEL";
const DEFAULT_MODEL: &str = "ggml-base.en.bin";
const MODEL_SOURCE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

pub struct Transcriber {
    context: WhisperContext,
}

impl Transcriber {
    pub fn load() -> Result<Self, String> {
        whisper_rs::install_logging_hooks();
        let path = match env::var_os(MODEL_VARIABLE) {
            Some(path) => PathBuf::from(path),
            None => {
                let path = cache_directory()?.join(DEFAULT_MODEL);
                if !path.exists() {
                    download(&format!("{MODEL_SOURCE}/{DEFAULT_MODEL}"), &path)?;
                }
                path
            }
        };
        let context = WhisperContext::new_with_params(&path, WhisperContextParameters::default())
            .map_err(|error| format!("could not load the speech model at {}: {error}", path.display()))?;
        Ok(Self { context })
    }

    /// Returns an empty string when the audio held no speech.
    pub fn transcribe(&self, samples: &[f32]) -> Result<String, String> {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        // Asking an English-only model to detect the language is an error.
        params.set_language(Some(if self.context.is_multilingual() { "auto" } else { "en" }));
        params.set_no_context(true);
        params.set_suppress_blank(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        let mut state = self.context.create_state().map_err(|error| error.to_string())?;
        state.full(params, samples).map_err(|error| error.to_string())?;
        let text: String = state
            .as_iter()
            .filter_map(|segment| segment.to_str_lossy().ok().map(|text| text.into_owned()))
            .collect();
        let text = text.trim();
        // Whisper describes non-speech in brackets, such as "[BLANK_AUDIO]" or "(keyboard clicking)".
        if text.starts_with(['[', '(']) {
            return Ok(String::new());
        }
        Ok(text.to_string())
    }
}

fn cache_directory() -> Result<PathBuf, String> {
    let home = env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .ok_or("no home directory to keep the speech model in")?;
    Ok(PathBuf::from(home).join(".cache").join("jarvis"))
}

fn download(url: &str, destination: &Path) -> Result<(), String> {
    info!("downloading the speech model to {} (about 150 MB, first run only)", destination.display());
    let failed = |error: &dyn std::fmt::Display| format!("could not download {url}: {error}");
    let partial = destination.with_extension("part");
    fs::create_dir_all(destination.parent().unwrap_or(Path::new("."))).map_err(|error| failed(&error))?;
    let response = ureq::get(url).call().map_err(|error| failed(&error))?;
    let mut file = fs::File::create(&partial).map_err(|error| failed(&error))?;
    io::copy(&mut response.into_body().into_reader(), &mut file).map_err(|error| failed(&error))?;
    fs::rename(&partial, destination).map_err(|error| failed(&error))
}
