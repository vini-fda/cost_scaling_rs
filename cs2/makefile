# makefile for cs2
# Compiler definitions:
# - COMP_DUALS to compute prices
# - PRINT_ANS to print flow (and prices if COMP_DUAL defined)
# - COST_RESTART to be able to restart after a cost function change
# - NO_ZERO_CYCLES finds an optimal flow with no zero-cost cycles
# - CHECK_SOLUTION check feasibility/optimality. Note: HIGH OVERHEAD!

# Override on the command line, e.g. `make CCOMP=gcc release`.
CCOMP ?= clang

# Platform-specific bits:
#   * On Windows, clang produces cs2.exe and the math functions live in the
#     C runtime (no -lm). We also skip -flto because clang on Windows defaults
#     to link.exe, which can't consume LLVM bitcode (would need -fuse-ld=lld).
#   * Elsewhere we keep cs2 (no extension), -lm, and -flto.
ifeq ($(OS),Windows_NT)
  BIN = cs2.exe
  LIBS =
  LTO_FLAG =
else
  BIN = cs2
  LIBS = -lm
  LTO_FLAG = -flto
endif

SRC = cs2.c
HEADERS = types_cs2.h parser_cs2.c timer.c

# Program feature flags (toggle here)
COMPILER_DEFS = -DPRINT_ANS -DCOMP_DUALS -DCOST_RESTART

# Native-CPU tuning. `-march=native` is x86-centric; on AArch64 clang accepts
# `-mcpu=native` instead. Detect the host arch at make time. Override with
# `NATIVE_FLAG=` to disable (useful for cross-compilation).
HOST_ARCH := $(shell uname -m 2>/dev/null)
ifeq ($(filter aarch64 arm64,$(HOST_ARCH)),)
  NATIVE_FLAG ?= -march=native
else
  NATIVE_FLAG ?= -mcpu=native
endif

# Build modes
CFLAGS_RELEASE = -O3 $(NATIVE_FLAG) $(LTO_FLAG) -DNDEBUG $(COMPILER_DEFS)
CFLAGS_PROFILE = -O2 -g -fno-omit-frame-pointer $(NATIVE_FLAG) $(COMPILER_DEFS)
CFLAGS_DEBUG   = -O0 -g $(COMPILER_DEFS)

# Default target
all: release

release: $(SRC) $(HEADERS)
	$(CCOMP) $(CFLAGS_RELEASE) $(SRC) -o $(BIN) $(LIBS)

profile: $(SRC) $(HEADERS)
	$(CCOMP) $(CFLAGS_PROFILE) $(SRC) -o $(BIN) $(LIBS)

debug: $(SRC) $(HEADERS)
	$(CCOMP) $(CFLAGS_DEBUG) $(SRC) -o $(BIN) $(LIBS)

clean:
	rm -f $(BIN) *~
