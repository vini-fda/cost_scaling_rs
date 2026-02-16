# makefile for cs2
# Compiler definitions:
# - COMP_DUALS to compute prices
# - PRINT_ANS to print flow (and prices if COMP_DUAL defined)
# - COST_RESTART to be able to restart after a cost function change
# - NO_ZERO_CYCLES finds an optimal flow with no zero-cost cycles
# - CHECK_SOLUTION check feasibility/optimality. Note: HIGH OVERHEAD!

# change these to suit your system
CCOMP=clang
CFLAGS=-g -Wall -DPRINT_ANS -DCOMP_DUALS -DCOST_RESTART
BIN=cs2

cs2: cs2.c parser_cs2.c types_cs2.h timer.c
	$(CCOMP) $(CFLAGS) -o $(BIN) cs2.c -lm

clean:
	rm -f $(BIN) *~
