# Owned record store: design (storage first cutover)

Status: design, revision 2. The previous single-cache carrier equals stock on all 370 native cases and detects all nine controls (`E-RECORD-STORE-ORACLE-NATIVE-150`); those artifacts remain historical evidence, not qualification of the two-slot correction. Source review was clear through `562c0b8e` and its diagnostic/evidence extension. Two-slot source, persistent multi-kind fault coverage and carrier ownership require renewed qualification and review. Full-system acceptance remains open: the original erased pair differs in retained picker behavior, and the strict local-UI-drain cohort refuses before completing the matrix (`E-RECORD-STORE-CUTOVER-PAIR-150`). Revision 1's design-review findings are folded into this report.

Sources: [the stock record-service contract](record-service-stock-contract.md) (here **RSC**, byte-read from `lz/id4.dis` with exact x70view C at `55436fc85c`), [the target design](storage-file-target-design.md) §§1.6, 4 and 6 (`E-STORAGE-FILE-TARGET-DESIGN-150`), the [populated record-area input](../emulation/README.md#populated-record-areas---record-nand) (`E-RECORD-AREA-MEDIUM-150`), the property-service pattern (`E-PROPERTY-SERVICE-ORACLE-150`, `E-PROPERTY-SERVICE-NATURAL-150`), and the recovery-emul native cases (`emulation/README.md`, "the 1.50 writer block loop"). **[INFERENCE]** marks design judgments.

## 1. Target and acceptance

The cutover owns the whole record service, image4 `[0x6ac2b4,0x6ae114)` (31 functions, 7,776 bytes), for all five record kinds, over the stock raw NAND service. It qualifies on the host by the native oracle of §5 (every case equal, every must-fail control failing) and, once for the cutover, by the full-system pair of §6.

## 2. The stock service this replaces

RSC holds the contract with citations. What the owned code must reproduce:

- Entries: open `0x6ac2b4`, register `0x6ac328`, close `0x6ac478`, read latest `0x6ac4c8`, append `0x6ac7d4`, erase all `0x6ac984`, collect `0x6acab8`, read at position `0x6acda0`, constant zero `0x6ae0fc`. Image4 calls them from `0x69d1d4` (open), five client sites each for register, close, read latest, append and erase all, `0x727d38`/`0x72827c` (collect) and `0x728334` (read at position); image5 calls `0x6ae0fc` at `0xdbad50`. No movw/movt or literal reference to `[0x89f8b0,0x89fa80)` exists outside the span.
- Geometry: two banks of one 128 KiB block per kind (logical pool blocks of the [record-area table](../emulation/README.md#populated-record-areas---record-nand)). Registration passes record bytes S, a total count split into N = count / 2 per bank (truncating), and sequence offset Q, 0 for all five clients:

| Kind | S | N | Program bytes | Staging buffer |
| ---: | ---: | ---: | ---: | --- |
| 0 | `0x60` | 254 | `0x60` | `0x8e16a0` |
| 1 | `0x1000` | 16 | `0x1000` | `0x8e00a0` |
| 2 | `0x200` | 31 | `0x200` | `0x8e1700` |
| 3 | `0x34` | 127 | `0x600` | `0x89fa80` |
| 4 | `0x600` | 10 | `0x600` | `0x8e10a0` |

- Open fills the five staging buffers with `0xff` (`0x6ad9a8..0x6ada08`); kind 3's `0x600` program carries the staging bytes past S, which no stock path writes after that fill.
- Staging: every seek writes S bytes of the sought record into the kind's staging buffer, and every check and prepare callback receives that buffer. Check may change the record in place (the area-3 validator corrects single-bit errors); read latest returns the post-check bytes.
- Last written record of a bank: binary search on occupancy (sequence halfword at Q, `0xffff` unwritten), not validity.
- Read latest (`0x6ac4c8..0x6ac7d0`): starting from the wrapper's current bank, examine up to B + 1 = 3 banks cyclically, closing and reopening the next bank each time; in each, find valid runs find-last then checks, stepping back one record per rejection. The first valid candidate is best; a later one replaces it when `c <= b && b - c >= T` or `c > b && c - b <= W` (T = 2N, W = `0x8000` - T), and the scan stops at the first candidate that does not. When the best is at position N - 1, find valid also runs on the following bank, only to rewrite the skipped-count halfword. Finally the best bank is reopened and searched again, so check runs again on the same records. A failed scan leaves the wrapper on the start bank + 1. Outputs: -14 or -30 write a null pointer; later failures leave it unwritten; the optional halfword receives the skipped count.
- Append (`0x6ac7d4..0x6ac978`): query the current bank index first, then read latest. The destination is read latest's bank; inner append there takes its sequence from the bank's last physically written record (even a rejected one) plus one, masked to 15 bits, or, in an empty bank, the seed: read latest's sequence + 1 masked, or the caller's seed stored unmasked when read latest failed (bank 0 is then the destination). If inner append fails (bank full), the next destination is the queried index + 1 mod 2, not read latest's bank + 1: when they differ, the full newest bank is chosen and erased. Inner append there is tried; on failure the bank is erased, its result ignored, and inner append is tried again; a second failure returns -255. Caller bytes are copied after the destination search, so a source equal to the staging buffer is overwritten first.
- Erase all: erase each bank in current-bank order, ignore results, re-register; returns 0.
- Collect: read all records of each bank into the caller buffer (B × N × S bytes), mark unused records' sequence halfword `0xffff`, swap banks to order them by first sequence as an unsigned number, compact occupied records to the front; byte count output, -255 when it is zero.
- Read at position: addressed by kind, needs no registration; byte position up to `0x3ffff`; zero-fills the output on failure; writes only `out[0] = 0` when the bank is out of range.
- Registration: unopened service nulls the client word and returns -14; other failures leave it; five wrappers and five slots.
- Ignored results: every program result; erase results in append and erase all. Append returns 0 after a failed program and -255 when an erase failure leaves the destination full.
- Cache and lock: two 128 KiB read regions under semaphore `0x4b`, the only `0x4b` user in image4, held only inside the cached read; failed reads are cached as hits.

## 3. Design

### 3.1 Core crate

`replacement/record_store`, `no_std`, no allocation, no stock addresses; sans-IO, so host tests, the oracle candidate and the guest run the same code.

- Types: `RecordKind` (`Kind0`..`Kind4`); `Bank` (`Zero`, `One`); `Sequence(u16)` for stored sequence halfwords, with `successor()` masked to 15 bits; `Seed(u16)` for the caller's unmasked seed; `RecordBytes`, `RecordCount`, `SequenceOffset`, `ProgramBytes` (u16 newtypes); `Position(u16)`; `BytePosition(u32)` for read at position; `Geometry`, validated with the stock open-slot rules (RSC).
- `RecordMedium`: `read(kind, bank, offset, &mut [u8])`, `program(kind, bank, offset, &[u8])`, `erase(kind, bank)`, each `-> Result<(), MediumError>`; bank-local byte offsets.
- `Staging`: the per-kind result buffer the core writes on every seek, as stock does. In the guest it is the stock staging buffer; on the host a plain array.
- `Callbacks`: `check(&mut [u8]) -> CheckVerdict` (`Accept`, `Reject`) and `seal(&mut [u8])`, given the staging bytes.
- Results: `read_latest -> Latest { bank, position, sequence, skipped, record: &[u8] } | Empty | NoneValid`; `append -> Appended { bank, position, sequence, placement, persisted }` with `Placement` = `InBank`, `Rotated`, `ErasedThenWritten` and `Persisted` = `Confirmed` or `ProgramFailed(MediumError)`, or `Err(NotAppended::DestinationFull { erase: Option<MediumError> })`; `erase_all -> Erased { errors }`; `collect`; `read_at`. Stock statuses exist only in the adapter.
- The search and append sequences of §2 are the core's behaviour, not an adapter shim **[INFERENCE]**: the check-call sequence, the queried-bank rotation and the rejected-predecessor sequence are observable through callbacks and flash, and the clients are stock. When every client's check is owned and idempotent, a simpler search can replace them (shim row in §7).
- State per area: kind, geometry, callbacks, current bank. Everything else is read from the medium.
- `BlockBuffer` selects and fills one of two 128 KiB regions and copies requested bytes into staging as one operation. Cache tags, ages and timestamp persist across entries and record kinds. An invalid first slot wins before an invalid second slot; otherwise the smaller unsigned age wins, with ties selecting slot 0. A hit in both slots selects slot 1. The selected tag is installed before the raw read, failed fills remain cached, and each access assigns the wrapping timestamp to its age before incrementing the timestamp. Both tags are invalidated before each program and erase.

### 3.2 Retained boundary

- Staging buffers at the stock addresses, filled with `0xff` at open and written on every seek; read latest returns the stock address.
- Program requests with the kind's stock length from the staging buffer, so kind 3 sends `0x600` bytes.
- Callbacks called with the staging buffer and their context, at the stock points.
- Statuses and output writes as §2.
- Handles: owned area records sit at the stock wrapper addresses `0x89f8b4 + 12i`, so client words keep the same values and no handle map exists **[INFERENCE]**.
- Lock: semaphore `0x4b`, through `0x724b68`/`0x724c00`, covers shared tag selection, raw region fill and the copy to per-kind staging, matching RSC `0x6ad7b0..0x6ad988`. It also protects tag invalidation. No callback, raw program or erase runs with this semaphore held.

The earlier one-region cache and failed-read parity exclusion were design inferences, not separately approved differences. They are withdrawn. The cache preserves stock's two-region selection, age/wrap, failed-fill memoization and invalidation behavior. Callback order, caller-visible bytes/statuses, persistent effects and the full-system menu/settings comparisons remain exact; cache-policy changes cannot waive them.

## 4. Placement

- Carrier: the retired span. Nine fixed entry veneers split it; the largest free run is `[0x6acda4,0x6ae0fc)`, 4,952 bytes, with gaps of 112, 332, 76, 776, 428, 304 and 740 bytes and 24 at the end. The stager places each section in a named run and the build asserts each fits; if the core does not fit, it goes to the RAM-push bank region and the span holds veneers only.
- RAM: two owned blocks use `[0x8a0080,0x8c0080)` and `[0x8c0080,0x8e0080)`. Wrappers use `0x89f8b4 + 12i`, and owned registration state uses `[0x89f8f8,0x89fa60)`. Owned tags occupy `[0x8e0080,0x8e0088)`, timestamp `[0x8e0088,0x8e008c)` and ages `[0x89fa60,0x89fa68)`; open `0x6ac2b4` initializes tags to `0xffffffff` and ages/timestamp to zero. This reuses retired record-service RAM and adds 128 KiB plus 16 net metadata bytes over the previous carrier. The obsolete tag interval `[0x89f8f0,0x89f8f8)` and saved destinations `[0x917fa8,0x917fe4)` remain forbidden. Carrier admission binds exact extents/initialization and checks aliases, retained references and overlaps before runtime ownership.
- Retirement checks (spec/11): a reference scan of the three images for the span and the retired data; fetch traps on the retired code and access traps on the retired data during the cutover pair.
- Backend shim: region read `0x6aad94`, per-kind writes `0x6aab1c`, `0x6aa94c`, `0x6ab07c`, `0x6ab2f4`, `0x6aacb8` and erases `0x6aabd8`, `0x6aaa08`, `0x6ab1e0`, `0x6ab458`, `0x6aad40`.
- Route: the owned code is staged into a candidate DAT (image4), present from reset; the root task's open at `0x69d1d4` reaches the owned veneer, so no handover exists in qualification. A post-boot RAM-push install would need a handover shim (reading stock wrappers and slots once, quiescence: no task inside the span and `0x4b` free); it is out of this cutover.

Real two-slot staging produces `private-data/scratch/RecordStoreCarrier/two-slot-cache/candidate.DAT` and `contract.json` from the frozen binary/symbols and pinned function index under `private-data/scratch/RecordStoreCarrier/two-slot-inputs-1`. Admission reports zero resolved retired references and zero obsolete-gap references; unresolved raw aligned candidates remain in reference evidence and lifetime traps. These artifacts are distinct from the unchanged synchronized and original carriers. Staging establishes ownership/layout admission, not native or full-system qualification.

## 5. Native oracle (host qualification)

- Runner: `fujimod-xm1-native-case`, stock role (canonical span) and candidate role (owned code linked into the span) from the same cases.
- Executed as firmware bytes: the span, the raw service `0x6a8c10..0x6ab4c4`, the region read and per-kind functions, image4 helpers `0x722054`, `0x7221c8`, `0x89aa90`, `0x89abb8`, the loader divide helpers `0x4d978`/`0x4da6c` mapped from `loader-40000.bin`, and the stock check and prepare callbacks of kinds 2 and 3 (`0x6a6c54`, `0x6a6c04`, `0x6a84d4`, `0x6a8484`) in their real cases.
- Trampolines to the medium model: `0x6faf00`, `0x6faf40`, `0x6fb904`, `0x6fb7c8`, `0x6fb57c`. Register-only stubs returning 0: `0x724b68`, `0x724c00`, `0x42634`, `0x4263c`.
- Case memory: mode word `0x828574` = 1 and the remap at `0x828580`, the descriptor table `0x7c8c6c`, the staging buffers, wrapper and slot RAM, and callback stubs.
- Medium model, declared: keyed by pool-relative logical block; bit 30 cleared from buffer addresses; erase sets `0xff`; program ANDs bytes; a case may fail one request (status returned, no change) or stop after a given number of persistence steps.
- Expectations: an independent Haskell model written from RSC, and for kinds 2 and 3 the hardware-captured area-2 banks (`E-AREA2-BANKS-150`) and area-3 record (`E-AREA3-VALIDATOR-NATIVE-150`). The oracle passes when the model equals stock and the candidate equals stock on every case. Cases whose stock outcome is a run diagnostic (registration with kind 6 or more, which reads uninitialised locals) are excluded.
- Cases, per kind geometry unless noted:
  - service: open twice; entries on an unopened service and an inactive wrapper (-14); registration refusals; close and re-register; `0x6ae0fc`.
  - read latest: erased; partial; full; both full; wrap across `0x7fff`; newest rejected; all rejected; rejection at position 0; best at position N - 1; start bank not the newest; a scan that fails and the next scan's order.
  - append: bank with room; bank full (rotation); queried bank different from the newest bank with the newest bank full; destination full after a failed erase (-255); predecessor rejected by check; predecessor sequence `0x7fff`; rotation erase-then-write with newest sequence `0x7fff`; read-latest failure with a nonzero caller seed; source equal to the staging buffer; fresh open then kind-3 append (the `0x600` tail).
  - callbacks: a correcting check and a sealing prepare stub; the real kind-2 and kind-3 callbacks on hardware records, including single-bit correctable and uncorrectable area-3 records.
  - erase all, collect (empty, partial, both banks out of order), read at position at and past bounds.
  - faults: a failed program in each append path; a failed erase in rotation and erase all; interruption after each persistence step of an append, a rotation and an erase all, each followed by read latest on the medium left behind.
- Observations: r0, r4-r11 and sp at return; client words; every caller byte written; the returned pointer and its S bytes; the skipped-count halfword; each callback call with the bytes it received and changed; the ordered program and erase requests with block, offset, length and payload; the final medium. Not compared: read requests, wrapper and slot internals, cache state, and staging bytes except where they are a returned record, a callback input or a program payload.
- Must-fail controls, each a candidate with one fault, each detected by the cases named: (1) sequence not masked to 15 bits (predecessor `0x7fff`); (2) no rotation when the destination is full (bank full); (3) rotation to read latest's bank + 1 instead of the queried bank + 1 (queried bank different); (4) no step back when the newest record is rejected (newest rejected); (5) check verdict ignored (all rejected); (6) full-bank test N instead of N - 1 (bank full); (7) cross-bank choice by plain numeric order (wrap across `0x7fff`); (8) erase all skipping one bank (erase all); (9) program length cut to S (fresh open then kind-3 append).

The eight persistent `sequence-` cases append to the original 370 identities. Their medium contains only exercised logical blocks `0x52..0x57` (768 KiB); the rejected first fixture also materialized unused erased blocks through `0x69`, exceeding the native loader's 2 MiB artifact limit. The rejected inputs remain historical evidence.

`--stock-output` and `--candidate-output` admit saved native observations only when regenerated execution inputs match exactly, except for expected observations, and the saved runner SHA-256 matches the selected runner. Candidate admission therefore binds the linked code and declared functions. Both roles still pass through the normal model and role comparators. Output directories must not overlap either canonical cached root. An aggregate using admitted observations records the cached roots without relabeling the original native reports.

The two-slot native qualification admits and compares 378 cases from retained and fresh native executions with zero model or candidate differences in `private-data/scratch/RecordStoreOracle/two-slot-native-aggregate-4/summary.json` (SHA-256 `071715274e76d793460f72476e87982c782e66cfc6b3a877bd4ccabccacebefa`). Admission retains 368 original stock/candidate comparisons, two original stock observations with newly executed candidates (`hardware-kind3-byteflip` and `hardware-kind3-prepare`), and eight corrected sequences freshly executed in both roles. `two-slot-admitted-cache-4/provenance.json` binds the three oracle executables without rewriting their original reports. The candidate is `4d98efe58027d9af5e529ddd4fa6b1c2b047d3488b7844d18dda1b1395786d4d`; the runner is `68e968b14203cd74420f5eab5233a38972046719b89be780bb749a59462cc16a`.

The retained old single-slot candidate fails `sequence-ab-a-second-a-fails` in `two-slot-old-single-control-4/summary.json`. All nine original focused controls are detected in `two-slot-control-{1..9}-4/summary.json`, with no stock/model difference. Separate admission negatives reject a different candidate, a different runner and either cached root aliased as output. These host results do not qualify picker 246 or the full-system cutover pair.

## 6. Cutover pair

- One stock-versus-owned pair for the cutover, each role from a zero-instruction reset of its DAT (canonical and candidate), as `E-PROPERTY-SERVICE-NATURAL-150`: once with erased record areas and once with `--record-nand` holding the hardware area-2 banks.
- Routes: cold boot to the shooting screen (registrations, area-2 gate, area-3 recovery), the property-service routes that append area 3, CUSTOM SET and the defaults reset. No identified route reaches the kind 0, 1 or 4 writes or the queue-2 lifecycle save that appends area 2 (`E-ENDING-RESET-CALLER-150`); those are host-only until a route exists.
- Compared: ordered entry calls with arguments (handles at the same addresses) and results, program and erase requests at `0x6fb7c8`/`0x6fb57c`, returned records, final record-area blocks, settings images and menus; fetch and access traps on the retired span and data.
- Prerequisite: a bridge request that reads NAND pages, for the final record blocks.

### Versioned finite endpoint

The approved schema-2 observation contract is `finite-post-settle-prefix-v2`. It retains the ordinary pre-key idle and press settle, requires null press failure and either `packet-consumed` or `budget-exhausted`, advances exactly 833,334 board ticks, then reads one UI-state snapshot without advancing the guest. Both admitted statuses follow successful key injection and the SDK latch check. `packet-consumed` additionally certifies its render predicate; `budget-exhausted` does not. Neither is a direct application key-dispatch acknowledgement. `prior-render-pending` returns before injection and refuses.

The consumer validates the exact tick advance, unchanged snapshot instruction boundary and raw-view address/alias agreement. It compares current/previous/pending mode, request, transition latch, endpoint menu/node/depth and the original press menu. Record entries, ordered arguments/results/records, NAND effects, final committed medium, reset/retirement guards, route declarations and exclusions are unchanged; an interval ending inside a record operation refuses. The separately versioned route artifact is `emulation/record-store-pair-finite-v2-routes.json`. This finite observation does not claim quiescence or future stability and does not change the generic strict-drain API. Historical schema-1, P4 and strict-drain receipts do not qualify the schema-2 cohort.

The bridge must support both the record reset-capture/read/fetch-guard schema and read-only `ui-state`. The separately qualified P4 package lacks the record capture schema and refuses finite-v2 cohort 1 during preparation; those four refusals execute no guest and establish no pair result. The record-capable SDK retains its engine, capture codec, retirement guards and NAND APIs, with only the approved UI-state producer/RPC added. Its fresh package and all four role identities must be frozen before a new cohort; P4 or historical record receipts do not qualify that package.

### Fixture-bound intended IMAGE SIZE oracle

`emulation/record-store-image-size-pending-choice-v1.json` is a separate expected-behavior input, not a change to frozen execution routes. Its SHA-256 is `194a177cc7a07563bf34df31d07c822225898d59f29c82e901dc059e6bf3c390`. It binds only populated NAND `eb33ae15ec676f5dfd9465223500c89179af5c90f14047b4e528a7db741e8682` and the exact named Crawl 368/371 paths `ok,down,ok,down,up` and `ok,down,ok,down,back`. Other populated fixtures refuse. Erased roles retain the original finite comparison and make no intended-interaction claim.

The owner independently specifies L 3:2 at entry, DOWN selecting M 3:2, unconfirmed DISP/BACK restoring L 3:2 and MENU/OK confirming M 3:2. The owner also specifies responsive, wrapping IMAGE SIZE choices. These are intended-functionality requirements, not new camera observations. The two record-pair paths exercise a non-edge DOWN/UP and BACK cancellation; they do not themselves exercise a nine-row wrap edge.

The oracle validates parent node `0x78ea98`, ID 1, setting 5, cursor 3, depth 1; picker node `0x784014`, ID 1, setting `0xf6`, cursor 0, depth 2; and DOWN at picker cursor 1. Root entry leaves the compared settings unchanged. The pre-open current bank must equal its independently published backup. Entering the picker may change only ordinary property `0x161` from 0 to 1; DOWN may change only the source-mapped size selection. These preconditions reject a no-op DOWN, a wrong entry selection or any unrelated setting change.

Public IMAGE SIZE property `0xf6` is composite. Pinned item tokens begin 1, 2; lookup `0x7a00cc` yields `(0,1)`, `(0,2)`. The corresponding ordinary properties are `0xf6` at byte `0x6f`, bits 5..6, and `0xf9` at byte `0x7d`, bits 5..6. The first dimension stays 0 while ordinary `0xf9` changes 1 to 2. Byte `0x6e`, bits 5..6, is ordinary property `0x161`, not IMAGE SIZE itself. Public `0xf6` synchronization calls `0x641438(1,0)`; the captured entry writer `0x62ae3c` is independently decoded as the slot-1 property-`0x161` setter.

The native entry hook `0xd5b350(1,...)` publishes the backup through `0x637ed0(0)`. BACK from actual parent ID 1 follows the generic `0xd5b350` leave branch through `0x637ed0(1)`, `0x61c90c` and `0x61daa8(1)`, restoring the 450-byte bank before renderer notification `0x933`. The property-`0x161` rollback is classified separately through this source-backed leave effect. No renderer-`0x5053`/application-`0x53` equivalence or unobserved state-machine input is assumed.

Owned UP must match the validated picker-entry settings and both menu projections; owned BACK must match the source-backed pre-open settings and parent projections. The entry/DOWN guards exclude unrelated changes from this transaction. Stock may show the intended result or the exact saved DOWN settings and menu projections. Only the latter is reported as an `accepted-functional-difference`, never ordinary UI equality. Record calls, returned bytes, ordered NAND effects, committed media, all five source-mode fields and every other UI interval remain exact.

The checker rehashes the separately bound source evidence `private-data/scratch/RecordStorePair/image-size-intended-source-evidence-1.json`, SHA-256 `31669ec89e194b6c432567d7f8d040469add71c3c5ea5c8b63e1f8ce6d52d494`. Qualification attempts, compiler diagnostics and review packets remain in private evidence. Source review alone does not admit the policy or establish a full cutover pass. Original verdicts and the completed 378-case native evidence retain their execution identities.

### Current full-system boundary

The two-slot five-key diagnostic records a raw match after `ok,down,ok,down,up`: both roles retain picker 246 at cursor 1 and setting byte `0x96cce1` at `0x49`, including both mirrors. Its permissive checker compares menus, settings, returned records, NAND effects and terminal medium without differences. This raw result does not establish consumption of budget-exhausted keys. Evidence remains `private-data/scratch/RecordStorePair/two-slot-picker-{stock,owned}-4/receipt.json`; raw verdict `two-slot-picker-verdict-4.json` has SHA-256 `3a75d56d36769edebc58b759ccd623cde1841cd69dc8381311e05431fc86e179`.

The fresh finite-v2 cohort2 records all 263 steps and three attach phases for erased/populated stock/owned roles under source `0c24b950f4cf33fcc2e432dc5b4cf844969714e5` and bridge SHA-256 `4201d431782b869a8f9395d1e0e97501ba934d1136de30f95a5dc7d243c0f545`. Raw erased comparison is `matched`; raw populated comparison is `different` at step 16, with stock `0x49` versus owned `0x29` at offset 125 in all three settings images, including `0x96cce1`. Both press and endpoint menus show picker 246, cursor 1 versus 0. Step 17 is the BACK sibling restored from parent 15, not a continuation after UP: stock stays at picker 246/cursor 1, while owned reaches setting 5/cursor 3 and changes offset 110 from `0xa9` to `0x89`.

Saved completion replies show `budget-exhausted` at steps 15, 16, 17 and 170 in both erased roles and populated stock, and at step 170 in populated owned. All have null failure and positive key-hold instruction counts; none is `prior-render-pending`. These facts satisfy the finite injected-input boundary, not an every-render-completed contract. The retained trace does not capture an independent application key-dispatch acknowledgement. The populated UI difference remains unresolved by ordinary stock equality.

The packet-only checker experiment from `8e1521610df8f638db3bceb7eb45b89e1cc4f474` reconsumes both cohort2 pairs using their original frozen bindings. Both `finite-v2-{erased,populated}-admission-verdict-3.json` reports refuse with `application key consumption was not established: budget-exhausted` (SHA-256 `d2036dd4066db96ce5bb4a1c4f96b5ae263fead25d785ee64b2bdaf6823a54e8`). That interpretation strengthened the original finite contract into a render-completion requirement. The experiment and `finite-v2-input-admission-audit-2.json` (SHA-256 `2e13b5387f16190afc3ff5cd02177e4045fb48afd0bcd015cf411a1bbcbeadba`) remain historical evidence, not the current input-admission policy. No guest was rerun.

Offline comparison independently checks all 267 populated intervals: all 19 external record calls, statuses, compared output bytes, four append inputs and four ordered NAND descriptors/full payloads match. Program effects occur only in first-start and steps 38, 48 and 76, not the divergent siblings. Both populated terminal-medium files have SHA-256 `6953afc364aa8abdefa705b87ad3859d9f09b4a45304f35842e171183ca1fb73`; both erased files have `37232d165d2fd710cce238760decea7fc843d415ca54d0afcbf260dde8f445b0`. The complete record projection is `finite-v2-populated-record-projection-2.json` (SHA-256 `a8eb49f9524de07b43a0c3b4e709a1222e1183d33fc386d59cc82177fd3a5811`). Failed-latest pointer residues are preserved within each role under the existing contract; their unrelated preimages are not cross-role outputs.

Only steps 16 and 17 differ in the 263-step settings/menu/source-mode projection; both restore parent 15, and every before-settings snapshot matches. DOWN at step 15 writes `0x29` to `0x49` in both roles through retained PC `0x62907c`. UP at step 16 writes `0x49` to `0x29` only in owned through the same retained writer; stock has no changed-field write. Mirror copies use `0x722134`. The source-mode fields agree. These observations identify no captured record/persistence propagation and no exclusive timing cause. They do not prove future equality.

The historical schema-1 finite observer recorded all 239 steps for erased and populated stock/owned roles. Its raw record/NAND comparisons and populated pair compare equal. Its erased pair differs after `ok,down,ok,down,up`: retained setting `0x96cce1` is `0x49` in stock and `0x29` in owned; picker 246 remains at cursor 1 in stock and returns to cursor 0 in owned. This historical difference is not excluded or normalized.

The earlier synchronized single-slot carrier's separate strict-drain cohort includes the original routes and the natural CUSTOM RESET append route. All four roles save 169 steps, then refuse the 833,334-board-tick local-UI-drain bound at step 170. Neither cohort has terminal medium evidence, so both erased/populated pair-check invocations refuse. Its erased prefix still differs at step 16 in both the press-time and drained menu. Those refusals and the original finite-cohort mismatch remain historical evidence; the two-slot five-key match does not qualify the full matrix.

The stock instruction-phase controls before picker DOWN, including shifts of ±3,105,188 instructions, do not reproduce the owned continuation (`E-RECORD-STORE-PICKER246-CONTROL-150`). Matching aggregate instructions does not match raw board time. Completed stock/old-owned five-key diagnostics retain equal terminal NAND media. In the DOWN window, stock has 157 reservation pairs with no both-group success and pending word `0x17030d0` stays at 1. Owned returns group indices 0 and 1 on line 256 at raw tick `316723160`; pending advances to 2 at tick `316725068`, clears at tick `316725670`, and notification `0x816` follows. Subsequent UP writes `0x29` at retained PC `0x62907c`/tick `330535479` and returns cursor 0. Stock makes no UP write and remains cursor 1/`0x49`. The branches at `0x102f204..0x102f214` skip pending arm whenever either reservation returns `0xffff`; the callback inside `0x1029430` requires pending 2. These source-backed conditions and raw observations establish the continuation chain (`E-PICKER246-CHAIN-STALL-150`).

Reset-to-attach observation retains every original watched byte and adds pool/counter/pending writes within the unchanged eight-range cap. The first 245 ordered pool writes match in address, width, PC and value. Then stock writes group-1 status 3 to 5 at `0x104002c`/tick `258592883` before reserving the next group pair at tick `258712133`. Owned reserves that pair at tick `215123169` before the status-3-to-5 write at tick `215160709`. The status transition is byte-backed in `0x103fd88..0x1040068`, a NONMATCHING draft at x70view commit `55436fc85c0560dd252b0410af6f2427fe198cd3`; no fact depends on its inferred function name. Added observation leaves all hit addresses, registers and instruction counts unchanged, apart from corrected diagnostic labels, and preserves each role's five key results. This localizes an earlier retained-event interleaving difference. Its initial board/event-phase origin remains unproved; neither a record-store semantic defect nor a universal timing law is claimed.

Backend tracing observes 12 region reads and 15 NAND read entries in stock, versus 22 and 25 in old-owned. Both start with regions `0xd00000,0xd20000`; the third stock request advances to `0xa40000`, while old-owned rereads `0xd00000`. This reflects the previous one-region policy, whose inferred permission is withdrawn in §3.2. Persistent multi-kind native sequences now retain RAM/cache across operations and inject block/occurrence-specific read faults: read A, read B, reread A with the second physical A read failing without writing its destination, then compare caller-visible bytes and status. Additional cases cover first-fill failure memoization, bank revisit, eviction, age ties/wrap and callback order. Backend request differences alone neither exclude the observed menu/settings difference nor prove its exclusive cause.

The pair contract is a finite observation of the named routes, not global PLAY readiness. Any replacement endpoint must retain raw mode/latch evidence, the press-time menu, exact settings/effects/media comparisons and retirement guards. Picker 246 acceptance remains open; a different endpoint cannot erase the observed cursor difference.

## 7. Shims

| Shim | What it does | Removal condition |
| --- | --- | --- |
| Entry veneers at the nine stock addresses | Stock callers reach owned code | Every client calls the owned API: settings storage for kinds 2, 3 and 4 **[INFERENCE on kind 4]**, diagnostics for kinds 0 and 1, the image5 caller of `0x6ae0fc` |
| Stock statuses and ignored write results | Adapter reports what stock reports | Clients consume typed results |
| Staging buffers as result and callback storage | Records returned and checked at the stock buffer addresses | No client reads them |
| Stock search and append sequences | Check-call order, queried-bank rotation, rejected-predecessor sequence | Every client's check owned and idempotent; then a simpler search |
| Stock program lengths | Kind 3 programs `0x600` bytes | Owned NAND layer with a qualified partial-program law |
| Handles at stock wrapper addresses | Client words keep stock values | No stock client holds a handle |
| Stock callbacks | Check and prepare of the five clients | Each client's seal and check owned (kind 3's by `E-AREA3-VALIDATOR-REPLACEMENT-150`) |
| Raw service backend | Region read, per-kind writes and erases | Owned `NandFlash` and `FlashMap` |
| Semaphore `0x4b` | Lock around cache selection, fill, staging copy and invalidation | Owned RTOS |

## 8. Interface with settings storage

Agreed with Cleanup118 (property service):

- The store supplies `append(area, record) -> Result<Appended, NotAppended>`, whose `persisted` field separates a confirmed write from one whose program failed, `read_latest(area) -> Latest { record, .. } | Empty | NoneValid`, and `erase_all`.
- Settings storage owns record layout, field access, the previous-value snapshot (`0x89ec00`) and defaults, and the bank-0 backup mirror: open `0x61da3c` reads `[0x96cc64,0x96cc74)` from backup memory `0x409c281c` through `0x6a6e68`, its first 16 bytes from the shadow `0x409c29b8`; close `0x61d944` copies back through `0x6a6f3c`, and `0x6a6ddc` writes the backup checksum and resyncs the shadow. `0x6a6f3c` calls the area-3 commit `0x6a86c4` when its end offset passes `0x140` (`0x6a6fa8..0x6a6fb4`), which the header close's 16-byte copy never does; `0x6a86c4` takes its 40 payload bytes from `0x409c295c` and appends only a changed record. A copy-back over the custom record therefore drives the store's kind-3 append.

## 9. Increments

Contracts for all three come from RSC now; the work runs in parallel and composes as each passes its host checks.

1. Core crate, the independent Haskell model, host tests of the core against the model over the generated cases, and the nine controls on the host.
2. Native oracle harness and case generator (§5) with both roles.
3. Carrier staging into a candidate DAT (veneers, section placement, reference scan), and the bridge NAND page read.

The cutover pair (§6) runs once 1-3 compose.

## 10. Open questions

- Whether the core fits the span's runs or needs the bank region.
- Whether any client keeps a staging pointer across another read of the same kind.
- Routes for the kind 0, 1 and 4 writes and the area-2 lifecycle save.
