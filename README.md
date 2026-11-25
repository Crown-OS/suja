# Suja

Hold-to-talk voice assistant for CrownOS. By default everything runs on the laptop: a 4B language model on the GPU, speech recognition and synthesis on the CPU, tools for the shell, files, notes, media, volume, brightness, timers and the web, and a companion orb in crownbar.

## Layout

| Crate | Purpose |
|---|---|
| `suja-proto` | crownos-ipc contract shared with crownbar |
| `suja-model` | the model seam: transcript, tool calls, stream events and the `ChatModel` trait |
| `suja-model-llama` | llama.cpp backend: KV-cached session, Qwen prompt format, lazy tool-call grammar; feature `cuda` (default) |
| `suja-model-http` | credentials, server-sent events and errors for the remote backends |
| `suja-model-openai` | OpenAI-compatible backend: LM Studio, Ollama, llama-server, vLLM, OpenAI |
| `suja-model-anthropic` | Claude backend: Messages API, streamed tool use, prompt caching |
| `suja-model-cli` | agent CLIs as experts: Claude Code (`claude -p`) and Codex (`codex exec`), with their own read-only tools |
| `suja-router` | builds the base model, its fallback and the experts from the `model` section of `suja.ron` |
| `suja-agent` | the conversation, the tool-calling turn loop and the handoff to experts, for any `ChatModel` |
| `suja-tools` | the nine tools and their allow lists |
| `suja-speech` | sherpa-onnx wrappers: VAD, streaming recognizer, synthesizers, sentence chunker |
| `suja-context` | focused window, clipboard and primary selection as text |
| `sujad` | the daemon: hotkey, capture, playback, orchestration, IPC |
| `suja` | command line: `talk`, `cancel`, `say`, `ask`, `status`, `watch`, and with `--features local` (CPU) or `--features cuda` `chat`, `eval` |

## Models

```sh
packaging/fetch-models.sh ~/.cache/crownos/suja/models
```

The daemon looks in `/usr/share/crownos/suja/models` first, then `~/.cache/crownos/suja/models`.

## Models and handoff

A fast **base** model talks to you. **Experts** are stronger models. When a request needs careful
reasoning, planning or long writing, the base calls the `handoff` tool and the expert answers you
directly, with the same tools. Say "ask claude ..." to go to an expert directly. See the `model`
section of `packaging/suja.ron.example`.

A backend needs a name and a credential, no code: `Llama(...)`, `OpenAiCompatible(...)`,
`Anthropic(...)`, `ClaudeCode(...)` or `Codex(...)`. The two CLI backends use your own `claude`
or `codex` login and their own read-only tools. A credential is `None`, `Environment(variable: "...")` or
`Secret(name: "...")` (crownos-secretsd). The config never holds a key.

With `Secret`, sujad asks crownos-secretsd for an HTTPS proxy session. The proxy adds the key to
requests for the bound host, so the key never enters sujad. Set it up once:

```sh
crownos-secrets add anthropic_api_key --kind api-key
crownos-secrets bind anthropic_api_key --agent exe:/usr/bin/sujad \
    --http-host api.anthropic.com --http-header x-api-key --http-format '{value}'
```

For OpenAI use `--http-host api.openai.com --http-header Authorization --http-format 'Bearer {value}'`.

## Build and run

```sh
cargo build --release
cargo run --release -p sujad            # needs ~3.6 GiB of free VRAM
suja watch                              # events in another terminal
suja ask "how much disk space is free"  # a turn from typed text
```

The hotkey is `Super` by default. sujad asks crownpositor for it through `crownos_keybind_v1`, and the user can change it in Settings > Keybinds > Applications.

- Tap it (release before 1 s): sujad sends the `HotkeyTapped` event.
- Hold it: the microphone opens at 1 s. Release at 2 s or later to send what you said. An earlier release drops it.
- Press another key or a mouse button while you hold it: the recording is dropped, and the other chord works as usual.

`suja talk toggle` works without the hotkey.

## Checks

```sh
cargo fmt --check && cargo clippy --workspace -- -D warnings && cargo test --workspace
cargo run -p suja --features cuda -- eval eval/toolcalls.ron
cargo run -p suja --features cuda -- eval --models eval/models-with-expert.ron eval/handoff.ron
```
