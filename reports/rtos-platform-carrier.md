# Aggregate RTOS platform carrier and fit result

`replacement/rtos_platform` declares the retired-carrier union of the source-reviewed groups that have a live-placement increment: S1, S2, S3a, S3b, S3c, own-first, semaphore, fixed-pool, task records and the global-lock roots and helpers. The union is the stock extents of those groups, not their native scratch. `carriers.json` records the stock extents, the fixed entry addresses and veneer symbols, the retained/owned absolute callees and the emitted demand of every whole function section.

## Fit

The union has enough total bytes but is too fragmented for three fixed-pool bodies.

| Fact | A | B |
| --- | --- | --- |
| carrier extents | 81 | 81 |
| total carrier bytes | 16,668 (`0x411c`) | 16,676 (`0x411c`) |
| emitted whole-section demand | 15,916 (`0x3e2c`) | 16,024 |
| largest entry-free carrier run | 672 (`0x2a0`) | 672 (`0x2a0`) |
| largest emitted section | 964 (`0x3c4`) | 980 (`0x3d4`) |

The largest section is `xm1_rtos_fixed_pool_timed_get_body`. The next two, `xm1_rtos_fixed_pool_get_body` (772/788) and `xm1_rtos_fixed_pool_release_body` (740/756), also exceed 672. The exact deficit is 292 bytes (`0x124`) in A and 308 (`0x134`) in B for the timed body; the three bodies together exceed the largest run by 1,064 bytes in A and 1,092 in B.

The largest run is the fixed-pool timed extent after its four-byte entry veneer (`0x4b818..0x4bab8` A, `0xdb7d8..0xdba78` B). Placing the body at the entry instead of a veneer raises the run to `0x2a4` (676 bytes), which still does not hold the timed body. Every other carrier extent is shorter or is split by a caller-visible entry at the same scale. The total slack is 752 bytes in A and 652 in B, so the failure is fragmentation, not total supply.

The fixed-pool adapter currently places all of its bodies in one `.text.fp_bodies` input section, so the platform link sees a single section of about `0x10b8` bytes, not the per-symbol sizes above. Per-function sections are a prerequisite to any scatter. Even after that split, the three bodies above exceed the largest run, so the fit table is the optimistic bound and the actual link is worse.

`//replacement/rtos_platform:stage-platform -- fit CARRIERS` reports the deficit and exits 1. The manual `//replacement/rtos_platform:platform-a` link fails before producing an image; no platform symbol map exists.

## Why the union cannot be merged further

Adjacent extents were checked for a veneer-free run large enough for the timed body. The largest candidates are the S3b activate/cancel/exit cluster (`0x44f2c..0x454f0`, `0x5c4`), the S3b wake/cancel-wake cluster (`0x46004..0x46534`, `0x530`), the S3c suspend/resume cluster (`0x46a00..0x46e48`, `0x448`) and the fixed-pool timed/release pair (`0x4b814..0x4bd4c`, `0x538`). Each contains caller-visible root entries at the interior boundaries, so the largest veneer-free run inside them is `0x1c4`, `0x1fc`, `0x280` and `0x2a0`. The S2 `select`/`irq_tail` and S1 `prepare` entries are interior and could drop their veneers, but the merged S2 run is `0x154` and the S1 find/prepare run is `0x5c`, so that does not change the result. The loader tail after the last carrier and the gaps between extents are not retired and are not used.

## Options

1. Split the three fixed-pool bodies at real function boundaries. The shared admission, allocator and timeout tails can become separate whole functions; each resulting section must be at most 672 bytes. This is the smallest change and does not move another subsystem's carrier. The task-record report already accepts a source-level split at a real boundary.
2. Add a larger retired carrier to the declared union. The general-heap carriers (`0x89cde4..0x89d4f8`, `0x714`; `0x89da64..0x89dfc0`, `0x55c`) and the interface/primary initializer carriers (`0x40430..0x40ab0`, `0x680`) are large enough. Each belongs to another cutover, so composing it needs that artifact's bodies or a superseding cutover and a parent decision; a bare carrier would overwrite the other artifact's code.
3. Keep the union but reduce the demand below the fragmentation limit. The three fixed-pool bodies are the only sections over 672 bytes, so option 1 is the direct form of this.

## Implemented mechanism

- `Union.hs` parses `carriers.json`, splits each carrier at its entry words, and checks fit. A section larger than the largest entry-free run is a typed `CarrierDeficit`; total demand over total supply is a `TotalDeficit`.
- `Artifact.hs` admits a linked image by checking the fit, every veneer symbol at its stock address and size 4, every whole section inside one entry-free run, every demand symbol at its recorded size, no data symbols, zero-or-fill slack words, direct branches to declared owned/retained addresses and a frame walk over the owned runs. It stages by replacing each carrier extent in both loaders, rebuilding the container, confining every changed DAT byte to the union extents and the main checksum, checking the staged loaders and restoring the canonical DAT.
- `StageMain.hs` exposes `fit` and `stage`; `Tests.hs` exercises stage, restore and confinement over a synthetic fitted carrier and refuses a demand deficit, a moved entry, a demand-size mismatch, a data symbol, a nonzero slack word, overlapping carriers and a change outside the union. The fixture is the pattern-byte container; no firmware content is stored.
- `//replacement/rtos_platform:platform-tests` passes. The group rlibs and the platform root build; the platform ELF is `manual` because the declared union does not fit.

## Task-record answer

The `0x11c` task-create body is smaller than the 672-byte largest run and fits the pooled union when the union itself fits. The task-record stock extents are declared carriers and the linker uses whole `.text.task_create_body`/`.text.task_delete_body` sections with entry veneers at A `0x4d094`/`0x4d1a4` and B `0xdd054`/`0xdd168`; no mid-function split is needed. It cannot be admitted live until the fixed-pool fragmentation is resolved, because the platform image as a whole does not fit.

## Limits

No platform image, symbol map, candidate DAT or subsystem-native comparison is claimed. The fit is host analysis of the recorded stock extents and emitted symbol sizes, not a link. The frame walk records its own external-call premises rather than claiming full closure. No live, system, MPU, timer, cross-core or hardware behavior is established. The cyclic family is not in this union; its reviewed carrier is native-only and awaits parent clearance.
