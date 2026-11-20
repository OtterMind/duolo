#!/usr/bin/env bash
# Duolo 语音命令示例：用 sox 录音，用 whisper.cpp 在本地转写。
#
# 约定：Duolo 启动本脚本开始录音；结束时发送 SIGINT，脚本停止录音、
# 把识别出的文字打印到 stdout 后退出。
#
# 依赖：brew install sox whisper-cpp
# 模型：默认读取 ~/.duolo/models/ggml-large-v3-turbo.bin，可用 DUOLO_WHISPER_MODEL 指定，
#       下载地址 https://huggingface.co/ggerganov/whisper.cpp/tree/main
set -euo pipefail

MODEL="${DUOLO_WHISPER_MODEL:-$HOME/.duolo/models/ggml-large-v3-turbo.bin}"
LANGUAGE="${DUOLO_WHISPER_LANG:-auto}"

if ! command -v rec >/dev/null || ! command -v whisper-cli >/dev/null; then
  echo "缺少 sox 或 whisper-cpp：brew install sox whisper-cpp" >&2
  exit 1
fi
if [[ ! -f "$MODEL" ]]; then
  echo "找不到模型 $MODEL" >&2
  exit 1
fi

audio="$(mktemp -t duolo-voice).wav"
trap 'rm -f "$audio"' EXIT

rec -q -c 1 -r 16000 -b 16 "$audio" &
recorder=$!
# 收到 SIGINT 时只结束录音，脚本继续往下做转写。
trap 'kill -INT "$recorder" 2>/dev/null || true' INT TERM
wait "$recorder" || true
wait "$recorder" 2>/dev/null || true
trap - INT TERM

whisper-cli -m "$MODEL" -l "$LANGUAGE" -nt -np -f "$audio" 2>/dev/null \
  | tr -s '[:space:]' ' ' \
  | sed -e 's/^ *//' -e 's/ *$//'
