// BalanceCraft's engine (crates/ffi): one call, JSON in and JSON out. See docs/mobile.md.
#pragma once

// Runs one request. The answer is never null and goes back to balancecraft_free once.
char *balancecraft_call(const char *request);

void balancecraft_free(char *answer);
