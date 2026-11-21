# General heap cutover

## Design and interface

The owned subsystem is `replacement/general_heap`: pinned `rlsf` 0.2.3, an owned allocation-layout core, a pointer-only stock compatibility front end and a Rust `GlobalAlloc`/`Allocator` front end over the same arena. The existing implementation on main is the starting point, not a second backend to replace. Reservation, file-handle and SQLite pools are outside this boundary.

Retained callers enter allocate `0x89cda4/0x89cde4`, free `0x89cdc4/0x89e560`, resize `0x89da3c/0x89da64`, calloc `0xbf1c1c/0xbf1c44` and trim `0x89e464`. Ordinary A32 AAPCS, eight-byte payload alignment, minimum nonnull zero-size allocation on success, null free, failed-resize ownership and context errno 12 remain boundary contracts. Stock pointer-only free and resize recover owned layout metadata from a prefix; Rust callers supply their original `Layout` directly. The image-5 calloc core migrates with image 4 and never reads a foreign allocator header.

The recursive lock pair `0x72b164/0x72b1cc` is the retained RTOS synchronization interface for this cutover, including semaphore `0x3f`, recursive task ownership and its invalid-context refusal. Graphics callbacks `0x890ef8/0x890ee8/0x890ed8` and service adapters `0x72a7ac/0x72a7fc` remain interfaces to retained clients. Their forwarding, output stores and distinct errors `-9/-42/-30` remain unchanged. The backing helper `0x72b110` remains present for unresolved external references; the owned allocator does not call it. It must not compete for the owned arena.

## Placement and staging

The allocator remains one linked image distributed over retired stock carriers: image 4 `[0x89cda4,0x89d4f8)`, `[0x89d4fc,0x89d908)`, `[0x89da3c,0x89dfc0)`, `[0x89e464,0x89e7c4)` and image 5 `[0xbf1c1c,0xbf1cd4)`. Their capacity is 5,372 bytes. `stage-general-heap` reuses image-4 staging and shared record-5 staging; there is no runtime overwrite of a live allocator. Fixed placement `[0x00a9fa00,0x00bf1a00)` is unnecessary while the artifact fits these carriers.

The sentinel at `0x89d4f8` is staged below phase-0 arena clearing. First use initializes owned state under the retained lock. The original arena is `[0xa71a00,0xbf1a00)`, or `[0xa71a00,0xbe1a00)` for the recipe-ceiling build, with a 32-byte end guard and charged state prefix. Retirement guards authenticate staged carriers and reject owned fetches outside admitted code. They do not tie every interior fetch to a declared entry or pending heap call after sealing. Unknown interior/computed callers and metadata readers remain final-pass obligations.

## What is compared

Allocation addresses intentionally differ. Compare the ordered logical interface calls by operation, context, request arguments, allocation identity assigned at creation, success or failure, minimum eight-byte alignment and errno before/after. A free or resize identifies its input by that allocation identity, not by a raw address. Resize compares preserved caller-visible bytes and failed-resize ownership; the full-system observer samples the requested calloc product bytes and requires zero, while the host suite checks the entire advertised usable extent. Do not compare private bin state, allocator instruction counts, scratch registers or allocation addresses. Compare full-system menu endpoints and rendered frame results separately from interface traffic. Require stock instructions inside the allocator extents and a deliberately wrong owned binary that the comparison detects; journal mutations alone are comparator controls, not faulty-allocator execution.

Cold boot tests boot publication. Runs testing only post-boot behavior must restore a saved role-specific post-boot checkpoint with the same executable identity. Capture/playback are accepted only where their emulator scenarios reach those end states; unsupported routes are listed rather than presented as passing.

## Declared corrections and caller evidence

