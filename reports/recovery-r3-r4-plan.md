# What R3 and R4 require on this body, and how recovery can be tested

This report defines general recovery-loader requirements and limits under [spec 10.1](../spec/04-recovery.md#101-evidence-levels) and [failure classification](../spec/04-recovery.md#103-required-failure-classification). It retains the X-M1 1.50 storage addresses, target checks, dry-run contract and non-destructive test alternatives. R2 and scoped R3 are demonstrated; the exact v5 rollback and reported ordinary operation repair the recorder failure, while formal R4 acceptance remains unclaimed. [Recovery status](recovery-status.md) owns present coverage and gate distinctions; [GitHub Issues](https://github.com/skeet70/latent-image/issues) owns open work.

## The central question, answered first

Do not deliberately brick the owner's only body to manufacture recovery evidence. Non-destructive staging, storage reads and target validation, bounded emulation and expendable-media rehearsal can exercise distinct parts of a recovery path. They cannot establish internal-media erase/program physics, silicon cache/reset behavior or arbitrary interrupted-update repair. The exact [v5 result](sd-boot-card-entry.md#260-physical-rollback-readback-status-20) is physical evidence for its single defined repair, not permission to induce another failure.

## What R3 requires

R3 is defined on top of R2 ("that path", spec 10.1), so the ordering is fixed: a bounded SD or RAM diagnostic through an application-independent early path first, then storage reachability and target identification with a dry run. The claim R3 would license is that a recovery loader can reach the storage it would need while writes stay unproved.

### Storage the staged payload must reach

The payload runs from the ROM's SD routes. Mode 1 stages `ffsys.bin` (a register-configuration image applied through `0x80000304`) and `ffboot.bin` to `0x40000000` before the jump to `0x00000000`, and mode 2 stages raw SD blocks from block 2 the same way ([window map](bootrom-window.md) sections 1 and 3, byte-read). The storage a repair concerns is the internal firmware media on the `0xfff52000`/`0xfff52800` controller (device select 1, 2 KiB units, 64 units per 128 KiB page group) and the SD card itself.

| Region | Media offset or span | Read-only check the payload can make |
| --- | --- | --- |
| Config block | `0x0` | marker words `55 aa ff 00 67 45 23 01 ef cd ab 89` at page-0 unit 0; load fields `+0x10` = `0x300`, `+0x14` = `0x11`, `+0x18` = `0x2` (window map section 9, byte-read) |
| Boot records | `0x40000` and `0x50000` | staged extents readable; vector and reset-entry sanity (window map section 9) |
| Segment table | `0x140000` | eight 36-byte entries, terminator `0xffffffff`, the size relations of [rebuild](update-container-rebuild.md) section 2.2 (byte-read) |
| Image records | `0x160000` (id 4), `0x2c0000` (id 5), plain windows per entries 6/7 at `0x5a0000`/`0xb40000` | record-header arithmetic: decoded size, stream length, chunk count |
| Trailer block `0x125` | `0x24a0000` | halfword `+0x1e` = `0xaaaa`; trailer words 0/1 equal main length minus `0x20` (UPDATE-01, byte-read) |
| Factory template | raw logical blocks `[7,10)` | template bytes readable and stable |
| Configuration and adjustment store | rewritten by the update's configuration completion through device index `0x29` over 33 blocks | body identity and version bytes; the comparison target for backup restoration (UPDATE-01, byte-read; the store's correspondence to media offsets stays unproved, remaining work 2) |
| Bad-block data | boot-record scan of `[0,0x13a)` | bad marks consistent with the cumulative-skip remap (UPDATE-01, byte-read) |
| SD card | the staging volume | the stock container file, read in full |

Reads through the mode-4 paged loader carry the compaction hazard: a failed page select inside `[0x40000, 0x80000)` shifts every later stage down 128 KiB while the load still returns success (E-BOOTROM-MODE4-150, byte-read). The payload should read with explicit unit-addressed block reads (`0x80000d08`) or model that skip, so its view of the media is positional rather than transfer-status-only.

### Identifying a valid stock update or restore target

