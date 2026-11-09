# Aggregate RTOS platform carrier

`replacement/rtos_platform` declares stock-extent carriers for S1, S2, S3a, S3b, S3c, S3d, own-first, semaphore, fixed-pool, task records, data queues and global-lock roots and helpers. `carriers.json` records entry veneers, retained callees and emitted demand. Carrier declaration, successful linking, native qualification and live ownership are separate claims.

## Earlier admitted baseline

The earlier admitted image emitted 105 alloc archive sections plus seven 4-byte fixed-pool entry stubs in `stage/entries.S`. A totaled 16,088 bytes and B 16,112 against 16,668 and 16,676 carrier-run bytes. These measurements and the artifact table below predate data-queue and linked S3d composition.

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

The global-lock suite now binds the aggregate: its cases still call the public ABI entries at `0x412cc/0x412e8/0x41304` and `0x427d4/0x427fc/0x42828`, while the relocated `xm1_rtos_global_lock_*` and `helper_*` bodies are mapped as extra executable regions from the staged union delta. The run against the relocated A/B image passes 64 cases and 288 controls with zero failures (`native/global-lock/results.json`, SHA-256 `febed531…43f4`; runner `de513039…117e`). The S2 aggregate suite binds the same relocated images. Its existing 560 rows per domain and all controls run through `cachedObservedBatch` with current admission; A passes all 560 rows and B passes 556. Two B public veneers had been bound to the wrong bodies: `s2_dispatch_select` branched to `xm1_rtos_scheduler_no_save` and `s2_dispatch_irq_tail` to `xm1_rtos_scheduler_irq_resume`. Corrected to `xm1_rtos_scheduler_select` and `xm1_rtos_scheduler_irq_tail`, those rows pass. The saved IRQ continuation now resolves `xm1_rtos_scheduler_irq_resume` through an ELF relocation (`adr`), and the A/B IrqReturn rows pass. The four B null-loop failures (`Select` and `NoSave`, local tag 18) came from the S2 oracle's `extraRegions` overlay, not the CPU. The overlay installed `helper_try` at `(0x2800,0x2828)` from the aggregate image and omitted the aggregate veneer at `(0x27fc,0x2800)`, so the call to `xm1_rtos_scheduler_s2_global_unlock = 0xd27fc` ran the stock `mov r1,#0` followed by the try body and returned without storing. The MOVW/MOVT path and the lock read were correct. Fix 1456051d adds `xm1_platform_veneer_global_lock_helper_try/release/observe` to the `extraRegions` names; the aggregate now passes A 560/560 and B 560/560 with all controls holding, and the `NullUnlock` expectation is unchanged. Evidence: `null-loop-fix-frozen/s2-fixed.json` (SHA-256 `3bc0183837c8f42c27237ede9c8cfb8120b76beb76c2b364c7b9c81a7f9ef987`), with the build and run verdict logs in the same directory.

Existing baseline family results do not qualify the current linked union. Every admitted family still requires aggregate execution and comparison against the current image identities.

## Linked lifecycle source boundary

The linked lifecycle source increment builds both actual union images with 218 allocated sections per domain. Emitted section bytes are A 27,671 / B 27,695 against A 27,800 / B 27,808 entry-free carrier bytes; the largest whole section is 824 bytes. The `.bin` identities are A `db78bc9263972cb19b330d8114165579dcf909c40391da9db40b27c64c5571f3` and B `261777dcaca552af5ae33ba81b17788c69380311d763ec233d1b0dad7d8025f8`. Platform synthetic tests, data-queue host behavior tests and lifecycle host behavior tests pass in that source increment; native aggregate acceptance is still required. These are emitted-code and placement measurements, not reclaimed-flash measurements.

The initial staging command emitted DAT `4c5d2d6d139a7e38d886ff27ab8d13e7ad8633b41d85b9df162d665fe090a21f` with loaders `eb0a36b326feadad3055b3d393f20624561dda2c78e2be3913aa536547d0b07a` / `519253a65723ed9ef0e490ad7c878739a5ad2e7543540243105c0d28da8f5937`. Review invalidates that candidate: staging changed allocated zero-valued switch-table entries to UDF, including A `0x4c608` / B `0xdcbc4`. The immutable `private-data/scratch/FirmwarePlatformJoin/source-increment.json` records that historical output, not an accepted firmware artifact. The staging repair preserves every allocated byte and fills only uncovered carrier slack; its public regression extracts the emitted DAT and checks body preservation, allocated zero words and slack fill. Corrected actual-DAT staging and review confirmation remain required.



