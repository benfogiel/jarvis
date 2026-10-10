use serde_json::{Value, json};
use std::env;
use std::time::Duration;

const PROVIDER_VARIABLE: &str = "JARVIS_LLM_PROVIDER";
const MODEL_VARIABLE: &str = "JARVIS_LLM_MODEL";
const API_KEY_VARIABLE: &str = "JARVIS_LLM_API_KEY";
const BASE_URL_VARIABLE: &str = "JARVIS_LLM_BASE_URL";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Anthropic requires a ceiling on every request. It is set well above a spoken reply because the
/// model's thinking counts towards it.
const ANTHROPIC_MAX_TOKENS: u32 = 16_000;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

pub struct Turn {
    pub role: Role,
    pub text: String,
}

/// Anything that can continue a conversation. Implement this to hook up a provider that speaks none
/// of the wire protocols below.
pub trait ChatModel: Send {
    fn reply(&self, instructions: &str, turns: &[Turn]) -> Result<String, String>;
}

#[derive(Clone, Copy)]
enum Protocol {
    OpenAi,
    Anthropic,
    Gemini,
}

struct Provider {
    name: &'static str,
    protocol: Protocol,
    base_url: Option<&'static str>,
    api_key_variable: Option<&'static str>,
    default_model: Option<&'static str>,
}

const fn openai_compatible(
    name: &'static str,
    base_url: &'static str,
    api_key_variable: Option<&'static str>,
) -> Provider {
    Provider {
        name,
        protocol: Protocol::OpenAi,
        base_url: Some(base_url),
        api_key_variable,
        default_model: None,
    }
}

/// Most providers speak OpenAI's chat completions protocol, so supporting a new one is usually a
/// line here, or no change at all with `openai-compatible` and a base URL.
const PROVIDERS: &[Provider] = &[
    Provider {
        name: "anthropic",
        protocol: Protocol::Anthropic,
        base_url: Some("https://api.anthropic.com/v1"),
        api_key_variable: Some("ANTHROPIC_API_KEY"),
        default_model: Some("claude-opus-5-5"),
    },
    openai_compatible("openai", "https://api.openai.com/v1", Some("OPENAI_API_KEY")),
    Provider {
        name: "gemini",
        protocol: Protocol::Gemini,
        base_url: Some("https://generativelanguage.googleapis.com/v1beta"),
        api_key_variable: Some("GEMINI_API_KEY"),
        default_model: None,
    },
    openai_compatible("groq", "https://api.groq.com/openai/v1", Some("GROQ_API_KEY")),
    openai_compatible("mistral", "https://api.mistral.ai/v1", Some("MISTRAL_API_KEY")),
    openai_compatible("xai", "https://api.x.ai/v1", Some("XAI_API_KEY")),
    openai_compatible("deepseek", "https://api.deepseek.com/v1", Some("DEEPSEEK_API_KEY")),
    openai_compatible("openrouter", "https://openrouter.ai/api/v1", Some("OPENROUTER_API_KEY")),
    openai_compatible("together", "https://api.together.xyz/v1", Some("TOGETHER_API_KEY")),
    openai_compatible("ollama", "http://localhost:11434/v1", None),
    openai_compatible("lmstudio", "http://localhost:1234/v1", None),
    Provider {
        name: "openai-compatible",
        protocol: Protocol::OpenAi,
        base_url: None,
        api_key_variable: None,
        default_model: None,
    },
];

fn setting(variable: &str) -> Option<String> {
    env::var(variable).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

fn chosen_provider() -> Result<&'static Provider, String> {
    let known = || PROVIDERS.iter().map(|provider| provider.name).collect::<Vec<_>>().join(", ");
    match setting(PROVIDER_VARIABLE) {
        Some(name) => PROVIDERS
            .iter()
            .find(|provider| provider.name.eq_ignore_ascii_case(&name))
            .ok_or_else(|| format!("unknown {PROVIDER_VARIABLE} \"{name}\"; expected one of: {}", known())),
        None => PROVIDERS
            .iter()
            .find(|provider| provider.api_key_variable.and_then(setting).is_some())
            .ok_or_else(|| format!("no language model configured; set {PROVIDER_VARIABLE} to one of: {}", known())),
    }
}

struct Endpoint {
    base_url: String,
    api_key: Option<String>,
    model: String,
}

