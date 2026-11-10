# Calibration cache and live-view publication

This package owns the bounded Rust cache consumer and parameter-publication implementation for X-M1 1.50. Source identity is x70view `55436fc85c0560dd252b0410af6f2427fe198cd3`; the source map is `reports/xm1-decomp.md`. The native harness binds IMAGE5 and IMAGE4 hashes before using their instruction bytes and the 504-byte defaults resource.

## Contracts

The cache ABI uses fixed backing at `0x41acd800`, size `0x420000` bytes (4.125 MiB). It does not qualify allocator failure or a null-cache path. Initialization, fallback, default-resource loading, reload windows and accessors compare the complete cache, pointer, storage-call journal, accessor results and preserved ABI. The primary path does not clear the whole cache. Fallback/default paths do. Deterministic storage callbacks do not qualify flash behavior.

Fallback/default cases require the Resource8-specific 16-million-instruction admission profile. Other cases retain four million. A stock aligned full clear executes 1,081,344 iterations of a three-instruction word loop; the default path clears twice. The linked Rust word loop uses four instructions per iteration. Four-million-instruction failures and rejected larger-budget requests remain preserved under their original wire/runner identities.

Publication snapshots each comparison group before writing, including aliased input. Its differential verdict compares complete state and retained-service receipts, ordered byte write values, returned status, callee-preserved registers, SP and CPSR control. White-balance copy receipts record r2 and require six bytes whenever called. Auxiliary scenario IDs use closed short labels: delta `first/second/both/equal/deferred`, mode `m0/m1/m2`, lanes `l0/l1/l2/l3`, auxiliary gates `g0/g1/g2/g3`. These labels retain every enum dimension; admission keeps the existing 64-character bound.

The private returning publication stack bound remains 208 bytes below entry SP. Linked owned roots use at most 200 bytes: entry 16, publisher 48, changed group 104, index-range next 24, length 8. Stock bare publisher uses at most 32 bytes and locked service 48. Retained callback receipts add no stack frame. All bytes outside the private span remain checked, including caller memory at and above entry SP. The caller-stack control must return with unchanged semantic results and `0xdeadbeef` at entry SP+4.

Returning owned-cache depths are at most 136 bytes for primary/fallback initialization, 152 for default initialization, 24 for reload and 32 for the accessor caller; the standalone defaults export uses at most 72. The stock cache roots use at most 44 bytes including the default loader and word-clear helper. These bounds fit the unchanged private span. Truncated cache provenance is not evidence of maximum stack depth; these bounds come from linked/source instructions.

Generic native-case reports may mark changed final memory as a diagnostic failure because these cases leave final memory unspecified in that outer model. The custom full-state differential verdict is separate. Runner protocol failure is never accepted as such a diagnostic mismatch. Publisher journals must contain complete write values; cache bulk-fill journals may truncate while complete cache bytes remain compared.

## Commands

Build the package and controls with the managed host launcher. Run native commands under the managed `emulator-slot --native` admission wrapper, not inside a second build reservation. `native-suite` runs at most four publisher cases concurrently in one scope.

```text
native-suite RUNNER IMAGE5 ELF OUTPUT
native-suite publisher-one RUNNER IMAGE5 ELF OUTPUT
native-suite publisher-case RUNNER IMAGE5 ELF OUTPUT NAME
native-suite publisher-rejudge RUNNER IMAGE5 ELF STOCK_CASE OWNED_CASE
native-suite publisher-resume RUNNER IMAGE5 ELF STOCK_CASE OWNED_CASE
native-suite publisher-control CONTROL RUNNER IMAGE5 CONTROL_ELF OUTPUT STOCK_CASE
native-suite cache RUNNER IMAGE4 ELF OUTPUT SCENARIO
native-suite cache-stage RUNNER IMAGE4 ELF OUTPUT SCENARIO
native-suite cache-execute RUNNER STOCK_CASE OWNED_CASE
```

`NAME` is `film`, `alias`, `request`, `white-balance`, `equality`, `aux-first`, `aux-second`, `aux-both`, `aux-equal` or `aux-deferred`. `CONTROL` is `wrong-field`, `wrong-dirty`, `ordering`, `request`, `alias`, `copy-zero`, `copy-five` or `caller-stack`. Cache scenarios are `primary`, `fallback`, `defaults`, `reload0`, `reload31`, `reload32`, `reload255`, `reload-max` and `accessors`.

Publisher rejudge and resume require the current IMAGE5 and ELF explicitly. They reconstruct the expected stock/owned pair from the selected closed scenario, gate and entry. Complete case contracts, initial registers, region permissions/bounds/bytes, callback bytes, function declarations and exact source-bound entry/code must match before native execution. Expected admission inputs are written under the owned case directory's `admission/` child. Rejudge authenticates both saved observations against case/wire/artifact and runner identities. Resume authenticates the saved stock observation and executes the admitted owned input. Stock-copy-as-owned is refused before invoking a runner. The obsolete three-input rejudge/resume forms are not supported.

## Scope and evidence

Historical observations remain bound to their original wire, runner and ELF identities. Do not relabel them as results from a changed binary. Qualification evidence lives under named private scratch roots and is not redistributable firmware. Preserve failures separately from passing custom verdicts. This package makes no camera, sensor, display, concurrency, installation or full-system qualification claim.
