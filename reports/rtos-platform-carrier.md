# Aggregate RTOS platform carrier: fit and placement

`replacement/rtos_platform` declares the retired-carrier union of the source-reviewed live-placement groups: S1, S2, S3a, S3b, S3c, own-first, semaphore, fixed-pool, task records and the global-lock roots and helpers. The union is the stock extents of those groups, not their native scratch. `carriers.json` records the stock extents, the fixed entry addresses and veneer symbols, the retained/owned absolute callees and the emitted demand of every whole function section.

## Fit passes after the fixed-pool factoring

The fixed-pool author's `db90918b` replaces the shared `.text.fp_bodies` section with whole-function sections, all at most 672 bytes, and factors the broad `inline(always)` at real boundaries (`release_locked` retained). The aggregate demand list was regenerated from the new A and B symbol tables.

| Fact | A | B |
| --- | --- | --- |
| carrier extents | 81 | 81 |
| total carrier bytes | 16,668 (`0x411c`) | 16,676 (`0x411c`) |
| emitted whole-section demand | 15,980 | 16,004 |
| largest entry-free carrier run | 672 (`0x2a0`) | 672 (`0x2a0`) |
| largest emitted section | 588 (`0x24c`, `admit`) | 588 |

`//replacement/rtos_platform:stage-platform -- fit CARRIERS` reports `fits` for both domains and exits 0. The task-record `0x11c` create body is well inside the 672-byte run and needs no mid-function split.

The fixed-pool frame demand from `b143abab` (`private-data/scratch/RtosFixedPoolsSections/section-frames.json`, SHA-256 `d0bf22c6…46f7b19`) is local/worst-case owned-chain bytes: admit 24/24, get 56/64, delete_pool 56/56, release 40/72, release_locked 32/32, create_pool 32/32, poll_unchecked 40/48, resume 8/8, take 8/8, poll 32/40, timed_entry 96/160, get_entry 96/160, poll_entry 64/104, unchecked_entry 64/112, create_entry 64/96, release_entry 64/136, delete_entry 64/120. Owned edges: get/poll/poll_unchecked to take; release to release_locked; every entry to admit plus its core; get and timed entry also to resume. Retained stock calls are outside the owned emitted chain.

## Placement does not converge

The aggregate link still produces no image. `ld.lld --enable-non-contiguous-regions` reports `address assignment did not converge` for both domains with the 82 entry-free carrier regions, and still fails after dropping runs below 64 bytes (56 regions). The failure is the linker's spill search over many small non-contiguous regions, not a physical fit: the total supply exceeds the demand by 688 bytes in A and 672 in B, and every section fits an individual run.

The manual `//replacement/rtos_platform:platform-a` and `:platform-b` targets are retained as the failing attempt. The generated `platform-*.ld` and `veneer-*.S` are the declared scatter; they are not a linked artifact.

## Next step

Place the whole sections deterministically instead of asking lld to search: link the extracted objects relocatably, bin-pack the named `.text.*`/`.rodata.*` sections into the carrier runs (first-fit decreasing), emit an explicit `*(<section>)` list per run, and link with relocations resolved by lld and no spill. This preserves whole functions and the recorded entry/veneer addresses. The alternative is a linker that converges on the same region map; no unproved carrier is added.

## Implemented mechanism

- `Union.hs` parses `carriers.json`, splits each carrier at its entry words, and checks fit.
- `Artifact.hs` admits a linked image by checking the fit, every veneer symbol at its stock address and size 4, every whole section inside one entry-free run, every demand symbol at its recorded size, no data symbols, zero-or-fill slack words, direct branches to declared owned/retained addresses and a frame walk over the owned runs. It stages by replacing each carrier extent in both loaders, rebuilding the container, confining every changed DAT byte to the union extents and the main checksum, checking the staged loaders and restoring the canonical DAT.
- `StageMain.hs` exposes `fit` and `stage`; `Tests.hs` exercises stage, restore and confinement over a synthetic fitted carrier and refuses a demand deficit, a moved entry, a demand-size mismatch, a data symbol, a nonzero slack word, overlapping carriers and a change outside the union. The fixture is the pattern-byte container; no firmware content is stored.
- `//replacement/rtos_platform:platform-tests` passes; the group rlibs and platform root build. `//replacement/rtos_platform:all` builds; the platform ELF is `manual` because placement does not converge.

## Limits

No platform image, symbol map, candidate DAT, staged loader or subsystem-native comparison is claimed. The fit is host analysis of the recorded stock extents and emitted symbol sizes. The frame walk records its own external-call premises rather than claiming full closure. No live, system, MPU, timer, cross-core or hardware behavior is established. The cyclic family, IRQ-control root and S3d are excluded/retained as directed.
