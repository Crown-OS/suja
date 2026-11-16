# Suja

Hold-to-talk voice assistant for CrownOS. Everything runs on the laptop: a 4B language model on the GPU, speech recognition and synthesis on the CPU, tools for the shell, files, notes, media, volume, brightness, timers and the web, and a companion orb in crownbar.

## Layout

| Crate | Purpose |
|---|---|
| `suja-proto` | crownos-ipc contract shared with crownbar |
| `suja-llm` | llama.cpp engine, KV-cached session, Qwen prompt format, lazy tool-call grammar |
| `suja-tools` | the nine tools and their allow lists |
| `suja-agent` | language-model worker thread and the tool-calling turn loop |
| `suja-speech` | sherpa-onnx wrappers: VAD, streaming recognizer, synthesizers, sentence chunker |
| `suja-context` | focused window, clipboard and primary selection as text |
| `sujad` | the daemon: hotkey, capture, playback, orchestration, IPC |
| `suja` | command line: `talk`, `cancel`, `say`, `ask`, `status`, `watch`, and with `--features local` `chat`, `eval` |

## Models

```sh
packaging/fetch-models.sh ~/.cache/crownos/suja/models
```

The daemon looks in `/usr/share/crownos/suja/models` first, then `~/.cache/crownos/suja/models`.

## Build and run

```sh
cargo build --release
cargo run --release -p sujad            # needs ~3.6 GiB of free VRAM
suja watch                              # events in another terminal
suja ask "how much disk space is free"  # a turn from typed text
```

Hold the hotkey (`Super+Ctrl` by default, see `packaging/suja.ron.example`) and speak. The `input` group is needed for the evdev hotkey; `suja talk toggle` works without it.

## Checks

```sh
cargo fmt --check && cargo clippy --workspace -- -D warnings && cargo test --workspace
cargo run -p suja --features local -- eval eval/toolcalls.ron
```
