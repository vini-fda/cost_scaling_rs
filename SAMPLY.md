# CPU Profiling with samply

[samply](https://github.com/mstange/samply) is a sampling CPU profiler for macOS and Linux that opens results in the [Firefox Profiler](https://profiler.firefox.com/) UI.

## Install

```bash
cargo install --locked samply
```

## Quick start

```bash
./profile.sh
```

This builds the binary using the `profiling` cargo profile (release optimizations with debug symbols, no stripping), generates test data if needed, runs 20 iterations of the 10k-node problem, and opens the Firefox Profiler in your browser.

## Options

```bash
./profile.sh                    # default: 20 iterations, 10k nodes, opens browser
./profile.sh --save-only        # save profile.json instead of opening browser
./profile.sh --iterations 50    # more iterations = more samples = better accuracy
./profile.sh --size 5000        # use 5k-node problem instead
```

## Viewing results

- **Live**: By default, `profile.sh` opens the Firefox Profiler at `http://localhost:3000+`.
- **Saved**: With `--save-only`, upload `profile.json` to https://profiler.firefox.com/ (click "Load a profile from file").

## What to look for

In the Firefox Profiler UI:

1. **Call Tree** tab — shows inclusive and self time per function. Sort by "Self" to find the hottest functions.
2. **Flame Graph** tab — visual overview of where time is spent.
3. Look for `cost_scaling_rs::` functions like `refine`, `discharge`, `price_update`, `relabel`.
4. If `core::panicking::panic_bounds_check` appears, bounds checking is a bottleneck and `unsafe` indexing in the hot loop may help.

## Troubleshooting

- **Empty symbols / `[unknown]` in profile**: The script builds with `--profile profiling` (defined in `Cargo.toml`), which keeps debug symbols and disables stripping. If symbols are still missing on macOS, generate a dSYM bundle manually:
  ```bash
  dsymutil target/profiling/cost-scaling-rs
  ```
  You can also resolve addresses after the fact with `atos`:
  ```bash
  atos -o target/profiling/cost-scaling-rs -l 0x100000000 <address>
  ```
- **Too few samples**: Increase `--iterations` or use a larger problem size. Each sample is taken every ~1ms, so a 300ms run only gets ~300 samples.
- **macOS permission errors**: samply cannot profile system-signed binaries (e.g. `/usr/bin/bash`). The script profiles the Rust binary directly to avoid this. If you still get permission errors, try running with `sudo` or check System Preferences > Privacy & Security > Developer Tools.

## Profiling the C reference implementation

The `cs2/` directory contains the original C implementation. To profile it:

### 1. Build with the profile target

```bash
cd cs2
make profile
```

This compiles with `-O2 -g -fno-omit-frame-pointer`, which keeps optimizations close to release while preserving debug symbols and frame pointers for accurate stack unwinding.

### 2. Run samply

The C binary reads DIMACS input from stdin, so use shell redirection:

```bash
samply record ./cs2/cs2 < testdata/case3.txt
```

This profiles the run and opens the Firefox Profiler in your browser.

To save the profile for later viewing instead:

```bash
samply record --save-only -o cs2-profile.json -- ./cs2/cs2 < testdata/case3.txt
samply load cs2-profile.json   # open it later
```

### 3. Tips

- **Use a large input** for meaningful samples. Small inputs finish in <1ms and produce very few samples. Generate larger GOTO problems with the Rust binary or use a bigger DIMACS file.
- **Don't wrap in `sh -c`** — samply cannot profile system-signed binaries on macOS. Always invoke `./cs2/cs2` directly.
- Since the C code uses `#include` to pull in `parser_cs2.c` and `timer.c`, all functions appear under `cs2.c` in the profiler. Look for `refine`, `discharge`, `price_update`, `relabel`, `price_refine` in the call tree.
