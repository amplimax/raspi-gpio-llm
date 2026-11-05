#!/bin/sh
set -eu
voice_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
project_dir=$(dirname -- "$voice_dir")
export GPIO_LLM_URL="${GPIO_LLM_URL:-http://127.0.0.1:11434/v1/chat/completions}"
exec "$project_dir/target/release/gpio-llm" --voice
