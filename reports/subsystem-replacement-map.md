# Subsystem unit plan for X-M1 1.50

## Scope, evidence and interpretation

Citations below abbreviate [the replacement inventory](replacement-inventory-baseline.md) as **Inventory**, [the heap survey](stock-heap-survey.md) as **Heap survey**, [the UI ownership report](recipe-bank-ui-ownership.md) as **UI report**, and [the file-service report](named-recipe-reach.md) as **File report**. `E-*` identifiers refer to [the evidence ledger](../docs/evidence.jsonl). Addresses and intervals are guest addresses, with exclusive interval ends, unless a cited source explicitly establishes physical backing. Evidence classifications remain those of the cited records. No X-M1 contract is inferred from sibling-model headers or names. Proposed boundaries, ordering and estimates not directly measured are marked **[INFERENCE]**.

The accepted design already permits function boundaries to change inside an owned subsystem. Compatibility is at retained interfaces: results, errors, externally visible state, artifacts, effects and required ordering. It does not require matching private frames, allocation addresses or instruction counts. Startup changes require cold-reset scenarios, and final retirement requires caller/callback/computed-reference closure plus continuous execution/data guards. [Source: `spec/11-incremental-firmware-replacement.md:11-19,33-43,49-57,73-83,95-99`; #117/#127/#155.]

**[INFERENCE] Planning conclusion:** stop choosing a new acceptance unit for each leaf. Choose a subsystem artifact, maintain one interface contract and one scenario matrix, and use the existing leaf proofs as internal regression evidence. Larger units do not erase retained private ABI, shared-object, device-law or initialization dependencies. Raw-byte comparisons belong only to structures still read by retained consumers; when every consumer moves inside the unit, compare logical objects and effects instead.

## A. Kernel/RTOS platform services

### 1. Interface, entry count, recovered ABI and errors

The installer #150 publishes root0 at `0x96bd20` and root1 at `0x96bd24`, from loader header words `0x40010` and `0xd0010`. Canonical roots are `0x4004dae0` and `0x400ddaa0`. It does not construct a kernel or populate service entries. The A/B initializers do that independently, and root1 publication precedes its late initialization. [Source: Inventory §8.12, especially lines 844-850,862-864,877-891,953-959; E-KERNEL-PHASE0-INSTALLATION-SOURCE-150; E-KERNEL-PHASE0-RESOURCE-ORDER-SOURCE-150; #150.]

There are **93 callable service slots per root** at offsets `0..0x170`, a tick pointer at `+0x174`, 120 eleven-word descriptors beginning `+0x178`, four callable extensions at `+0x1618/+0x161c/+0x1620/+0x1624`, and a stack-top word at `+0x1628`. Thus there are **97 callable slots per root, 194 slot instances across two domains**, not 1,419 services. Each complete root has 1,419 words / `0x162c` bytes. All target addresses are enumerated by the publisher sources; complete semantics, argument contracts and error domains for all 97 slots are not recovered by the inspected contracts. [Source: `replacement/primary_initializer/src.rs:18-50,71-100`; `replacement/interface_initializer/src.rs:19-49,57-85`; `replacement/README.md:129`; Inventory §8.12.]

Most image4 wrappers select current core by calling root0 `+0x1618`, returning 0 for CP14 discriminator `0x4003`, otherwise 1. Task creation instead selects by descriptor class: class1 root0, class2 root1; activation recomputes the current-core root. Code location, task class and executing core are different identities. Image4 has a class map for IDs1..104; the lower kernels have 120 task records. Neither count is the public service count. [Source: Inventory §8.12 lines 879-889; §8.13 lines 1074-1084,1106-1126.]

| Recovered interface family | Recovered entries and ABI | Results and limits |
| --- | --- | --- |
| Task registration/create/activate | Wrapper `0x725dd8(D)` consumes seven descriptor words, only six necessarily initialized. Root `+0x80`: auxiliary registration A/B `0x4d39c/0xdd360`; root `+0`: create `0x4d094/0xdd054`; current-root `+0xc`: activate `0x44f2c/0xd4eec`. Encoded service ID is `(class<<16)|id`; create takes six words `{2,arg,entry,priority,size,base}`. | Auxiliary/create duplicate `-41`; activation context/IRQ refusal `-25`, invalid selection `-18`, missing object `-42`, already pending `-43`. Four natural startup explicit activations return `-25`; separate auto-ready produces runnable tasks. [Inventory §8.13 lines 1057-1124,1144-1159; E-STARTUP-FIRST-RUNNABLE-SOURCE-150; E-STARTUP-TASK-STOCK-QUALIFIED-150.] |
| Deletion/termination/release | Cleanup `0x72612c(0)` is reached by root104. Storage release `0x726098(index)` takes a zero-based index, not encoded task ID; deletion calls it at `0x72623c`. | Cleanup supplies `-63` if its lower operations return. Release returns 0 but clears only reserved state; it is not task deletion or runnable unlinking. Complete deletion ABI/error/lifetime closure remains open. [Inventory §8.13 lines 1100-1102,1167; §8.17 lines 1613-1617; #151.] |
| Critical sections and locks | Pair `0x7235d8/0x7236b0`; selected-root IRQ save/restore `+0x164/+0x168`; identity/retry/release/observe `+0x1618/+0x161c/+0x1620/+0x1624`. The pair carries a private prior-mask value in r10. | Try-lock `0x427d4` / unlock `0x427fc` can return `-1`; some storage/release wrappers ignore these returns. The observed release control clears a peer lock after failed acquisition. That is stock behavior, not a mutual-exclusion guarantee. [Inventory §8.8; §8.13 lines 1100-1102,1214-1228; #142/#151.] |
| Semaphores/resources and flags | Resource create `0x7247ac(id,1)` uses `+0x64` with `{0,1,1}`; take `+0x7c`; give/contextual give `+0x6c/+0x70`. Flag set uses `+0x88/+0x8c`; standby wait `0x724480(1,&out,-1)` uses `+0x9c`, A/B `0x48b30/0xd8af0`. | Flag waits write the whole flag word, not just requested bits; mode0 tests all bits, attribute4 clears the whole word. Wrappers special-case statuses0 and `-50`; other failures can log/callback and return. Full counts, ordering, context and all error-number mappings remain incomplete. [Inventory §8.15 lines 1398-1408; §8.17 lines 1573-1593,1602.] |
| Message/mailbox storage and transfer | Constructor `0x724cdc(id,{count,size})` uses `+0xa0`, `+0xe4` and failure cleanup `+0xa4`. Send `0x72503c` consumes `{u16 kind,u8 field2,u8 field3,u32 length,u32 payload}`; acquire `+0x170`; context selector `+0x148`; submit `+0xac/+0xb0`. Receive `0x725318(queue,dst,timeout)` uses `+0xc8`, then release `+0xf8`. | Success0; finite full/empty queue `-50`; masked failed-send release can refuse before freeing. Callback-modified send inputs are loaded after callback; receive copies then releases and ignores release status. Constructor failure does not undo every prior publication. [Inventory §8.12 lines 961-969; `emulation/README.md:498-514`; E-XM1-QUEUE-CALLER-BOUNDARY-150.] |
| Task/context query | Root `+0x16c`, A `0x4118c -> 0x4c620`, B `0xd118c -> 0xdc5e0`; image4 adapter `0x723cb8`. | Lower query refuses masked IRQ/nonzero context with `-25`; wrapper yields identity0 on failure. Node validity is a prerequisite, not validated by the query. [Inventory §8.15 lines 1398-1408; Heap survey lines 95-99.] |
| Scheduler, timers, IRQ and exception entry | Initial scheduling is a direct nonreturning call `0x43688/0xd3688`, not an ordinary root service return. Auto-ready is `0x43f9c/0xd3f58`; first trampolines `0x43728/0xd3728`; resume uses `SPSR/LDM^`. Static timer activation is `0x4bf3c/0xdbefc`. Vectors and diagnostic callbacks provide additional entry paths. | Initial SP/PC transfer is recovered, but general preemption/resume, timer semantics, interrupt nesting/acknowledgment and complete exception entry/restore closure are not qualified. SVC0 target production and diagnostic restore remain #145 obligations. [Inventory §8.13 lines 1124-1130; §8.16 lines 1455-1472; #127 runtime ranks4-7; #145.] |

**[INFERENCE] One service layer or several?** Use one **RTOS-platform ownership artifact** for the two domain roots, task/scheduler objects, synchronization, messages, timer/IRQ/context core and their image4 adapters. Keep several public subinterfaces within it: root-call ABI, descriptor/object compatibility view, direct startup/scheduler entry, vector/exception entry and timer/device backend. Replacing only the 97 root targets is not the whole layer: retained code also directly calls scheduler/CPU helpers and reads root-published object pointers. Idle/standby policy can remain a startup subcomponent; its WFI/IRQ backend belongs to the platform. [Grounding: root descriptors in `replacement/primary_initializer/src.rs:71-83`; Inventory §§8.12,8.13,8.16,8.17.]

### 2. Extent, owned objects and initialization

Installed A service entry starts span `0x41108..0x4d8fc`, B `0xd1108..0xdd8c0`, roughly 50KiB of address span each; this is an entry-address envelope, **not a measured sum of kernel code**. It includes gaps and does not bound every last body, helper, vector or image4 wrapper. Whole-kernel code bytes remain unknown. Known finite image4 pieces are critical pair `[0x7235d8,0x723790)` /440 original bytes; reservation transition `[0x725cc4,0x725da8)` /228 bytes; submit/release `[0x725dd8,0x72612c)` /852 bytes; installer 336 bytes; dispatcher376 emitted bytes in a396-byte original carrier. [Source: publisher service arrays cited above; Inventory §§8.8-8.10,8.12,8.13; #146.]

Each kernel owns 120 creation records C (40-byte stride), task nodes T (56), ID-map words, auxiliary records (12), auxiliary nodes (16), scheduler domains (132), runnable list heads/bitmaps and current/selected/dispatch words. A C/T bases are `0x4005beac/0x4005ff0c`; B `0x400ebe6c/0x400efecc`. Root descriptors expose C fields and T saved-SP/resume-PC addresses. First-entry uses C argument/entry, T stack-top and a trampoline; resumed exceptions use a different frame path. [Source: Inventory §8.13 lines 1104-1130; publisher descriptor routines.]

Image4 also owns 120 task reservation triples at `0x40a0fc44`, 240 separate-storage triples at `0x409c3904`, separate wrapper pairs at `0x40a5b204`, two `0x4b000` pools, cursors `0x4089ea98/0x4089ea90`, shared lock `0x409c280c`, message handle/size arrays, callbacks and allocation cursors. #150 initializes1,457 word stores /5,828 distinct bytes, but does not reset the two initialized-data storage cursors, create tasks or install vectors. Kernel zeroing, domain/trampoline initialization, tick/timer registration and later root callbacks remain separate duties to absorb. [Source: Inventory §8.12 lines 858-889,961-969; §8.13 lines 1126,1247-1276; §8.16 lines 1447-1466.]

### 3. Pieces to fold in

#140/#144 provide both table/descriptor publishers; #142 provides the bounded critical pair; #143 the reservation transition; #150 publication/bookkeeping; #151 the qualified 852-byte submit/release wrapper, not lower kernel creation; #152 shared idle; #146 finite startup dispatch. #155 separately qualifies dispatcher+installer and submit+critical+reservation joins. None of these joins owns constructors, activation, auto-ready or scheduling. [Source: Inventory §§8.8-8.13,8.17; E-STARTUP-CONSUMER-NATURAL-COMPARISON-150; E-STARTUP-TASK-COMPOSITION-QUALIFIED-150; #117/#150/#151/#155.]

### 4. Dependencies and unknowns

The platform depends on actual dual-core identity, mapped code/data and aliases, private TCM and banked state, exclusive monitors/barriers, IRQ/FIQ masking, cache maintenance, MMIO notification and timer laws. The timer-derived diagnostic elapsed word already differs when startup instruction timing changes; accepted comparison proves reload-minus-count from actual `0xfff91004/0xfff91000` transactions. It does not waive arbitrary clock-derived state. Standby IRQ bank semantics at `0xfffb4080..0xfffb40ac` and the concrete task93 mask1 flag producer remain missing. Physical cache coherence, complete callback/frame writers, general scheduling interleavings, all deletion/reuse and synchronization semantics remain open. [Source: Inventory §§8.8,8.13,8.16 lines 1526-1542,8.17 lines 1571-1593; E-LOADER-ORCHESTRATION-NATURAL-QUALIFIED-150; #127/#145/#151/#152.]

### 5. Proposed qualification boundary

**[INFERENCE]** Qualify at callers' root/direct/vector interfaces: operation, typed domain/task/object IDs, arguments, returned status and output bytes; object creation/lifetime, ready/blocked transitions, callback delivery, message ownership/order, saved task/IRQ state and required MMIO effects. Preserve raw C/T/root descriptor layout only where a retained caller reads it. Otherwise compare logical runnable sets and semantic object state, normalizing pointers by allocation/object identity and offset, never by guessing which scalars are pointers. Compare required per-task/per-object ordering and cross-core happens-before edges, not the old global instruction interleaving.

**[INFERENCE]** Reuse natural cold-reset creation of 104/102/103/93, explicit masked `-25`, later auto-ready and first B93/A104 entries; add full create/activate/delete/reuse, preemption, blocked-wakeup, queue exhaustion, semaphore/flag timeout, nested critical and IRQ/timer/exception scenarios. Menu trajectories exercise UI/input/render messaging; capture/develop/save trajectories exercise broader completion and resource lifetimes, but cannot presently qualify through #139/#124's unresolved capture/device frontier. Continuous actual-content fetch guards must cover all replaced targets, direct/vector entries, copies and aliases. The existing menu endpoint alone is not scheduler equivalence. [Grounding: Inventory §§8.12-8.13,8.16-8.18; #127/#139/#151/#155; spec lines 73-99.]

### 6. Size, order and blockers

**[INFERENCE]** This has the largest runtime-retirement leverage: several roughly 50 KiB service-entry address spans plus image4 wrappers and shared objects, with no defensible exact code total yet. Develop it as a single platform project, with internally testable task/scheduler/synchronization/message families rather than a public ticket per function. Start after recovering the complete 97-slot semantic manifest and direct/vector surface, and grounding IRQ/timer/context laws. A task/message-only initial cut is possible, but must retain the IRQ/timer backend explicitly and must not be called whole-kernel retirement. The immediate blockers are interface semantic coverage, raw-object consumers, exception/context closure, cross-core lock/wakeup laws and resource reclamation. [Grounding: #127 runtime ranks3-7; Inventory §§8.12-8.17.]

[RTOS platform options](rtos-platform-options.md) tests the µITRON 4.0 hypothesis against the 97-slot manifest, surveys reusable kernels and Rust runtimes, and records the selected base: a mainstream Rust runtime (embassy) under a shrinking stock-ABI compatibility shim, with the board glue and thread layer still to write.

## B. General heap and distinct allocator/resource families

### 1. Interface, count, ABI and errors

The general heap has **nine recovered allocator operation entries**: four public wrappers, four reentrant cores and trim. It additionally has one backing helper, two lock functions, three graphics callbacks and three named client/service adapters; these are not extra independent heap backends. Direct calls and callback-table publication both reach it; it is not solely behind the #150 root. All recovered allocator entries are A32 ordinary integer/pointer AAPCS with 32-bit arguments/results. [Source: Heap survey lines 24-45,69-99; E-STOCK-HEAP-SURVEY-150; Inventory §8.14.]

| Operation | Public / core addresses | ABI and error contract |
| --- | --- | --- |
| allocate | `0x89cda4 / 0x89cde4` | Public `(bytes)`, core `(context,bytes)`; payload/null in r0;8-byte alignment; zero takes a minimum 16-byte-chunk path; normalization/backing failure writes context errno 12. [Heap survey lines 26-31.] |
| free | `0x89cdc4 / 0x89e560` | Public `(payload)`, core `(context,payload)`; null accepted, no meaningful result; foreign nonnull pointers not validated. [Heap survey lines 32-33.] |
| resize | `0x89da3c / 0x89da64` | Public `(old,bytes)`, core `(context,old,bytes)`; null old allocates; zero requests minimum chunk; failure preserves old allocation. [Heap survey lines 34-35.] |
| calloc | `0xbf1c1c / 0xbf1c44` | Public `(count,element_bytes)`, core `(context,count,element_bytes)`; stock multiplication wraps 32-bit; success clears `(word(payload-4)&~3)-4` bytes. [Heap survey lines 36-37,93.] |
| trim/backing | `0x89e464 / 0x72b110` | Trim `(context,pad)->1/0`; backing `(context,signed_increment)->old_break/0xffffffff`, failure errno 12. [Heap survey lines 38-39.] |
| locking/adapters | `0x72b164 / 0x72b1cc`; graphics `0x890ef8/ee8/ed8` | Recursive task owner; semaphore 0x3f; invalid identity or semaphore failure hard-loops. Service adapters `0x72a7ac/0x72a7fc` additionally return `-9/-42/-30` and write `0x9121b8`; these are not newlib errno values. [Heap survey lines 40-45,95-99.] |

Aligned allocation, standalone usable-size, mallinfo/mallopt/statistics entries have not been positively identified. Summing the published direct-call census gives 74 sites across the nine operations, including wrapper/internal calls, not 74 independent users and not indirect closure. The calloc core is a confirmed cross-image metadata reader and must migrate in the same cutover. [Source: Heap survey lines 43,69-93; #148.]

### 2. Extent, structures and initialization

Stock allocator entry-body/literal extents total 4,336 bytes, plus 352 bytes of actual move/clear helpers,84 backing-helper bytes and 172 shared lock/unlock bytes. External allocator state is 1,092 bytes; break cursor 4 and recursive owner/depth 8 are separate. The original envelope is `[0xa71a00,0xbf1a00)` /1.5 MiB, with 32-byte guard; the recipe carve lowers the ceiling to `0xbe1a00`, losing 64 KiB. State includes bins at `0x89d4f8`, boundary tags at payload-4, top/remainder/free links, break and lock owner/depth. [Source: Heap survey lines 22,55-67,260-270; E-STOCK-HEAP-SECONDARY-POOL-150; E-STOCK-HEAP-REPLAY-HELPERS-150.]

Direct `rlsf` 0.2.3 is selected, not installed. Its measured comparison image is 4,224 text bytes and 5,392 whole-image bytes, including adapters/diagnostics/BSS but **excluding a production stock compatibility bridge**. Its external comparison state is 1,148 bytes. Provision pool/bin configuration before timed operations, establish owned layout/alignment provenance for pointer-only stock free/resize, and establish a task-aware cross-core external lock. Rust must use `GlobalAlloc`/`Allocator` directly over the same backend, not call through stock compatibility. [Source: Heap survey lines 107-118,234-241,260-278; Inventory §8.14 lines 1292-1296; #148.]

Other #127 runtime-rank9 families must remain separate:

| Family | Recovered interface and owned state | What is not established |
| --- | --- | --- |
| Supposed “image4 allocator” `0x7235d8` | It is actually the #142 critical-section/IRQ-state adapter, not another heap. | Do not invent a second allocator ticket for this address. [Heap survey line63; #148.] |
| Task and service-storage reservations | `0x725c6c/0x726098` and `0x723470/0x7233dc`; two 0x4b000 pools, indexed 12-byte records, cursors and shared lock. #143 owns only 228 bytes of the task transition; errors include `-33` size/capacity and `-18` reserved. | Full pool reclamation, indirect callers and lifetime closure. Best folded into RTOS objects, not replaced with arbitrary malloc/free semantics. [Inventory §§8.9,8.12-8.13; `replacement/README.md:139-163`.] |
| File-handle allocator | `0x71ef18`, release `0x71eff4`; 16 slots at `0x9101c0`; semaphore 0x3c; tag sequence `0x910200` wraps and skips allocated tags. | Complete ABI, exhaustion/cancel/late-callback safety and whole implementation bytes remain unqualified. Fold into file-service ownership. [Heap survey line65; File report §6.8 lines282,297.] |
| SQLite memsys3-style pool | Seven-method table `0x1446cac`: malloc `0x140ce3c`, free `0x14439cc`, resize `0x144392c`, size `0x140cde8`, roundup `0x140ce14`, init `0x140ce58`, shutdown `0x140cefc`. Configured base `0x1f9f8000`, size 0x300000, 393,214 eight-byte blocks; option 8 installs methods then initializes. | This is not newlib usable-size. Full per-method ABI/errors, workload/caller closure and replacement qualification are not supplied. [Heap survey line67; E-STOCK-HEAP-SECONDARY-POOL-150.] |

### 3. Pieces to fold in

#147 is a defect contract, not implementation: nonnull resize of `[0x7ffffff5,0xffffffff]` returns null/errno12 but leaks recursive depth; replacement must reject before locking and unlock every return. #148 supplies family/ABI/backing recovery, a complete finite no-card trace, direct rlsf selection and admitted ARM replay. No production heap implementation or stock bridge is qualified. #143/#142 belong to different reservation/synchronization families. [Source: Inventory §8.14; E-STOCK-HEAP-RESIZE-LOCK-150; E-STOCK-HEAP-NATURAL-TRACE-150; E-STOCK-HEAP-REPLAY-150; #147/#148.]

### 4. Dependencies and unknowns

Heap locking depends on kernel task identity/semaphore services and valid unmasked task context, not simply a nonzero task ID. Image5 calloc code is initially absent and installed later, requiring lifecycle-aware code admission. Indirect clients, hidden metadata readers and address-sensitive clients remain open. The admitted tape has 4,490 mallocs and 4,039 frees, no logical resize/calloc/over-aligned requests/failures or two-core contention; its finite no-header-read result is not global absence. Capture/development/storage workload and silicon deadlines remain unknown. [Source: Heap survey lines 93-99,165-201,207-215,276-278; #148.]

### 5. Proposed qualification boundary

**[INFERENCE]** Own the complete malloc/free/realloc/calloc/reentrant/trim/backing closure and callbacks in one heap artifact, with a stock-ABI front end and Rust-Layout front end. Compare allocation success/error, required alignment, payload preservation/zeroing, failure ownership, errno, lifetime and available-capacity guarantees; compare pointer identity only where a retained client's contract requires it. Deliberately do not preserve stock arithmetic defects or leaked locks. Prove checks before lock and balanced release independently of allocator placement. Exercise zero/max/overflow, failed realloc preserving old bytes, calloc multiplication overflow, exhaustion/coalescence/reuse, over-alignment and concurrent task/core schedules.

**[INFERENCE]** Reuse the admitted no-card cold-boot/menu round trip and 8,529-operation replay as one baseline, then add editor/render trajectories and later capture/develop/save when those device scenarios qualify. No pixel equality requires identical heap addresses. If in-place addresses change, every retained graphics callback and reentrant entry must migrate in the same packaged cutover. The original lock can be an explicitly retained interim backend, but that is allocator ownership, not full synchronization retirement. [Grounding: Heap survey lines 128-145,189-215,272-278; spec lines 79-99.]

### 6. Size, order and blockers

**[INFERENCE]** Best ready large-unit candidate: approximately 4.3 KiB recovered stock allocator code, with a measured 4.2 KiB rlsf comparison implementation and a still-unmeasured production bridge. Implement this before the full RTOS cutover, using one explicit lock boundary, then migrate that backend when the RTOS lands. Blocking facts are complete entry/client/header-reader closure, pointer-only layout provenance, checked calloc/resize contracts, representative failures and concurrent locking. Rank9 pools are not prerequisites for replacing this general heap and should not expand its scope silently. [Grounding: #127 runtime ranks8-9; Heap survey lines 260-278; #148.]

## C. One startup artifact, reset through application readiness

### 1. Interface, entry count, ABI and errors

This is a lifecycle boundary, not a single C ABI or the #150 root. Its external input is an admitted reset/ROM/update/run/input state; its output must be a booted application with executable publication, actors, settings/calibration/input and render readiness. Internally it has first/secondary-stage handoffs, two loader continuations, one phase entry, one installer, common task submission, three distinct initial task bodies for four task IDs, two later executable producers and one image5 initializer before the actual UI actor. A complete global startup entry count is not recovered because alternate/computed/restored/exception origins remain open. [Source: Inventory §§8.6-8.7,8.10-8.18; #141/#145/#149-#155.]

The phase entry `0x69cdf8(r0=phase)` is supplied by image4 header. Primary phases are 0,1,3,2; peer phases 4,2; cross-core observed order is not a mandatory total order. Phase2 returns callback `0x69cc6c`, others 0; task wrapper statuses are ignored. Phase0 marker rejection loops after root publication, not an error return. Initial scheduler entry is nonreturning saved-SP/PC transfer; first trampolines supply argument in r0 and cleanup LR, while exception resume is a separate mechanism. Root104 may return after cleanup returns `-63`. Image5 `0xd985a4` is a synchronous initializer, not an event loop; task1 `0xc22f60` is the main UI actor. [Source: Inventory §§8.10-8.13,8.16-8.18; #146/#149/#151/#152/#154.]

### 2. Extent, state and initialization

| Recovered part | Known extent or responsibility |
| --- | --- |
| Early owned stages | Current composed placements: record1 `[0x6000,0x64ac)` and record2 `[0x10400,0x10a6c)`; retained record helpers, vectors, resources and later services remain. [Source: `replacement/README.md:7-12,21-27`; Inventory §8.6.] |
| Interface publishers/loaders | A/B leaves each `[base+0x430,base+0x9ac)` /1,404-byte original carriers. Loader orchestration each340-byte original carrier,280-byte owned payload plus60-byte owned padding. [Inventory §8.3 retirement view,§8.16; #141/#144/#149.] |
| Phase/install/task glue | Dispatcher 376 emitted /396 original; installer 336; submit/release 852; critical carrier 3,296 with startup code, not 3,296 extra independent critical bytes; reservation 228. [Inventory §§8.8-8.13.] |
| Initial task bodies | Root104 `[0x69d160,0x69d2b4)`340; shared idle102/103 `[0x69cbf0,0x69cc00)`16; standby93 `[0x69cd58,0x69cdf8)`160. [Inventory §8.17 lines1550-1557.] |
| Later executable producers | Image5 producer `0x6a96c0` reads compressed flash into staging `[0x43e40000,0x44120000)`, decodes `[0xbf1a00,0x10e3860)`, separately clears BSS `[0x1460000,0x171be40)`. Plain producer `[0x6a9844,0x6a9938)`244 loads `[0x1100000,0x1460000)`; no recovered relocation/copy-after-decode requirement. [Inventory §8.15 lines1304-1314.] |
| Later initialization/application | Three plain extents, 15 hooks, 14 task submissions; image5 reverse 3-hook constructor list and 21 direct initializer calls; actual UI outer body `[0xc22f60,0xc2327c)`796 and pump `[0xbfbd20,0xbfbd40)`32. [Inventory §§8.15,8.18.] |

Startup owns vector/mapping/stack setup, workspace clearing, both publication roots, finite bookkeeping, task descriptors and initial pools/ready sets, load/staging/BSS lifetimes, extent-ready resources, constructor/callback publication and eventual actor initialization. Image5/plain code bytes and resources are megabytes, but they are **loaded payload, not the size of startup implementation**. Full transitive startup-code bytes are not measured. [Source: Inventory §§8.6,8.12-8.18; #153/#155.]

### 3. Pieces to fold in

Reuse #126/#135/#136/#137 early-stage composition; #140/#144 table publishers; #145 caller binding; #146 phases; #149 qualified finite loader carriers; #150 installer; #151 wrapper; #142/#143 joined critical/reservation; #152 qualified idle; #153 host-admitted 244-byte plain producer; #154 recovered application boundary. #155 currently has **two separate qualified partial joins**, not one end-to-end startup artifact. #153 natural owned loading and #154 owned-ready-loop acceptance remain open; root/standby have no qualified ownership. [Source: Inventory §§8.6-8.18; E-LOADER-ORCHESTRATION-NATURAL-QUALIFIED-150; E-INITIAL-IDLE-NATURAL-QUALIFIED-150; E-LATER-PLAIN-PRODUCER-HOST-150; E-APPLICATION-ENTRY-LOOP-SOURCE-150; #117/#149-#155.]

### 4. Dependencies and unknowns

The graph is a partial order. B93 can block and B103 enter before A102 creation. Image5 initializer returns and UI1 is submitted before plain loading completes. Eligible plain hooks can run before the whole raw read returns, and their tasks can consume code before the worker done flag. Done/progress/callback acceptance is not executable readback. Root initializes 97 resources and executor workers, then branch-dependent later tasks; its hardware/configuration branch cannot simply be renamed ordinary boot. [Source: Inventory §§8.13 lines1148-1155;8.15 lines1325-1396;8.17 lines1595-1619;8.18 lines1700-1708.]

Remaining blockers include #145 SVC0/genuine diagnostic restore and earlier-B resource origins; #151 retained kernel creation/activation/scheduler; #152 standby event producer and IRQ/cache/MMIO law; #153 asynchronous decoder fill/drain/resource/visibility semantics; #154 committed UI1 task entry, completed initialization, repeated input/pending work and acknowledged rendering. Accepted boot ROM, calibration, fonts and microcode must remain separately declared; physical SD boot and deployment remain #60/#85, not emulator prerequisites. [Source: Inventory §§8.11-8.18; #127/#145/#149-#155; spec lines95-119.]

### 5. Proposed qualification boundary

**[INFERENCE]** Build and qualify one cold-boot artifact with an outer reset-to-ready contract, plus ongoing accepted application trajectories. Replace whole startup orchestration rather than preserving per-leaf in-place limits as product requirements. Any relocation still needs admitted executable placement and caller/resource migration. At retained service seams compare ordered operation/arguments/results and required commits, loader target identities, ready sets and first task contexts, initialized application state and callback ownership. Normalize private addresses and frame layout only after every external consumer is migrated. Preserve mandatory partial-order edges, not historical timing or cross-core call interleaving.

**[INFERENCE]** Require cold reset through first-start language/date, live view, shooting menu, FILMSIM/back/reentry, editor cancel/confirm and repeated ordinary/pending-work inputs with acknowledged complete render results. Add shutdown/failure/recovery and capture/save only under their independently grounded contracts. Require wrong publication/image/target/context/lifetime and cross-boundary controls, continuous retired-code/data guards from publication through last consumer, and no checkpoint inherited from obsolete startup assumptions. A menu screenshot, marker or submitted task is not readiness. #149's proven timer relation is a reusable example of justified timing normalization, not a blanket timestamp exclusion. [Grounding: Inventory §§8.16-8.18; #155; spec lines43,73-99.]

### 6. Size, order and blockers

**[INFERENCE]** High integration leverage, but not the next fully ready independent retirement: known carriers are several KiB, with transitive code/device duties unmeasured. Treat startup composition as one continuing artifact now, and integrate heap/kernel/menu units behind its retained seams as they mature. End-to-end execution-independent acceptance follows platform, root/standby, later load and owned application-readiness closure, not simply concatenating qualified leaves. The existing two joins cannot be counted twice as one completed #155 artifact. [Grounding: #117/#127/#155; Inventory §§8.12-8.18.]

## D. All modal dispatch and its controller set

### 1. Interface, entry count, ABI and errors

The global dispatch root is `[0xbfb028,0xbfb124)` /252 bytes. Its census has 3,105 direct callers, **174 controller IDs and 49,738 cells**. This is one dispatch entry plus computed action/transition callback surfaces, not 174 ordinary functions. Complete unique action/transition-body count and bytes are not established in the inspected reports. Modal75 alone has 2,208 captured cells, 294 action slots and one captured transition slot; these are table slots, not necessarily unique functions. #128 explicitly excludes global replacement of all 174. [Source: UI report lines48,77-111,137,157-169; #128.]

Stock entry r0 is controller ID. It captures cell/action/transition roots and zero-extended state/event/width halfwords, returns 0 without prehook for null cell table, calls prehook `0xd0cc68(id,state,event)`, invokes optional action, rereads signed transition halfword after action, calls an optional transition or stores its negation to state, and returns the **action** result. Stock preserves r4-r11/SP, but callbacks can observe private register roles, r11, LR and stack scratch; ordinary AAPCS alone does not discharge those observations. No universal dispatcher error enum is recovered: controller action results carry their own contract. [Source: UI report lines79-111; E-MODAL-DISPATCHER-ABI-150; Inventory §8.18 line1696.]

The current 56-byte hook routes only full-word 75 to owned dispatch and every other value to original stock+4 with exact stock-prefix register/flags/stack state. That fallback is a required retained implementation, not global dispatch retirement. Known modal75 actions include SET `0xc72e10`, confirm `0xc75ed4`, leave `0xc76650` and cancellation `0xc77794`, table action codes0x5a/0x73/0x81/0x98. [Source: UI report lines163-177; E-RECIPE-MODAL-CONTRACT-150; E-MODAL-ENTRY-SEAM-150; #128/#133.]

### 2. Extent, state and initialization

Dispatch uses cell/action/transition root arrays at `0xcf58b8/0xcfe2a8/0xd0013c`, widths `0xcd5e78`, 174 current-state halfwords at `[0x1467588,0x14676e4)` and adjacent event halfwords at `0x14676e4`. Initialization `0xc22bd0` clears the state array, not the event array. Modal75 cells occupy `[0xcd1ae4,0xcd3f8c)`, actions `[0xd04080,0xd0452c)`, transitions `[0xd01370,0xd01378)`; bank publication legitimately changes some table words. [Source: Inventory §8.18 lines1688,1696; UI report lines223-227.]

Known action extents: SET `[0xc72e10,0xc742a8)`5,272 bytes, confirm `[0xc75ed4,0xc75f38)`100, leave `[0xc76650,0xc76a64)`1,044; total 6,416 before other actions/dispatch/cancellation. A native linked dispatcher was 272 bytes. The broader controller code extent is unknown; 49,738 cells are not a code-size estimate. Owned editor buffers, frame snapshots, mode/gate cells, property/commit functions, tables and render resources must be classified per lifecycle. [Source: UI report lines96,284-296,343; #133.]

### 3. Pieces to fold in

#121 classifiers, #123 row-state binding, #132 frame snapshot service, #133 SET/confirm/leave and bounded editor lifecycle, plus #128 dispatcher/cancellation are reusable. #133 has 85 action pairs, 10 composed editor pairs, a real-controller 55-case UI migration and packaged cardless lifecycle/render/rollback evidence. #128's selected package has 40 owned75 and 214 stock fallback completions with matched ten captures and 224-word rollback, but is still a scoped diagnostic with caller/frame/object/table/alias/lifetime obligations. Generic state30 cancellation remains native-only; observed FILMSIM stays state0. [Source: Inventory §§8.1-8.5; UI report lines207-235,292-365,390-440; #128/#133.]

### 4. Dependencies and unknowns

Controllers depend on input/scanner and task1's event loop, settings/property gates, frame/editor/buffer services, rendering, persistence and mode-specific capture/playback/device services. Global closure has 6,908 obligations in the later caller-continuation diagnostic; modal75's scoped diagnostic still has 6,828, including 6,507 unattributed pointer escapes. They are conservative obligations, not proven executable callers. Table initialization/use-point integrity, private callback ABI, asynchronous/nested writers, aliases, unknown IDs and late lifetimes remain open. Wireless NAME is controller76/state36 and does not supply controller75/state30 cancellation. [Source: Inventory §8.2 lines312-328; UI report lines111,123-137,157-169,221-239; #128.]

### 5. Proposed qualification boundary

**[INFERENCE]** Own **the dispatcher and all 174 controllers' decision/state code together**, with one typed event/controller/action interface and an explicit retained service boundary. Compare externally admitted event sequence, captured inputs, action results, public state/property changes, callback/service sequence, cancellation/handback and visible render completion. Internal callback addresses, LR identities and private frame layouts cease to be equality requirements only once every consumer of them is inside the unit; remaining retained services keep their actual ABI. Replace the scoped seam/fallback at global cutover instead of accumulating174 routing exemptions.

**[INFERENCE]** Qualify cold first-start and shooting menus, Q/SETUP, all accepted mode/controller trajectories, repeated entry/back, unhandled input, editor cancel/confirm and ordinary-settings preservation. Reuse packaged FILMSIM and bank Rename/Save Current/cancel/confirm/leave/render tests as baseline scenarios, not coverage of 174 controllers. Capture, RAW conversion, playback and persistence branches retain their current device/output blockers; do not remove them from the accepted controller inventory merely to pass a menu-only suite. Global table/caller census becomes an obligation inventory, while scenario acceptance uses logical events/results and acknowledged planes rather than instruction timing. [Grounding: #127 application ranks1-8; Inventory §§8.2-8.5,8.18; spec lines19,53-57,79-99.]

### 6. Size, order and blockers

**[INFERENCE]** High user-visible leverage, with 174 controllers and at least 6.4 KiB of already recovered three-action code, but no defensible whole-controller byte total. Select this as the application-side large unit after a complete controller/action manifest and service categorization. It can progress alongside the kernel over named retained runtime services. Readiness is lower than heap: callback-private ABI and unclosed tables are immediate blockers, with capture/playback/storage laws blocking broader lifecycle scenarios. A one-ID pilot must not remain the final planning unit. [Grounding: #128 explicitly excludes the global goal; #127 rank1; UI report closure and lifecycle sections.]

## E. Additional clean boundary: menu-frame snapshot service

### 1. Interface and count

There are **11 recovered entries**: classifier `0x61ca2c`, snapshot `0x61cd14`, store `0x61ceac` and eight accessors `0x61d0e4..0x61d334`. They are direct-call AAPCS seams, not installed kernel services. Snapshot emits 11 words /44 bytes; null destination returns 0 without write, detached frame emits documented defaults and returns 1. Accessors provide depth/node/type/class/cursor/current/page/navigation. No general negative-error enum is recovered; zero-row division can enter the retained SVC path. [Source: UI report lines390-410; E-MENU-FRAME-SNAPSHOT-NATIVE-150.]

### 2. Extent, data and initialization

Stock extents total 1,752 bytes: 744 classifier + 264 snapshot + 104 store + 8*80 accessors. The module itself has no static allocation/initializer; it reads live frame `0x9571d0`, value arrays/cursor/navigation cells and writes caller snapshots or existing slot tables. Saved-snapshot reader `0x61ce1c` and three slot tables remain stock. [Source: UI report lines392-410,430.]

### 3. Existing ownership

#132 provides the Rust service and 307 original/owned native pairs with literal expectations, including declared SVC/memory-guard boundary outcomes; the bank store call is migrated. There are 3,027 stock call sites still entering stock entries, so global retirement/publication is not done. [Source: UI report lines412-440; E-MENU-FRAME-SNAPSHOT-NATIVE-150.]

### 4. Dependencies and unknowns

Retained loader division `0x4d978`, frame/value/slot lifetime and bounds, SVC/exception integrity and computed/Thumb references remain relevant. Static census finds only direct BL references in its examined domain, not universal computed-transfer absence. [Source: UI report lines408-410,414-434.]

### 5. Qualification boundary

**[INFERENCE]** Qualify all 11 entries as one service, comparing return values, the 11 semantic words, slot selection, exact permitted writes, caller ABI and zero/null/boundary failure outcomes. Publish the whole service once, migrate all direct callers and enforce old-body exclusion across menu/edit/reentry trajectories. A broad modal subsystem can absorb it directly rather than keep it as another tiny permanent ticket. [Grounding: UI report lines414-440; spec lines39-43.]

### 6. Size/order

**[INFERENCE]** Highest readiness but low incremental leverage: roughly 1.75 KiB and a nearly closed ordinary call surface. It is a useful early retirement inside the menu unit, not a substitute for the maintainer's larger-unit goal. Packaged 11-entry publication, guarded integrated trajectory and retained divide/exception closure are the remaining gates. [Grounding: UI report line440; #127/#132.]

## F. Storage/file service, a promising interface with unresolved backend laws

### 1. Interface and count

At least **four asynchronous file-operation entries** are recovered: open `0x71c648(cb,path,mode)->tag`, read `0x71c6fc(cb,handle,length,buffer)`, write `0x71c75c(cb,handle,length,source)` and close `0x71c7bc(cb,handle,0,0)`. They route requests 0x2c17..0x2c1a to media task0x1c. Synchronous base forms and completion callbacks extend the surface; this is not a complete filesystem API census. Open modes 0/1/2/3 are read/create-truncate/append/“s”. Completion codes 0,3,0xe,8,6 mean success, prepare, commit, raw0x24 class and missing-file respectively; task-context refusal uses callback 9. [Source: File report §6.2 lines190-205; Inventory §8.3 lines343-346.]

### 2. Extent, data and initialization

Known workers include open `0x7115a4`, read `0x71edbc`, write `0x7116e4`, close `0x711768`; handle allocation `0x71ef18`, release `0x71eff4`; 16 handle slots at `0x9101c0`; callback slot `0x91028c`; two-slot volume registry `0x814730/0x814738`; close resolves tables `0x8fc880/0x8fc87c` and capacity `0x944468`. Whole file/media implementation bytes, complete initialization and filesystem metadata layout remain unknown in this evidence set. [Source: Heap survey line65; File report §§6.2,6.8 lines282-299.]

### 3. Existing pieces

Recipe storage code/codec and its bounded synthetic completion cases, 17 stock producer/relay cases, selected queue failure experiment, #129 save facade/media-permission work and #124 media observations can fold into a file/media project. They do not own the stock filesystem, writable close/flush or scheduler. [Source: File report §6.7-6.8; Inventory §8.3 and lifecycle notes; E-XM1-QUEUE-CALLER-BOUNDARY-150; #127 storage runtime rank12.]

### 4. Dependencies and unknowns

Tags are allocation lifetimes, not durable identities. Producers can ignore enqueue/wait failures; absence of callback is not cancellation. Lock/wait/wake services, pending wake bits, shared callback reuse, media attach/detach serialization, volume identity, card errors, flush and physical storage recovery remain unresolved. Writable close requires an actually opened filesystem object; missing-file fixtures cannot qualify it. [Source: File report lines274-299; `emulation/README.md:498-514`.]

### 5. Qualification boundary

**[INFERENCE]** Own file task/requests, handle manager and completion/cancellation machinery behind the four caller-facing operations, retaining one explicit block/media backend until separately qualified. Compare path/mode/length, callback status/count/order, handle lifetime, file/directory contents and durable close/flush effects, not internal handle slot or queue layout. Qualify read/write/append, missing/full/read-only/media loss, failed post/wait, delayed callback, cancel and tag wrap/reuse. Cold boot plus menu settings and capture/save/playback are appropriate system scenarios, but writable storage and JPEG completion cannot be declared qualified while their backend laws remain open. [Grounding: File report §§6.2,6.8; #124/#127/#129/#139.]

### 6. Size/order

**[INFERENCE]** Potentially high leverage over save/playback/external-interface behavior, but code bytes and full API count are not recovered and readiness is low. Schedule after the kernel messaging/lifetime boundary and independent media/error laws, rather than reuse menu-only completion mocks as file ownership. The 16-slot handle manager belongs here, not in the general heap cutover. [Grounding: #127 runtime ranks7,9,12; File report §6.8.]

## G. Display/render service, clean actor seams but not yet a complete renderer contract

### 1. Interface and count

UILIB task33 entry `0x1109130`, queue33 `{count 0x30,size 0x14}`, and VGLIB task73 entry `0x6d7540`, queue73 `{0x30,0x38}`, are recovered actor interfaces. UILIB's finite receive/dispatch step is `[0x11090a4,0x1109130)`. The row-availability probe `0xd52684(field,row)->state` is a separate synchronous service: 0 offered, 2 disabled, 3 hidden. These are at least two actor seams plus one probe, **not a complete rendering API count**. [Source: Inventory §8.18 lines1676-1694; `replacement/service/README.md:9-10,32-44`.]

### 2. Extent, data and initialization

UILIB's finite step is 140 bytes; row probe is0x6038 /24,632 bytes with further callees/tables. UILIB initializes through `0x1108cec/0x1108bc8`, publishes a consumer token after successful receive and initializes byte `0x15e470d` only through command family0x1000. Graphics planes, scanout/rectangle state, buffers and completion/ack state are observable, but total renderer code/data ownership and initialization duties are not recovered. [Source: Inventory §8.18 line1694; `replacement/service/README.md:9-10,78-84`; `emulation/README.md:1038,1247`.]

### 3. Existing pieces

#123 owns only a typed probe binding, not the 24 KiB probe or renderer. #133/#128 have same-SDK stock/owned rendered-frame comparisons; scanout JSON and native plane bytes exist, and `render-compare` checks complete frame/state/source spans without requiring equal clocks. No whole display/render executable service is owned. [Source: `replacement/service/README.md:17-30,78-84`; UI report rendered differential sections; `emulation/README.md:1038,1139`; Inventory §8.3.]

### 4. Dependencies and unknowns

Queues, graphics workers, resource/font data, vector graphics, memory/buffer ownership, display MMIO, scene/sensor input and completion/acknowledgment laws remain dependencies. Token equality or one screenshot is not completion. Physical panel output, color fidelity, target timing and broader processing device laws remain unproved. [Source: Inventory §2 live-view/development rows; §8.18 lines1694,1706-1708; #127 runtime rank11.]

### 5. Qualification boundary

**[INFERENCE]** A future unit can own UI render actors, command interpretation and host-CPU rendering behind typed requests and completed frame publication, retaining a separately grounded scanout/device backend. Compare command/result ordering, buffer lifetime, full-plane pixels and native frame/scanout composition only after completion/ack. Reuse cold menu, FILMSIM/editor/back/reentry and live-view overlay trajectories; later playback/capture requires qualified producers. Timing may change, but frame readiness, no tearing/partial publication and buffer ownership cannot be waived. [Grounding: Inventory §8.18 line1708; `emulation/README.md:1139,1247`; spec lines79-99.]

### 6. Size/order

**[INFERENCE]** Large potential executable retirement, with a measured24.6KiB probe alone, but full extent/API/device semantics remain unmapped. Rank behind heap, modal controllers and RTOS boundaries. It is a plausible later subsystem, not evidence that replacing the small UILIB step would own rendering. [Grounding: #127 runtime rank11; `replacement/service/README.md`; Inventory §8.18.]

## Cross-cutting qualification rules for larger units

- **[INFERENCE] One artifact, one externally meaningful acceptance matrix.** Component native/admission proofs remain regressions; do not require a new observer/consumer/control program per internal function. Reuse the existing typed receipt, raw journal, artifact, scenario and rendering infrastructure. [Grounding: spec lines62-99; #155.]
- **[INFERENCE] Separate compatibility adapters from owned internals.** Keep exact ABI/layout only at retained consumers, then migrate or remove those adapters in a clean cutover. Known object-pointer escapes in root descriptors, calloc headers and modal callback frames cannot be normalized away before that migration. [Grounding: Inventory §§8.12-8.14; UI report dispatcher ABI; spec lines13,53,79.]
- **[INFERENCE] Timing changes require semantic ordering and grounded deadlines, not equal instruction counts.** Use per-object/task traces, required synchronization edges and source-proven timer relations. Actual IRQ/MMIO timing laws and silicon bounds remain independent evidence requirements. [Grounding: Inventory §8.16 lines1526-1541; Heap survey lines213-215,278.]
- **[INFERENCE] Preserve non-success outcomes.** Natural masked activation `-25`, root cleanup's possible `-63`, flag output semantics, ignored lock errors, partial message publications and loader error-side done/progress effects are distinct behaviors. Decide intentional corrections explicitly, as already required for the heap resize leak. [Grounding: Inventory §§8.12-8.17; #147/#151.]
- **[INFERENCE] Whole-unit retirement is scenario-scoped but complete within that scope.** Both cores/aliases, callbacks, copied/constructed references, exception frames, code publication epochs and object lifetime need continuous coverage. Menu arrival after a watch ends is continuation evidence, not guarded retirement through the menu. [Grounding: spec lines41-43,73,91-99; #128/#145/#152/#155.]

## Closing ranking: leverage and readiness

The following ranks are **[INFERENCE] planning judgments**, not measured effort or an assertion of stock call order. Leverage counts categories and retired implementation scope per subsystem qualification; readiness reflects the cited existing contract/runtime evidence and missing prerequisites. Exact whole-unit code totals are unknown except where stated.

| Leverage rank | Candidate | Code/category leverage per qualification | Readiness now | Proposed execution order and principal gate |
| --- | --- | --- | --- | --- |
| 1 | Kernel/RTOS platform | 97 callable slots per domain plus direct/vector entry, roughly 50 KiB entry-address envelope per core, task/scheduler/sync/message/timer/IRQ categories together | Medium-low: publishers, wrappers, objects and natural initial runnable sets are recovered; whole service/device/context contracts are not | Start semantic manifest/device-law recovery now; implement after stable IRQ/context/lock seam, while other units use named retained services. [Inventory §§8.12-8.17; #127 ranks3-7.] |
| 2 | Full modal controller set | 174 controllers/49,738 cells, dispatch/action/state/editor/navigation ownership; total bytes unknown | Medium-low: bounded dispatcher/actions/render evidence exists, broad callback/table/caller and per-controller behavior remain open | Application-side next large project alongside RTOS; absorb snapshot service, close complete controller/service manifest, then remove scoped fallback. [UI report closure/lifecycle sections; #128/#133.] |
| 3 | Whole startup artifact | Several KiB known carriers plus unmeasured transitive startup, loader/init/task/root/load/application-readiness categories | Partial joins ready, end-to-end independence not ready | Maintain one composed artifact now; final acceptance after reached kernel/root/standby/later-load/application gaps close. [Inventory §§8.12-8.18; #149-#155.] |
| 4 | Storage/file service | File operations,16-slot handle manager, completion/cancellation, save/playback/external-interface retirement; bytes unknown | Low: request ABI exists, writable flush/lifetime/media law unresolved | After kernel messaging and independent storage-error/device evidence. [File report §§6.2,6.8; #127 ranks9,12.] |
| 5 | Display/render actors/services | At least 24.6 KiB probe plus unmapped actor/render code, menus/live-view/playback display categories | Low: good visible-output oracle, incomplete renderer API and backend law | After controller/platform boundaries, with independently grounded render/scanout completion. [Inventory §8.18; #123/#127 rank11.] |
| 6 | General heap | 4,336 stock allocator bytes plus helpers, all malloc/free/resize/calloc/trim clients behind one backend | Highest large-unit readiness: selected rlsf and admitted trace/replay; bridge/concurrent locking still missing | **First implementation/cutover candidate**; preserve two front ends and migrate the whole known closure, then broaden workload/lock qualification. [Heap survey lines24-99,189-278; #147/#148.] |
| 7 | Menu-frame snapshot service | 1,752 bytes/11 entries/3,027 existing call sites; one narrow service | Highest narrow-unit readiness, but low new capability leverage | Fold into the full menu project; optional early global service cutover, not another chain of leaf tickets. [UI report lines390-440; #132.] |

**[INFERENCE] Recommended new unit plan:** general heap first; full menu/controller ownership and RTOS semantic/device-law work as the two substantial development tracks; one continuously composed startup artifact as their integration vehicle; then storage and renderer projects when their backend laws are sufficient. Keep distinct reservation, file-handle and SQLite pools out of the general-heap cutover. This preserves #117's capability/retirement goal while changing the acceptance unit from function-sized patches to externally qualified subsystems. [Grounding: #117/#127; spec lines13-19,33-43,64,166-174; subsystem evidence above.]
