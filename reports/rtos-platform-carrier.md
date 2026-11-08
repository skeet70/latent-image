# Aggregate RTOS platform carrier: admitted A/B image

`replacement/rtos_platform` declares the retired-carrier union of the source-reviewed live-placement groups: S1, S2, S3a, S3b, S3c, own-first, semaphore, fixed-pool, task records and the global-lock roots and helpers. The union is the stock extents of those groups, not their native scratch. `carriers.json` records the stock extents, the fixed entry addresses and veneer symbols, the retained/owned absolute callees and the emitted demand.

## Current whole-section demand

The current sources emit 105 alloc archive sections plus seven 4-byte fixed-pool entry stubs in `stage/entries.S`. A totals 16,088 bytes and B 16,112 against 16,668 and 16,676 carrier-run bytes, a 580-byte slack in both domains. The largest whole section is `xm1_rtos_fixed_pool_get_body` (520 A / 520 B); the largest entry-free run is 672 bytes.

The global-lock helpers are now `helper_try`/`helper_release`/`helper_observe` in `.text.global_lock_helper_*` (40/44/8 bytes), with the three 28-byte entry functions in `.text.global_lock_*`. The platform veneers for the helper bodies were corrected to those symbols, and the demand list was regenerated from the emitted symbol table rather than the retired `_body` names.

## Placement

`//replacement/rtos_platform:place-platform` parses the relocatable section table, fits the S2 code and literal pool first into one run window whose start addresses span at most 4095 bytes, then places every remaining whole section best-fit into the runs. It emits one explicit output section per input section at its absolute address, the fixed veneer sections at their stock addresses and the declared external symbols. `admitPlacement` re-checks the concrete assignment: every section exactly once, alignment, run containment, disjointness and the S2 pool distance. The S2 check is the actual `R_ARM_LDR_PC_G0` range (4095 bytes), not a bundle assumption.

The A and B images are linked from a `ld.lld -r --gc-sections` relocatable object that carries the archives, `veneer-*.S` and `entries.S`, so every named body and entry is present before placement.

| Fact | A | B |
| --- | --- | --- |
| relocatable alloc sections (archive + asm) | 112 | 112 |
| emitted bytes | 16,088 | 16,112 |
| carrier-run supply | 16,668 | 16,676 |
| slack | 580 | 580 |
| largest placed section | 520 | 520 |

| Artifact | SHA-256 |
| --- | --- |
| platform-a.elf | `76a369fc27994ca614e6797701c21868b29df363bdbb02bdc6b28811adf07300` |
| platform-b.elf | `4fc856b35f36b1d686220af85de025f26c597cb6bd10da521539713c65be155e` |
| platform-a.bin | `b63a97531ebc344e6ed45cc741196bc2c8c855f84ff0ec8315fb694c161c1037` |
| platform-b.bin | `5b778fcf7c434116cd11309666b6f928dd01309160bc01c8966cb8404f43e6bd` |
| candidate.DAT | `4485cae39ad5125029f197734d7beb20785ab42b9e9e3d4f40c217b9a6b07039` |
| loader-a staged | `60611658357d25030d3c0cbd055e9e42f308a169de2b2e7248e99b318d5ed0b3` |
| loader-b staged | `632d9dd4ac8764f9d43cd6bc20331108493861fd2fbbccbbdd9f31dcdbcef19a` |

The Bazel `platform-a/b` ELFs carry different file hashes from the preserved manual link (`c8d53cd0…` / `868049f9…`) because of link metadata, while their `.bin` outputs are byte-identical to the table above.

Inputs are preserved under `private-data/scratch/PlatformPlacement/current-20261108/`: the two relocatable objects, section tables, generated scripts, placement manifests, ELF/bin/symbols/disassembly and the staged candidate. The staged DAT was produced by `stage-platform stage` against the canonical `stock-roundtrip.DAT` (`43d67afa…cd1490`) and both loaders; `stage` rebuilds, confines every changed DAT byte to the carrier union and the main checksum, re-parses the staged loaders and restores the canonical DAT byte-for-byte.

## Frame and callee binding

The fixed-pool frame demand from `b143abab` is bound to the emitted image: local/worst-case owned-chain bytes are admit 24/24, get 56/64, delete_pool 56/56, release 40/72, release_locked 32/32, create_pool 32/32, poll_unchecked 40/48, resume 8/8, take 8/8, poll 32/40, timed_entry 96/160, get_entry 96/160, poll_entry 64/104, unchecked_entry 64/112, create_entry 64/96, release_entry 64/136, delete_entry 64/120. The retained full owned+examined maxima excluding async nesting are create 96, delete 196, get 212, timed_get 240, poll 104, release 196, unchecked 112; suspended get 188 and transfer peak 212. The generic FP judge's 176 failed role expectations are not 176 passed judgments; the family comparator is the scoped authority.

