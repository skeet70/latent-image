# Target payloads

This directory has no payload implementation. Current version-specific target hooks and linker layouts live in [`host/firmware/`](../host/firmware/) and [`menu/`](../menu/); the [recovery kit](../recovery/README.md) documents the demonstrated 1.50 repair. Those bindings do not establish a general AF payload ABI, safe RAM ownership or reversible AF execution. Language and boundary policy belongs to [current principles](../docs/principles.md#implementation-and-build): Haskell/Rust for decision/execution code, C only at technically necessary engine or ABI boundaries.

Before building a candidate for loading, record toolchain flags/hash, target ABI, disassembly, expected original bytes, external calls, exact write set, synchronization, entry/return/cancellation, stack/state budgets, and removal path. See [phase 4](../spec/03-phases-and-gates.md#phase-4--establish-a-reversible-execution-and-observation-loop).
