#!/usr/bin/env bash
# Render the sources produced by examples/gen_diagrams.rs.
# Run from the repository root, or supply an absolute directory.
set -euo pipefail
diagram_dir="${1:-target/diagrams}"

for problem in goto netgen netgen-maxflow netgen-assignment; do
  for theme in light dark; do
    typst compile "$diagram_dir/$problem.typ" "$diagram_dir/$problem-$theme.png" \
      --ppi 144 --input "theme=$theme"
    typst compile "$diagram_dir/$problem.typ" "$diagram_dir/$problem-$theme-detail-{p}.png" \
      --ppi 144 --input "theme=$theme" --input view=details
  done
done
