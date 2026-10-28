# Area-3 record validator replacement

The replacement for the X-M1 1.50 area-3 record validator at `0x006a84d4` and the executable contract that qualifies it. This package is host/emulator only; it does not touch persistent settings.

## Contract

[contracts/area3-validator-150.json](contracts/area3-validator-150.json) is the typed target contract. `//replacement/validator:area3-qualify` loads it, then compiles it against the three retained native cases in `private-data/analysis/x-m1-1.50/native-case/area3-validator/`: entry, engine profile, function extents, artifact digests, instruction and wall budgets, and the independently grounded accepted/corrected/rejected results must all agree before anything executes. A contract that disagrees is refused, not weakened.

Record layout, confirmed on `record-accept.bin` (the independently recomputed row `0x0000015c` and column `0x8d6dd54f` in the area-3 README section):

| Offset | Meaning |
| --- | --- |
| `+0x00..+0x2b` | 11 little-endian protected data words (352 bits) |
| `+0x2c..+0x2f` | row parity, bit `i` = parity of protected word `i`, bits 0..10 |
| `+0x30..+0x33` | column parity, XOR of the 11 protected words |

At the stock boundary `r0` is the 52-byte record buffer and `r1` points at the protected-length word `0x2c`; `r0` returns 0 accept or 1 reject. A single protected-data bit error is corrected in place and accepted; every other syndrome is rejected without modifying the record.

The contract binds the permitted change set: one record byte on a corrected outcome, scratch writes only in the declared stack region, no persistent change. The stock ABI state `r4-r11` and `sp` must be preserved and `pc` must reach the case's exit sentinel.

## Candidate

[validator.rs](validator.rs) is the `no_std` Rust replacement. It has no allocation, no runtime dependency beyond `core`, and exposes both the typed API and the stock ABI `extern "C" fn area3_validate(*mut u8, *const u32) -> u32`. ARM compiler and ABI qualification belongs to the shared build contract (`//replacement/toolchain`); this package consumes that artifact and does not qualify a host ABI as completion.

[faulty.rs](faulty.rs) is the plausible faulty candidate used as a negative control: it treats row parity as the whole integrity check, so an even number of bit flips inside one word is accepted without correction. It must fail the qualification, and the reduced counterexample reports which two-bit record exposes it.

[adapter.rs](adapter.rs) is a line-protocol host driver over the same candidate code. It refuses out-of-domain requests with the contract's closed refusal set: unsupported protected length, nonzero stored row-parity high bits, malformed record hex, malformed length field, unknown verb.

## Running

```sh
bazel build --jobs=1 //replacement/validator:area3-qualify
bazel-bin/replacement/validator/area3-qualify \
  --case-dir private-data/analysis/x-m1-1.50/native-case/area3-validator
```

The contract, host candidate, native runner, ARM image and ARM symbols resolve from the binary's declared Bazel data; only the owner-supplied case directory is passed. An explicit `--contract/--candidate/--runner/--arm-image/--arm-symbols/--arm-base` overrides the declared artifact.

`--scope capability` (the default) requires the retained native cases, the native runner and the cross-compiled ARM candidate. `--scope logic` confines the run to the host adapter and the contract oracle; it never reports capability. In either scope, `status: partial` means the declared domain was not fully executed.

`--mutations all|single|two` selects the mutation domain, `--stock all|none|COUNT` bounds original-binary execution, `--limit` bounds the selected subjects, and `--jobs` bounds the parallel native cases (default 1). `--faulty PATH` adds the negative control and its reduced counterexample. No result is cached: a run always re-reads the contract, cases and candidate, so the evidence identity is the invocation itself.

Exit codes: 0 qualified, 1 behavioral failure, 2 refused input, 3 partial, blocked or unknown.

## Coverage

The selected record is `record-accept` (52 bytes, SHA-256 pinned in the contract). The sweep enumerates the complete protected-data mutation domain for that record:

