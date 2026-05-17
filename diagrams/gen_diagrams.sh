#!/usr/bin/env bash
# Regenerate the README diagrams in light and dark variants.
# Run from any directory: `bash diagrams/gen_diagrams.sh`.
set -euo pipefail
cd "$(dirname "$0")"

typst compile sample.typ sample-light.png --ppi 200
typst compile sample.typ sample-dark.png  --ppi 200 --input theme=dark
