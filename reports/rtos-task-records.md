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
| `task-delete-missing-definition` | resets C/T or clears `C+0x24` regardless | `-41`, no write | `C+0x24` clear |
| `task-delete-stale-binding` | reads `T+0x8` and takes the stale record's priority | `-18`, no write | `Reset` and `T+0x8` is not this slot's C |
| `task-publish-exists-last` | sets `C+0x24` before the T fields | DMB, then `C+0x24` after the C/T writes | every successful create or reset |

`Clear` remains allowed on a live current task, which is the stock `0x72612c`/`0x728bac` path; it writes only `C+0x24` and does not touch T. A later create refuses that id until the task is dormant.

## Carrier

The current emitted demand includes each four-byte entry and the shared reset helper in the create group.

| Domain | Stock create bytes | Owned create group bytes | Stock delete bytes | Owned delete group bytes |
| --- | --- | --- | --- | --- |
| A | `0x110` | `0x168` | `0x104` | `0x108` |
| B | `0x114` | `0x168` | `0x104` | `0x108` |

The artifact does not fit those stock extents. It remains linked at native-only scratch `0x50000`/`0x50200`; no live address is admitted. An in-place cut needs 92 fewer bytes in A and 88 in B, or a grounded shared carrier preserving the retained entry addresses.

## Verification

- Host: the 15 task-record cases and the integrated layout, semaphore and fixed-pool host suites pass.
- Native: the existing `Emulation.CachedNativeBatch.cachedObservedBatch` oracle passes 31 cases per loader, 62 total and 466 controls, with zero failures. Current stock observations are reused and the changed owned emissions execute. Four emitted `smoke-create`/`smoke-delete` cases return and match across both domains. The same technical review cycle clears the shared typed implementation; evidence is `E-RTOS-TASK-RECORD-HOST-150` and `private-data/scratch/MainTaskRecords/review-packet.json`.
- Each case checks the actual `nativeStatus`, caller SP, R4-R11, CPSR I/F and the declared C/T/map bytes. Equal cases compare stock and owned bytes; repair cases assert the stock effect and the owned no-write. Controls refuse a changed CPSR I or F, SP, R4, status, declared region, equal-region byte and publication barrier.

Native-fixture acceptance does not establish full-native, system, live-placement or hardware qualification.

## Retained callers

- Create wrapper `0x725dd8` selects the task-domain root through `(&DAT_0096bd20)[class-1]` and calls `+0x0` at `0x725fa8` with the packed id and six-word stack request.
- Delete wrapper `0x72612c` selects the current-core root. Its current-task path uses `r1=0`; its remote path uses `r1=1`. `0x7289b0` uses reset after cancel/terminate, while `0x728bac` clears before exit.
- These wrappers hold the image4 critical pair and consume the stock `i32` result. A replacement entry branch must preserve LR, the arguments and the callee-save ABI.