`admit` walks the owned runs from the emitted body entry symbols (not the veneer addresses) and treats the declared retained/owned externals as leaves. The current walk records two premises rather than full call-graph closure: the S2 exception-save path reaches a conditional cross-extent branch the A32 walker models as a tail call and reports `StackUnbalanced` at `0x43f88` A / `0xd3f84` B. The demand list checks every emitted whole function's size and every non-veneer symbol's run containment.

## Bazel wiring and neighbor artifacts

`//replacement/rtos_platform:platform-a` and `:platform-b` now build the relocatable object (`bodies-*.o`), run `place-platform`, and link with the generated explicit script; the `.bin` outputs reproduce the preserved images byte-for-byte (`b63a9753…c1037` / `5b778fcf…3e6bd`). The old `--enable-non-contiguous-regions` spill path is gone.

The exception carrier's fixed extents (A `0x40100..0x40410`, `0x40cac..0x40d98`, `0x430a8..0x432f8`, `0x43810..0x43834`; B `0xd0100..0xd0410`, `0xd0cac..0xd0d98`, `0xd30a8..0xd32f8`, `0xd3810..0xd3834`) are disjoint from every platform carrier and entry address, so the two artifacts can coexist. Cyclic is not adoptable as supply: its qualified core bodies occupy native scratch A `0x4c28c..0x4c94c` / B `0xdc24c..0xdc90c`, which overlaps the platform's S3c/scheduler carriers. IRQ's shared body is at scratch `0x50000`, outside the declared union and not a stock extent; its entries at `0x41108/0x41124/0x4d8fc` (A) and `0xd1108/0xd1124/0xdd8c0` (B) are outside the platform carriers. Both remain separate artifacts rather than being silently folded into the union.

The global-lock suite now binds the aggregate: its cases still call the public ABI entries at `0x412cc/0x412e8/0x41304` and `0x427d4/0x427fc/0x42828`, while the relocated `xm1_rtos_global_lock_*` and `helper_*` bodies are mapped as extra executable regions from the staged union delta. The run against the relocated A/B image passes 64 cases and 288 controls with zero failures (`native/global-lock/results.json`, SHA-256 `febed531…43f4`; runner `de513039…117e`). The S2 aggregate suite binds the same relocated images. Its existing 560 rows per domain and all controls run through `cachedObservedBatch` with current admission; A passes all 560 rows and B passes 556. Two B public veneers had been bound to the wrong bodies: `s2_dispatch_select` branched to `xm1_rtos_scheduler_no_save` and `s2_dispatch_irq_tail` to `xm1_rtos_scheduler_irq_resume`. Corrected to `xm1_rtos_scheduler_select` and `xm1_rtos_scheduler_irq_tail`, those rows pass. The saved IRQ continuation now resolves `xm1_rtos_scheduler_irq_resume` through an ELF relocation (`adr`), and the A/B IrqReturn rows pass. The four remaining B failures are the `Select` and `NoSave` null-loop rows with the B local tag 18. First divergent instruction and read: the owned null window's `movw r0,#0x804` at B `0xd40c4` runs with the incoming `r0 = 0x400e4728`, whose bit 13 the result `0x400c2804` retains, and the following `ldr r1,[r0]` at `0xd40cc` reads that address. The A incoming `r0 = 0x40054768` has bit 13 clear, and A's `ldr` reads the lock `0x400c0804`. The stock null window does not use this `movw` pair and reads the lock `0x400c0804`. First divergent effect: the stock writes the context word at `0x400e4564` where the owned order holds the helper's `DMB`. The global-unlock expectation is unchanged; the owned B image does not release the lock, and the missing release is not normalized away. Evidence: `private-data/scratch/PlatformPlacement/current-20261108/aggregate-oracles/s2-cached.json` and `s2-adr2.json`.

The same binding is still required for the S3a/S3b/S3c, own-first, semaphore, fixed-pool and task-record suites; each derives case regions from its own constants.

## Limits

The global-lock and S2 aggregate suites run on these relocated inputs; the staged candidate is not executed. The S2 suite leaves the four B null-loop rows described above. The owned-run frame walk starts at the emitted body entries and treats declared externals as leaves, but the S2 exception-save path is not modeled by the walker (`StackUnbalanced` at `0x43f88` A / `0xd3f84` B); the contract records those as premises rather than claiming closure. Cyclic and IRQ remain outside the union for the reasons above, S3d is unqualified and excluded, and no live, system, MPU, timer, cross-core or hardware behavior is established. Full 97-contract RTOS/firmware acceptance is not claimed.
