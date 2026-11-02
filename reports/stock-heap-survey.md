# X-M1 1.50 general heap survey

## Scope and decision

[#148](https://github.com/skeet70/latent-image/issues/148) covers the general C heap and its future Rust replacement, not every stock resource reservation. This survey establishes the newlib/dlmalloc family, concrete entry ABIs, the direct-call surface, one cross-image chunk-header dependency, backing bounds and the [#147](https://github.com/skeet70/latent-image/issues/147) resize defect. Static findings are E-STOCK-HEAP-SURVEY-150 and E-STOCK-HEAP-RESIZE-LOCK-150. One complete no-card boot/menu trace is admitted. Complete indirect-call closure, whole-firmware task/core/IRQ attribution and representative capture/development/storage traces remain open.

**Select direct `rlsf` 0.2.3 as the replacement allocator core.** The reviewed, admitted comparison below establishes its finite workload costs and input-independent bookkeeping tradeoff against stock, Talc and BuddyAlloc. It is not the fastest mean or smallest native footprint on this tape. No production allocator is installed, and this backend choice does not qualify the stock compatibility front end, a cross-core lock, representative workloads or silicon deadlines.

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

Additional retained client adapters do not introduce another allocator. `0x006dbf38(r0=bytes)` forwards unchanged to malloc. `0x0072a7ac` consumes r1=bytes, r2=output pointer and signed r3: nonnegative r3 returns `-9`, while negative r3 calls malloc, stores the payload through r2 and returns 0, or returns `-42` on null. Errors are also stored at `0x009121b8`. `0x0072a7fc` takes the payload in r1, rejects null with `-30` and that error-cell write, otherwise calls free and returns 0. These service-layer errors are distinct from newlib's context errno and null result. Their argument roles beyond the observed branch are not assigned a pool or alignment meaning.

## Confirmed resize defect

For a valid non-null old allocation, `0x0089da7c` takes the recursive lock before `bytes+11` normalization. Requests in inclusive range `[0x7ffffff5, 0xffffffff]` fail the signed-size/overflow checks. The core stores 12 through context, sets the result to null and branches at `0x0089dabc` to `0x0089dc28`, bypassing the unlock call at `0x0089dc20`.

The owner and depth cells are `0x40a6c7d8` and `0x40a6c7dc`. The rejecting task retains one additional depth level. A different task subsequently entering the heap waits on semaphore `0x3f` with timeout `0xffffffff`. This is a static control-flow finding, not an observed stock workload hang. Natural stock oversized-call reachability is outside #147's scope.

The [allowed-calls precondition](../transport/README.md#retained-heap-call-preconditions) requires owned callers to reject sizes above `0x7ffffff4` before stock resize, with checked arithmetic before comparison. A replacement must reject oversized requests before locking and release its lock on every return. It must not emulate the leaked lock depth.

## Backing memory and distinct mechanisms

The initialized break cell `0x00829a08` contains `0x00a71a00`. Backing helper `0x0072b110` accepts an increment only when its word-arithmetic test `old_break + increment + 0x20 <= 0x00bf1a00` succeeds. It then advances the cell and returns the old break. There is no independent lower-bound check in that helper; it relies on its allocator caller for shrink validity. This is a reserved RAM interval, not an operating-system page mapping or an unlimited memory source.

The stock envelope `[0x00a71a00, 0x00bf1a00)` is `0x180000` bytes (1,572,864). The `0x20` guard reduces the maximum break span to 1,572,832 bytes before allocator metadata, initial alignment, top-chunk and page-growth overhead. The recipe fixed carve changes the bound instruction at `0x0072b130` from `0xe34030bf` to `0xe34030be`, producing upper limit `0x00be1a00`: a 65,536-byte loss and envelope `0x170000` bytes (1,507,328), with at most 1,507,296 break-span bytes before overhead. `host/firmware/BuildRecipeBank.hs` records the publication and capacity loss. A heap-allocated RAM-push bundle instead consumes a live allocation; its relocation address does not by itself prove the fixed carve was applied.

The compiled-layout/source-family reconstruction accounts for 1,092 external allocator-state bytes: the 258-word bin array `[0x0089d4f8,0x0089d900)` occupies 1,032 bytes; `sbrk_base` and trim threshold at `0x0089d900`/`0x0089d904` occupy 8; BSS `[0x009adda4,0x009addd8)` occupies 52. The BSS anchor has top pad at offset 0, the ten-word mallinfo working copy at 4, and the two maximum counters at `0x2c`/`0x30`, confirmed by allocation loads/stores and the pinned newlib declarations. This is reconstructed object extent, not a runtime symbol-size measurement. The backing cursor's 4 bytes and recursive lock owner/depth's 8 bytes are separate support state; the lock is shared by the replacement comparison.

`0x007235d8`, previously called an image-4 allocator in an observation report, is a selected-root synchronization/IRQ-state adapter paired with `0x007236b0`, not general heap allocation. Its entry selects service `+0x1618`, calls selected-root `+0x164` and may synchronize through `+0x161c`; the paired restore uses `+0x168`. Its callers must not be counted as malloc users.

The file-handle helper `0x0071ef18` waits on semaphore `0x3c` and scans sixteen handle slots at `0x009101c0`; it is not this heap. [#143](https://github.com/skeet70/latent-image/issues/143)'s task-storage reservation uses cursor `0x4089ea98`, pool base `0x40a10200`, indexed records and lock `0x409c280c`, also outside this heap replacement. Their retained lifetimes remain separate obligations.

A second boundary-tag family in plain/image 6 is a distinct SQLite memsys3-style pool. Its method table `0x01446cac` selects malloc `0x0140ce3c`, free `0x014439cc`, resize `0x0144392c`, size `0x0140cde8`, roundup `0x0140ce14`, init `0x0140ce58` and shutdown `0x0140cefc`. Its size accessor returns `2*(word(payload-4)&~3)-4`, not newlib's usable-size formula. The application publishes fixed base `0x1f9f8000` and size `0x300000`; the recovered configuration chain installs this method table through option 8 and then invokes its initializer. The resulting configured interval is `[0x1f9f8000,0x1fcf8000)`, with 393,214 eight-byte pool blocks. This matches the [pinned SQLite source family](https://raw.githubusercontent.com/sqlite/sqlite/d1d89140c037d3d7c7641da37ca3bff8df146bed/src/mem3.c), not an identified linked revision. These negative-header accesses must not be counted as newlib metadata consumers. Runtime activation, physical mapping/reservation and complete computed bridge closure remain unproved.

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

The heap lock queries task identity through `0x00723cb8`, whose selected-root service calls are indirect. The canonical A/B services reach `0x0004c620`/`0x000dc5e0`: a nonzero kernel-context word or set CPSR.I returns `-25`; their wrappers return identity zero on failure. The lock hard-loops on zero identity. A different owner waits indefinitely; the same numeric owner increments recursion depth. These facts establish context and blocking preconditions, not the complete allocating-task set or a cross-core exclusion proof.

Graphics callback users and direct image-5 users establish clients outside the wrapper-only surface. Static inspection here does not attribute every client to a runtime task or core. No positive interrupt-to-heap allocation path was established, and absence has not been proved. A masked interrupt or nonzero kernel context cannot obtain the lock's admitted task identity. Clear CPSR.I and a zero context word alone do not establish task-mode execution; the observer preserves CPSR mode separately. Allocator locking must not be used from the IRQ-masked trace-ring callback described in [the observation report](af-observe-instruments-150.md).

## Pinned candidate fitness

These verdicts come from the published crates.io source archives, not compilation or target execution. Versions are exact release pins; the ledger preserves archive/checksum provenance and source ranges. A fit core is not a qualified stock bridge. All timing bounds below exclude initialization, payload zero/copy and external lock wait. A finite RAM capacity mathematically limits a linear scan, but does not establish an acceptable real-time deadline.

| Candidate pin | `no_std`, ARM32 and reclaim | Time and external locking | Verdict |
| --- | --- | --- | --- |
| [`rlsf` 0.2.3](https://docs.rs/crate/rlsf/0.2.3/source/src/tlsf.rs) | Unconditional `no_std`, explicit 32-bit layouts, caller-provided pool, variable Layout allocation and coalescing individual free. | O(1) allocate/free bookkeeping; moving resize is O(old size). Mutable core accepts an external RTOS lock. | **Fit, selected allocator core.** The admitted comparison is below; stock-front-end provenance and production locking remain qualification boundaries. |
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

The admitted comparison uses direct `rlsf`, fixed/manual Talc as a labelled linear-scan comparator, and an explicitly aligned/locked BuddyAlloc core. The embedded wrapper is not a distinct backend and its global critical-section dependency is not justified here. Excluded unmodified releases are not benchmarked as if they supplied the required reclaiming heap. These measurements do not establish silicon cycles, a dual-core RTOS lock, whole-caller compatibility or a production cutover.

## Natural allocation observer design for review

Existing natural startup observers record selected initializer, scheduler and publication boundaries; they do not record heap entry/return pairs or allocation lifetimes. Their traces therefore cannot supply a general-heap size/alignment/lifetime benchmark. The new observer is `emulation/fujimod-xm1/src/heap_observe.rs` with the pure `heap_state.rs` reducer and receipt comparison. Independent source review preceded its execution. Source qualification and incomplete natural observations do not establish admitted heap lifetimes.

Reuse pinned FujiMod `Machine`, source identity admission, natural reset/checkpoint handling, instruction-point snapshots and classified memory watches used by `emulation/fujimod-xm1/src/startup_callers.rs` and `kernel_install.rs`. Do not patch guest allocator bytes, synthesize task identity, change return values or inject failing sizes into the natural workload.

- Admit canonical DAT, boot ROM and eight mapped images by identity, including image 2. Record observer identity, run document, checkpoint provenance and memory map. Start at reset for boot lifetimes; a later checkpoint needs a complete live-chunk inventory before any peak-live result is accepted.
- Observe both cores at every identified heap core, graphics adapter, backing/trim and lock entry/return. Sample current guest and physical instruction bytes, all argument/result registers, LR, SP, CPSR mode/IRQ/FIQ bits, per-core instruction counters, selected-root identity and the exact current-task source used by `0x00723cb8`. Record root/task evidence separately from inferred names. Observe exception entry/return or equivalent engine context so IRQ use is not inferred merely from task identity.
- Pair entries with actual returns using pending invocation stacks keyed by task identity and exception context, with per-core active-context tracking. Blocking or preemption can produce non-LIFO returns across tasks on one core; preserve each suspended context's nested calls separately. An LR hit alone cannot pair returns across nested calls, task switches or nonreturning paths. Keep wrapper/core parentage and emit only one logical allocation event for a calloc/realloc operation; preserve internal operations separately for cost/lock analysis.
- Record requested size/count/alignment where present, normalized size, input/output pointers, pre/post chunk size, errno, break, lock owner/depth, lock acquisition/release counters and task/core. Track successful allocation identifiers and exact free/realloc ownership. Failed realloc preserves its old lifetime; moved realloc ends it only on success. Distinguish requested live bytes, stock usable bytes and backing high-water. Report peak over the interleaved global event order and per-task/core mix.
- Watch reads of each live chunk header and named allocator state from outside the retired cores, with physical/guest aliases classified. A hit identifies an internals consumer; a finite no-hit route is not whole-program absence. Retain attribution and code identity for every hit.
- Bound event storage, pending frames, live allocations and instruction budgets. Overflow, unmatched return/free, unknown allocation origin, unsupported execution, invalid task attribution, code mismatch, unresolved alias or watch loss must mark the receipt incomplete and prohibit derived full-trace/peak claims. Emit the exact finite stop and preserved partial records rather than silently dropping events.
- First admit a no-card natural boot/menu round trip and a matching observer-disabled route with equal user-visible outcomes and no guest writes from the observer. Extend to existing reviewed modal/editor and image-development trajectories only where the original observer/route already qualifies. Capture or storage routes require their own qualified host-only scenario; menu coverage is not representative of all camera allocation.
- Independently review mechanism source and controls before runs. Controls must detect dropped events, wrong return pairing including non-LIFO returns from different tasks on one core, foreign free, incorrect realloc failure lifetime, incorrect task/core/exception attribution, overflow and byte-identity mismatch. Guest runs use `//emulation:emulator-slot`, at most two slots host-wide, and explicit scratch output paths.

The observer covers the reentrant allocation/free/resize/calloc cores plus backing, trim and lock entry/return; public wrappers remain transparent retained callers. It samples raw current task records from `0x40054760`/`0x400e4720`, derives the task index from the canonical 56-byte tables at `0x4005ff0c`/`0x400efecc` and preserves kernel-context words `0x400545a4`/`0x400e4564` with CPSR mode. The serialized `exception_depth` field carries that raw context word, not an inferred count of nested interrupts. Source identity, current code/physical/TCM views and selected-root service bindings are checked before accepting samples. [The supported command and artifacts](../emulation/README.md#general-heap-observation) describe its finite no-card route and partial-result behavior. Initial SDK packages pass 144 tests, including 27 heap-reducer tests covering non-LIFO returns from two tasks on one core, exception contexts, nested calls, foreign frees, failed/moved realloc, calloc reconciliation and storage/clock bounds. The source-reviewed lifecycle package `5f81658` passes 153 tests, including 36 heap/lifecycle cases.

After admitted traces exist, replay identical ordered operations against stock and each fit candidate using the qualified ARMv7-R build/ABI and a reviewed cycle observation method. Measure allocation/free bookkeeping separately from size-proportional zero/copy and from lock wait. FujiMod instruction counts are not silicon cycles. Report worst observed and distribution costs, analytical bounds, requested/usable/backing peaks, fragmentation, metadata, emitted code size and actual task/core contention schedules. Do not replay pointer addresses as placement requirements unless the caller survey proves that requirement. An emulator lacking a qualified cycle model cannot answer target-cycle acceptance by renaming instruction counts.

### Source and natural-run qualification

The reviewed observer and receipt consumer compile in the ordinary SDK package. The first two build attempts failed before any observer execution: the Bazel Nix source closure omitted the new files, then Rust rejected a private-module import and an uninferred watch-handle type. The corrected source closure and existing binary-local module pattern produce a passing SDK package. These failures are packaging/compiler evidence, not guest behavior.

Producer `60842bf` refuses its enabled route at the first malloc-core entry because raw coprocessor access does not expose the declared R-profile identity input. Its disabled route completes but remains a superseded-producer result. Producer `f172b30` uses the already-qualified `rprofile_identity` API, passes identity/root/task sampling, then refuses the canonical-code check at the same first heap boundary. Both raw hits name core 0, PC `0x0089cde4`, request `0x180`, LR `0x0089cdb8`, SP `0x00a23c78` and aggregate instruction count 29,399,554. Neither enabled output contains an admitted event or lifetime.

The matching `f172b30` disabled route completes the fixed 450,000,000-instruction menu round trip with per-core counts 385,422,633 and 64,577,367, startup menu `(69,52,12)` and return menu `(1,5,2)`. Its framebuffer matches the older disabled result. A successful disabled route does not admit the incomplete enabled output. No enabled/disabled semantic comparison or genuine-receipt refusal control has run.

The diagnostic producer `d8a78fa` passes the same 144 SDK tests, then its single enabled route identifies the actual preflight mismatch: image-5 calloc wrapper `0x00bf1c1c` contains zero on core 0 instead of canonical instruction `0xe1a02001`. Its halted failure checkpoint is preserved. This is an early image-5 residency observation at that finite first heap boundary, not an image-4 code mutation or a universal image-load order.

#### Systematic CODE extent qualification

The reviewed `replacement/media_permit/StockInventory.hs` derivation checks all 21 declared guard groups as 31 complete function windows against four digest-pinned images. One actual derivation invocation emits every result; two earlier driver compilation attempts failed before derivation on partial-list and redundant-import warnings. The successful driver invocation returns a failing overall verdict for the original declarations and preserves the individual findings.

The derived windows expose two one-word truncations: the resize range omits its `0x0089dfbc` literal, and the callback-adapter range omits malloc's `0x00890f04` return. The corrected ends are `0x0089dfc0` and `0x00890f08`. The original two-instruction flashloader callsite guard is replaced by its completely derived containing function `[0x0073b3a0,0x0073b728)`, including the bounded dispatch table and trailing literals.

Twenty-seven function windows derive successfully. Four remain explicit library refusals: CP14 MRC at `0x000427b8`/`0x000d27b8`, and system-register access at `0x0004c640`/`0x000dc600` within the task-query functions. Their canonical bytes remain guarded. The native-call derivation library and its instruction admission are unchanged; these refusals do not qualify the four functions as native-callable.

#### Image-5 heap-code admission

The [later-image loading survey (#153)](https://github.com/skeet70/latent-image/issues/153) identifies the producer. Flashloader command `0x4c00` calls `0x006a96c0` at `0x0073b4ac`. The producer configures the decoder at `0x006ae360` with destination `0x00bf1a00` and staging `0x43e40000`, reads the compressed record, drains at `0x006ae3f4` and reaches pre-return `0x006a9840`. Hardware driver `0x006843c8` supplies the decoder source/destination/count, and its model-bus writes populate decoded image 5. Source inspection and the early zero readback identify a lifecycle premise that the original all-images-resident preflight did not admit.

The changed guard passes independent source review and reaches Admitted in the preserved `b3ecd0c` prefix, before a later watch-storage refusal. Its closed states are Unloaded, Installing and Admitted. It binds the original producer caller, selected task/context and saved frame, destination/staging arguments and decoder-drain order. A successful producer result admits the calloc code only after complete code/header readbacks agree with the canonical source through both cores' low/high guest aliases and physical memory. Image-5 installation writes are permitted only as model-bus writes within the recovered image extent while Installing; all other guarded-code writes remain forbidden. Initial calloc-code readbacks must be zero. Reinstallation cannot reopen the frozen guard.

Before admission, halfword instruction points across the complete calloc wrapper/core span in both guest aliases refuse entry before execution. After admission those shadow points are removed while retaining the ordinary calloc entry observation. The actual header transfer at `0x0069d2e0` separately checks the canonical image/code view, done cell and target `0x00d985a4`. The state records observer admission of the selected heap code. Recovered plain hooks and callbacks can precede header entry, so that transfer is not used as the admission gate.

The independent receipt consumer reconstructs this state machine from raw-boundary-bound snapshots and serialized memory/code views. The 36 executed heap/lifecycle tests cover pre-admission aliases, missing decoder stages, wrong context/frame/arguments, failed storage, missing code identity and reopening alongside the lifetime reducer cases. Additional genuine-receipt controls target a coherent premature calloc fetch and coherent corruption of the initial code view; they have not run.

#### Fetch clock qualification

The single enabled `5f81658` route records the image-5 producer in task 60, domain 0, SVC mode with kernel context zero, and a completely zero initial image-5 header/calloc view through physical memory and both cores' two aliases. Producer entry `0x006a96c0` is at 29,279,717 instructions; decoder entry `0x006ae360` is at 29,284,475. The first watched interval then refuses `MissingFetch` before any heap event. Its raw fetches are stamped 29,279,718 and 29,284,475. The paired disabled route passes the same menu/frame endpoint as prior disabled runs.

The former `end+1` expectation for the stopped next fetch is wrong for this pinned hook order. The SDK increments before instruction hooks, the earlier point hook rolls back a stopped entry, and later fetch hooks still run. A resumed start fetch is `begin+1`; a stopped next fetch is `end`. The source correction retains exact ordered fetch matching and tightens every CPU event to the interval endpoint. Independent source review covers that correction and a public SDK fixture that checks both same-coordinate fetches, the unexecuted second instruction and its later resumed coordinate. The `b3ecd0c` SDK build passes 154 tests, including 36 heap/lifecycle and 20 public SDK integration tests, including the new fixture.

#### Bounded watch-storage qualification

The single `b3ecd0c` enabled route progresses through image-5 code admission and 16,199 reduced heap events, then refuses the unchanged 256 MiB plain-watch bound at counts `[140975427,3941368]`, with one pending invocation and 16,204 watch batches. There is no machine failure, guarded-code write or observed external header read in that prefix. The matching disabled route completes 450,000,000 instructions with the unchanged menu/frame endpoint. The enabled receipt remains incomplete and supplies neither global peaks nor a representative benchmark trace.

The preserved prefix contains successful logical malloc returns in scheduler domain 0 from task 33 (1,100), task 72 (2) and task 73 (291), all on physical core 0, SVC mode, kernel context zero and IRQ unmasked. These are finite positive calling-context observations, not task-name attribution, a complete task/core mix or proof that IRQ/core-1 allocation never occurs. Image-5 producer pre-return is observed in task 60 at counts `[27839893,3221942]`; the separate done/header transfer and first header instruction occur in task 59 at `[28788904,3222321]` and `[28788905,3222321]`.

Lossless host gzip of the preserved 268,434,764-byte watch prefix produces 7,601,489 bytes. That is a storage observation, not allocator performance. The schema-3 source keeps the same route and raw rows, streams bounded gzip with trailer/CRC/EOF checks, raises the reducer/boundary ceilings to 131,072/262,144, preserves a failed terminal batch and marks finish errors incomplete. Compressed output remains bounded at 256 MiB, decoded output at 1 GiB and each decoded line at 64 MiB. Independent review found no issues in this revision, its 11 I/O fixtures or the four additional genuine-receipt refusal controls.

#### Admitted natural heap trace

The `0fffa83` package builds with 165 passing SDK tests, including 47 heap/lifecycle/I/O and 20 public SDK integration tests. One enabled and one disabled canonical no-card route each complete 450,000,000 instructions with counts `[385422633,64577367]`, menus `[69,52,12]` then `[1,5,2]` and the same final frame. The independent consumer admits both receipts, raw boundaries, code/task bindings, exact watch projections, gzip EOF/CRC and lifetime replay. All ten genuine-receipt controls refuse at their intended checks (E-STOCK-HEAP-NATURAL-TRACE-150).

The admitted route contains 51,214 reduced events and 51,220 watch batches, with no pending invocation, code write or observed external live-header read. Its 4,490 logical mallocs all succeed; 4,039 logical frees leave 451 allocations live. Requests span 0 through 19,200 bytes across 73 distinct sizes. Eight-byte ordinary allocation alignment comes from the recovered malloc ABI, not a separately supplied alignment argument. No logical realloc or calloc occurs.

| Scheduler domain / task | Physical core | Malloc calls | Free calls |
| --- | --- | ---: | ---: |
| 0 / 33 | 0 | 3,648 | 3,405 |
| 0 / 72 | 0 | 2 | 17 |
| 0 / 73 | 0 | 840 | 617 |

Every completed logical call has SVC mode, kernel context zero and IRQ unmasked. At most one logical call is pending at a time, so this route contains no overlapping logical allocation calls or two-core heap contention. Those observations cover the named entry surface and route, not whole-program IRQ/core-1 absence. Task labels remain numeric scheduler identities rather than inferred task names.

Peak requested live bytes are 115,185 and peak stock usable live bytes are 116,880. Final requested/usable live bytes are 113,309/115,052. The break grows from `0x00a71a00` to a high-water `0x00a8f000`, 120,320 backing bytes. The final reduced boundary counters `[349805630,21293810]` precede the checked route endpoint; no later observed heap call changes these lifetime totals. Lossless watch storage is 25,046,399 compressed bytes for 872,292,909 decoded bytes. These are finite menu-workload measurements, not allocator replay timing or target cycles.

## Allocator selection and admitted replay

The independently reviewed `ab26725` replay package builds with 227 passing SDK tests. Its replay test binary has 62 tests: 15 tape/consumer tests and 47 shared heap/reducer/I/O tests. Each exact-pinned `no_std` ARMv7-R Cortex-R4F subject builds and links. One case for each of stock, `rlsf` 0.2.3, `talc` 5.1.1 and `buddy-alloc` 0.6.0 at each original/recipe capacity admits, eight cases total (E-STOCK-HEAP-REPLAY-150). All eight genuine serialized replay controls refuse at their intended checks: missing step, allocation result, footprint, statistics, counter, diagnostic role, payload data and code watch.

Every case replays the same 8,529 logical operations, including all 4,490 successful mallocs and 4,039 frees, without removing failures or changing sizes. It restores the preserved first-heap-call checkpoint and executes under isolated core 0/task 73 with interrupt delivery suppressed. Original tasks 33/72/73 remain trace attributions, not recreated scheduling. Native subjects take original owned Layouts; this is not an installed pointer-only stock bridge. The actual retained lock/unlock execute for allocator calls. Code, immutable constants/unwind data, raw frames, payload patterns, physical block statistics and instruction counts bind the receipts. Allocation-success divergence refuses the case.

The initial source qualification preserves the SDK type/staging failures, the oversized non-GC Buddy link and the first stock replay's fixed-quantum refusal before any guest instruction. The corrected source retains the pinned 1,000-instruction processor quantum and links only reachable sections with all nine exported ABI roots retained. The 256 KiB isolated image window is unchanged. A Buddy recipe launcher reports a transient shell export-file warning; its command exits 0 and its actual receipt admits. No failed or superseded artifact is substituted for a passing case.

### Allocator instruction costs

All values below are **emulated A32 entered instructions, not silicon cycles**. They include each allocator call and its actual uncontended retained lock. Initialization, diagnostics, host payload fills and target zero/copy work are excluded. The tape has no logical resize or calloc, so their implemented composition paths supply no runtime workload samples.

| Backend / operation | Samples | Mean | Median | p95 | p99 | Maximum |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Stock malloc | 4,490 | 503.3 | 497 | 510 | 639 | 1,284 |
| Stock free | 4,039 | 494.7 | 487 | 521 | 526 | 526 |
| rlsf malloc | 4,490 | 580.9 | 586 | 586 | 586 | 586 |
| rlsf free | 4,039 | 550.5 | 556 | 584 | 584 | 584 |
| Talc malloc | 4,490 | 565.3 | 569 | 579 | 583 | 583 |
| Talc free | 4,039 | 533.0 | 536 | 555 | 555 | 555 |
| Buddy malloc | 4,490 | 588.5 | 573 | 684 | 769 | 880 |
| Buddy free | 4,039 | 670.9 | 645 | 847 | 988 | 1,035 |

Both capacities have the displayed quantiles. Buddy malloc means are 588.470/588.487 before rounding; the other allocator means are identical across capacities. These distributions are finite observations, not worst-case execution-time proofs. Stock has the lowest means here. Talc beats rlsf's mean, free cost and observed allocation maximum on this tape, but its allocation scan remains dependent on free-list shape.

### Live footprint and allocator state

Requested peak/final bytes are 115,185/113,309, with 451 objects live at the final boundary. All eight cases preserve those requested lifetimes and successful outcomes. The two capacities have identical live-block metrics below.

| Backend | Peak allocated block bytes | Final allocated block bytes | Final live headers/rounding bytes | External state bytes | Permanent pool reserve bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| Stock | 118,640 | 116,856 | 3,547 | 1,092 | 0 |
| rlsf | 121,168 | 119,360 | 6,051 | 1,148 | 16 |
| Talc | 119,968 | 118,176 | 4,867 | 32 | 384 |
| Buddy | 154,448 | 151,792 | 38,483 | 52 | 49,520 |

Final live overhead is allocated block bytes minus requested bytes at that same boundary, not the difference between independent peaks. Permanent pool reserve is managed bytes minus allocated/free blocks; it includes pool metadata, sentinels and unavailable tail. Live headers are already in allocated block bytes. Native external state includes the comparison status word. Stock's 1,092 bytes are the compiled-layout reconstruction above, not a runtime symbol-size measurement. Its backing cursor and the common retained lock state remain separate support bytes.

### Final free-space geometry

The fraction is `(free bytes - largest free extent) / free bytes` at the final boundary. Stock's uncommitted backing is added to free bytes and coalesced geometrically with its top chunk. The stock summary's minimum-largest-free value of 16 covers only the committed break and is not comparable with full-pool candidate values. These geometric extents are not maximum serviceable payload requests, growth-page guarantees or allocation-failure probabilities.

| Backend / capacity | Final free bytes | Largest free extent bytes | Geometric free fragmentation |
| --- | ---: | ---: | ---: |
| Stock original | 1,455,976 | 1,454,192 | 0.123% |
| Stock recipe | 1,390,440 | 1,388,656 | 0.128% |
| rlsf original | 1,453,456 | 1,442,464 | 0.756% |
| rlsf recipe | 1,387,920 | 1,376,928 | 0.792% |
| Talc original | 1,454,272 | 1,443,296 | 0.755% |
| Talc recipe | 1,388,736 | 1,377,760 | 0.790% |
| Buddy original | 1,371,520 | 1,048,576 | 23.546% |
| Buddy recipe | 1,305,984 | 1,048,576 | 19.710% |

Buddy's extent metric includes its power-of-two order geometry. A smaller percentage in the smaller pool is not increased capacity or better request coverage.

### Linked comparison images

| Native subject | `.text` bytes | Immutable payload bytes | Reserved BSS bytes | Whole image extent bytes |
| --- | ---: | ---: | ---: | ---: |
| rlsf | 4,224 | 4,240 | 1,152 | 5,392 |
| Talc | 4,312 | 4,328 | 32 | 4,368 |
| Buddy | 6,792 | 7,212 | 64 | 7,280 |

These images include owned Layout adaptation, diagnostics, zero/copy and retained-lock tail-call adapters. They exclude a production stock compatibility bridge. Reserved BSS/image extents include linker alignment; they are not the same quantity as the state structure's size. Recovered stock allocator entry-body/literal extents total 4,336 bytes, with 352 additional call-derived copy/clear-helper bytes: move `[0x0089d908,0x0089da3c)` is 308 bytes and clear `[0x00043550,0x0004357c)` is 44. The backing helper is 84 bytes and shared lock/unlock 172 bytes. Those recovered extents and the native comparison images are not matched minimal production binaries. In a stock receipt, `payload_text_bytes=4224` describes the unused helper fixture, not stock allocator code.

E-STOCK-HEAP-REPLAY-HELPERS-150 supersedes the earlier helper attribution. Original cited artifacts and all replay results remain unchanged.

### Decision and production boundary

Direct rlsf is selected for input-independent constant bookkeeping, a finite allocation maximum below stock's on this tape, and live/reserved space costs far below the measured Buddy configuration. It is not selected by mean speed: stock and Talc are faster here, and Talc uses less native allocator state. Talc remains a linear-scan comparator without an admitted deadline; Buddy's bounded-order core pays substantially more live rounding and permanent pool reserve in this configuration.

The [two front-end contracts](replacement-inventory-baseline.md#814-general-heap-survey-and-replacement-boundary-147-148) remain the cutover boundary. New Rust code uses its owned Layout directly over rlsf, not the stock bridge. Stock pointer-only calls need owned layout/alignment provenance or a qualified common-alignment policy; no alignment is guessed from pointer divisibility. The complete wrapper/core/calloc/trim/callback closure must migrate together, and no retained calloc may read a foreign chunk header. Arithmetic and size checks precede locking, and every normal return releases the lock. The resize leak is not a compatibility requirement.

This selects an allocator core, not a production heap installation. Representative capture/development/storage traces, over-aligned requests, real allocation failures, logical resize/calloc workloads, cross-core contention and silicon cycle/deadline evidence are not supplied. Whole-firmware indirect/address-sensitive caller closure and task/core/IRQ absence remain unproved. No camera, SD card or persistent device operation occurred.

## Whole-subsystem cutover contract

The cutover unit is the whole general heap, not another allocation-function pilot. The existing stock addresses remain the entry surface for direct, computed and callback arrivals. Source inspection identifies the initialized graphics array at `[0x890f08,0x890f18)`: resize, allocate, nonreturning error and free targets occupy consecutive words. Concrete constructed-address indirect calls include allocate `0x829d70`, resize `0x82ef64` and free `0x82c664`. The three allocation callbacks continue forwarding to patched public entries. The adapters `0x72a7ac/0x72a7fc` retain their distinct service errors and output/error cells; their incoming service-table identities remain unresolved.

Production admission is conditional on no unknown interior entry outside the guarded scenarios, no retained metadata reader beyond `payload-4`, and no retained writer to retired allocator data reused as code. The stock front end owns fixed alignment-eight provenance and recoverable prefix metadata. Its `payload-4` word retains the stock boundary-tag size/low-flag encoding, with integrity data elsewhere in the prefix. Rust `GlobalAlloc`/`Allocator` use original Layouts directly over the same rlsf backend, not the stock compatibility front end. An exact-byte fetch/write guard and derived owned instruction set qualify the exercised package, not whole-firmware absence.

The explicit interim synchronization backend is stock recursive lock/unlock `0x72b164/0x72b1cc`. Checked calloc multiplication replaces the stock wrapping product; oversize resize rejects before locking instead of retaining #147's leaked depth; zero malloc/resize remains minimum-nonnull on success; fixed-pool trim returns zero instead of releasing page-rounded top backing. The backing helper remains retained pending external-reference closure and must not independently allocate into the owned arena.

Placement and initialization must account for phase0's conditional clear `[0x89ec00,0xbf1a00)`, which includes the arena. An initialized sentinel in retired bin storage below that interval can survive the clear; state in the arena must be initialized under the retained lock before the first allocation. This is an ownership design, not evidence that semaphore creation, cross-core publication or silicon concurrency has been qualified. E-GENERAL-HEAP-CUTOVER-SOURCE-150 records the source facts and residual premises.

## Reproduction and evidence anchors

Owner-supplied canonical images, DAT and ROM are pinned; raw firmware and listings are not redistributed. The ledger preserves static anchors and source pins, the reviewed observer cohorts, the admitted natural tape, secondary-pool/state reconstruction and the eight-case replay/control manifest. Review of E-STOCK-HEAP-WATCH-BOUND-150 and E-STOCK-HEAP-NATURAL-TRACE-150 independently checked their artifact hashes, raw trace metrics and gzip EOF. E-STOCK-HEAP-REPLAY-150 binds the replay source, builds, preserved failures, frozen target artifacts, raw cases and derived measurements. The integration owner should run `//tests:research-contracts` after integrating the additive ledger records.
