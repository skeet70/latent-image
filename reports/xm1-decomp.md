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

A function is byte-exact when its C definition's name is not marked NONMATCHING in the translation unit that compiles it, its address has a `function-ranges.json` row, and the tree builds. x70view's build (`decomp/tools/build_step.py`, `compare_result`) marks a name when the pattern `/\*\s*NONMATCHING:\s*(.*?)\s*\*/\s*\n[^\n(]*?\b(\w+)\(` matches the source file: a `/* NONMATCHING: reason */` comment followed, on its next nonblank line, by that name and `(`. The build fails when an unmarked function differs from stock and when a marked function matches (`NOWOK`), so on a building tree marked and differing coincide.

The marker counts wherever the name follows it in that file, a prototype included. Generated files repeat markers above their prototype lists, and 1,463 NONMATCHING definitions at this commit carry no marker directly above the definition, only above a prototype. `dispatch_destroy_channel` (`0x00724f08`) is one: its marker is at `tu_007246f0.c:29`, above the prototype, and the definition at line 265 has none. Reading only the line above a definition, as the first lookup guide and x70view's `build_index.py` do, reports those 1,463 as exact. Conversely a marker followed by a split head (`static int` on one line, the name on the next) marks nothing; there are none at this commit.

A function with C but no range row is unverified, never exact. Definitions without a symbol are not placed at any address.

## Evidence rules

- Exact C (no NONMATCHING marking on a committed, building tree) is a byte-verified reconstruction of that function. A fact read from it is `source-inspected`, cited with the x70view commit.
- NONMATCHING C is a reading aid: arguments may be missing and types guessed. A fact taken from it needs confirmation from the bytes (disassembly), our tools or a run before it is cited as established.
- Names and types are inferences, many transferred from the X70 1.10 donor. A name is never evidence of X-M1 behaviour; the code is.
- Cite the commit (`build-support/xm1-decomp.sh rev`), not the branch: the branch moves and will merge into x70view main.
- The decomp is not product code. Owned code stays Rust; the decomp is the specification we translate from and test against.
- Read x70view through committed refs only. Its working tree is the owner's live work: never modify it, check it out or add worktrees there.

## Helper

`//build-support:xm1-decomp` (`build-support/xm1-decomp.sh`) reads x70view with `git show`, `git ls-tree` and `git archive` at a commit; it never touches x70view's working tree. It finds x70view beside the main checkout (`../x70view`, also from a worktree) unless `XM1_DECOMP_REPO` names it. Every command prints `commit SHA REV` first; `--rev REV` selects another commit (default `decomp/xm1-coverage`).

- `devenv shell -- bazel run //build-support:xm1-decomp -- rev` prints the commit the branch resolves to.
- `devenv shell -- bazel run //build-support:xm1-decomp -- lookup 0x00724f08` prints the function-ranges row (an address inside a function selects that function), every symbol at the address, and every C definition as `file:line`, status (`exact`, `nonmatching`, `unranged` or `unplaced`) and, when marked, the marker's `file:line` and reason.
- `devenv shell -- bazel run //build-support:xm1-decomp -- lookup menu_cursor_dispatch_event --rev 55436fc85c0560dd252b0410af6f2427fe198cd3` looks up a name: symbol names, `stem:name` symbols, definitions and range names match whole, never callers or prototypes.
- `devenv shell -- bazel run //build-support:xm1-decomp -- show 0x0063270c` prints each definition's C, preceded by its marker line when one applies.
- `devenv shell -- bazel run //build-support:xm1-decomp -- coverage` prints the counts below.
- `devenv shell -- bazel run //build-support:xm1-decomp -- boundary` runs the boundary check against the pinned images of the mapping table; `SECTION=PATH:OFFSET:SIZE:BASE` arguments replace them. It exits 1 on any mismatched or unmapped row.

Lookups read an index of the commit, built once by `//build-support:xm1-decomp-index` (Haskell; definition recognition follows x70view's `cdefs.py`, header inclusion and symbol scoping its `coverage_sources.py` and `build_symbols.py`) and cached in `private-data/scratch/xm1-decomp/index/COMMIT-INDEXER.tsv`. `//tests:xm1-decomp` checks the commands against a fixture repository.

## Coverage at 55436fc85c

Functions are function-ranges rows; with C means at least one definition is placed at the row's address; a function is NONMATCHING when any of its definitions is marked. Definitions are counted, not marker lines: app has 14,856 marker lines for 14,747 NONMATCHING definitions in range plus one outside.

| Unit | Functions | With C | Exact | NONMATCHING | Definitions | Exact definitions | NONMATCHING definitions |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| kernel0 (loader A) | 223 | 223 | 204 | 19 | 223 | 204 | 19 |
| kernel1 (loader B) | 219 | 219 | 200 | 19 | 219 | 200 | 19 |
| main stream-0016025c (image 4) | 9,494 | 9,336 | 5,802 | 3,534 | 9,336 | 5,802 | 3,534 |
| main stream-002c025c (image 5) | 20,658 | 20,415 | 12,919 | 7,496 | 20,416 | 12,920 | 7,496 |
| overlay44 | 1,703 | 1,543 | 787 | 756 | 1,543 | 787 | 756 |
| overlay46 | 422 | 419 | 309 | 110 | 420 | 310 | 110 |
| overlay47 | 9,191 | 8,836 | 5,985 | 2,851 | 8,941 | 6,090 | 2,851 |
| main streams | 30,152 | 29,751 | 18,721 | 11,030 | 29,752 | 18,722 | 11,030 |
| overlays | 11,316 | 10,798 | 7,081 | 3,717 | 10,904 | 7,187 | 3,717 |
| all | 41,910 | 40,991 | 26,206 | 14,785 | 41,098 | 26,313 | 14,785 |

That is 97.8% of functions with C and 62.5% exact (kernels 91.4%); by bytes, 95.6% and 30.7% of 8,672,100. The 919 functions without C have no definition in the tree, at most a prototype or a call. Definitions exceed functions with C by 107 because some functions are defined more than once. Outside the ranges: 712 app, 27 kernel0 and 27 kernel1 definitions have no symbol (static helpers), and one app NONMATCHING definition (`FUN_01d8c388`) has a symbol but no range row. The plan's 99.3% with C and 63.8% exact were not reproduced by this count; their method is not recorded.

## Boundary check

Every one of the 41,910 function-ranges rows was hashed from our own pinned images at its extent and compared with the row's `sha256`: 41,910 matched, 0 mismatched, 0 unmapped. Each of the seven section images also hashes to the rows' `stock_sha256`. The decomp's addresses and extents are therefore extents of our images. Rerun with `boundary` when the pin moves.

## Limitations

- NONMATCHING C is a draft; 14,785 functions are in that state.
- Names and types are inferences, many from the X70 donor.
- Exactness assumes the tree at the cited commit builds; this repository does not run x70view's build. The tip's message (`return 1 failing files to work in progress`) records the branch's own gate. A function defined twice is counted NONMATCHING when either definition is marked.
- The branch moves and will merge into x70view main; counts and lines hold at the cited commit only.
- No boot ROM or first-stage tree exists for X-M1.
