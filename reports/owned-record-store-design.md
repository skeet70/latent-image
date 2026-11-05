# Owned record store: design (storage first cutover)

Status: design, revision 2. Revision 1 had one review (RecordStoreDesignReview); its findings are folded in here. Under spec/11's review and rerun rules the next review is the cutover review at merge. Nothing here is built or qualified.

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
- Per-entry block buffer: within one entry the core keeps the last bank block read and drops it after any program or erase. Read budget: at most four block reads per read latest (three scan banks and the re-search) and five per append; stock reads at most two per entry from cold **[INFERENCE]**.

### 3.2 Retained boundary

- Staging buffers at the stock addresses, filled with `0xff` at open and written on every seek; read latest returns the stock address.
- Program requests with the kind's stock length from the staging buffer, so kind 3 sends `0x600` bytes.
- Callbacks called with the staging buffer and their context, at the stock points.
- Statuses and output writes as §2.
- Handles: owned area records sit at the stock wrapper addresses `0x89f8b4 + 12i`, so client words keep the same values and no handle map exists **[INFERENCE]**.
- Lock: semaphore `0x4b` taken only around each raw read, through `0x724b68`/`0x724c00`, as stock.

Accepted differences **[INFERENCE]**: no cross-entry cache, so read requests differ (more reads, and a failed read is retried instead of served as a cached hit). Neither changes a return value, a client byte or a persistent effect when reads succeed: stock invalidates its cache before every program and erase (`0x6ade6c..`), and no other writer touches the record blocks. The oracle does not compare read requests; a failed read is a fault case whose stock outcome the oracle records without requiring the candidate to match it, and the difference is listed in the cutover record.

## 4. Placement

- Carrier: the retired span. Nine fixed entry veneers split it; the largest free run is `[0x6acda4,0x6ae0fc)`, 4,952 bytes, with gaps of 112, 332, 76, 776, 428, 304 and 740 bytes and 24 at the end. The stager places each section in a named run and the build asserts each fits; if the core does not fit, it goes to the RAM-push bank region and the span holds veneers only.
- RAM: the per-entry block buffer (128 KiB) and the area records reuse retired data, the stock cache regions `[0x8a0080,0x8e0080)` and the wrapper array `0x89f8b4`; the slots `0x89f8f8`, cache tags `0x8e0080..0x8e008c` and `0x917fa8` are retired.
- Retirement checks (spec/11): a reference scan of the three images for the span and the retired data; fetch traps on the retired code and access traps on the retired data during the cutover pair.
- Backend shim: region read `0x6aad94`, per-kind writes `0x6aab1c`, `0x6aa94c`, `0x6ab07c`, `0x6ab2f4`, `0x6aacb8` and erases `0x6aabd8`, `0x6aaa08`, `0x6ab1e0`, `0x6ab458`, `0x6aad40`.
- Route: the owned code is staged into a candidate DAT (image4), present from reset; the root task's open at `0x69d1d4` reaches the owned veneer, so no handover exists in qualification. A post-boot RAM-push install would need a handover shim (reading stock wrappers and slots once, quiescence: no task inside the span and `0x4b` free); it is out of this cutover.

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

## 6. Cutover pair

- One stock-versus-owned pair for the cutover, each role from a zero-instruction reset of its DAT (canonical and candidate), as `E-PROPERTY-SERVICE-NATURAL-150`: once with erased record areas and once with `--record-nand` holding the hardware area-2 banks.
- Routes: cold boot to the shooting screen (registrations, area-2 gate, area-3 recovery), the property-service routes that append area 3, CUSTOM SET and the defaults reset. No identified route reaches the kind 0, 1 or 4 writes or the queue-2 lifecycle save that appends area 2 (`E-ENDING-RESET-CALLER-150`); those are host-only until a route exists.
- Compared: ordered entry calls with arguments (handles at the same addresses) and results, program and erase requests at `0x6fb7c8`/`0x6fb57c`, returned records, final record-area blocks, settings images and menus; fetch and access traps on the retired span and data.
- Prerequisite: a bridge request that reads NAND pages, for the final record blocks.

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
| Semaphore `0x4b` | Lock around raw reads | Owned RTOS |

## 8. Interface with settings storage

Agreed with Cleanup118 (property service):

- The store supplies `append(area, record) -> Result<Appended, NotAppended>`, whose `persisted` field separates a confirmed write from one whose program failed, `read_latest(area) -> Latest { record, .. } | Empty | NoneValid`, and `erase_all`.
- Settings storage owns record layout, field access, the previous-value snapshot (`0x89ec00`) and defaults, and the bank-0 backup mirror: open `0x61da3c` reads `[0x96cc64,0x96cc74)` from backup memory `0x409c281c` through `0x6a6e68`, its first 16 bytes from the shadow `0x409c29b8`; close `0x61d944` copies back through `0x6a6f3c`, `0x6a6ddc` writes the checksum and resyncs the shadow, and when offset + length passes `0x140` it calls the area-3 commit `0x6a86c4`, which takes its 40 payload bytes from `0x409c295c`. The mirror's copy-back therefore drives the store's kind-3 append.

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
