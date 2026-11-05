# Startup artifact scope after J8: application readiness (#154)

Scope of what the composed startup artifact (#155) must own after roadmap step J8 to reach #154's booted application, and the readiness check its composed run must establish. Sources: [replacement-inventory-baseline.md](replacement-inventory-baseline.md) (RIB) §§8.15-8.18 and §8.20, [subsystem-replacement-map.md](subsystem-replacement-map.md) (SRM) §C and [spec/11](../spec/11-incremental-firmware-replacement.md#ownership-stages). RIB and SRM line numbers are those of this commit; listing citations are to the pinned decoded images (`lz/id4.dis`, `lz/id5.dis`). Planning judgments are marked [INFERENCE].

## 1. Question and answer

The §8.20 roadmap ends at J8 = J7 + the 32-byte main-UI controller pump `[0xbfbd20,0xbfbd40)`, observing task1 entry, the first owned pump call and repeated input (RIB:1871, 1894), then step 9: one reset-to-menu run with all pieces and continuous guards, then the #155 scenario matrix (RIB:1895). SRM §C defines the artifact's output as "a booted application with executable publication, actors, settings/calibration/input and render readiness" (SRM:115) and puts inside startup "task descriptors ... constructor/callback publication and eventual actor initialization" (SRM:130) and the later-initialization extent "three plain extents, 15 hooks, 14 task submissions; image5 reverse 3-hook constructor list and 21 direct initializer calls; actual UI outer body `[0xc22f60,0xc2327c)` 796 and pump `[0xbfbd20,0xbfbd40)` 32" (SRM:128). After J8 all of that except the pump stays stock (RIB:1798).

Proposed scope [INFERENCE on the step order and unit cuts; the extents are SRM's]:

- **J9 image5 initialization**: the image5 initializer `[0xd985a4,0xd986b0)` with its 21 calls and the reverse 3-hook constructor list it walks, entered through `*(load+0xc)`. The plain-extent hooks and submissions (SRM:128) follow later with the loader actors that run them (section 4).
- **J10 constructors**: the listed task constructors (section 4) with all their duties (SRM:130), including UI1's `0xd0a238`.
- **J11 main UI actor**: task1's body `[0xc22f60,0xc2327c)` with its once-duty `0xbfb124`, absorbing J8's pump as its leaf.
- **Readiness**: a composed-startup endpoint that replaces menu-id equality with a semantic first-ready invariant (section 3).

Retained after J11, behind named shims (section 5): RTOS services (#156), dispatch/normalizer/controllers and their state including `0xc22bd0` (#128), the heap's clients (#148 owned at J7), standby93 (#152), the loader actors and decoder semantics (#153), every other actor body, settings/calibration, and render.

## 2. Where J8 leaves the artifact

- J1-J6 qualified (`E-STARTUP-J1-COMPOSED-NATURAL-150` through `E-STARTUP-J6-COMPOSED-NATURAL-150`); J7, the heap cutover, is not yet composed.
- J8, the owned pump leaf, ran on J6 through the application phase: equal pair, every pump call through the owned code in the composed role, 19 controls (`E-STARTUP-J8-COMPOSED-NATURAL-150`). The scoped loader-continuations observer that refused earlier is removed (`E-APPLICATION-PUMP-FETCH-MULTISET-REFUSAL-150`).
- Existing observer evidence of task1's first entry: RIB:1477.

## 3. Readiness the composed run must establish

From RIB:1788 and 1800-1807:

1. Executable publication: image5 and plain published (J6). Admitted retained receipt: image5 entry 32,011,226..35,842,556; plain 40,407,475..55,646,819 (RIB:1780-1782).
2. Actor initialization: task1's committed C/T/current-core/stack entry, then `0xbfb124` and `0xc22bd0` completed (RIB:1768, 1802). Submission is not scheduling (RIB:1752).
3. Settings/calibration/input: the erased-settings first-start scene with retained provenance; observable only as each role's return values of `0xed4acc` and `0xed4afc` (`0xed4afc` returned 98 in the retained receipt, RIB:1780), compared across roles. `0xef8c90` is called with a null completion callback on this route (`mov r0,#0` at `0xd98658`, `lz/id5.dis:431877-431878`; RIB:1749), so its fanout stays a declared shim. None of this validates calibration (RIB:1742, 1786).
4. Render completion: `RenderFacts::probe` conjuncts (RIB:1788): released inputs, zero pending/busy/suppression/batch, UILIB initialized byte `0x15e470d` = 1 (set only by its `0x1000` command family, RIB:1774), graphics completion/ack, listed actors (UILIB last) waiting.

Invariant [INFERENCE]: a **sample** is taken whenever task1 is blocked on queue1, that is, its status is waiting and it is inside `0x725318(1,buf,-1)` reached from task1's `BL 0xdb5124` at `0xc2314c` (return address `0xc23150`), where the pending-work checks ran with byte `0x949dd4` zero. task1 sets op `0x107` (at `0xc23130` and `0xc22fb8`) before both of its calls, so it always blocks (`lz/id5.dis:50482-50680`); the call at `0xc22fd4` (return address `0xc22fd8`), taken when `0x949dd4` is nonzero, skips the pending checks and discards every command except `0x837`/`0x838`, so it is not a sample. **First-ready** is the first sample at which 1-4 all hold, task1 included among the waiting actors. It is **stable** if, over a declared settle interval after it with no queue1 send, `RenderFacts::probe` stays drained and conjuncts 1-3 hold at every probe sample and at the interval's end; a queue1 send or a failed conjunct inside the interval makes the sample not stable, and the next sample becomes the candidate. task1 first blocks long before readiness (task1 enters at 36,115,325, RIB:1477; UILIB33 is submitted at 40,587,849, RIB:1782), so earlier samples are recorded as not-ready rather than refused. Menu id, PC, zero task status and token equality are not conjuncts (RIB:1788).

Implementation: a schema-5 contract (`"application": {}` in the manifest) gives `//emulation:composed-startup` an application phase that records task1's committed context, its initialization chain, every queue1 receive under a stack scope over task1's stack, every pump call and handback, readiness samples with the first-ready search above, and the settled inputs with visible captures, and checks them per role and across roles (emulation/README.md, composed-startup application phase). The first-ready search ends at the first press at 80,000,000, so the existing menu route and its `(69,52,12)` and `(1,5,2)` checks stay as they are. Publication is read at the producers' caller returns `0x73b4b0` and `0x73b558`, outside the J6 producer carriers. A fifth conjunct binds readiness to the UI policy block at `0x944580`: the gate task1 passes before its receive at `0xc2314c` (no deferred or priority UI work) holds through the settle interval; first-ready also records the Xm1MenuState node, cursor and level word. On the J6 composition, with task1 and the pump stock in both roles, first-ready is stable at 55,867,999 retained and 56,504,168 composed instructions, the pair compares equal and all 17 application controls refuse (`E-STARTUP-APPLICATION-PHASE-150`).

## 4. Owned units after J8

### J9 image5 initialization

- Status: owned in place over `[0xd9856c,0xd986b0)` with the entry word unchanged (`//replacement/image5_init`, piece `image5-initializer`); the host oracle runs the stock and owned bytes on 23 contract cases and they agree, and the piece composes with J8 pending the step-9 pair (`E-STARTUP-J9-IMAGE5-INIT-HOST-150`). The hook wrappers and constructors stay retained (J10).
- Image5 initializer `[0xd985a4,0xd986b0)`: 8-byte frame, returns the final marker's r0; 21 direct calls to 20 distinct targets (`0x72872c` twice, with `0x01010100` and `0x11010100`) in source order with their arguments; the 11 local zero-result checks with their local spins; ignored results stay ignored (RIB:1736-1738; `lz/id5.dis:431830-431896`). `0xd9856c` `[0xd9856c,0xd985a4)` is the reverse 3-hook dispatcher, not a constructor (RIB:1743, 1356); it reads the hook table `[0xbf1a20,0xbf1a30)` through two constants, `0xbf1a30-4` built by `movw`/`movt` at `0xd98574`/`0xd98578` and the literal `0xbf1a28` at `0xd985a0` (`lz/id5.dis:431816-431829`); J9's placement admission checks both if the table moves. J9 owns the walk and the three hook wrappers `0x107277c`, `0x1066304`, `0x1062114`, which each call their constructor and stop on a nonzero result (`lz/id5.dis:1141970-1141973, 1129591-1129594, 1125526-1125529`; RIB:1358-1364).
- Entry and placement: reached through `BLX *(load+0xc)` on the task59/task60 route `0x73cdc0 -> 0x69d2b4` (RIB:1321, 1690), so the owned initializer is published at the stock entry word in decoded image5 or that word is migrated; closure of the computed reference is part of J9's admission (RIB:1792, 1800, 1811).
- Retained: every called service, and the loader actors that reach and run published code: flashloader task60's request server `0x73b3a0` on queue 60 (commands `0x4c00`-`0x4c08`; it calls the J6 producers at `0x73b4ac` and `0x73b554`, `lz/id4.dis:293869ff`; RIB:1347) and elfloader95's plain-extent worker `0x73bc54` with its vector/MPU publication, progress events, deferred callbacks and the 15 plain hooks and 14 submissions (RIB:1346, 1378-1384). Both are image4 actors active before image5 is published. [INFERENCE] They are #153's loading unit; the plain hooks and submissions SRM:128 places in startup follow as a later step once #153 owns those actors.

### J10 constructors

- The set: the constructors J9's calls reach (`0xd80d44` usb36, `0xec5350` syscont2, `0xdca604` reccont3, `0xdb63e0` playcont4, `0xed10b4` USB controller (task id unrecovered, RIB:1748), `0xef8c90` saclcac43, `0xed2b00` mujin35, `0x1055d10` rplib64, `0x69f000` flashext80) and the three hook constructors (`0x10726f4` redeye56, `0x1066280` face55, `0x1062090` facerecog62), plus UI1's `0xd0a238` (RIB:1743-1750, 1758). The plain-hook constructors (UILIB33's `0x110f9d8`, netdrv) wait for the plain-hook step; smaphk72's `0x892334` and VGLIB73's `0x6d7f88` have no recorded caller (RIB:1763-1764) and stay retained until one is recovered.
- Each constructor's duties as in stock: descriptor words 0..5, mailbox/queue creation and its failure path (two of the hook constructors return `-255` on mailbox failure, RIB:1364), the wrapper call (J4), and the duties around it: `0xd80d44` clears `[0x1469cd0,0x146a184)` after submission (RIB:1745); `0xef8c90` invokes the retained initialization fanout (RIB:1749); `0x69f000` leaves its argument word `D+20` unwritten (RIB:1752).
- Descriptor word 6 (`D+24`) is left unwritten by stock constructors and the wrapper classifies it against `0xa5f0a5f0`, choosing auxiliary flags 0 on a match and 4 otherwise (RIB:1061, 1364, 1405, 1752). An owned constructor must give the wrapper the same classification as stock at each call; keeping the residue value is not the contract once the constructor frame is owned.
- UI1's `0xd0a238` is 120 bytes: it creates queue1 through retained kernel service `0x724cdc(1,{0x30,0x54})`, skips submission if that fails, then builds the descriptor (entry `0xc22f60`, priority 6 stored as 5, stack `0xc00`, argument 0) and calls `0x725dd8` (`lz/id5.dis:286717-286749`; RIB:1758, 1796). It is reached from syscont2's `0xec5678`, which writes 256 to `0x15c1e88`, `0x15c1e90` to `0x15c1e8c` and 0 to `0x15c20a4` before the call and maps its result to 1/255 after (`lz/id5.dis:734439-734453`); `0xec0aa8` only calls `0xec5678`. Seam: the retained `BL` at `0xec56a0`; syscont2's body stays retained.
- [INFERENCE] Constructors are one unit because SRM:130 assigns constructor publication to startup as a whole; owning only UI1's would be a function-sized cut (spec/11:35).

### J11 main UI actor (task1)

- Owned: `[0xc22f60,0xc2327c)` and the once-duty body `[0xbfb124,0xbfb1b8)`; the J8 pump `[0xbfbd20,0xbfbd40)` stays a qualified leaf inside this unit at its stock placement. Its stock caller is `0xc23274`; the Rust caller and handback are mapped from the linked artifact.
- `0xbfb124`: calls `0xd36310(0x1000)`, whose result is the descriptor's fifth word, then forwards the five-word descriptor through `0xdb4b34`, calls `0x633594` when that returns nonzero, calls `0xd48240` and `0xd38130(0)`, and writes `0x972e84` and `0x954d68` (`lz/id5.dis:9635-9671`; RIB:1768).
- Loop body behaviour (`lz/id5.dis:50482-50680`): loop head `0xc22f84` calls `0xd3cbd4` and `0x63270c(0,0,0,0,0)`; the byte at `0x949dd4` selects a receive mode in which every command except `0x837`/`0x838` is cleared after `0xd3ad10(0)`; `0xdb5124` is called with op `0x107` at both sites and blocks in `0x725318(1,buf,-1)` (its `0x106` poll at `0xdb5144` is unreachable from task1); entry through `0xc22fd4` (byte `0x949dd4` nonzero) skips the pending-work checks; command `0x944580`, argument `0x944584` and modifier `0x944588` are cleared on nonzero receive status and also when `0xd5cf50()` returns 1 (the clear at `0xc231ec`); masked commands below `0x600` move bits `0xc0` into the modifier and pass predicates `0xd0b318`, `0xd0b438`, `0xd0c954`, `0xd35f3c`, `0xd5cf50`; commands `0x200`-`0x203` are stored to `0x95200c`; the pending-work chain calls `0xd3b5ec`, `0xd19d18`, `0x636004`, `0xd1734c`, `0xd17370`, `0xd3b0dc`, `0xd3ab00`, `0xd3ad5c`; after dispatch `0xd36ea4`, `0xd36e60`, `0xd35e14`, and `0xd0cd10` for command 9 with zero modifier; globals `0x94f5e0`, `0x950148`, `0x951528`, `0x950098`; every path reaches `0xc23274: BL 0xbfbd20` then `0xc23278 -> 0xc22f84` (RIB:1770).
- Retained: every callee listed above, `0xdb5124` and `0x725318` with packet release through kernel slot `+0xf8` (RIB:1772), and `0xc22bd0` with its 174-halfword clear `[0x1467588,0x14676e4)` (controller state, #128, SRM:162).

### J11 dispatch contract

The Rust piece is `replacement/task1_actor`. Its entry remains `0xc22f60` and its once-duty entry remains `0xbfb124`; J8 remains a separate qualified pump leaf in the same startup composition. Both decomp definitions are NONMATCHING at x70view `f659f02b850fcba58de0f9b09ee24ad09a24c2f4`. The contract below is confirmed against stock `lz/id5.dis:50482-50680` and `9635-9671`, not inferred from the decomp's names or types.

- Startup calls the once-duty and then retained controller initialization `0xc22bd0`, once each before looping. The once-duty writes 1 to `0x972e84`, obtains a buffer from `0xd36310(0x1000)`, passes the five words `{0x918ab8,0x93bea0,0x96b400,0x95b098,buffer}` to `0xdb4b34`, stores its result at `0x954d68`, calls `0x633594` on nonzero result, then calls `0xd48240` and `0xd38130(0)`. The bytes of `0x633594` overwrite r0 before reading it (`lz/id4.dis:23658-23678`); it has no incoming argument contract.
- Each iteration calls `0xd3cbd4` and `0x63270c(0,0,0,0,0)`, clears `0x94f5e0`, and samples hold byte `0x949dd4`. With hold clear, pending work is serviced in this priority: word `0x950148 == 1` calls `0xd3b5ec` then clears that word; word `0x951528 == 1` calls `0xd19d18` and `0x636004(0)`; `0xd1734c() == 1` calls `0xd17370`; nonzero word `0x950098` calls `0xd3b0dc`; `0xd3ab00() == 1` sets `0x94f5e0` to 2; `0xd3ad5c() == 1` sets it to 1. The first matching duty skips receive and reaches post-dispatch.
- Otherwise, the retained receive adapter gets four words `{0x107,0x944580,0x944584,0x94458c}`. Its queue and packet lifetime stay retained. Nonzero status clears the command, argument and low 16-bit modifier, then calls the pump without post-dispatch. Successful status with `(command & ~0xc0) < 0x600` moves bits `0xc0` into the modifier, removes them from the command and calls `0xd0b318`. All successful receives call `0xd0b438`; acceptance is `0xd0c954() == 0`, or, only if that is nonzero, `0xd35f3c() == 0`, or command in `{6,0x204,0x210,0x11,0xe}`. `0xd5cf50() == 1` suppresses acceptance.
- In held mode, no pending duty is checked. After those predicates, commands `0x837` and `0x838` call `0xd3ad34`; all other commands call `0xd3ad10(0)` and are cleared. Accepted commands reach post-dispatch; rejected commands are cleared. Service return values not tested by the stock body remain ignored.
- Post-dispatch calls `0xd36ea4`, `0xd36e60` and `0xd35e14`; command 9 with zero low 16-bit modifier additionally calls `0xd0cd10`. Commands `0x200` through `0x203` update `0x95200c`. Every path calls J8's pump and returns to the loop head. Commands are reread after retained calls, including before the held-command decision and the command latch.
- Direct owned writes are the task's stack, `0x972e84`, `0x954d68`, `0x94f5e0`, `0x950148`, `0x944580`, `0x944584`, the low halfword at `0x944588`, and `0x95200c`. Retained callees own their effects separately. The loop never returns; receive may block indefinitely. The observer's sites are semantic labels mapped from the emitted artifact, not a requirement to retain stock instruction layout.
- Retained `avout_send_initialization` (`0xdb4b34`, exact at the cited decomp commit, `app/drivers/display/avout_service.c:61-71`) reads the five descriptor words without modifying them and forwards a six-word payload with word 1 unwritten. Equality of that retained service's uninitialized stack word is outside this increment's contract.


## 5. Retained seams after J11 (spec/11:37 shims)

| Shim | Owner | Removal condition |
| --- | --- | --- |
| Kernel queues, `0x724cdc`, `0x725318` receive and release, scheduler, IRQ/timer | #156 (#151, #145) | RTOS platform services owned |
| Dispatch `[0xbfb028,0xbfb124)`, normalizer `0xbfb20c`, hooks `0xbfb1b8`/`0xbfb1e0`, `0xc22bd0`, 174 controllers and state | #128 (SRM:162, 174) | modal dispatch unit owned |
| task1's loop predicates and pending-work/post-dispatch services (image5 callees and image4 `0x63270c`, `0x636004`) | [INFERENCE] #128 for the image5 controller services; the image4 ones unassigned | each service's owner recovered and owned |
| Heap clients | #148 (J7) | heap closure complete |
| Standby93 producer | #152 | owned standby |
| J9's other callees `0x72872c`, `0x6c62a8`, `0x73aca4` (command-7 adapter into task60), `0x69cf84`, `0xdb01fc`/`0x6e27ac`, `0xd986b0`, `0x1056c24`, `0x6b7df8` | their services' owners; [INFERENCE] task60's adapter with #153 | each service owned |
| J9's caller route `0x73cdc0 -> 0x69d2b4` and the task59 initcont state machine | #153 / startup later-load route | owned natural loading |
| Loader actors task60 `0x73b3a0` and elfloader95 `0x73bc54` (with the plain hooks and submissions), image5/plain decoder semantics | #153 | owned natural loading, then the plain-hook step in startup |
| Actor bodies syscont2, reccont3, playcont4, usb36, saclcac43, mujin35, rplib64, flashext80, smaphk72 | their subsystems (no issue) | each subsystem owned |
| UILIB33, VGLIB73 and render completion | display/render project (SRM:240-266) | renderer owned |
| Settings/calibration/adjustment cache `0xed4acc`, `0xef8c90` fanout | none | accepted settings contract |

## 6. Prerequisites and order

1. J8 is done on J6 (`E-STARTUP-J8-COMPOSED-NATURAL-150`), with the pump's sites at owned PCs in the application phase. J7 lands independently; whichever lands second rebases.
2. Done for the stock actor (`E-STARTUP-APPLICATION-PHASE-150`, on J3 and J6): task1 attribution, queue1 provenance, pump sequences, the readiness check and 17 controls in `//emulation:composed-startup`. J8-J11 reuse it with owned PCs in the contract's application sites.
3. A per-composed-DAT reset checkpoint and timer re-recording past about 6.14M (RIB:1879-1880).
4. Source recovery for J9's computed-entry closure, J10's constructor extents and transitive services (unmeasured, SRM:130), and callers of `0x892334` and `0x6d7f88`, read from the X-M1 decomp (`E-XM1-DECOMP-SOURCE-150`), with each function's exactness from `xm1-decomp lookup` and NONMATCHING facts confirmed in the listings; runs then confirm.

## 7. Mapping to #154 acceptance (RIB:1811-1816)

| Criterion | Discharged by | Still open |
| --- | --- | --- |
| Bound entry, initialization, readiness, dependencies | J9 call contract, J10 constructors, J11 once-duty, section-3 invariant, section-5 shims | computed-call and global-writer closure for `*(load+0xc)` and the descriptor entry words; the plain hooks wait for #153; settings/calibration validity stays a declared shim |
| Packaged artifact enters owned orchestration from cold reset | step 9 over J1-J11 with admitted placement at the stock entry words or migrated callers (`*(load+0xc)`, `0xec56a0`, `0xc23274`) | J9 placement design |
| Device-driven owned loop with independently checked visible results | J11 repeated receive-backed and pending-work iterations with acknowledged full-plane results (RIB:1803) and paired visible admission (RIB:1815) | observer port (section 6.2) |
| Repetition, cancellation, handback, failures | J11 cancel/back, no-op, receive/service failures; unreachable routes named (RIB:1804) | reachable route census |
| Readiness/effect/refusal controls and continuous exclusion | the RIB:1805 refusal set and the 14 controls (RIB:1815) run against the composed pair; lifecycle-aware guards over J9-J11 carriers from publication (RIB:1806) | control port (section 6.2) |
| Inventory and ledger scope | section 5 shims and per-step records | none |

## 8. Open facts

- Identities of interface pointers `0x89f784/0x89f78c` published by `0xd986b0` (RIB:1744).
- Reachable cancel/failure routes; modal75 state0 BACK is not the state30 editor cancel (RIB:1804).
- The section-3 invariant is a proposal; `RenderFacts::probe` was not built as acceptance (RIB:1788).
- When #153 owns the loader actors, the plain-hook step's extent and constructors.
