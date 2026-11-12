# X-M1 1.50 decompilation in x70view

Ledger: `E-XM1-DECOMP-SOURCE-150`. Pinned commit: x70view `55436fc85c0560dd252b0410af6f2427fe198cd3` (branch `decomp/xm1-coverage`, committed 2026-11-04T10:03:00-07:00). Every number below is at that commit.

## What the source is

x70view's `decomp/firmwares/xm1-150/` holds C for the X-M1 1.50 firmware (DAT SHA-256 `43d67afa1a2bc9ae8481bc0acf02cb085ed0f515122dcbd26addcae102cd1490`, the same DAT as ours), compiled with GCC 4.6.4 for ARM (`-marm -march=armv7-a -mfloat-abi=soft -O1 -fno-omit-frame-pointer`, per-file overrides in a first-line `/* flags: ... */` comment). Its build links each function at its stock address and compares the emitted bytes with the stock extent pinned in `function-ranges.json`; bytes of functions it does not reproduce stay stock. Many names, types and translation units were transferred from the X70 1.10 decomp (the donor).

Layout under `decomp/firmwares/xm1-150/`:

- C: `app/<subsystem>/...` (one build unit linking both main streams and the three overlays) and `kernel0/src`, `kernel1/src` (one build unit each). `tu_<address>.c` files are provisional address-ordered units; named files are understood code.
- Symbols: `symbols/kernel0.txt`, `symbols/kernel1.txt`, `symbols/app/*.txt`, rows `ADDRESS name` or `ADDRESS stem:name` (a name scoped to source files of that stem). The symbol address, not the name, places a definition: kernel0's `00040b00 FUN_00040b04` puts `FUN_00040b04` at `0x00040b00`.
- Extents: `function-ranges.json`, one row per function (41,910 rows): `address`, `size`, `sha256` of the stock bytes, `unit`, `section`, the section's `stock_sha256` and an inferred `name`.
- Recovered types: kernel sync, sched, object and time headers; the menu state at `0x009571d0` (`app/native_recovery/xm1_menu_state.h`); the UI command block at `0x0094f64c` (`app/native_ui_commands/xm1_ui_commands.h`); settings images (`app/settings/*.h`, `app/native_record_settings/xm1_record_settings.h`); OSAL dispatch tables (`app/platform/osal/dispatch/*.h`).

There is no boot ROM and no first-stage (boot record) tree for X-M1.

## Mapping

| x70view unit / section | Address range | Our image | File extent | SHA-256 (ours = `stock_sha256`) |
| --- | --- | --- | --- | --- |
| `main` / `stream-0016025c` | `0x0061c400`, 2,631,680 bytes | image 4, `private-data/analysis/x-m1-1.50/lz/id4-0061c400.bin` | whole file | `f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16` |
| `main` / `stream-002c025c` | `0x00bf1a00`, 5,185,120 bytes | image 5, `lz/id5-00bf1a00.bin` | whole file | `3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a` |
| `overlay44` | `0x01100000`, `0xac5a0` bytes | plain image, `lz/plain-01100000.bin` | offset 0 | `2dfaa5d4bd025d68e991a329cc1377d5da16de656148dc4027d9a6bc0c349f5c` |
| `overlay46` | `0x011c0000`, `0x1cca0` bytes | plain image | offset `0xc0000` | `a9dee59faaec1765d9f3a2dd7f75fd7d423a8b8061599b5d7f6752b6bdd4cf84` |
| `overlay47` | `0x011e0000`, `0x26a5e0` bytes | plain image | offset `0xe0000` | `0cf67b7221c3b0755092da0ae4c6d6fb817f2b32771ee53dff691fc59b28770c` |
| `kernel0` | `0x00040000`, 56,032 bytes | loader A (core 0), `decode/loader-40000.bin` | whole file | `21366ffff7cebc6176b8a8125bf3a9974835a51070a58ea325f7eed8aabd8882` |
| `kernel1` | `0x000d0000`, 55,968 bytes | loader B (core 1), `decode/loader-d0000.bin` | whole file | `8fb0c322a22443a3783cdab9071db17212a5629f6375646218ac1e043dc672ec` |

