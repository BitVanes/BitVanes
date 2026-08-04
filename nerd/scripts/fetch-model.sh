#!/usr/bin/env sh
# Fetch + verify the bundled Tier-2 NER model into nerd/models/.
#
# Pure POSIX sh + curl + sha256sum/shasum. No Python, no build tools, runs
# anywhere. The URLs + SHA256 values are pinned to match models/MANIFEST.toml
# (the source of truth) — change both together when bumping the model.
#
# Usage:
#   nerd/scripts/fetch-model.sh           # fetch into nerd/models/
#   nerd/scripts/fetch-model.sh /other/dir
#
# Exit codes: 0 success, 1 download/verify failure.
#
# The downloaded files are .gitignored — they're a build/release artifact, not
# source. CI / release-prep runs this; the runtime just loads them locally.

set -eu

REV=24c7e5aba9ae350923357a6f0b92571be34037ec
BASE="https://huggingface.co/Xenova/bert-base-NER/resolve/${REV}"

OUT="${1:-$(cd "$(dirname "$0")/.." && pwd)/models}"
mkdir -p "$OUT"

# name | url-path | expected-sha256
ARTIFACTS="
model_quantized.onnx|onnx/model_quantized.onnx|caaee70a5518ec7f9e46e5308fcc9263a8c227703a9ce46cf61c69a552349648
tokenizer.json|tokenizer.json|343989712a36cd8b253efeaf8baf6a08b9d2583f78e395e83832e8ee9f8d8ee1
"

# Pick a sha256 tool (Linux/Windows Git Bash ship sha256sum; macOS ships shasum).
if command -v sha256sum >/dev/null 2>&1; then
  SHA256() { sha256sum "$1" | cut -d' ' -f1; }
else
  SHA256() { shasum -a 256 "$1" | cut -d' ' -f1; }
fi

echo "Fetching BitVanes NER model into $OUT (Xenova/bert-base-NER @ ${REV})"
echo

printf '%s\n' "$ARTIFACTS" | grep -v '^$' | while IFS='|' read -r name path expected; do
  dest="$OUT/$name"
  if [ -f "$dest" ] && [ "$(SHA256 "$dest")" = "$expected" ]; then
    echo "  ✓ $name (cached, sha256 ok)"
    continue
  fi
  printf "  ↓ %s ... " "$name"
  if curl -fsSL --max-time 600 "${BASE}/${path}" -o "$dest"; then
    actual=$(SHA256 "$dest")
    if [ "$actual" = "$expected" ]; then
      echo "ok ($(wc -c < "$dest" | tr -d ' ') bytes, sha256 verified)"
    else
      echo "FAIL"
      echo "    sha256 mismatch: expected $expected" >&2
      echo "                   got      $actual" >&2
      rm -f "$dest"
      exit 1
    fi
  else
    echo "FAIL"
    echo "    download failed: ${BASE}/${path}" >&2
    exit 1
  fi
done

echo
echo "Done. nerd will load these from $OUT at runtime (no network at runtime)."