The saved semaphore aggregate timeout reaches `0x44e58` (`blx r3`), calls the `0x44318` veneer containing `0xea0034de`, and refuses instruction fetch at `0x51698`. The manifest resolved that callback to native scratch instead of linking its implementation. The source composition now links the S3d component and removes its scratch external substitutions. No passing joined timeout result follows from that source change alone.

S3b's old batch helper removed the complete `stack` observation before a comparator required its incoming-frame bytes. That makes every comparison fail independently of service results. The S3b loader now retains that observation, compares only its declared live incoming range, and keeps every other writable region and ordered device write visible. Controls challenge task state, globals and the incoming frame, and separately check private-stack insensitivity. Its equivalent-input role varies ABI scratch-register inputs rather than patching an address adjacent to a relocated veneer.

Shared S3b dispatch, S3c resolution/locking, lifecycle helpers and blocking queue operations are emitted once in the aggregate where applicable. Standalone fixed-slot scheduler artifacts retain their existing inlining constraint. The queue union remains its eleven owned extents; the retained wait-release helper at A `0x44568` / B `0xd4528` is not carrier supply.

The own-first, S3d and task-record oracles accept complete aggregate symbols and placement metadata. Source wiring is not aggregate execution evidence. Current joined build, placement, staging, family qualification and review results must be bound separately before integration.

The unchanged frozen A/B union images subsequently pass 62 semaphore cases, including both actual timed-timeout caller returns, and 62 task-record cases with 466 controls and four emitted create/delete smoke cases. Semaphore stock observations are reused in both domains; owned semaphore roles execute afresh. Task-record roles execute afresh under current admission. These results are recorded in `private-data/scratch/FirmwarePlatformJoin/native-increment-01.json`.

The first semaphore attempt refuses native admission because zero-size LLVM `T` labels become zero-size function guards. The aggregate symbol parser now omits those labels while retaining every positive-size function; its regression test passes. S3a/S3c CLI input selection now permits explicit aggregate binary arguments to override Bazel's standalone defaults. Those two CLI changes compile, but their new family cohorts have not executed.

The subsequent S3b attempt refuses storage admission before native cases: 107,014,287,360 available bytes against the unchanged 107,374,182,400-byte reserve. The build launcher succeeds; the qualification application exits 1. No S3b aggregate or control pass is claimed for that attempt.

S3b fresh observations now use the same batch-level result location as reuse admission. Historical `fresh/` results remain readable only through their original receipts and unchanged runner/wire identity checks.

The cyclic/detached-pop timer source composition uses real retired extents and no native scratch supply. Its first emission refuses placement at A 30,915 / B 30,939 bytes against A 30,328 / B 30,336 entry-free supply. Shared cyclic/timer boundaries reduce each domain by 348 bytes. A typed four-word queue service interface and shared plain/timed send continuation remove another 480 bytes without changing timeout admission or the `-53`/`-55` dispatch distinction. The resulting actual A/B union images build and place at 30,087 / 30,111 emitted bytes. Their refreshed inventory contains 235 positive-size non-veneer functions per domain. Earlier refused placements and host-test results remain in `private-data/scratch/FirmwarePlatformJoin/cyclic-source-01/qualification.json`; the placed images are frozen separately under `emitted-02`. Corrected DAT staging and new-image native qualification remain required.



## Limits

The earlier global-lock and S2 aggregate suites run on their relocated inputs; the staged candidate is not executed. S2 passes 560/560 in both domains for that cohort. The owned-run frame walk treats declared externals as leaves; its cross-extent continuation repair still requires actual-image closure evaluation. Cyclic/timer source now passes actual union placement but has not passed native aggregate admission. IRQ remains outside the union. Current aggregate family acceptance, corrected actual-DAT byte preservation and composed frame closure remain open. No live, system, MPU, timer, cross-core or hardware behavior is established. Full RTOS/firmware acceptance is not claimed.