The overlay sizes are x70view's `firmware/x-m1/1.50/firmware.json` section sizes; the plain image's remaining bytes are padding to the next slice.

## Exactness rule

A function is byte-exact when it has a `function-ranges.json` row and a C definition in a `.c` file, outside preprocessor-excluded code, whose name is not marked NONMATCHING in the translation unit that compiles it, and the tree builds. x70view's default graph build (`decomp/tools/build_step.py`, `compare_result`) marks a name when the pattern `/\*\s*NONMATCHING:\s*(.*?)\s*\*/\s*\n[^\n(]*?\b(\w+)\(` (Python `re.S`) matches the source file: a `/* NONMATCHING: reason */` comment followed, on its next nonblank line, by that name and `(`. The build fails when an unmarked function differs from stock and when a marked function matches (`NOWOK`), so on a building tree marked and differing coincide. With `X70_BUILD_GRAPH=0` the legacy `build.py` path marks a name from every `.c` file of the unit instead.

The marker counts wherever the name follows it in that file, a prototype included. Generated files repeat markers above their prototype lists, and 1,463 NONMATCHING definitions at this commit have no NONMATCHING comment directly above the definition; the build finds their marker above a prototype or between a split head's return type and name. `dispatch_destroy_channel` (`0x00724f08`) is one: its marker is at `tu_007246f0.c:29`, above the prototype, and the definition at line 265 has none. Reading only the comment above a definition, as x70view's `build_index.py` does, reports those 1,463 as exact. When the line after a marker has no `name(` (a head split as `static int` then `name(...)`), the lazy reason runs on to the next `*/` in the file and marks the name after that comment instead.

The index gives every definition one status:

- `exact`: in a `.c` file, compiled, unmarked, with a range row.
- `nonmatching`: marked in its translation unit.
- `header`: defined in a header; whether the build emits and compares it is not determined here.
- `conditional`: inside `#if` code whose condition the index cannot decide.
- `excluded`: inside code the preprocessor drops (`#if 0`, or a branch decided by the translation unit's macros, such as hostap's `CONFIG_X70_RTOS`). It is not compiled; the function's stock bytes stay.
- `unranged`: placed by a symbol but without a range row, so never verified.
- `unplaced`: no symbol names it.

