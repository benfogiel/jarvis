# Jarvis

A highly capable assistant.

Built with [Bevy](https://bevy.org).

## Usage

```sh
cp .env.example .env    # then add your API key
cargo run
```

The first run downloads a 150 MB speech recognition model and asks for microphone access. Building
needs `cmake` and a C++ compiler.

## Configuration

Everything is set in `.env`; [.env.example](.env.example) lists every option.

- **Language model:** set `JARVIS_LLM_PROVIDER` to `anthropic`, `openai`, `gemini`, `groq`,
  `mistral`, `xai`, `deepseek`, `openrouter`, `together`, `ollama`, `lmstudio` or
  `openai-compatible`, along with that provider's API key and `JARVIS_LLM_MODEL`.
- **Voice:** set `ELEVENLABS_API_KEY` and `JARVIS_ELEVENLABS_VOICE` to speak with an
  [ElevenLabs](https://elevenlabs.io) voice. Without them it uses the system voice.
- **Personality:** edit `INSTRUCTIONS` in [src/voice/mod.rs](src/voice/mod.rs).