A valid target is a `FWUP0002.DAT` that passes the container contract the stock updater enforces, paired with a media state a repair can actually fix. The field contract is decoded and host-verified ([rebuild](update-container-rebuild.md), 58 pinned-stock checks and a byte-for-byte round trip, E-UPDATE-REBUILD-150).

- Header type word 4; the 512-byte identity block passing the matcher `0xd0a2b4` against the body's current identity; the version words passing the format gate `0xd0a590` and the body policy at `0xd0a728`, which accepts an equal version for body selector 0 (byte-read).
- The three checksum words: the main complement-sum at raw `0x20c` over the complemented main range, and the two auxiliary complement-sums at raw `0x24c`/`0x258` (byte-read).
- Main length at most the selection-0 capacity `0x24c0000`; selector word 1; both auxiliary restore lists present, within their pair bounds (`off+len <= 0x10000` and `off+len <= 0x42000`) and equal to this body's stock pairs (E-ADJ-MOVERS-150, byte-read).
- Trailer words 0/1 tracking main length minus `0x20`, words 5/6 tracking the id-4 load and size, and the segment-table size relations (byte-read).
- On the media side, the installed record satisfying the ROM's own entry contract (marker, config fields, the six-unit `+0x404` accept condition) and the record-side gates (trailer halfword, segment lookup), each checkable read-only.

Two eligibility baselines have no early-path source. The matcher compares against an encoded configuration identity at `0x96b402`-`0x96b411` supplied by application code (`0x639b1c` → `0x73acfc` → `0x72b8c4` → `0x69f600`), and the current version comes from `0x69f520`, also application code (byte-read). An early payload must source both from persistent storage or state its own comparison basis; how the identity is encoded in the adjustment area is unmapped. An all-`0xff` first identity record returns matcher success (byte-read), which bounds the eligibility model but is not an installation proposal.

### The dry run

A dry run validates what a repair would do without writing anything.

- Write boundaries. The stock writer programs selection-0 logical blocks `[0,0x126)` at the `0x20000` stride, media offset equal to image offset, destroying the old trailer first and programming the trailer block last (UPDATE-01, byte-read and static-inference). The dry run computes this span from the validated container and checks it against the media it read.
- Exclusions. A repair must either replay the configuration completion's chain (template refresh from blocks `[7,10)` plus restore-list restore) or leave the configuration store alone. Which blocks those are cannot be stated today, because the writer's raw-block space versus its device-index space and any overlap between loaded segments and the configuration store are open items (UPDATE-01 remaining work 2). Closing them is part of the dry-run work.
- Prerequisites. The container checks above; this body's own calibration backup in two verified durable locations, since the container's restore lists preserve data across a successful update run and cannot resurrect bytes a failed install destroyed; the bad-block budget `0x14 - [0x828578]` and the 314-entry remap capacity (byte-read); and the read-back windows that would verify a repair afterwards.

## R3 reachability

The read primitives exist in decoded form. The ROM supplies internal-media routines (init `0x80000b74`, page select `0x80000c2c`, block read `0x80000d08`, paged loader `0x80000dec`) and the SD FAT stack (mount `0x80001ba0`, open `0x80001dc4`, read `0x80001e88`, block layer from `0x80001368`), all byte-read with their register maps ([window map](bootrom-window.md) sections 3, 4 and 8), and the container contract has a verified host parser. A payload can call the ROM routines at their fixed addresses or re-implement the MMIO sequences from the register map. Whether ROM code remains callable from the staged record is unproved, since the supplied boot records reprogram MPU and execution state themselves (UPDATE-01).