- 352 single-bit flips: every one must be corrected back to the original record.
- 61,776 distinct two-bit flips: every one must be rejected without modification.
- 56 independently constructed record/parity cases: additionally constructed valid records with recomputed parity, single-bit errors on those records, single row-parity-bit and column-parity-bit corruptions, and row+column parity pairs.

Coverage excludes other records, parity-byte mutations inside the 352/61,776 enumeration, stored row-parity bits 11..31, storage reads, ECC visibility, timing, writeback and camera actions. The stock writer `0x006a8484` is not used: constructed records come from the documented parity construction and their expectations are confirmed by original-binary execution, not by the writer or the replacement.

## Coverage prerequisite

`status: qualified` and `capability: true` require the complete declared domain in one run: all three grounded anchors, the 352 single-bit and 61,776 two-bit mutations, all 56 independently constructed record/parity cases, original-binary execution of every one of them, and the ARM candidate executed in the native harness on every one of them, each returned with matching observations, preserved ABI state and permitted effects. `--mutations single|two`, `--stock none|COUNT` and `--limit` narrow the run; the report names the gaps in `coverage_gaps` and the status stays `partial` or `blocked`. A sampled run cannot qualify.

## Qualified run

The complete capability run passed on 2026-10-28 under the fixed containment. Outside the slot: `XM1_EMULATOR_SLOTS=1 XM1_EMULATOR_MEMORY_MAX=8G`. Inside the canonical emulator-slot payload: `taskset -c 0,1 cpulimit -i -l75 -- env 'GHCRTS=-N1 -M4G -A16m' area3-qualify ... --jobs 1`. `-i` includes the native runner children, and the RTS limits apply only to the real command. The unchanged 85 C guard and the 8 GiB swap-free cap apply to the whole slot.

- Anchors 3/3; original closure 62,184/62,184; ARM candidate 62,184/62,184; refusal probes 6/6.
- 352 single-bit, 61,776 two-bit and 56 record/parity subjects all returned and matched under both executions, with preserved ABI state and permitted effects.
- The host faulty candidate was detected on 5,488 subjects. Its reduced witness is four flips in one protected word reduced to two (byte 0, bits 0 and 1), and the original binary rejects it.
- The cross-compiled faulty ARM artifact was run as a bounded control on that exact witness: the ARM case returned with r0 expected 1 and observed 0, the original binary rejected the witness, and the anchors and the area3 ABI and effect checks stayed satisfied.
- 41 min 34 s service runtime, 1.6 GiB peak, exit 0, `status: qualified`, `capability: true`.

## Artifact identities

The frozen run's raw report is `full-20261028.json` under `private-data/analysis/x-m1-1.50/area3-qualification/`, SHA-256 `18eced51de1b98cfd59e3500f797d466d5ea5ab81592977bd28c2b313f1369dc`. The same directory holds `identities-20261028.json` with the post-run digests of the contract, native runner, host candidate and both ARM images and symbol tables. Each generated case binds the ARM image digest and the case loader verifies every artifact digest at execution.

## Qualification outcomes

| Runner diagnostic | Qualification |
| --- | --- |
| `returned` with oracle equality and permitted effects | pass |
| `returned` with any observation or effect mismatch | behavioral failure |
| write outside the record correction and scratch regions | behavioral failure |
| `unknown_call`, `unsupported_instruction` | unknown; prevents qualification |
| `engine_error`, memory/fetch guard, stub contract, invalid case, host timeout | blocked; prevents qualification |
| wall or instruction budget exhaustion | blocked; prevents qualification |
| `memory_provenance_total` above the engine's retained 4096 entries | truncated effect evidence; prevents qualification |

The native adapter's `passed` status is not sufficient for a required positive: this command additionally requires `status: returned`, the contract oracle, the ABI state and the effect check. A matching negative diagnostic is recorded as a diagnostic, not as capability success.
