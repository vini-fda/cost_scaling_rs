# makefile for cs2
# Compiler definitions:
# - COMP_DUALS to compute prices
# - PRINT_ANS to print flow (and prices if COMP_DUAL defined)
# - COST_RESTART to be able to restart after a cost function change
# - NO_ZERO_CYCLES finds an optimal flow with no zero-cost cycles
# - CHECK_SOLUTION check feasibility/optimality. Note: HIGH OVERHEAD!

CCOMP = clang
BIN = cs2

SRC = cs2.c parser_cs2.c timer.c

HEADERS = types_cs2.h

# Program feature flags (toggle here)
COMPILER_DEFS = -DPRINT_ANS -DCOMP_DUALS -DCOST_RESTART

# Build modes
CFLAGS_RELEASE = -O3 -march=native -flto -DNDEBUG $(COMPILER_DEFS)
CFLAGS_PROFILE = -O2 -g -fno-omit-frame-pointer -march=native $(COMPILER_DEFS)
CFLAGS_DEBUG   = -O0 -g $(COMPILER_DEFS)

# Default target
all: release

release: $(SRC) $(HEADERS)
	$(CCOMP) $(CFLAGS_RELEASE) $(SRC) -o $(BIN) -lm

profile: $(SRC) $(HEADERS)
	$(CCOMP) $(CFLAGS_PROFILE) $(SRC) -o $(BIN) -lm

debug: $(SRC) $(HEADERS)
	$(CCOMP) $(CFLAGS_DEBUG) $(SRC) -o $(BIN) -lm

clean:
	rm -f $(BIN) *~