pub fn from_environment() -> Result<Box<dyn ChatModel>, String> {
    let provider = chosen_provider()?;
    let name = provider.name;
    let endpoint = Endpoint {
        base_url: setting(BASE_URL_VARIABLE)
            .or(provider.base_url.map(String::from))
            .ok_or_else(|| format!("{name} needs {BASE_URL_VARIABLE}"))?
            .trim_end_matches('/')
            .to_string(),
        api_key: setting(API_KEY_VARIABLE).or_else(|| provider.api_key_variable.and_then(setting)),
        model: setting(MODEL_VARIABLE)
            .or(provider.default_model.map(String::from))
            .ok_or_else(|| format!("{name} needs {MODEL_VARIABLE}"))?,
    };
    if let (Some(variable), None) = (provider.api_key_variable, &endpoint.api_key) {
        return Err(format!("{name} needs an API key in {variable} or {API_KEY_VARIABLE}"));
    }
    Ok(match provider.protocol {
        Protocol::OpenAi => Box::new(OpenAiChat(endpoint)),
        Protocol::Anthropic => Box::new(AnthropicMessages(endpoint)),
        Protocol::Gemini => Box::new(GeminiContent(endpoint)),
    })
}

fn post(url: &str, headers: &[(&str, &str)], body: &Value) -> Result<Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .into();
    let mut request = agent.post(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut response = request.send_json(body).map_err(|error| format!("{url}: {error}"))?;
    let status = response.status();
    let text = response.body_mut().read_to_string().map_err(|error| format!("{url}: {error}"))?;
    if !status.is_success() {
        return Err(format!("{url} answered {status}: {text}"));
    }
    serde_json::from_str(&text).map_err(|error| format!("{url} sent unreadable JSON: {error}"))
}

fn spoken(text: String, response: &Value) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(format!("the model returned no text: {response}"));
    }
    Ok(text.to_string())
}

fn joined_text<'a>(parts: impl IntoIterator<Item = &'a Value>) -> String {
    parts.into_iter().filter_map(|part| part["text"].as_str()).collect()
}

struct OpenAiChat(Endpoint);

impl ChatModel for OpenAiChat {
    fn reply(&self, instructions: &str, turns: &[Turn]) -> Result<String, String> {
        let Endpoint { base_url, api_key, model } = &self.0;
        let mut messages = vec![json!({"role": "system", "content": instructions})];
        messages.extend(turns.iter().map(|turn| {
            let role = match turn.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            json!({"role": role, "content": turn.text})
        }));
        let authorization = api_key.as_ref().map(|key| format!("Bearer {key}"));
        let headers: Vec<(&str, &str)> =
            authorization.iter().map(|value| ("Authorization", value.as_str())).collect();
        let response = post(
            &format!("{base_url}/chat/completions"),
            &headers,
            &json!({"model": model, "messages": messages}),
        )?;
        let text = response["choices"][0]["message"]["content"].as_str().unwrap_or_default();
        spoken(text.to_string(), &response)
    }
}

struct AnthropicMessages(Endpoint);

impl ChatModel for AnthropicMessages {
    fn reply(&self, instructions: &str, turns: &[Turn]) -> Result<String, String> {
        let Endpoint { base_url, api_key, model } = &self.0;
        let messages: Vec<Value> = turns
            .iter()
            .map(|turn| {
                let role = match turn.role {
                    Role::User => "user",
                    Role::Assistant => "assistant",
                };
                json!({"role": role, "content": turn.text})
            })
            .collect();
        let response = post(
            &format!("{base_url}/messages"),
            &[
                ("x-api-key", api_key.as_deref().unwrap_or_default()),
                ("anthropic-version", ANTHROPIC_VERSION),
            ],
            &json!({
                "model": model,
                "max_tokens": ANTHROPIC_MAX_TOKENS,
                "system": instructions,
                "messages": messages,
            }),
        )?;
        if response["stop_reason"] == "refusal" {
            return Err(format!("the model declined to answer: {}", response["stop_details"]));
        }
        spoken(joined_text(response["content"].as_array().into_iter().flatten()), &response)
    }
}

struct GeminiContent(Endpoint);

impl ChatModel for GeminiContent {
    fn reply(&self, instructions: &str, turns: &[Turn]) -> Result<String, String> {
        let Endpoint { base_url, api_key, model } = &self.0;
        let contents: Vec<Value> = turns
            .iter()
            .map(|turn| {
                let role = match turn.role {
                    Role::User => "user",
                    Role::Assistant => "model",
                };
                json!({"role": role, "parts": [{"text": turn.text}]})
            })
            .collect();
        let response = post(
            &format!("{base_url}/models/{model}:generateContent"),
            &[("x-goog-api-key", api_key.as_deref().unwrap_or_default())],
            &json!({
                "system_instruction": {"parts": [{"text": instructions}]},
                "contents": contents,
            }),
        )?;
        let parts = response["candidates"][0]["content"]["parts"].as_array();
        spoken(joined_text(parts.into_iter().flatten()), &response)
    }
}
