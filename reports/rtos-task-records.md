# Task-record create/delete root slots

This increment owns task create `+0x0` and task delete `+0x4` in both X-M1 1.50 loaders. It does not touch the S3 scheduler services, the global lock, or the eventflag, mailbox, data-queue and fixed-pool families.

## Source

- x70view `55436fc85c0560dd252b0410af6f2427fe198cd3`, `decomp/firmwares/xm1-150/kernel0/src/object/object_setup.c:118-156` (`kernel_create_task`) and `object_configuration.c:16-51` (`kernel_delete_task`); kernel1 is the same C with the relocated tables and `KERNEL_CORE`.
- Pinned disassembly `private-data/analysis/x-m1-1.50/decode/loader-40000.dis:13323-13455` (A create `0x4d094..0x4d1a4`, delete `0x4d1a4..0x4d2a8`) and `loader-d0000.dis:13307-13440` (B create `0xdd054..0xdd168`, delete `0xdd168..0xdd26c`).
- Manifest v6 contracts at slots `+0x0` and `+0x4`; wrappers `0x725dd8`, `0x72612c`, `0x7289b0`, `0x728bac`.

The bodies mask I and F through the stock pair A `0x42560`/`0x42568` (B `0xd2560`/`0xd2568`) and restore the saved bits. They ignore `r0[31:16]` and write the executing loader's own C/T tables and creation-id map. Create returns `-41` when `C+0x24` is set and `0` otherwise. Delete returns `0` on every stock path; `r1 == 1` resets C and T, any other value clears only `C+0x24`.

## Owned core

`replacement/rtos_task_records` implements the records allocation-free over a `Machine` seam. `TaskRequest` parses external priorities `1..=16` into the shared `rtos_layout::Priority` and refuses stack sizes whose rounding would wrap. `TaskSlot` is `1..=120`. The emitted adapter parses slot and delete mode, masks through the retained stock pair and calls the same typed operations as the host API. Create shares one deferred-request body: existence, dormant-state and binding checks precede request reads, then priority and size validation precede the remaining loads. Create and reset-delete share the definition writes and T reset/publication. The C/T records keep the stock layout, including `T+0x18/+0x1c`, `T+0x34` and the byte fields at `T+0xc..+0xe`.

## Named differences from stock

| Id | Stock | Owned | Trigger |
| --- | --- | --- | --- |
| `task-id-out-of-range` | indexes `uxth(r0)-1` and writes outside the tables | `-18`, no write | slot `0` or `>120` |
| `task-priority-out-of-range` | stores `priority-1` unchecked | `-18`, no write | priority `0` or `>16` |
| `task-stack-bytes-wrap` | rounds `(bytes+3)&~3` with wrapping | `-18`, no write | bytes above `0xffff_fffc` |
| `task-create-live-record` | rewrites a ready, blocked or suspended T without unlinking it | `-41`, no write | `C+0x24` clear and `T+0xc` nonzero |
| `task-create-stale-binding` | reads `T+0x8` after writing C and takes the stale record's priority | `-18`, no write | `T+0x8` is not this slot's C |
| `task-delete-live-record` | resets a ready, blocked or suspended T without unlinking it | `-41`, no write | `Reset` and `T+0xc` nonzero |
| `task-delete-missing-definition` | clears `C+0x24` regardless | `-41`, no write | `Clear` with `C+0x24` clear |
| `task-delete-stale-binding` | reads `T+0x8` and takes the stale record's priority | `-18`, no write | `Reset` and `T+0x8` is not this slot's C |
| `task-publish-exists-last` | sets `C+0x24` before the T fields | DMB, then `C+0x24` after the C/T writes | every successful create or reset |

`Clear` remains allowed on a live current task, which is the stock `0x72612c`/`0x728bac` path; it writes only `C+0x24` and does not touch T. A later create refuses that id until the task is dormant.

`Reset` accepts a withdrawn definition when the task is dormant and bound to that slot. It restores stock defaults without changing saved SP, resume words or pending priority. Live-state and stale-binding refusals still apply.

## Carrier

The standalone emitted demand includes each four-byte entry and the shared reset helper in the create group.