The bounded marker establishes R2 (E-SDBOOT-BLINK-WARM-OBSERVED-150). The later independent bootstrap reaches installed native storage, loads the relevant image, restores the recorder call in RAM and resumes to UI, demonstrating scoped R3 reachability (E-STAGED-RECOVERY-SUCCESS-150, [result](sd-boot-card-entry.md#237-entry-latch-recovery-reached-application-ui)). That does not implement every general stock-target check above. Mode-1 versus mode-2 selection, the successful warm switch edge and a reproducible cold-entry recipe remain unresolved.

## What R4 requires

R4 is a specifically defined failure repaired through this route, with verified data restoration and normal operation. The failure classes of spec 10.3 differ sharply in whether they can be induced safely and what their repair needs.

| Failure class (spec 10.3) | Candidate way back | Inducible safely on this body | Repair through the early route needs |
| --- | --- | --- | --- |
| Host process or USB session stalls | end session or ordinary restart | yes, by stopping a host process during transient-only work | nothing of the SD route; the class exercises incident handling only |
| RAM payload crashes | cold start discards volatile changes | yes, under the G4 preflight of a deliberately failing payload (spec 03) | nothing of the SD route |
| Application invalid, ROM entry survives | independent SD recovery | no; a persistent corruption of the only body, barred by spec 07 section 3.3 | staging (R2), media read and target identification (R3), and a write route: erase, program, verify, remap and marker discipline, plus the configuration completion or the backup restore |
| Calibration or EEPROM corrupted | body-specific restore through an independent route | no | a genuine persistent backup of this body's data (only the 1.00 adjustment area is preserved, E-S03-DURABLE-PRESERVATION, reported) and a validated writer |
| Boot metadata or controller initialization damaged | depends on ROM and board design | no, on any body | possibly external hardware; the mapped boot records halt rather than fall back (UPDATE-01), so ROM entry itself may be gone |
| Lens firmware or physical damage | separate repair | out of scope | excluded from body recovery (spec 10.3) |

An interrupted flash may leave a missing marker with old or mixed firmware (UPDATE-01 residues S1–S3); a bad container may leave a marker with unusable code. Repair evidence must bind the actual residue and physical target. The recorder-v2-v5 rollback repairs one known installed-block state; it does not establish repair of every page/block residue or remap shift.

## Exercising recovery short of bricking the body

Four routes exercise recovery without sacrificing the owner's body, and one vendor route needs no induced failure at all.

| Route | Proves | Does not prove | Cost |
| --- | --- | --- | --- |
| A. Emulation in the native runner | the software's decisions: FAT and staging logic, container validation, the payload's parser and report logic, the writer's block loop, remap rebuild and configuration completion against a modeled medium, with interruption injection at every persistence step | anything depending on the storage controller or real media: status semantics (the `+0x404` field, blank test, page-number decode), bad blocks, spare and window behavior, erase and program physics, power-loss residue, SD card init | harness work only; no camera time |
| B. Partial or reversible failure | the transient classes' own way back (restart, cold start) and incident handling | the SD repair; no failure class is both repair-relevant and reversible without the route | analysis only; the persistent variants need invasive runs |
| C. Staging without committing | entry, staging, execution and reporting on a healthy body; with the R3 additions, media read, target identification and the dry run; the repair pipeline run against expendable SD media | the payload-driven write to internal media | card preparation and a few reviewed runs |
| D. A sacrificial target | the full induce-and-repair cycle for the chosen class on hardware whose loss is acceptable | other residue classes; this body's own media specifics | hardware, owner authorization to acquire it (C7), that body's backup first |
| E. The vendor route (reported) | nothing here; it is a repair, not a test | our route entirely | ZJ02217 acquisition; it resets adjustment data (E-SD-BOOT-LEAD) |

### Route A

The native runner executed real 1.50 firmware code in the area-3 validator case and the film-sim offer-list cases (E-AREA3-VALIDATOR-NATIVE-150 and E-FILMSIM-EMUL-150, emulated) under a closed Cortex-R5F profile with exact input provenance and independently derived expectations. The recovery path decomposes the same way. The ROM's mode-1 FAT logic (mount, 8.3 open, cluster follow, file read) and the mode-2 block staging can run as stock-code closures over a synthetic SD image with the block layer replaced by declared stubs; the config apply at `0x80000304` can run as a capture of its register-write sequence compared against the decoded map; a staged payload's own parser, checksum and report logic can run whole; and the native writer's block loop, remap rebuild and configuration completion can run against a modeled media backend with an interruption injected at each step of UPDATE-01's persistence table. Container validation needs no runner and is already host-tested (185 synthetic contract assertions, E-UPDATE-REBUILD-150).

The runner maps no peripherals and fails closed on MMIO (the `unknown-mmio` fixture; the [harness README](../emulation/README.md) excludes exceptions, peripherals/MMIO, DMA and hardware timing). Every hardware-polling loop, the SD command issue and wait, the media controller's page select and acknowledge, and the DMA descriptor writes need a stub or a device model, and whatever a stub declares becomes an assumption the case cannot check. Emulation proves the software's branch behavior under declared device responses, including injected failures. It cannot prove device behavior, which is what the storage controller and real media contribute.

### Route B

The transient classes are enterable and leaveable without the SD route and exercise only their own way back. The configuration-residue class (S4) is the one persistent state whose next boot works (UPDATE-01), and it is left behind by a settings reset or an ordinary update, but entering it requires interrupting a real configuration commit, and its undo is partial: the updater's restore-list restore reads the snapshot taken at update time, so it cannot restore bytes a failed install already destroyed. The boot-affecting residues are by construction not leaveable by an ordinary update, because the updater runs from the application those residues prevent from loading. No class both exercises the SD repair and can be entered and left without it.

### Route C

The cardless documented-button tests are evidence-negative for their tested conditions (E-SD-BOOT-ENTRY-HOLD-150). Subsequent card-present warm execution establishes an early foothold and installed-storage reachability, but does not bind a controlled entry recipe or selected SD mode. Future no-write staging experiments would need separate owner authorization and a discriminating endpoint; the recovered body needs no repeated recovery attempt.

### Route D

C7 already describes this scenario: a spare identical body or an already-bricked compatible board, a defined corruption, required backups, what must survive, what is written and success measurements. It is the only route that exercises the repair act for real. It proves the chosen class on that hardware; it does not prove other residue classes, and each body's bad-block map is its own. Acquisition is the owner's decision (C7).

### Route E

The service manual's MAIN PWB Initialize (E-SD-BOOT-LEAD, reported) installs firmware onto a main board with no firmware, which is the limiting invalid-application state, so a documented repair for this class exists that needs no induced failure. It requires the service package ZJ02217 from Fujifilm's service server (unobtained), it resets the flash ROM, formats internal memory and resets adjustment data to defaults, and its behavior on a non-blank board with an invalid application is unestablished. It is the external fallback for a real failure and the answer to whether a bricked body is necessarily a dead camera.

### What the stock update already showed

The owner's stock 1.00-to-1.50 update exercised the native writer through the ordinary updater (E-STOCK-150-OWNER-REPORT). The exact v5 recovery later exercised the evidence-gated early physical-block writer and verified complete RC0 readback (E-RECOVERY-FRAGMENTED-V5-STATUS20-150). Neither observation establishes power-loss residue behavior or general calibration restoration.

## Judgement on a safe or sure test

Emulation and non-destructive staging cover software decisions, transfer boundaries, reporting, storage reads and target checks under declared models. Expendable media can rehearse a writer. No model validates erase/program physics or every interrupted-update residue on the owner's body. Sacrificial compatible hardware is an optional route to evidence for additional failure classes, not a prerequisite for the already demonstrated recorder repair.

## The capability to hold ready before a first install

A general recovery capability needs independently reachable entry, validated storage/target reads, precomputed write boundaries, the body's calibration in two verified durable locations, a reviewed writer and meaningful readback. The reusable [recovery kit](../recovery/README.md) instead supports the exact recorder-v2-v5 repair and host preparation/inspection commands. Preserve its successful card; do not advertise it as a full stock reflash or calibration restore.

## The one-body verdict and the options

On 2026-10-09 the owner chose explicit informed acceptance rather than deliberately corrupting his only body (RECOVERY-R4-PRACTICE-01). That decision does not authorize later operations. Persistent camera changes still need reviewed bytes, a named owner go and the stated residual risks under [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs). A real failure repaired through the route supplies evidence; formal gate acceptance must be recorded with its evidence scope in the relevant report and ledger.

The unavailable factory ZJ02217 package is not an input dependency for project-authored SD payloads. Its documented initialization clears adjustment data, and its behavior on a non-blank damaged board remains unproved. Acquiring that package or sacrificial hardware is the owner's decision.

## Attainability

R2 and scoped R3 are demonstrated. V5 physical rollback/readback and owner-reported ordinary operation close the recorder incident and recovery-adjacent checks. Controlled route/timing selection, complete persistent backup/restore coverage and additional failure-class repairs remain distinct requirements. Formal R4 acceptance remains unclaimed, and these findings authorize no new camera run.
