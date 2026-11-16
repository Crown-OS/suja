#!/bin/sh
# Installs the speech and language models suja needs into ${1:-/usr/share/crownos/suja/models}.
# Pass --skip-llm to leave the 2.3 GiB language model out (for example on a CI image).
set -eu

MODELS_ROOT="${1:-/usr/share/crownos/suja/models}"
SKIP_LLM="${2:-}"
SHERPA="https://github.com/k2-fsa/sherpa-onnx/releases/download"

fetch() {
    url="$1"; dest="$2"
    curl --fail --location --retry 3 --silent --show-error --output "$dest" "$url"
}

install -d -m 0755 "$MODELS_ROOT/vad" "$MODELS_ROOT/stt" "$MODELS_ROOT/tts" "$MODELS_ROOT/llm"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT INT TERM

fetch "$SHERPA/asr-models/silero_vad.onnx" "$MODELS_ROOT/vad/silero_vad.onnx"

for archive in \
    "asr-models/sherpa-onnx-streaming-zipformer-en-2023-06-26.tar.bz2:stt" \
    "tts-models/sherpa-onnx-pocket-tts-int8-2026-01-26.tar.bz2:tts"; do
    path="${archive%%:*}"; kind="${archive##*:}"
    fetch "$SHERPA/$path" "$workdir/model.tar.bz2"
    tar --extract --bzip2 --no-same-owner --file "$workdir/model.tar.bz2" --directory "$MODELS_ROOT/$kind"
done

if [ "$SKIP_LLM" != "--skip-llm" ]; then
    fetch "https://huggingface.co/unsloth/Qwen3-4B-Instruct-2507-GGUF/resolve/main/Qwen3-4B-Instruct-2507-Q4_K_M.gguf" \
        "$MODELS_ROOT/llm/Qwen3-4B-Instruct-2507-Q4_K_M.gguf"
fi
chmod -R a+rX "$MODELS_ROOT"
echo "suja models installed in $MODELS_ROOT"
