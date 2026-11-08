# Task-record create/delete root slots

This increment owns the two remaining stock-only root slots, task create `+0x0` and task delete `+0x4`, in both X-M1 1.50 loaders. It does not touch the S3 scheduler services, the global lock, or the eventflag, mailbox, data-queue and fixed-pool families.

## Source

- x70view `55436fc85c0560dd252b0410af6f2427fe198cd3`, `decomp/firmwares/xm1-150/kernel0/src/object/object_setup.c:118-156` (`kernel_create_task`) and `object_configuration.c:16-51` (`kernel_delete_task`); kernel1 is the same C with the relocated tables and `KERNEL_CORE`.
- Pinned disassembly `private-data/analysis/x-m1-1.50/decode/loader-40000.dis:13323-13455` (A create `0x4d094..0x4d1a4`, delete `0x4d1a4..0x4d2a8`) and `loader-d0000.dis:13307-13440` (B create `0xdd054..0xdd168`, delete `0xdd168..0xdd26c`).
- Manifest v6 contracts at slots `+0x0` and `+0x4`; wrappers `0x725dd8`, `0x72612c`, `0x7289b0`, `0x728bac`.

The bodies mask I and F through the stock pair A `0x42560`/`0x42568` (B `0xd2560`/`0xd2568`) and restore the saved bits. They ignore `r0[31:16]` and write the executing loader's own C/T tables and creation-id map. Create returns `-41` when `C+0x24` is set and `0` otherwise. Delete returns `0` on every stock path; `r1 == 1` resets C and T, any other value clears only `C+0x24`.

## Owned core

`replacement/rtos_task_records` implements the records allocation-free over a `Machine` seam. `TaskRequest` validates the external priority `1..=16` and stack bytes whose rounding would wrap. `TaskSlot` is `1..=120`. `create` and `delete` mask through the retained stock pair; `create_at_status` and `delete_at_status` are the narrow ABI paths. `reset_task` is the shared T reset and publication, placed in the create carrier for the native artifact. The C/T records keep the stock layout, including `T+0x18/+0x1c`, `T+0x34` and the byte fields at `T+0xc..+0xe`.

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

The stock extents are `0x110` bytes for A create, `0x114` for B create and `0x104` for each delete. The owned A create body plus the shared reset helper is `0x174` bytes and the delete body is `0x108`; B is the same modulo the extra create instruction. They do not fit. The emitted artifact is therefore linked at the native-only scratch `0x50000`/`0x50200` and is not a live placement. A live cutover needs a larger proven carrier or a reduction of at least 100 create bytes and 8 delete bytes.

## Verification

- Host: 15 `rtos_task_records` cases and 19 `rtos_layout` cases pass; the layout additions are `TaskDefinition` fields, `TaskCreationId`, `DefaultStackWord` and the A/B table words.
- Native: the task-record oracle uses `Emulation.CachedNativeBatch.cachedObservedBatch` with current input admission and comparison. It runs 31 cases per loader, 62 cases total and 466 controls, with 0 failures. The `smoke-create` and `smoke-delete` cases execute the emitted entries in both domains and match.
- Each case checks the actual `nativeStatus`, caller SP, R4-R11, CPSR I/F and the declared C/T/map bytes. Equal cases compare stock and owned bytes; repair cases assert the stock effect and the owned no-write. Controls refuse a changed CPSR I or F, SP, R4, status, declared region, equal-region byte and publication barrier.