The following corrections retain the approved behavior already on main. Evidence is X-M1 1.50 image bytes and the image-4, image-5 and image-6 census in the [heap survey](stock-heap-survey.md#caller-census-and-metadata-dependencies). The pinned x70view revision `55436fc85c0560dd252b0410af6f2427fe198cd3` has no C definition covering `0xbf3914`; the image-5 instructions supply that caller evidence.

| Correction | Firmware-bound trigger and original behavior | Intended behavior | Remaining retained boundary and caller evidence | Dependence signal |
| --- | --- | --- | --- | --- |
| Checked calloc product | `0xbf1c44` multiplies two words modulo 2^32; overflowing products can allocate a smaller object | Return null and context errno 12 before locking when the mathematical product exceeds `0xffffffff` | Image-5 external core callers `0xbf395c` (4,33) and `0xbf398c` (1, computed word size) cannot overflow. Image-6 wrapper caller `0x1404e9c` passes (1,20), stores the result and tests only null. The third core call `0xbf1c34` forwards the public wrapper | An indirect caller depends on a nonnull wrapped allocation |
| Oversized resize rejection | Nonnull resize at `0x89da64` for `[0x7ffffff5,0xffffffff]` returns null/errno 12 but leaks one recursive depth | Reject before locking, preserving the old allocation and balanced locking | Image-5 `0xbf6004` and graphics `0x8917a4` store/test the result, not lock state. The 15 image-6 resize sites listed in the survey likewise store the result and branch on null, sometimes freeing the old allocation. Exact-C `hostapd_register_probereq_cb` at `0x1388af4` tests only null. No traced site compares old/new pointers or reads lock state | A consumer observes recursive owner/depth or relies on the leaked acquisition |
| Zero-size resize | Stock `realloc(p,0)` keeps a minimum live allocation at p and may leave an unsplittable tail | Return p with a live minimum allocation: rlsf 0.2.3 shrinks in place | The two image-4/image-5 resize clients and all 15 image-6 sites consume the returned pointer and test only null. Graphics callback `0x890ed8` forwards it. Zero malloc retains the stock 12-byte usable minimum | A client reads beyond the contracted extent; no zero-size pointer-movement difference is produced by this backend |
| Fixed-pool trim | `0x89e464` may release page-rounded top backing and return 1 | Acquire/release the lock and return 0 without releasing backing | The sole direct caller is stock free at `0x89e770`, inside the retired allocator. It ignores the result. There is no direct retained trim caller in the census. The backing helper's ABI stays present but the owned heap never calls it | An indirect trim caller depends on return 1 or on subsequent reuse of released backing |
| Capacity rejection before lock | Requests above arena capacity through `0x7ffffff4` take stock's lock at `0x89ce24` before failing; they can block or spin with zero task identity | Return null/errno 12 without locking | The 17 concrete resize clients test pointer/null, not lock ownership. Graphics/service allocation adapters forward allocation failure; service adapters map it to their existing errors. The natural route reaches no failed allocation. No traced caller requires a failed request to synchronize; other malloc callers' dependence remains unclosed | A caller relies on failed allocation blocking, lock recursion, or invalid-context spinning |
| Invalid pointer header refusal | Stock free `0x89e560` and realloc `0x89da64` use caller-supplied pointers without validating an owned prefix | Spin if a nonnull pointer fails the owned 16-byte header validation | The traced resize sites use allocation-origin pointers; failure cleanup frees the old allocation. Free-wrapper sites are counted, not all pointer flows traced, especially image-6 Wi-Fi clients. No dependence on an invalid pointer was established | A retained caller frees/resizes a foreign, aliased, stale or corrupted pointer and depends on stock continuing |

These are finite caller findings, not proof that no address-built caller exists. A newly found dependence is fixed at its client or isolated in a named compatibility shim under the specification's correction policy. Allocation-address equality is not an allocator promise.

## Implementation

The existing Rust allocator and stock veneers are reused unchanged. This slice strengthens their acceptance consumer: logical calls must agree in global order, calloc count and element size are compared separately, a null allocation must report errno 12, and final rendered frames must agree. Allocation addresses remain normalized by lifetime identity. The new tests detect reordered calls from different tasks, exchanged calloc arguments with equal products, missing calls, changed success and changed errno.

The role-specific observer commands let each emulator slot cover only guest execution. Artifact staging writes sibling-image paths relative to the contract directory, so `run-output keep` can move a complete staging directory without invalidating it. The executable negative control changes owned binary code before normal staging; it does not mutate a journal or stub stock execution.

## Current qualification

The [generated heap evidence](../replacement/README.md#general-heap) is written from the current staging contract, admitted comparison, compact lifetime summaries and executed-mutant comparison. It continues E-GENERAL-HEAP-CUTOVER-NATURAL-PAIR-150 rather than adding a second current record for the same topic. Rebuilding reproduces the existing firmware artifact; the new evidence qualifies the strengthened observer and comparator, not a new allocator algorithm.

The positive pair tests cold boot, language/start-menu navigation and the shooting-menu endpoint. Its 4,490 malloc and 4,039 free calls agree in global order in both roles; realloc/calloc/trim each have zero cases, reported explicitly in `qualification.json` in stock/owned order. Neither role reports a guest/watch failure, and the rendered endpoint frames agree. The wrong binary stores errno 12 through the context on malloc, including successful allocations. It passes artifact/staging admission and every role-local check, executes 2,552,771 allocator fetches and reaches both menu endpoints. The comparator rejects it with `stock/owned interface calls differ at ordered call 0`. This is an executed traffic control, not the older malloc-to-trim variant rejected by the per-role errno invariant or a mutated journal.

## Stock-ABI qualification

The [generated record](../replacement/README.md#general-heap) is E-GENERAL-HEAP-ABI-QUALIFICATION-150.

`heap-observe --abi-stock|--abi-owned` boots each role's DAT through the natural no-card route, presses Up on the final menu and halts at the next heap core entry. In both roles that is task 33 entering free `0x89e560` on core 0 with the heap lock free, so the heap holds the route's natural live state. The harness then calls the retained entries (malloc `0x89cda4`, free `0x89cdc4`, realloc `0x89da3c`, calloc `0xbf1c1c`, trim `0x89e464`) one at a time on a separate stack with interrupt delivery off, under the real retained lock, current-task and semaphore code. Every call must return to the harness, preserve r4-r11 and SP, run no core-1 instruction and fetch only bound spans whose bytes equal the role's image; one loader word differs from the decoded loader at the seed in both roles, so that span is a fetch bound only. The caller sets errno to a marker before each call and fills each allocation with a slot pattern, as a caller would.

The 99-step scenario (`emulation/fujimod-xm1/src/heap_abi_scenario.rs`) covers zero malloc, shrink, grow, same-chunk and zero-size resize, realloc(NULL), failed resize preserving the old bytes, calloc zeroing including reused dirty memory, zero calloc, free(NULL), capacity rejection, exhaustion in 128 KiB blocks, failed resize and calloc at exhaustion, a fragmented request that must fail, coalescence of the released blocks, a second exhaustion that must reach the same count, a large calloc over reused memory, trim and the declared corrections. Per call it records the result class, alignment, movement, errno, owner/depth change and payload checks; allocation addresses are not compared. `--abi-check` requires the stock and owned journals to be equal step by step except at the declared corrections, and each role to keep its own invariants (aligned allocations, errno 12 exactly on failure, balanced lock, intact payloads, equal exhaustion counts).

Results: the owned ABI matches stock at 94 steps and differs only as declared at 5. Executed stock bytes show the #147 leak: each oversized resize returns null/errno 12 with one more recursive depth, while owned stays balanced. Stock calloc allocates the wrapped products of 0x10000 x 0x10001 and 0x40000000 x 4; owned returns null/errno 12. Stock trim returned 0 here, so backing release was not exercised. Both roles reach 10 exhaustion successes, refuse the fragmented request, coalesce the released blocks into a 1.25 MiB allocation and reach 10 again. The Rust `GlobalAlloc` front end, run by `heap-bridge-replay --abi` after replaying the admitted tape into the production heap, matches stock at 75 steps; 19 steps have no front-end operation (16 null frees, 3 zero-size requests) and the calloc overflow is the caller's `Layout` failure. Over-alignment has no stock counterpart; the host suite covers alignments up to 16 KiB.

`--abi-controls` corrupts the owned and front-end journals one property at a time (failed or errno-setting success, leaked free depth, lost resize prefix, lost failed-resize bytes, dirty calloc, moved shrink, trim returning 1, wrapped calloc, oversized-resize leak, missing and reordered steps); all 12 owned and 9 applicable front-end controls are refused on their target property. The executed traffic mutant, staged as the owned role, completes its run and is refused at the first step for writing errno 12 on a successful malloc.

## Stack headroom

`J7HeapCanonicalTests` walks stock bytes with `StockStack` on the J6 composition and checks, entry by entry, that each owned entry's demand including callees is at most its stock counterpart's (stock dominance). Demands in bytes, including the retained lock/wait chain:

| Entry | Stock | Owned |
| --- | --- | --- |
| malloc `0x89cda4` | 504 | 464 |
| `_malloc_r` `0x89cde4` | 496 | 464 |
| free `0x89cdc4` | 464 | 432 |
| `_free_r` `0x89e560` | 456 | 432 |
| realloc `0x89da3c` | 552 | 464 |
| `_realloc_r` `0x89da64` | 544 | 464 |
| trim `0x89e464` | 432 | 432 |
| calloc `0xbf1c1c` | 512 | 464 |
| `_calloc_r` `0xbf1c44` | 504 | 464 |

The deepest owned path is the retained lock `0x72b164` (408 bytes) called from the core entry's own frame: 56 bytes for malloc, realloc and calloc (six saved registers and a 32-byte request area), 24 for free and trim. The bridge, `with_heap` and lock acquisition are inlined into the core entries; the heap operations they call (`allocate_stock`, `calloc_stock`, `resize_stock`, `free`) are not inlined, so their frames are never below the lock call.

The walker now follows the constructs the earlier task walks stopped at. `0x4a960` is a kernel lock release whose lock address the routine spilled to `[r11, #-0x28]`; the walker tracks words a frame stores and reloads until a frame address is copied or a callee may write that far above its entry SP. `0x4360c` is the kernel abort's `svc #0x1010`; each loader's SVC handler stores 64 bytes on the task stack and calls its exception reporter `0x40cac`/`0xd0cac`, both checked from loader bytes. The reporter's hook (`0x4004f114`/`0x400df0d4`) holds only the image entry's mode-2 return `0x69cc6c` (S7: each hook's only store is its setter, called only after the loader's mode-2 call). `0x72b694` and `0x892c38` call words at fixed addresses; a word every scanned store writes with a constant, or with the argument a setter's every caller in stock or owned code passes, holds its image value, 0 inside the cold-boot clear `[0x89ec00,0xbf1a00)`, or those constants (S6), and a possibly zero word is called only after a zero test of its register. `0x829a0c` holds `0x72b210` or `0x72b318`, `0x890f14` holds `0x890ee8` and the diagnostic hook `0x89f794` holds 0 or `0xed4a8c`; with the owned image-5 initializer composed (J9), its own call of setter `0x69cf84` passes `0xed4a8c`. A path ends after a call when no path of the callee returns, as after a call of `0x43604`, whose `svc #0x1010` is followed by `b .`.

No published heap-calling task has an established whole-task bound. Under S1-S7, tasks 1, 2, 3, 4, 7, 35, 43, 55, 56, 62, 64, 73 and 80 (stacks 0xc00-0x1400) reach the kernel abort `0x4d978`, whose fatal report recurses: SVC `0x43604`, reporter `0x40cac`, diagnostic callback `0x69cc6c`, hook `0xed4a8c`, `0xed468c`, `0x6ab794`, `0x6fb7c8` and `0x4d978` again; a report nested in a report has no static bound. Task 33 (0x1000) stops at `0x7250cc`, a call through the message-trace word `0x4089ea94` that a setter fills from its argument. Task 36 (0xc00) stops at `0xd80e68`, a USB state-table record field indexed by RAM state. Task 72 (0xc00) stops at `0x892c00`, a callback array in an object. Root and plain producer targets that reached the abort refuse at the same recursion. The only bounded task demands remain root 104 (856 of 1024 bytes) and the task-60 producers (672/520 of 3040), under S1-S5.

S8 takes the fatal report as terminal: a nonzero SVC starts a report after which the task does not resume normal operation, so a bound under S8 covers the paths before the report. S9 adds declared register-branch target sets (`Replacement/StartupArtifact/CallbackTargets.hs`, data in `CallbackDeclarations.hs`, checked by `callback-targets-tests`): each declaration names its images by SHA-256 and the code allowed to write the words it reads, and a site closes only when every scanned store lies there with a known value and no table word it reads is written. On both the stock (J6) and owned (J7) compositions the check closes the message-trace site `0x7250cc` ({`0x723330`, `0x726478`}), task 55's `0x106cfcc` and task 62's `0x106444c`/`0x1064464`. It leaves task 35's `0xed1494` and the seven state-chart dispatchers open, on a store of unknown value into an index or state word or a dispatcher caller passing a non-constant machine.

Per-task whole bounds are not the acceptance criterion: several stock tasks have none from bytes. Each walk's stop under S1-S9, with what lies behind it (byte research, including target sets the walker does not consume):

| Task | Entry | Stack | Stop | Behind it |
| --- | --- | --- | --- | --- |
| 1 | `0xc22f60` | 0xc00 | `0x6f16e0`, argument 6 of `0x6f1440` | {`0x6ae500`, `0x73b924`} through two struct-field hops; then recursion `0x6334dc`↔`0x633594`, bounded only by consecutive failures of `[0x829a0c]` |
| 2, 3, 4, 7, 43, 64 | `0xec2668`, `0xe00a80`, `0x1215480`, `0xecc058`, `0xf01bc8`, `0x1054cf4` | 0x1000, 0x1000, 0x1000, 0xc00, 0x1000, 0xc00 | state-chart dispatchers `0xec09b4`, `0xdf7594`, `0x11e8ca0`, `0xec9bf8`, `0xefcb58`, `0x10548ec` | 129, 1564, 1414, 73, 787 and 25 actions; actions that post events re-enter the dispatcher, a recursion with no static bound (2, 3, 4, 64) |
| 33 | `0x1109130` | 0x1000 | `0x89becc`, a divmod wrapper's `push {sp, lr}` | then a computed `ldr pc` at `0x118d0b8` |
| 35 | `0xed2ce0` | 0xc00 | `0xed1494` | 18 handlers; then 16 completion callbacks passed as arguments (`0x70f744`-`0x71c448`) |
| 36 | `0xd81218` | 0xc00 | `0xd80e68`, USB chart | 119 actions, then about 100 further open branches |
| 55, 56, 73 | `0x106ceec`, `0x1071ca4`, `0x6d7540` | 0xc00, 0x1400, 0x1000 | `0x106d000`, `0x10724b8`, `0x6d6fec` | callbacks taken from message payloads; senders with computed channels are not excluded |
| 62 | `0x1064224` | 0x1000 | `0x106444c`, closed by its declaration | the walker refuses there because the set holds the list terminator's 0 and a compare of the event after the zero test forgets it; no further open branch |
| 72 | `0x892110` | 0xc00 | `0x892c00` | callback arrays in client-built graphics commands |
| 80 | `0x69f0e0` | 0xc00 | `0x89bf08`, the divmod wrapper | `0x6a1c44` calls word `0x95b074`, 0 until a setter runs, without a zero test |

The criterion is stock dominance: each owned allocator entry that replaces a stock entry has a worst-case depth, callees included, no greater than the stock entry's, so a stack-safe stock execution stays stack-safe. All nine entries meet it; trim equals stock.

The one-level fatal path is unmeasured: with fatal reports excluded the abort `0x4d978` is bounded, but the reporter `0x40cac` stops at `0x6f16e0`.

## Final-pass queue

- Reach calloc, realloc and trim from full-system scenarios where the current menu route does not enter them. The stock-ABI qualification calls them at the retained entries from a natural seed; it does not show which retained callers reach them.
- Exercise cross-task/core contention, interrupt-context refusal and stock trim's backing release; the stock-ABI qualification covers uncontended failure, exhaustion, coalescence and reuse on one task.
- Extend capture/development/save/playback coverage at each supported emulator frontier; physical timing and unmodeled hardware require the camera.
- Close unknown computed references, metadata readers beyond `payload-4`, external arena writers and address-sensitive clients before a firmware-wide retirement claim.
- Exercise image-6 Wi-Fi clients, including hostapd and netstack, their 15 resize calls, calloc(1,20), malloc/free lifetimes and alias-pointer handling.
- Qualify the separate `general-heap-recipe-image` artifact with the recipe ceiling, not only the standard arena image.
- Qualify `general-heap-fixed-image` at the `0x00a9fa00` ceiling with card-backed capture, saved-image playback and Wi-Fi heap demand. The current fixed-placement scenarios cover only cardless inputs.
- Check DMA-loaded instruction visibility and later MPU changes on the camera before promoting the fixed-range image.
- Close interior jumps into retired carriers after sealing, when guards no longer bind a fetch to an entry or pending invocation.
- Close task-stack headroom. Under S1-S7 thirteen published tasks reach the recursive fatal report through the kernel abort `0x4d978`, and tasks 33, 36 and 72 stop at an argument-filled callback word, a RAM-indexed state table and an object callback array. Under S8 all sixteen stop at the register branches the [stack-headroom table](#stack-headroom) names, and the reporter `0x40cac` stops at `0x6f16e0`; bounds need closed target sets for those branches. The census also lacks image-6 Wi-Fi tasks and any heap caller outside the published tables.

The current no-card scenario does not claim a captured image or playback rendering. The [capability inventory](replacement-inventory-baseline.md#2-coarse-capability-outline) separately records blocked emulator playback/RAW conversion and incomplete sensor-backed capture. The final-pass scenarios must use their supported device routes as those frontiers close, not bypass heap calls or synthesize successful captures.
