# X-M1 1.50 general heap survey

## Scope and decision

[#148](https://github.com/skeet70/latent-image/issues/148) covers the general C heap and its future Rust replacement, not every stock resource reservation. This survey establishes the newlib/dlmalloc family, concrete entry ABIs, the direct-call surface, one cross-image chunk-header dependency, backing bounds and the [#147](https://github.com/skeet70/latent-image/issues/147) resize defect. Static findings are E-STOCK-HEAP-SURVEY-150 and E-STOCK-HEAP-RESIZE-LOCK-150. It does not establish complete indirect-call closure, task/core attribution or representative allocation traces.

`rlsf` is the baseline candidate for the trace-driven comparison, not a benchmark-selected or qualified replacement. No allocator is installed. The final selection remains open until reviewed observation and target-architecture measurements establish the workload, fragmentation, capacity and contention costs.

## Implementation identification

The stock implementation matches newlib's reentrant dlmalloc implementation structurally, not just the resize prologue. The comparison reference is [`newlib/libc/stdlib/mallocr.c` at commit `4aa696c8d6294897411cf78cae87f7f4680e9687`](https://chromium.googlesource.com/native_client/nacl-newlib/+/4aa696c8d6294897411cf78cae87f7f4680e9687/newlib/libc/stdlib/mallocr.c). This identifies the implementation family and API mapping, not the exact compiler, newlib release or source revision linked into the camera.

| Stock structure | Newlib reference and correspondence |
| --- | --- |
| Allocation core `0x0089cde4` | `_malloc_r`: `request2size`, minimum 16-byte chunks, 8-byte rounding, overflow rejection before locking, small/large bins, last remainder, top chunk and sbrk expansion. Reference lines 2328-2357 and following allocation body. |
| Resize core `0x0089da64` | `_realloc_r`: null delegates to allocation; lock precedes size normalization; reject writes `ENOMEM` and returns without unlocking; otherwise shrink/split, expand into neighboring free/top chunks or allocate/copy/free. Reference lines 2767-3020, including the same defective return at 2817-2820. |
| Free core `0x0089e560` | `_free_r`: null returns before locking, reads boundary tags, backward/forward coalesces, relinks bins, combines the top and may trim. Reference free body beginning at 2630. |
| Zeroed allocation `0x00bf1c44` | `_calloc_r`: 32-bit `n * elem_size`, allocate, derive usable extent from chunk header and clear it. Reference lines 3184-3247. The multiplication has no overflow check in this stock implementation. |
| Trim core `0x0089e464` | `_malloc_trim_r`: lock, page-rounded top release, sbrk(0) contiguity check, negative sbrk, update top/counters and unlock on success/failure. Reference lines 3290-3370. |
| Public wrappers | `malloc`, `free`, `realloc`, `calloc` load the context pointer through `0x0089c970` and forward to the reentrant forms. The initial pointer value is `0x0089c978`. This does not prove it is switched for every task. |

The allocator's bin state begins at `0x0089d4f8`, with linked free chunks and in-band size/flag words. The payload is chunk base +8; `payload-4` holds size with its low two flag bits. These are implementation details, not a portable allocation ABI.

## Entry points and error convention

All listed entries are A32. Register arguments and pointer/size results are 32-bit; these recovered interfaces use the ordinary integer/pointer AAPCS boundary. `context` names the reentrancy structure whose first word is errno. Do not substitute Rust allocation panic behavior for these C failure results.

| Entry | Arguments | Result and special cases |
| --- | --- | --- |
| `malloc` `0x0089cda4` | r0 = bytes | r0 = payload or null; minimum chunk 16 and payload alignment 8. Zero requests take the minimum-chunk path, not an unconditional null return. |
| `_malloc_r` `0x0089cde4` | r0 = context, r1 = bytes | Same allocation. Normalization overflow writes 12 (`ENOMEM`) through context and returns null before locking. Backing refusal also writes 12. |
| `free` `0x0089cdc4` | r0 = payload | No meaningful result; null is accepted. |
| `_free_r` `0x0089e560` | r0 = context, r1 = payload | Same release; invalid or foreign non-null pointers are not validated by this ABI. |
| `realloc` `0x0089da3c` | r0 = old payload, r1 = bytes | r0 = resized payload or null. On allocation failure the old allocation remains. |
| `_realloc_r` `0x0089da64` | r0 = context, r1 = old payload, r2 = bytes | Null old pointer delegates to `_malloc_r`. Zero bytes normalize to a minimum chunk; the zero-size-free build option is not present. Oversized non-null requests have the lock defect below. |
| `calloc` `0x00bf1c1c` | r0 = count, r1 = element bytes | r0 = cleared payload or null. The core multiplies modulo 2^32; overflow is not rejected independently. |
| `_calloc_r` `0x00bf1c44` | r0 = context, r1 = count, r2 = element bytes | Same operation. On success clears `(word(payload-4) & ~3) - 4` bytes, not just the requested product. |
| `_malloc_trim_r` `0x0089e464` | r0 = context, r1 = pad | r0 = 1 on successful release, 0 otherwise. Called internally by free at `0x0089e770`. |
| `_sbrk_r`-like backing `0x0072b110` | r0 = context, r1 = signed increment represented in a word | Old break on success, `0xffffffff` on upper-bound rejection, setting context errno to 12. Zero queries the break; trim passes a negative increment. |
| `__malloc_lock` `0x0072b164`, unlock `0x0072b1cc` | context argument is not used for ownership | Recursive task ownership and semaphore `0x3f`; no normal failure result. Invalid task identity or semaphore failure hard-loops. |
| Graphics adapters `0x00890ef8`, `0x00890ee8`, `0x00890ed8` | Allocation, free and resize arguments above | Forward to the three public image-4 wrappers; callback table publications make these indirect entry surfaces. |

An aligned-allocation entry, standalone usable-size entry, mallinfo, mallopt or statistics API has not been positively identified. The direct calls to the known lock occur only in allocation, resize, free and trim in the inspected listings. That does not prove an unlocked/computed or separately implemented API absent. No extra alignment or placement promise is inferred from a sibling model or from the allocator family.

## Confirmed resize defect

For a valid non-null old allocation, `0x0089da7c` takes the recursive lock before `bytes+11` normalization. Requests in inclusive range `[0x7ffffff5, 0xffffffff]` fail the signed-size/overflow checks. The core stores 12 through context, sets the result to null and branches at `0x0089dabc` to `0x0089dc28`, bypassing the unlock call at `0x0089dc20`.

The owner and depth cells are `0x40a6c7d8` and `0x40a6c7dc`. The rejecting task retains one additional depth level. A different task subsequently entering the heap waits on semaphore `0x3f` with timeout `0xffffffff`. This is a static control-flow finding, not an observed stock workload hang. Natural stock oversized-call reachability is outside #147's scope.

The [allowed-calls precondition](../transport/README.md#retained-heap-call-preconditions) requires owned callers to reject sizes above `0x7ffffff4` before stock resize, with checked arithmetic before comparison. A replacement must reject oversized requests before locking and release its lock on every return. It must not emulate the leaked lock depth.

## Backing memory and distinct mechanisms

The initialized break cell `0x00829a08` contains `0x00a71a00`. Backing helper `0x0072b110` accepts an increment only when its word-arithmetic test `old_break + increment + 0x20 <= 0x00bf1a00` succeeds. It then advances the cell and returns the old break. There is no independent lower-bound check in that helper; it relies on its allocator caller for shrink validity. This is a reserved RAM interval, not an operating-system page mapping or an unlimited memory source.

The stock envelope `[0x00a71a00, 0x00bf1a00)` is `0x180000` bytes (1,572,864). The `0x20` guard reduces the maximum break span to 1,572,832 bytes before allocator metadata, initial alignment, top-chunk and page-growth overhead. The recipe fixed carve changes the bound instruction at `0x0072b130` from `0xe34030bf` to `0xe34030be`, producing upper limit `0x00be1a00`: a 65,536-byte loss and envelope `0x170000` bytes (1,507,328), with at most 1,507,296 break-span bytes before overhead. `host/firmware/BuildRecipeBank.hs` records the publication and capacity loss. A heap-allocated RAM-push bundle instead consumes a live allocation; its relocation address does not by itself prove the fixed carve was applied.

`0x007235d8`, previously called an image-4 allocator in an observation report, is a selected-root synchronization/IRQ-state adapter paired with `0x007236b0`, not general heap allocation. Its entry selects service `+0x1618`, calls selected-root `+0x164` and may synchronize through `+0x161c`; the paired restore uses `+0x168`. Its callers must not be counted as malloc users.

The file-handle helper `0x0071ef18` waits on semaphore `0x3c` and scans sixteen handle slots at `0x009101c0`; it is not this heap. [#143](https://github.com/skeet70/latent-image/issues/143)'s task-storage reservation uses cursor `0x4089ea98`, pool base `0x40a10200`, indexed records and lock `0x409c280c`, also outside this heap replacement. Their retained lifetimes remain separate obligations.

## Caller census and metadata dependencies

The census counts direct A32 instruction sites targeting each listed entry in canonical image-4/image-5 listings. Boot stages A/B, loaders A/B, image 2 and the plain/image-6 listing have no direct reference to these targets in the inspected listing domain. Overlapping `filmsim`, `update` and `update-review` copies are not additional callers. Data accidentally decoding as branch instructions, Thumb, aliases, arithmetic construction and copied/runtime-published pointers prevent treating a textual census as a retirement proof.

| Target | Image 4 | Image 5 |
| --- | ---: | ---: |
| `0x0089cda4` | 10 | 0 |
| `0x0089cdc4` | 43 | 0 |
| `0x0089da3c` | 2 | 0 |
| `0x0089cde4` | 3 | 5 |
| `0x0089da64` | 1 | 1 |
| `0x0089e560` | 4 | 1 |
| `0x00bf1c1c` | 0 | 0 |
| `0x00bf1c44` | 0 | 3 |
| `0x0089e464` | 1 | 0 |

Image-4 allocation wrapper callers: `0x006dbf40`, `0x0072a7d8`, `0x00890f00`, `0x00891238`, `0x008913a4`, `0x008914f4`, `0x008916b8`, `0x008916f4`, `0x00891900`, `0x008919f8`.

Image-4 free wrapper callers: `0x006d9550`, `0x006d9bd4`, `0x006d9f58`, `0x006da064`, `0x006da07c`, `0x006da7cc`, `0x006da80c`, `0x006da820`, `0x006daab0`, `0x006daad8`, `0x006dae5c`, `0x006db598`, `0x006db91c`, `0x006dc440`, `0x006dcc90`, `0x006dcd78`, `0x006dcd88`, `0x006dcdd0`, `0x006dcef4`, `0x006dcf04`, `0x006dcfc0`, `0x006dcfd0`, `0x006dd01c`, `0x006dd560`, `0x006dd664`, `0x006dd674`, `0x006dd784`, `0x0072a820`, `0x00890ef0`, `0x008912fc`, `0x0089130c`, `0x00891488`, `0x008914b8`, `0x008914c8`, `0x00891628`, `0x00891708`, `0x00891744`, `0x008917b8`, `0x008917f0`, `0x0089182c`, `0x00891834`, `0x008919ac`, `0x00891ac8`.

Image-4 resize wrapper callers are `0x00890ee0` and `0x008917a4`. Its allocation core callers are `0x0089cdb4`, `0x0089dd50`, `0x0089ddc4`; resize core caller `0x0089da54`; free core callers `0x0089cdd4`, `0x0089d4cc`, `0x0089dc64`, `0x0089ddb0`; trim caller `0x0089e770`. These include internal allocator calls and wrapper forwarding, not independent clients.

Image-5 allocation core callers are `0x00bf1c4c`, `0x00bf54dc`, `0x00bf5f6c`, `0x00bf6dd0`, `0x00bf887c`; resize caller `0x00bf6004`; free caller `0x00bf6018`; calloc core callers `0x00bf1c34`, `0x00bf395c`, `0x00bf398c`.

The calloc body is a confirmed allocator-internals reader outside image 4: it reads `payload-4` and clears the usable extent. Retaining it over a different allocator's metadata can clear beyond the allocation. A cutover must replace this core and wrapper together with the allocation/free/resize closure, not only the public image-4 wrappers. No standalone stock usable-size call was identified. Other negative-offset loads are not automatically chunk readers; complete allocation-pointer flow and address-sensitive consumer closure remain open.

## Tasks, cores and interrupt context

The heap lock queries task identity through `0x00723cb8`, whose selected-root service calls are indirect. Identity zero hard-loops. A different owner waits indefinitely; the same owner increments recursion depth. These facts establish task-context and blocking assumptions, not the complete set of allocating tasks or a multicore exclusion proof.

Graphics callback users and direct image-5 users establish clients outside the wrapper-only surface. Static inspection here does not attribute every client to a runtime task or core. No positive interrupt-to-heap allocation path was established, and absence has not been proved. An interrupt may retain a nonzero interrupted-task identity; the task-id check alone cannot exclude interrupt allocation or unsafe recursive access. Allocator locking must not be used from the IRQ-masked trace-ring callback described in [the observation report](af-observe-instruments-150.md).

## Pinned candidate fitness

These verdicts come from the published crates.io source archives, not compilation or target execution. Versions are exact release pins; the ledger preserves archive/checksum provenance and source ranges. A fit core is not a qualified stock bridge. All timing bounds below exclude initialization, payload zero/copy and external lock wait. A finite RAM capacity mathematically limits a linear scan, but does not establish an acceptable real-time deadline.

| Candidate pin | `no_std`, ARM32 and reclaim | Time and external locking | Verdict |
| --- | --- | --- | --- |
| [`rlsf` 0.2.3](https://docs.rs/crate/rlsf/0.2.3/source/src/tlsf.rs) | Unconditional `no_std`, explicit 32-bit layouts, caller-provided pool, variable Layout allocation and coalescing individual free. | O(1) allocate/free bookkeeping; moving resize is O(old size). Mutable core accepts an external RTOS lock. | **Fit, preferred trace-comparison baseline.** Stock alignment recovery and front-end contracts still need implementation and qualification. |
| [`talc` 5.1.1](https://docs.rs/crate/talc/5.1.1/source/src/base/INTERNALS.md) | `no_std` unless the optional std error scanner is enabled, supplied regions and word-sized chunks, variable allocation/free/resize. | Allocation can scan O(free chunks); fixed/manual sources permit O(1) free/in-place resize. `TalcLock` accepts `lock_api::RawMutex`; source callbacks also need bounds. | **Conditional comparator.** Not admitted as an input-independent bounded-time allocator without an explicit scan bound and deadline. |
| [`embedded-alloc` 0.7.0](https://docs.rs/crate/embedded-alloc/0.7.0/source/src/tlsf.rs) | `no_std`; TLSF backend is `rlsf`, LLFF is linked-list first fit; both serve variable Layouts/free. | TLSF bookkeeping is O(1), LLFF scans holes. Every operation uses the process-global `critical-section` provider. Statistics walk blocks under that lock. | **Conditional TLSF wrapper, not a distinct backend.** Needs a dual-core RTOS-aware critical-section implementation. Single-core IRQ masking is insufficient. Direct `rlsf` avoids this global dependency. LLFF does not meet constant bookkeeping. |
| [`offset-allocator` 0.2.0](https://docs.rs/crate/offset-allocator/0.2.0/source/src/lib.rs) | Unconditional `std`; metadata is two preallocated Vecs. Variable offsets and handle-based free, not Layout/pointer allocation. | O(1) two-level bin selection/split/two-neighbor merge after initialization. Mutable core can be externally locked; constructor/reset require an existing allocator. | **Unfit as released.** Fixed arrays, `no_std`, aligned pointer allocation and recoverable handles require a separate port/bridge. Metadata exhaustion can fail with free managed bytes. |
| [`buddy-alloc` 0.6.0](https://docs.rs/crate/buddy-alloc/0.6.0/source/src/buddy_alloc.rs) | `no_std`, supplied-memory core, variable power-of-two blocks and individual free. | O(log(pool/leaf)) search/split/merge and free-order discovery; fixed pool bounds orders. Mutable core accepts an external lock. | **Conditional core-only comparator.** Its exported `GlobalAlloc` ignores `Layout.align` and adds no synchronization. Require a power-of-two leaf, proven absolute alignment and recoverable adapter metadata. Traces must settle internal fragmentation. |
| [`forge-alloc` 0.3.7](https://docs.rs/crate/forge-alloc/0.3.7/source/src/layout/size_classed.rs) | `no_std` with default std disabled; `StaticBacked`, bump arena, reusable slabs and fixed size classes. Pointer-width atomic lowering needs target qualification. | Slot pop/push O(1), routing O(fixed class count), external serialization possible. Fallback and hardening change the bound. | **Unfit as supplied general-heap composition; conditional for fully covered fixed classes.** Oversized, over-aligned and exhausted-class requests fall back to backing. `BumpArena<StaticBacked>` does not individually reclaim those frees. A general reclaiming fallback reintroduces allocator selection. |
| [`bumpalo` 3.20.3](https://docs.rs/crate/bumpalo/3.20.3/source/src/lib.rs) | `no_std` but imports `alloc`, obtains chunks from the global heap; arbitrary Layouts. Arbitrary individual frees retain memory until reset/drop. | Fast bump O(1); growth/cleanup and resize inherit allocator/chunk/copy work. Sharing its Cell-based state requires external locking. | **Unfit as the general heap.** Phase-lifetime scratch within owned code is a separate use. |
| [`slab` 0.4.12](https://docs.rs/crate/slab/0.4.12/source/src/lib.rs) | `no_std` with default std disabled, but Vec-backed uniform `T` slots and integer keys, not variable-sized heap blocks. | Vacant slot operations O(1) with capacity; growth invokes another allocator and may relocate the storage. External locking does not change that. | **Unfit as the general heap.** Bounded typed object tables above a heap are a separate use. |

`rlsf` stores first/second-level bitmaps and a configurable table of free-list heads. On 32-bit its granularity is 16 bytes, used header 8 bytes and free header 16 bytes; aligned allocations can need an extra locator and padding. Provisioning belongs outside the timed path. Bin dimensions must cover this heap's maximum admitted request and alignment overhead, not copy the README's 64 KiB example. `embedded-alloc` fixes a 32-by-32 pointer table on 32-bit, 4,096 bytes before bitmap/wrapper fields; direct `rlsf` permits a smaller justified configuration. These are source layout quantities, not measured footprint.

`rlsf`'s stable public `deallocate(ptr, align)` requires the original alignment, and `reallocate` must use that alignment. Unknown-alignment free is crate-private; public usable-size recovery is behind `unstable` and excluded from API-stability guarantees (`src/tlsf.rs:991-1025,1149-1186`, [README](https://docs.rs/crate/rlsf/0.2.3/source/README.md)). Stock `free(ptr)` therefore needs owned recoverable layout metadata or a proven common-alignment policy, not an alignment guessed from pointer divisibility. Retained calloc's header read is an independent compatibility problem, not solved by choosing in-band or external backend metadata.

Talc's release documents O(n) allocation and scans in `full_search_bin`. Its free/resize need the original Layout including size. The locking wrapper releases/reacquires around copies above `0x4000`; shorter copies remain locked ([wrapper source](https://docs.rs/crate/talc/5.1.1/source/src/sync/mod.rs)). That is a contention policy to measure, not equivalent to a single whole-operation lock. The separate optional no-in-place-realloc path needs its own source review before use.

Buddy blocks are aligned relative to a post-metadata pool base; larger orders alone do not prove larger absolute pointer alignment. Its tables and bitmaps consume pool memory, and free-list links consume free blocks. Power-of-two rounding and minimum leaves add internal slack before any alignment adapter overhead. No fragmentation percentage for the X-M1 workload is established without traces.

Forge's default classes cover 8 through 1,024 bytes with fixed per-class capacities. General fallback, class exhaustion and over-alignment must have explicit reclaim/failure contracts. Its std-only mmap/guard-page/NUMA layers are not available on this RTOS. Canary adds size/padding, and poison-on-free writes the usable extent, making free size-dependent ([hardening source](https://docs.rs/crate/forge-alloc/0.3.7/source/src/hardening/poison.rs)). The upstream README reports Miri/Kani/loom and bare-metal cross-compilation; those are upstream claims, not independently exercised target evidence. The pre-1.0 API and fix history do not establish a maturity or reliability ranking.

The later benchmark set is direct `rlsf`, plus an explicitly aligned/locked buddy core if its contract is completed. Talc is useful only as a labelled linear-scan fragmentation comparator with an admitted bound. Benchmark the embedded wrapper only if adopting `critical-section` is independently justified. Do not benchmark the excluded unmodified releases as if they supplied the required reclaiming heap. None of these sources supplies measured X-M1 cycles, a dual-core RTOS lock, whole-caller compatibility or a production cutover.

## Natural allocation observer design for review

Existing natural startup observers record selected initializer, scheduler and publication boundaries; they do not record heap entry/return pairs or allocation lifetimes. Their traces therefore cannot supply a general-heap size/alignment/lifetime benchmark. The new source candidate is `emulation/fujimod-xm1/src/heap_observe.rs` with the pure `heap_state.rs` reducer and receipt comparison. It is ready for independent source review before execution, not qualified observation evidence.

Reuse pinned FujiMod `Machine`, source identity admission, natural reset/checkpoint handling, instruction-point snapshots and classified memory watches used by `emulation/fujimod-xm1/src/startup_callers.rs` and `kernel_install.rs`. Do not patch guest allocator bytes, synthesize task identity, change return values or inject failing sizes into the natural workload.

- Admit canonical DAT, boot ROM and seven mapped images by identity. Include image 2 in the static census. Record observer identity, run document, checkpoint provenance and memory map. Start at reset for boot lifetimes; a later checkpoint needs a complete live-chunk inventory before any peak-live result is accepted.
- Observe both cores at every identified heap core, graphics adapter, backing/trim and lock entry/return. Sample current guest and physical instruction bytes, all argument/result registers, LR, SP, CPSR mode/IRQ/FIQ bits, per-core instruction counters, selected-root identity and the exact current-task source used by `0x00723cb8`. Record root/task evidence separately from inferred names. Observe exception entry/return or equivalent engine context so IRQ use is not inferred merely from task identity.
- Pair entries with actual returns using pending invocation stacks keyed by task identity and exception context, with per-core active-context tracking. Blocking or preemption can produce non-LIFO returns across tasks on one core; preserve each suspended context's nested calls separately. An LR hit alone cannot pair returns across nested calls, task switches or nonreturning paths. Keep wrapper/core parentage and emit only one logical allocation event for a calloc/realloc operation; preserve internal operations separately for cost/lock analysis.
- Record requested size/count/alignment where present, normalized size, input/output pointers, pre/post chunk size, errno, break, lock owner/depth, lock acquisition/release counters and task/core. Track successful allocation identifiers and exact free/realloc ownership. Failed realloc preserves its old lifetime; moved realloc ends it only on success. Distinguish requested live bytes, stock usable bytes and backing high-water. Report peak over the interleaved global event order and per-task/core mix.
- Watch reads of each live chunk header and named allocator state from outside the retired cores, with physical/guest aliases classified. A hit identifies an internals consumer; a finite no-hit route is not whole-program absence. Retain attribution and code identity for every hit.
- Bound event storage, pending frames, live allocations and instruction budgets. Overflow, unmatched return/free, unknown allocation origin, unsupported execution, invalid task attribution, code mismatch, unresolved alias or watch loss must mark the receipt incomplete and prohibit derived full-trace/peak claims. Emit the exact finite stop and preserved partial records rather than silently dropping events.
- First admit a no-card natural boot/menu round trip and a matching observer-disabled route with equal user-visible outcomes and no guest writes from the observer. Extend to existing reviewed modal/editor and image-development trajectories only where the original observer/route already qualifies. Capture or storage routes require their own qualified host-only scenario; menu coverage is not representative of all camera allocation.
- Independently review mechanism source and controls before runs. Controls must detect dropped events, wrong return pairing including non-LIFO returns from different tasks on one core, foreign free, incorrect realloc failure lifetime, incorrect task/core/exception attribution, overflow and byte-identity mismatch. Guest runs use `//emulation:emulator-slot`, at most two slots host-wide, and explicit scratch output paths.

The source candidate observes the reentrant allocation/free/resize/calloc cores plus backing, trim and lock entry/return; public wrappers remain transparent retained callers. It samples raw current task records from `0x40054760`/`0x400e4720`, derives the task index from the canonical 56-byte tables at `0x4005ff0c`/`0x400efecc` and preserves kernel-context words `0x400545a4`/`0x400e4564` with CPSR mode. The serialized `exception_depth` field carries that raw context word, not an inferred count of nested interrupts. Source identity, current code/physical/TCM views and selected-root service bindings are checked before accepting samples. [The supported command and artifacts](../emulation/README.md#general-heap-observation) describe its finite no-card route and partial-result behavior. Reducer controls include non-LIFO returns from two tasks on one core, exception contexts, nested calls, foreign frees, failed/moved realloc, calloc reconciliation and storage/clock bounds; none has executed in this source increment.

After admitted traces exist, replay identical ordered operations against stock and each fit candidate using the qualified ARMv7-R build/ABI and a reviewed cycle observation method. Measure allocation/free bookkeeping separately from size-proportional zero/copy and from lock wait. FujiMod instruction counts are not silicon cycles. Report worst observed and distribution costs, analytical bounds, requested/usable/backing peaks, fragmentation, metadata, emitted code size and actual task/core contention schedules. Do not replay pointer addresses as placement requirements unless the caller survey proves that requirement. An emulator lacking a qualified cycle model cannot answer target-cycle acceptance by renaming instruction counts.

## Reproduction and evidence anchors

Owner-supplied canonical image-4 and image-5 bytes are pinned by the existing builders; raw firmware and listings are not redistributed. Private source paths, digests and static anchors are retained in the two evidence records. The survey used listing reads and exact target searches, the pinned upstream source, and LLVM's existing objcopy/objdump to generate the missing image-2 listing with explicit scratch output paths. No new observer/checker executed, no guest ran and no benchmarks ran. The integration owner should run `//tests:research-contracts` after integrating the ledger append.