Conditions are evaluated per translation unit over its include closure (the unit's `-I` directory, `-Isrc/` and `-include` flags included) and its `-D` options, at the definition's closing brace; a head that straddles `#if` branches is shown from the `#if` before it. A macro definition counts as defined when the conditions around it hold under the unit's unconditional definitions, and as possibly defined when they are undecided; definition order is not modelled. A macro defined only in undecided code, or with a non-numeric body, leaves a condition undecided. Definitions generated by a function-like macro whose body defines a function named by a parameter, pasted after a prefix or not (`DISCOVERY_REJECT` in `ov47_05b_discovery_rejects.c`, `OUTPUT_PORT_COMPLETION` in `output_route_initialization.c`), are recognized at each invocation.

## Evidence rules

- Exact C (no NONMATCHING marking on a committed, building tree) is a byte-verified reconstruction of that function. A fact read from it is `source-inspected`, cited with the x70view commit.
- NONMATCHING C is a reading aid: arguments may be missing and types guessed. A fact taken from it needs confirmation from the bytes (disassembly), our tools or a run before it is cited as established.
- Names and types are inferences, many transferred from the X70 1.10 donor. A name is never evidence of X-M1 behaviour; the code is.
- Cite the commit (`build-support/xm1-decomp.sh rev`), not the branch: the branch moves and will merge into x70view main.
- The decomp is not product code. Owned code stays Rust; the decomp is the specification we translate from and test against.
- Read x70view through committed refs only. Its working tree is the owner's live work: never modify it, check it out or add worktrees there.

## Helper

`//build-support:xm1-decomp` (`build-support/xm1-decomp.sh`) reads x70view with `git show`, `git ls-tree` and `git archive` at a commit; it never touches x70view's working tree. It finds x70view beside the main checkout (`../x70view`, also from a worktree) unless `XM1_DECOMP_REPO` names it. `rev` prints the bare commit; every other command prints `commit SHA REV` first. `--rev REV` selects another commit (default `decomp/xm1-coverage`).

- `devenv shell -- bazel run //build-support:xm1-decomp -- rev` prints the commit the branch resolves to.
- `devenv shell -- bazel run //build-support:xm1-decomp -- lookup 0x00724f08` prints the function-ranges row (an address inside a function selects that function), every symbol at the address, and every C definition as `file:line`, status and, when marked, the marker's `file:line` and reason.
- `devenv shell -- bazel run //build-support:xm1-decomp -- lookup menu_cursor_dispatch_event --rev 55436fc85c0560dd252b0410af6f2427fe198cd3` looks up a name: symbol names, `stem:name` symbols, definitions and range names match whole, never callers or prototypes.
- `devenv shell -- bazel run //build-support:xm1-decomp -- show 0x0063270c` prints each definition's C, preceded by its marker line when one applies.
- `devenv shell -- bazel run //build-support:xm1-decomp -- coverage` prints the counts below.
- `devenv shell -- bazel run //build-support:xm1-decomp -- boundary` runs the boundary check against the pinned images of the mapping table; `SECTION=PATH:OFFSET:SIZE:BASE` arguments replace them. It exits 1 on any mismatched or unmapped row.

Lookups read an index of the commit, built once by `//build-support:xm1-decomp-index` (Haskell; definition recognition follows x70view's `cdefs.py`, header inclusion and symbol scoping its `coverage_sources.py` and `build_symbols.py`, markers its `build_step.py`, plus the preprocessor model above) and cached in `private-data/scratch/xm1-decomp/index/COMMIT-INDEXER.tsv`. `//tests:xm1-decomp` checks the commands against a fixture repository, including the extracted tree's removal.

## Coverage at 55436fc85c

Functions are function-ranges rows. With C: at least one non-excluded definition is placed at the row's address. A function is NONMATCHING when any of its definitions is, else exact when one definition is exact, else unverified (only header or conditional definitions). Definitions are counted, not marker lines: app has 14,856 marker lines for 14,747 NONMATCHING definitions in range plus one outside.

| Unit | Functions | With C | Exact | NONMATCHING | Unverified | Definitions | Exact | NONMATCHING | Header | Conditional |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| kernel0 (loader A) | 223 | 223 | 204 | 19 | 0 | 223 | 204 | 19 | 0 | 0 |
| kernel1 (loader B) | 219 | 219 | 200 | 19 | 0 | 219 | 200 | 19 | 0 | 0 |
| main stream-0016025c (image 4) | 9,494 | 9,344 | 5,810 | 3,534 | 0 | 9,344 | 5,810 | 3,534 | 0 | 0 |
| main stream-002c025c (image 5) | 20,658 | 20,415 | 12,919 | 7,496 | 0 | 20,416 | 12,920 | 7,496 | 0 | 0 |
| overlay44 | 1,703 | 1,543 | 787 | 756 | 0 | 1,543 | 787 | 756 | 0 | 0 |
| overlay46 | 422 | 419 | 309 | 110 | 0 | 420 | 310 | 110 | 0 | 0 |
| overlay47 | 9,191 | 8,703 | 5,779 | 2,851 | 73 | 8,729 | 5,796 | 2,851 | 76 | 6 |
| main streams | 30,152 | 29,759 | 18,729 | 11,030 | 0 | 29,760 | 18,730 | 11,030 | 0 | 0 |
| overlays | 11,316 | 10,665 | 6,875 | 3,717 | 73 | 10,692 | 6,893 | 3,717 | 76 | 6 |
| all | 41,910 | 40,866 | 26,008 | 14,785 | 73 | 40,894 | 26,027 | 14,785 | 76 | 6 |

The Functions to Unverified columns count functions, the rest definitions. That is 97.5% of functions with C and 62.1% exact (kernels 91.4%); by bytes, 95.4% and 30.4% of 8,672,100. The 1,044 functions without C have no compiled definition the index recognizes. 256 definitions lie in preprocessor-excluded code, 255 of them in hostap headers such as the `CONFIG_X70_RTOS` branches of `wpabuf.h` (`wpabuf_len` at `0x01333a74` among them). Definitions exceed functions with C by 28 because some functions are defined more than once. Outside the ranges: 660 app, 27 kernel0 and 27 kernel1 definitions have no symbol, and one app NONMATCHING definition (`FUN_01d8c388`) has a symbol but no range row.

## Boundary check

Every one of the 41,910 function-ranges rows was hashed from our own pinned images at its extent and compared with the row's `sha256`: 41,910 matched, 0 mismatched, 0 unmapped. Each of the seven section images also hashes to the rows' `stock_sha256`. The decomp's addresses and extents are therefore extents of our images. Rerun with `boundary` when the pin moves.

## Limitations

- NONMATCHING C is a draft; 14,785 functions are in that state.
- Names and types are inferences, many from the X70 donor.
- Exactness assumes the tree at the cited commit builds; this repository does not run x70view's build. The tip's message (`return 1 failing files to work in progress`) records the branch's own gate. A function defined twice is counted NONMATCHING when either definition is marked.
- The preprocessor model is partial: conditions it cannot decide give `conditional`, and a macro defined only by the compiler or an unseen command line would be misread as undefined. `header` definitions may never be emitted.
- The branch moves and will merge into x70view main; counts and lines hold at the cited commit only.
- No boot ROM or first-stage tree exists for X-M1.

## Owned application NAND CPU boundary

`replacement/application_nand/src/lib.rs` implements the task100 profile NAND request boundary with owned controller state, a fixed profile block map, spare-tag scanning, bad-block retirement, copy-on-write block replacement and NAND-masked byte programming. Native exports use the `xm1_app_nand_` prefix. Configuration, map mounting and the storage actor's metadata readiness are separate states. The actor owns readiness validation and cache maintenance; the NAND controller uses PIO rather than DMA.

Native exports use the owned PowerLifecycle OSAL semaphore exports, `xm1_power_semaphore_wait` and `xm1_power_semaphore_signal`. Semaphore 36 remains held from configuration through shutdown; semaphore 52 covers controller/map state transitions and requests. Acquisition/release failures are typed `SemaphoreFailure` values and returning request/configuration exports report failure. The legacy void shutdown export relies on the OSAL's error reporting. This source package requires those owned OSAL symbols at application link time.

Configuration requests the shared pad route `(selector 0x70a, owner 29, level 11)` through owned `xm1_power_resource_route_request`; shutdown releases level zero after semaphore 36 and before semaphore 52. This follows `0x006edbb8` through `0x006499d0` and requires the shared route constructor. The reached device-enable branch at `0x006f1c5c..0x006f1cec` updates presence byte `0x0094129c` using repeated reads of phase word `0x009412a0`. The NONMATCHING power-control C's 70-tick delay is absent from these actual bytes; the owned branch does not introduce it.

Interrupt mode uses owned PowerLifecycle current-task, clear-current-flags, flag-wait/notification and IRQ-registration exports. `events.rs` registers IRQ source `0x4a`, writes NAND completion acknowledgement `0x80009`, waits for task flag `0x100` with timeout plus ten, writes completion acknowledgement `0x80001`, and releases the IRQ token. Its ISR clears the private atomic waiter before notifying the real task. Cleanup attempts token release and clears the waiter; release failures propagate. No completion callback substitutes for a device interrupt.

The clear-current-flags provider is `xm1_power_clear_current_flags() -> i32` in owned PowerLifecycle `shutdown_rtos.rs`, commit `d943a3f2f5574037fe92f85c500ea7c12b3980a5`. It reuses the existing current-task/class encoding, calls owned `xm1_rtos_objects_family_flag_clear(id, 0)`, and returns the actual status. Application integration consumes that provider through PowerLifecycleNext.

`physical.rs` exposes typed `PhysicalNand` access under the same semaphore-52 guard, with bounded physical blocks `0..0x400`, block-state queries, reads, writes, erase policies and bad-block scanning. `xm1_app_nand_erase_physical` consumes the five-word `0x006fb13c` descriptor; `xm1_app_nand_write_physical` consumes the eight-word `0x006faf5c` descriptor. These are independent of the profile map. Their controller commands and bad-block marking reuse the profile driver's primitives without updating profile-map accounting. The physical erase/write CPU extents are `0x006fc4cc..0x006fc638` and `0x006fc638..0x006fca60`. Erase policy reads live configuration byte `0x276` through the pointer at `0x0096ca50`, matching `0x0069f8fc`; the configuration buffer must be initialized.

`xm1_app_nand_read_physical` consumes the six-word `0x006fba08` request: first physical block, block count, destination, include-spare boolean, event-mode boolean and timeout. The data transfer uses masked controller mode. With spare output enabled, each 2048-byte page reserves a 64-byte spare record, of which the stock CPU path writes only the first 16 bytes; the remaining 48 bytes remain unchanged.

The profile engine is now `ApplicationNand<E: MediaEndpoint>`: one mapping, replacement, verification and masked-write implementation is shared by the native `Controller` and the host `MemoryNand` image endpoint. `MemoryNand` owns data, spare, per-block failure plans, operation records, cancellation and an optional journal, and `MemoryProfile` mounts physical blocks `0x380..0x3ff` over it. The Audio profile write is `LogicalBlock::new(41)` with 33 blocks (`0x420000` bytes) through `MemoryProfile::write_blocks`; it uses the same mapper and replacement path as the native `xm1_app_nand_write_blocks` export, not a second mapper. Source generation `SharedProfileEndpointV1` is explicit in `ModelObservation`; native source commit `847d9707` stays frozen and its exported ABI, command order, semaphore/route guards, status mapping and cancellation remain the native path.

Two closed host laws are retained and named in the observation and `LawIdentity`: `G1StrictRejectBeforeMutation` (pinned FujiMod `3d90035d` `parts/nand.rs::program`, reject any 0->1 request before mutation) and `LegacyAndCommitFailedStatus` (the pinned FujiMod EXR II storage-controller completion behavior, project-synthetic for X-M1 hardware: AND the programmable bits, commit them, then report `ProgramFailed`). The strict law is the medium-part law; the legacy law is the controller-model law at the `MemoryProfile` seam. Neither is a hidden default; case and cache provenance must include the law identity and its `LawProvenance`. The pinned native `mark_bad` caller ignores `ProgramFailed` per page and returns success. On an erased block both laws program the bad marker on every page. On a programmed block the strict law rejects page 0 before mutation, leaving its OOB prefix unchanged, while the legacy law writes page 0's bad-marker byte and commits the ANDed data before reporting failure; both callers still return success. That difference is an open strict-law incompatibility, not a pass, and the synthetic legacy law does not discharge the G1 device, full-FTL or hardware gate.

`//replacement/application_nand:host-tests` compiles the same crate root as a host test and covers the Audio 41/33 round trip under both laws, strict rejection before mutation, legacy committed prefix, both `mark_bad` outcomes including page-0 OOB, cancellation/drop, journal overflow, injected read/program/erase failures, layout refusal and law provenance. No ARM, FTL, Audio or hardware qualification is claimed by this source generation.

The specification pin remains `55436fc85c0560dd252b0410af6f2427fe198cd3`. Request wrappers are `nand_operation_context.c: nand_configure_wait` (`0x006faf00`, extent `0x40`) and `nand_request_wrappers.c` (`shutdown 0x006faf40/0x1c`, indexed physical read `0x006fb058`, physical bad-block query `0x006fb1f4/0xac`, map build `0x006fb2a0/0xc8`, mapped block write `0x006fb38c/0x110`, mapped block read `0x006fb49c/0xe0`, mapped erase `0x006fb57c/0xc8`, mapped byte write `0x006fb7c8/0x13c`, mapped byte read `0x006fb904/0x104`). A mapped request descriptor's final word selects a mapping handle, not a physical identity map. Task100 startup supplies physical base `0x380`, physical count `0x80`, logical count `0x6c`, handle zero.

Deeper mapping and controller C is partly NONMATCHING. Held image4 bytes establish the following CPU details: initialization `0x006fbbe4..0x006fbce4` never reads its normalized request, saves the controller context, programs the controller through `0x006f0be4..0x006f0c7c`, and selects geometry table entry three at `0x00829844 + 3*24` (1024 blocks, 2048-byte pages, 64 pages per block). Configuration does not issue a ready callback or mount a map. The controller boundary is real volatile access to `0xfff52000`, including row/column setup, command packing, the PIO aperture at `+0x400`, busy polling and identification status bit 24. Page programming at `0x006f0f6c..0x006f1440` has no C definition at the pin; its PIO sector command/trigger sequence was read from the held bytes.

Application clock addresses are recovered from application bytes, not assumed from boot: `0x006edb70` forms `~0x77000 = 0xfff88fff`, and `0x006edb74/0x006edb7c` subtract `0xfd7`, addressing `0xfff88028`. Clock resource 34 at `0x007c86d4 + 34*8` contains `{1, 0x800}`; `0x00649c4c` selects `0xfff88030` and sets mask `0x800`. The owned controller enables those actual application clock bits. Clock enable remains set across shutdown, matching the retained resource lifetime.

The map scanner is `0x006fea48..0x006fed00`; tag preparation is `0x006ef084`, tag decoding `0x006fe9b0`, and free-block selection `0x006fd06c`. The indexed physical read loop at `0x006fc1bc..0x006fc408` decrements its requested count only for a good block (`0x006fc3cc`); its NONMATCHING C incorrectly decrements for bad blocks too. Native request parsing uses bounded typed block/byte ranges and reports an unavailable map rather than substituting physical identity.

Tagged reads return a typed completion extent separately from transport errors. Verification compares a full page only when all 2048 bytes were returned. An incomplete tagged read becomes a verification mismatch and takes the existing rewrite/retirement path; unread buffer tails do not establish successful verification.

The selected task100 descriptors use polling mode zero and observer zero. Both polling and real interrupt mode are implemented; nonzero indexed-read observers are rejected as `UnsupportedObserver`. Device completion, failure status, IRQ delivery and the persistence of programmed data remain physical NAND laws; source inspection does not qualify them on hardware.

No common boot/application polling primitive was extracted. Boot `0x2db8` reads status once before checking its counter (`0x2dd4`), then decrements and reads again (`0x2e08..0x2e10`), permitting `N+1` busy reads. Application `0x006f0b90..0x006f0be4` uses `max(625*timeout,1)` and permits `N` busy reads. Boot transfer reset writes command-column `0x8023`; application `nand_set_command_column` normalizes that to `0x80a3`. These instruction-level differences remain explicit.

`boot_skip.rs` reads the boot handoff through `[0x4061c00c]`, then the published header's `+4` field. It accepts the boot-owned table at `0x4310` only after checking the 314-entry bound, physical block range and strict ascending order. The storage actor can refresh that same table through the owned physical bad-block query before constructing a new read-only accessor.