| Domain | Stock create bytes | Owned create group bytes | Stock delete bytes | Owned delete group bytes |
| --- | --- | --- | --- | --- |
| A | `0x110` | `0x168` | `0x104` | `0x108` |
| B | `0x114` | `0x168` | `0x104` | `0x108` |

The standalone artifact does not fit those stock extents and remains linked at native-only scratch `0x50000`/`0x50200`. The actual platform union supplies the grounded shared carrier, preserves the retained entry addresses and stages the complete task-record bodies in corrected DAT `5f639314db9ba160a12b907dc2f3bde4ae7c7902b93c531458617727a4e3fccb`. Aggregate qualification uses effective carrier slices extracted from those staged loaders, not the standalone scratch bodies or a body-only overlay.

## Verification

- Host: the 15 task-record cases and the integrated layout, semaphore and fixed-pool host suites pass.
- Each case checks the actual `nativeStatus`, caller SP, R4-R11, CPSR I/F and the declared C/T/map bytes. Equal cases compare stock and owned bytes; repair cases assert the stock effect and the owned no-write. Controls refuse a changed CPSR I or F, SP, R4, status, declared region, equal-region byte and publication barrier.

Native-fixture acceptance does not establish full-native, system, live-placement or hardware qualification.

The actual union's per-entry frame report records 48 bytes of owned create-path demand and 40 bytes for delete. All four roots retain explicit mask/restore source leaves: A `0x42560`/`0x42568`, B `0xd2560`/`0xd2568`, reached at depths 40 and 32 respectively. Composed closure is conditional on those leaves' stack, IRQ and ABI contracts and the caller stack's backing, headroom and live-byte ownership. `stage-frame-03/contract.json` does not establish unconditional live, device or concurrency ownership.

## Retained callers

- Create wrapper `0x725dd8` selects the task-domain root through `(&DAT_0096bd20)[class-1]` and calls `+0x0` at `0x725fa8` with the packed id and six-word stack request.
- Delete wrapper `0x72612c` selects the current-core root. Its current-task path uses `r1=0`; its remote path uses `r1=1`. `0x7289b0` uses reset after cancel/terminate, while `0x728bac` clears before exit.
- These wrappers hold the image4 critical pair and consume the stock `i32` result. A replacement entry branch must preserve LR, the arguments and the callee-save ABI.

## Current source integration

The task package from `agent/RtosTaskRecords` at `d2c8f0a5` is ported onto `3fe7a82d` without its historical SDK ancestry. The integration includes the independent native Suite, packaged System/SystemCheck model and CLIs, scheduler task-record semantics, publication boundaries, wrapper-owner guards and loader-header arming bindings. Current task-capture, NAND and logical-sample SDK support is retained unchanged.

The prior immutable qualification is bound by `private-data/scratch/RtosTaskRecords/natural-final-01/reset-source-fix-01/integration-packet.json`. Its native result is 66 comparisons and 496 controls, not a natural lifecycle qualification. Both retained natural captures still refuse `MissingLifecycle DomainA ResetDeleted`; the Reset source correction does not rebind or promote those observations.

Candidate plan emission requires the current composed candidate CLI to call `systemPlansForPackaged` using the proved platform stage and final candidate bytes. The historical prerequisite is `39dcabbe`; candidate composition itself belongs to its independent current source port.

Current host qualification passes all five selected tests: task-records host, task system, scheduler pair, scheduler semaphore and platform tests. The system-check, system-plan and native oracle CLIs and both platform binaries build in the same invocation `6dc63684-8693-44a4-9bb2-780059ff33b0`, bounded at 6 GiB RAM plus 2 GiB swap.

The newly built native oracle executes against immutable runner `/nix/store/j83w3gv3gyxab7g51ghxc68b24zpxan9-fujimod-xm1-native-0.1.0/bin/fujimod-xm1-native-case`: 66 comparisons, 496 controls and four emitted smokes pass with zero failures. Its receipt is `private-data/scratch/TaskCurrentPort/native-01/results.json`. The approved `75ce3d8c` admission wrapper runs that host-native qualification at 12 GiB RAM plus 8 GiB swap. No guest capture or SDK realization is part of this qualification.

