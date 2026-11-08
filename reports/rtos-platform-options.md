# RTOS platform options for #156: µITRON 4.0 fit, arewertosyet.com survey and base selection

This report answers whether projects listed at [arewertosyet.com](https://arewertosyet.com/) help the [RTOS platform project](rtos-platform-manifest-draft.md), tests the µITRON 4.0 hypothesis against the recovered service manifest, and records the base selection with its feasibility checks and the first build trial (§4.11). X-M1 claims cite repository evidence: **Draft** = [the RTOS manifest draft](rtos-platform-manifest-draft.md), **Manifest** = `private-data/scratch/RtosManifest/manifest.json`, **Map A** = [subsystem map section A](subsystem-replacement-map.md#a-kernelrtos-platform-services), **Inventory** = [the replacement inventory](replacement-inventory-baseline.md) §§8.8-8.17. External claims carry URLs, checked on 2026-11-02. **[INFERENCE]** marks anything no cited source states directly. Evidence from other Fujifilm models (fffw wiki and headers, NORTi) is labelled **non-X-M1** and is used only as a hypothesis source.

## 1. Constraints from the repository

### 1.1 CPU, domains and memory

- Two A32 cores, each with its own kernel domain and root: root A `0x4004dae0`, root B `0x400ddaa0`, published at `0x96bd20/0x96bd24` [Draft:11; Map A.1].
- Most image4 wrappers select the root for the current core through root0 `+0x1618`. This returns 0 exactly when the CP14 discriminator is `0x4003`, otherwise 1. Task creation instead selects the root by descriptor class: class1 is A, class2 is B. Code location, task class and executing core are three separate identities [Map A.1].
- Cross-core facts that are established: shared locks built from LDREX/STREX, a startup completion store with DSB, activation notifying the peer through `0x433e4/0xd33e4`, and private TCM with banked state [Draft:59-61; Map A.4]. Physical coherence, notification registers and lost-wake races are open [Draft:59-61].
- Rust toolchain: the pinned rustc 1.98.1 targets `armv7r-none-eabi` with `-C target-cpu=cortex-r4f`. It builds `core` and `compiler_builtins` itself and compiles one crate root per Bazel macro, with no registry or network [`replacement/toolchain/README.md:3,20-28`]. **[INFERENCE]** Any third-party crate or C kernel has to be vendored into the Bazel graph. `docs/principles.md:29` forbids parallel CMake/Cargo-style builds, which rules out west/CMake and cargo build scripts as the build of record.

### 1.2 Service ABI that retained callers keep using

- Each root has **97 callable slots**: 93 at `0..0x170` plus four extensions at `+0x1618..+0x1624`. It also holds a tick pointer at `+0x174`, 120 eleven-word descriptors from `+0x178` and a stack-top word at `+0x1628`, for a total of 1,419 words [Draft:11; Map A.1].
- Manifest v7 (E-RTOS-SLOT-MANIFEST-V5-150) recovers all 97 contracts per domain from the x70view decomp's kernel C (§4.13), including the mailbox pointer preconditions in §4.14. Extents: 80 derived by StockInventory, 15 taken from byte-exact decomp functions, and 2 (`+0xc0`, `+0xd4`) still refused ([manifest draft](rtos-platform-manifest-draft.md#v6-contracts-from-the-decomp)).
- Retained code depends on more than the slots [Draft:37-46]:
  - it calls the scheduler and CPU helpers directly;
  - auto-ready `0x43f9c`, initial scheduler `0x43688`, trampolines `0x43728` and resume through `SPSR/LDM^` are direct entries;
  - it reads raw C (40-byte) and T (56-byte) records through root descriptors.
- A confirmed external raw-layout consumer is the image4 stack check `0x725b74..0x725c6c`, called from periodic callback `0x69cc18`. It reads C+8 and C+20 through `root+0x184/0x190+44*(id-1)`, compares the task-ID fill pattern and reports `-64` [Draft:46].
- Tick-pointer consumers sit at `0x7257a8..0x725940`. The elapsed adapter uses ten times the wrapping tick difference [Draft:48].

### 1.3 Error codes and objects

| Value | X-M1 meaning | Where | Source |
| --- | --- | --- | --- |
| -18 | invalid signed class/id/domain selection | activate `+0xc` | Manifest A/B `0xc`; Draft:24 |
| -25 | nonzero context or IRQ masked | activate `+0xc` and the other task-context services (manifest v6) | Manifest; Draft:24,29 |
| -41 | C+36 already nonzero / eventflag record +8 already nonzero | create `+0`, eventflag create `+0x80` | Manifest; Draft:23 |
| -42 | creation record absent | activate | Manifest |
| -43 | already pending activation | activate | Manifest |
| -50 | finite queue full/empty, flag-wait wrapper special case | `+0x9c`, `+0xac`, `+0xb0`, `+0xc8` | Manifest; Draft:28 |
| -63 | cleanup `0x72612c(0)` result if lower operations return | image4 | Map A.1; Inventory:1633 |
| -64 | stack-check failure | image4 `0x725b74` path | Draft:46; Inventory:1635 |
| -60, -62 | message-wrapper returns; sibling `os_error.h` names them MBX_INVALID_LENGTH and MBX_FULL (non-X-M1) | image4 message wrappers | `E-ITRON-VOCAB-150` |
| -1 | try-lock `0x427d4` / unlock `0x427fc` | loader helpers | Map A.1 |

Objects are 120 C/T task records per kernel, eventflag records (12 B) and control blocks (16 B), scheduler domains (132 B), ready lists and bitmaps, resources/semaphores (97 constructed by root104), mailbox/message objects with pools, and timer records of 24 bytes [Map A.2; Draft:23-29,43; E-RTOS-SLOT-MANIFEST-V5-150].

### 1.4 IRQ, timer and blocking behaviour

- IRQ entries are `0x40204/0xd0204`. The pinned emulator model configures controllers at `0xfffb3000/0xfffb4000` with 128 sources and active-bitmap arbitration. Physical routing, EOI and nesting are not qualified [Draft:40,56].
- The repository does not establish whether the controller is an ARM GIC. **[INFERENCE]** That question decides several candidates below.
- Timer: the loader's elapsed value is reload minus count from `0xfff91004/0xfff91000`. Divider, wrap and deadlines are open [Draft:57].
- Stock tasks block inside kernel waits on their own stacks. Examples: B93 waits on flag `0x724480(1,&out,-1)` and is observed reaching that block [Draft:27,58]; take `+0x7c` and receive `+0xc8` can block [Manifest]. T nodes save SP/PC, and resume goes through an exception-return frame [Draft:37-40].

### 1.5 Publication

- Recovered interfaces, layouts and distilled findings are publishable. Proprietary code, instruction sequences and tables are not [`docs/principles.md:66-70`; `spec/11-incremental-firmware-replacement.md:31`]. Original Latent Image code is AGPL-3.0-only, and upstream notices must be kept [`docs/principles.md:74`].
- Consequences for this survey:
  - Reused third-party code must be AGPL-combinable.
  - A µITRON-shaped interface written from recovered contracts is publishable.
  - NORTi documentation can inform the work but cannot be redistributed.

## 2. µITRON 4.0 hypothesis tested against X-M1 evidence

External oracles:

- µITRON 4.0 specification Ver. 4.00.00, English, from ERTL: https://www.ertl.jp/ITRON/SPEC/FILE/mitron-400e.pdf, landing page https://www.ertl.jp/ITRON/SPEC/mitron4-e.html. The TRON Forum Ver. 4.03.00 PDF (https://www.tron.org/wp-content/themes/dp-magjam/pdf/specifications/en_US/TEF024-S001-04.03.00_en.pdf) returned HTTP 500 to automated fetches.
- MiSPO NORTi Version 4 User's Guide: http://www.mispo.co.jp/document/no4guide.pdf.

Leads (non-X-M1):

- Discord notes from Daniel Cook and tiredboffin say the kernel is ITRON but not fully spec-compliant.
- fffw wiki: EXR-family cameras (dual Cortex-R4F, 2010-2016) run "an unidentified dual-kernel µITRON-like real-time operating system", and "the FF RTOS API was ported to this new dual-processor RTOS while keeping the same API". The earlier in-house ARM platform ran NORTi under the same FF RTOS API (https://github.com/tiredboffin/fffw/wiki/Fujifilm-Camera-History-%E2%80%90--RE-perspective#classical-antiquity-2010-2016).
- The EXR Processor page lists EXR II as "Dual Cortex-R4F r1p4" (FF4227). It says firmware "strongly suggests ... simultaneous execution on two ARM CPUs, each running its own instance of the RTOS kernel", and notes that dual-processor µITRON on shared-memory Cortex-R4 is not standard (https://github.com/tiredboffin/fffw/wiki/EXR-Processor-Family). The page offers no further detail on the dual-kernel design. These are other-model observations. The X-M1 facts in §1.1 are what bind us.

### 2.1 Constants and argument shapes: exact matches

| X-M1 evidence | µITRON 4.0 | NORTi 4 |
| --- | --- | --- |
| -18, -25, -41, -42, -43, -50 with the meanings in §1.3 | E_ID −18, E_CTX −25, E_OBJ −41 (cre_tsk: "task is already registered"), E_NOEXS −42, E_QOVR −43 (act_tsk: "overflow of activate request queuing count"), E_TMOUT −50 ("polling failure or timeout") [spec §7.8 and act_tsk/cre_tsk pages, mitron-400e.pdf] | same values [no4guide.pdf §7.1] |
| create `+0` packet `{2,arg,entry,priority,size,base}` [Draft:23] | T_CTSK `{tskatr, exinf, task, itskpri, stksz, stk}`; TA_ACT = 0x02 "Task is activated after the creation" [spec cre_tsk; §7 constants] | same packet [no4guide.pdf T_CTSK] |
| creation does not make tasks runnable; a separate auto-ready scan of 120 records builds ready sets [Draft:23,37] | consistent with TA_ACT handling for statically created tasks **[INFERENCE]** | |
| activate id0 selects the current task [Manifest `0xc`] | TSK_SELF = 0 [spec act_tsk] | |
| resource create `{0,1,1}` [Draft:26] | T_CSEM `{sematr=TA_TFIFO 0, isemcnt 1, maxsem 1}` [spec cre_sem] | |
| eventflag create `+0x80` `{attribute 0 or 4, pattern 0}`, duplicate -41; attribute 4 clears the whole word [Manifest v6 `0x80`; Draft:27] | T_CFLG `{flgatr, iflgptn}`; TA_WSGL 0x00, TA_CLR 0x04; cre_flg E_OBJ [spec] | |
| flag wait mode0 tests all requested bits and returns the whole flag word; timeout −1 [Draft:27; Manifest `0x9c`] | TWF_ANDW 0x00; p_flgptn receives the pattern; TMO_FEVR −1 [spec] | |
| timer activation enables records whose flag bit1 is set [Draft:43] | TA_STA 0x02, cyclic handler operational after creation [spec] **[INFERENCE]** match | |
| IRQ save returns the prior mask, restore takes it [Manifest `0x164/0x168`] | none: loc_cpu/unl_cpu take no mask | `vdis_psw()` returns the PSW, `vset_psw(psw)` restores it [no4guide.pdf] |

### 2.2 Slot order matches µITRON 4.0 chapter order

**[INFERENCE]** The table at `0..0x170` appears to follow µITRON 4.0 chapter order, one slot per service call, with `acre_*` and `ref_*` omitted. Every partially recovered slot between `0x64` and `0x154` lands at the predicted position:

| Offsets | Predicted sequence | Anchors from the Manifest | Unknown slots and tier |
| --- | --- | --- | --- |
| `0x64-0x7c` (7) | cre_sem, del_sem, sig_sem, isig_sem, wai_sem, pol_sem, twai_sem | `0x64` create, `0x6c` give-task, `0x70` give-contextual, `0x7c` take+timeout | `0x68, 0x74, 0x78`: high |
| `0x80-0x9c` (8) | cre_flg, del_flg, set_flg, iset_flg, clr_flg, wai_flg, pol_flg, twai_flg | `0x80` T_CFLG-shaped register, `0x88/0x8c` set task/interrupt, `0x9c` wait+timeout | `0x84, 0x90, 0x94, 0x98`: high |
| `0xa0-0xc8` (11) | cre_dtq, del_dtq, snd_dtq, psnd_dtq, ipsnd_dtq, tsnd_dtq, fsnd_dtq, ifsnd_dtq, rcv_dtq, prcv_dtq, trcv_dtq | `0xa0` create, `0xa4` failure cleanup, `0xac` task submit returning -50 when full (polling send), `0xb0` contextual submit, `0xc8` receive+timeout writing a pointer word | `0xa8, 0xb4-0xc4`: high |
| `0xcc-0xe0` (6) | cre_mbx, del_mbx, snd_mbx, rcv_mbx, prcv_mbx, trcv_mbx | none; `0xcc/0xd0` bodies sit in the create/delete address cluster (below) | all six: medium |
| `0xe4-0xf8` (6) | cre_mpf, del_mpf, get_mpf, pget_mpf, tget_mpf, rel_mpf | `0xe4` pool create, `0xf8` buffer release | `0xe8-0xf4`: high |
| `0xfc-0x104` (3) | set_tim, get_tim, isig_tim | none | medium |
| `0x108-0x114` (4) | cre_cyc, del_cyc, sta_cyc, stp_cyc | `0x108/0x10c` in the create/delete cluster; static timer activation `0x4bf3c` lies between the `0x100` body `0x4bdc4` and the `0x110` body `0x4bfcc` | medium |
| `0x118, 0x11c` (2) | unresolved (alarm or overrun calls?) | none | low |
| `0x120-0x154` (14) | rot_rdq, irot_rdq, get_tid, iget_tid, loc_cpu, iloc_cpu, unl_cpu, iunl_cpu, dis_dsp, ena_dsp, sns_ctx, sns_loc, sns_dsp, sns_dpn | `0x148` context selector (sns_ctx). `0x128` target `0x4c620` is the body the `0x16c` current-task query calls (`0x4118c -> 0x4c620`, Map A.1) | 13 unknown: high |
| `0x158-0x160` (3) | interrupt management, order unresolved (dis_int/ena_int candidates for 28-byte `0x158/0x15c`; cre_isr or def_inh for `0x160`, which is in the create cluster) | none | low |
| `0x0-0x60` (25) | task management, task-dependent sync and task exception calls; the order departs from the spec (activate at index 3, delay at index 19) | `0x0` create, `0xc` activate, `0x4c` delay | `0x4`: medium (create cluster); 21 others: low |

Supporting structure [Manifest targets]:

- Every create/delete candidate pair lives in one address cluster: `0x0/0x4`, `0x64/0x68`, `0x80/0x84`, `0xa0/0xa4`, `0xcc/0xd0`, `0xe4/0xe8`, `0x108/0x10c`, `0x160` at A `0x4d094..0x4d8fc`. The other services lie in ascending address order by family. This fits a separate dynamic-creation module **[INFERENCE]**.
- For all 17 slots checked, B targets equal A targets plus `0x8ffc0`, `0x8ffc4` or `0x90000`. **[INFERENCE]** B is the same kernel build relocated, so one recovery covers both domains.

Tier counts for the 70 unknown slots per domain: **30 high** (one named candidate between anchors, in a family whose call count exactly fills the gap), **14 medium** (named candidate, consistent position and address cluster, no anchor inside the family), **26 low** (21 task-region slots, `0x118`, `0x11c`, `0x158-0x160`).

### 2.3 Where X-M1 departs from µITRON 4.0 and NORTi

1. **Encoded IDs and dual domains.** IDs are `(class<<16)|id`, the kernel rejects a bad class with -18, and there are two roots plus extensions `0x1618..0x1624` (identity, and the single-try acquire, release and observe of a lock word) [Draft:23-24,29; manifest v6]. µITRON 4.0 is uniprocessor. Multiprocessor µITRON exists only as later extensions such as TOPPERS FDMP/FMP (https://www.toppers.jp/fdmp-kernel.html, https://www.toppers.jp/fmp-kernel.html).
2. **Current-task query.** `+0x16c` takes no argument and returns the running task's id, or 0 in handler context or with IRQ masked (manifest v6). µITRON get_tid returns TSK_NONE (= 0) from non-task contexts [spec get_tid]. NORTi `vget_tid()` returns the ID directly [no4guide.pdf].
3. **IRQ save/restore with mask** (`0x164/0x168`) follows the NORTi `vdis_psw/vset_psw` shape, not µITRON 4.0.
4. **Non-standard slots and layout.** `0x170` buffer acquire sits outside the family order [Manifest]. The tick pointer, 120 raw descriptors and stack-top word inside the root, and their external raw-layout readers, are not part of any µITRON API [Draft:46-48].
5. **Critical pair.** The pair keeps a private prior mask in r10. Its lock is the global word 0x400c0804 that both cores' IRQ vectors also hold (§4.13); `+0x161c` tries once and returns -1 when the word is held [Draft:59; Map A.1].
6. **Task-region order** differs from the spec order (§2.2). `ref_*` and `acre_*` are absent throughout, so retained callers cannot use them.
7. **Codes -60, -62, -63, -64 and -1** are outside the µITRON 4.0 error list (−5..−58) [spec §7.8] and the NORTi list [no4guide.pdf §7.1]. They occur in image4 wrappers or loader helpers, not root services (§1.3).

### 2.4 Kernel layer or FF RTOS API layer

**[INFERENCE]** The 97 root slots are the **kernel layer**: µITRON-ordered calls with µITRON constants, NORTi-like extensions and dual-core additions.

- Root targets are loader-resident (`0x41108..0x4d8fc`) [Map A.2].
- The **FF RTOS API layer** is the set of image4 wrappers that call through the current-core root: `0x725dd8` submit, `0x7247ac` resource create, `0x724cdc` message constructor, `0x72503c` send, `0x725318` receive, `0x724480` flag wait, `0x723ce8` delay and `0x723cb8` current task [Map A.1; Draft:25-28].
- Those wrappers add FF-style policy:
  - a composite "message" built from a data queue (fits the `0xa0` family), a fixed pool (`0xe4`) and handle arrays;
  - a send callback before inputs reload;
  - special-casing of 0 and -50, with logging or callback on other failures;
  - identity 0 on failure;
  - their own codes -63 and -64 [Map A.1; Draft:28].
- `E-ITRON-VOCAB-150` already matched the image4 wrappers' error-log codes to the sibling fffw `object_id.h` FFOS names (TASK_CRE, FLAG_WAI_OR, SEM_WAIT, SEM_PUT, EVT_SEND, MSG_RCV, CRE_MPF, HEAP_MEM, CRE_DTQ) and four wrapper returns to sibling `os_error.h` codes (-0x32 OS_ERR_TIMEOUT, -0x29 OS_ERR_OBJ_TYPE, -0x3c, -0x3e) [docs/evidence.jsonl `E-ITRON-VOCAB-150`]. Those names are non-X-M1 leads. CRE_MPF and CRE_DTQ at the message constructor agree with the data-queue plus fixed-pool reading of the `0xa0` and `0xe4` families above **[INFERENCE]**.
- This matches the fffw description of an FF API over a µITRON-like kernel. That description is non-X-M1; the X-M1 evidence above is what supports the split.

### 2.5 Effect on recovering the 70 unknown slots

- The manifest's rule "Conventional ITRON names or sibling-model headers do not fill these gaps" [Draft:30] still holds. A µITRON name can enter the manifest only as a hypothesis field, and becomes `recovered` only on X-M1 body evidence.
- What changes is the method. For 44 slots (30 high, 14 medium) the question becomes whether a slot matches a named call, and the spec page for that call supplies the signature, error set and negative cases to test. For the 26 low slots, the candidate set drops to roughly 30 named calls (task management, task sync, task exception, alarm, overrun, interrupt).
- **[INFERENCE] Order of work:**
  1. Admit the status-register instructions that cause 68 of the 74 extent refusals [Draft:17]. Nearly every µITRON service masks interrupts, so few unknown bodies can be derived until this is done.
  2. Test distinguishing predictions on the high tier:
     - `0x78` (pol_sem) returns -50 without blocking when the count is 0;
     - `0x68/0x84/0xe8` (deletes) return -42 for absent IDs and release waiters with E_DLT (−51);
     - `0x90` (clr_flg) ANDs the pattern;
     - `0xf0` (pget_mpf) returns -50 on an empty pool;
     - `0x130/0x138` (loc/unl_cpu) change the CPU-lock state that `0x14c` (sns_loc) reports;
     - `0x12c` (iget_tid) works from interrupt context.
  3. Medium tier: mailbox, time and cyclic families.
  4. Task region last, by body features: blocking versus non-blocking, i-variant context checks, self-only calls.
- Expect deviations at each step (§2.3). The spec is a test generator, not an oracle of record.

## 3. Projects on arewertosyet.com, plus TOPPERS and other µITRON kernels

AGPL column: MIT, Apache-2.0 and BSD combine with GPLv3-family works (https://www.gnu.org/licenses/license-list.html#apache2). MPL-2.0 is file-level copyleft and GPL-compatible (https://www.gnu.org/licenses/license-list.html#MPL-2.0). AGPLv3 §13 permits combining with GPLv3 (https://www.gnu.org/licenses/agpl-3.0.html). Shim column: whether the project supplies preemptive threads that can block in a kernel wait on their own stacks (§4.1).

### 3.1 Active list (https://arewertosyet.com/)

| Project | ARMv7-R / multicore | Model and objects | License / AGPL | Maintenance | Fit | Shim threads |
| --- | --- | --- | --- | --- | --- | --- |
| Ariel OS | no ARMv7-R port; thread arch ports are cortex_m, riscv, xtensa, native (https://github.com/ariel-os/ariel-os/tree/main/src/ariel-os-threads/src/arch); SMP on RP2040/RP235x; AMP not documented (https://raw.githubusercontent.com/ariel-os/ariel-os/main/book/src/multithreading.md) | tickless preemptive priority threads **and** embassy executors; threads spawnable at runtime with static stacks, THREAD_COUNT bound (same page); thread flags, channel, mutex, wait_queue, event, lock (https://github.com/ariel-os/ariel-os/tree/main/src/ariel-os-threads/src) | MIT OR Apache-2.0 (https://github.com/ariel-os/ariel-os/blob/main/Cargo.toml); yes | v0.6.0 2026-10-07 (https://github.com/ariel-os/ariel-os/releases/tag/v0.6.0) | reuse components: threads beside an async core; needs an ARMv7-R arch port | yes, after porting |
| embassy | `platform-aarch32` executor on main; CI builds armv7r-none-eabi (thread and interrupt executors) and armv7r-none-eabihf (thread); interrupt executor needs GICv1/v2; "only usable on single-core chips" unless SEV/WFE are wired between cores (https://raw.githubusercontent.com/embassy-rs/embassy/main/embassy-executor/Cargo.toml). Released 0.10.0 names it `platform-cortex-ar`, thread executor only, on aarch32-cpu 0.1 (https://crates.io/crates/embassy-executor/0.10.0) | cooperative async executor; embassy-sync mutex, semaphore, channel, signal, waitqueue (https://raw.githubusercontent.com/embassy-rs/embassy/main/embassy-sync/src/lib.rs); no pools | MIT OR Apache-2.0 (https://crates.io/crates/embassy-executor); yes | 0.10.0 2026-03-20 (https://crates.io/crates/embassy-executor) | reuse components: executor, timer queue, sync; one instance per core **[INFERENCE]** | no |
| FreeRTOS kernel (+ lobaro FreeRTOS-rust) | ARM_CRx_No_GIC, ARM_CRx_MPU, ARM_CR5 ports (https://github.com/FreeRTOS/FreeRTOS-Kernel/tree/main/portable/GCC); tasks start in ARM state, or Thumb when the entry address has bit 0 set (https://raw.githubusercontent.com/FreeRTOS/FreeRTOS-Kernel/main/portable/GCC/ARM_CRx_No_GIC/port.c); AMP is one kernel per core (https://docs.freertos.org/Documentation/02-Kernel/03-Supported-devices/04-Demos/ST-Microelectronics/STM32H7_Dual_Core_AMP_RTOS_demo) | preemptive priority threads; wrapper exposes task, queue, semaphore, mutex, event group, timers (https://github.com/lobaro/FreeRTOS-rust/tree/master/freertos-rust/src); runtime creation through the FreeRTOS heap (https://raw.githubusercontent.com/lobaro/FreeRTOS-rust/master/README.md) | MIT (https://github.com/FreeRTOS/FreeRTOS-Kernel); yes; exclude the wrapper's GPLv2 Linux port (same README) | V11.3.3 2026-10-30 (https://github.com/FreeRTOS/FreeRTOS-Kernel/releases/tag/V11.3.3) | direct use behind a shim (C kernel); wrapper examples cover Cortex-M and Windows only | yes |
| Hubris | arm_m only (https://github.com/oxidecomputer/hubris/tree/master/sys/kern/src/arch); multicore undefined (https://hubris.oxide.computer/reference/) | static tasks, synchronous IPC, notifications; no runtime creation (same) | MPL-2.0 (https://github.com/oxidecomputer/hubris/blob/master/LICENSE.txt); yes | 2026-10-27 (https://github.com/oxidecomputer/hubris/releases/tag/all-sp-v1.84.0) | design reference (MPU isolation, build-time task table) | no |
| NuttX | armv7-r in tree with MPU and SMP (https://github.com/apache/nuttx/tree/master/arch/arm/src/armv7-r); AMP via RPTUN/OpenAMP (https://nuttx.apache.org/docs/latest/components/drivers/special/rptun/index.html) | POSIX threads, semaphores, message queues; no armv7r Rust target (https://doc.rust-lang.org/nightly/rustc/platform-support/nuttx.html) | Apache-2.0; yes, audit bundled third-party notices (https://raw.githubusercontent.com/apache/nuttx/master/LICENSE) | 13.1.0 2026-10-04 (https://github.com/apache/nuttx/releases/tag/nuttx-13.1.0) | reuse components: Cortex-R port layer reference | yes (C) |
| RIOT-OS | no Cortex-R; ARM7 only for A32 (https://github.com/RIOT-OS/RIOT/tree/master/cpu) | preemptive threads, msg, mailboxes (https://api.riot-os.org/group__core.html) | LGPL-2.1 (https://raw.githubusercontent.com/RIOT-OS/RIOT/master/LICENSE); yes via conversion | 2026.10 (https://github.com/RIOT-OS/RIOT/releases/tag/2026.10) | design reference | yes (C) |
| rt | arch/arm includes armv7-r with GICv2, GICv3, Hercules VIM, PL190, PL192, Sitara INTC and Sitara VIM backends, MPU, lazy VFP (https://git.rtng.org/rt/rt/src/branch/main/arch/arm); no multicore found (https://git.rtng.org/rt/rt/raw/branch/main/src/rt.c) | preemptive fixed priority; sem, event, queue, pool, mutex, rwlock, cond, barrier, notify, once, timer; objects static only, `rt_task_init` only before `rt_start` (https://git.rtng.org/rt/rt/raw/branch/main/include/rt/task.h) | Apache-2.0 (https://git.rtng.org/rt/rt/raw/branch/main/Cargo.toml); yes | 0.20.1 2026-09-14 (https://crates.io/crates/rt) | direct use behind a shim, or port-layer reuse (C kernel built through cc/bindgen) | yes |
| RTIC | Cortex-M and RISC-V backends only (https://raw.githubusercontent.com/rtic-rs/rtic/master/rtic/Cargo.toml) | SRP over interrupt priorities, single stack, static tasks | MIT OR Apache-2.0; yes | v2.4.0 2026-10-21 (https://github.com/rtic-rs/rtic/releases/tag/v2.4.0) | design reference | no |
| Tock | no ARMv7-R (https://github.com/tock/tock/tree/master/arch); nightly pin (https://raw.githubusercontent.com/tock/tock/master/doc/Getting_Started.md) | event-driven kernel plus MPU processes | Apache-2.0 OR MIT; yes | 2.3.0 2026-10-08 (https://github.com/tock/tock/releases/tag/release-2.3.0) | design reference | no |
| Wasefire | no ARMv7-R; nightly (https://raw.githubusercontent.com/google/wasefire/main/rust-toolchain.toml) | one Wasm applet at a time (https://google.github.io/wasefire/print.html) | Apache-2.0; yes | 2026-10-22 commit | not useful | no |
| Zephyr / zephyr-lang-rust | kernel supports Cortex-R4; ARMv7-R selects ISA_ARM and ISA_THUMB2, and C builds as Thumb-2 unless `COMPILER_ISA_THUMB2=n` (https://raw.githubusercontent.com/zephyrproject-rtos/zephyr/main/arch/arm/core/Kconfig); SMP IPIs need a GIC (https://raw.githubusercontent.com/zephyrproject-rtos/zephyr/main/arch/arm/core/cortex_a_r/smp.c); AMP via OpenAMP (https://docs.zephyrproject.org/latest/services/ipc/ipc_service/ipc_service.html); Rust CMake fails for Cortex-R (https://raw.githubusercontent.com/zephyrproject-rtos/zephyr-lang-rust/main/CMakeLists.txt) | preemptive/cooperative threads, full object set, runtime creation (https://docs.zephyrproject.org/latest/kernel/services/index.html) | Apache-2.0; yes | v4.5.0 2026-10-28 (https://github.com/zephyrproject-rtos/zephyr/releases/tag/v4.5.0) | reuse components (cortex_a_r port reference); west/CMake conflicts with `docs/principles.md:29` | yes (C) |

### 3.2 Outdated list (https://arewertosyet.com/)

| Project | ARMv7-R / multicore | Model and objects | License / AGPL | Maintenance | Fit | Shim threads |
| --- | --- | --- | --- | --- | --- | --- |
| aerugo | Cortex-M7 and x86 only (https://github.com/n7space/aerugo/tree/main/arch) | non-preemptive tasklets; nightly (https://github.com/n7space/aerugo) | MIT OR Apache-2.0; yes | 2023-12-12 | design reference | no |
| bern | Cortex-M only (https://gitlab.com/bern-rtos/bern-rtos/-/tree/main/arch/src); now pins stable (https://gitlab.com/bern-rtos/bern-rtos/-/blob/main/rust-toolchain.toml) | preemptive priority threads spawned at runtime, semaphores and mutexes whose timeout is not implemented (https://gitlab.com/bern-rtos/bern-rtos/-/blob/main/kernel/src/sched.rs), pools, MPU processes (https://gitlab.com/bern-rtos/bern-rtos/-/tree/main/kernel/src) | MIT; yes | 0.3.1 2025-09-14 (https://crates.io/crates/bern-kernel) | reuse components: scheduler and sync (port layer rewritten for A32; timeouts added) | yes, after porting |
| drone | Cortex-M and RISC-V (https://github.com/drone-os); nightly (https://github.com/drone-os/drone-core/blob/master/src/lib.rs) | interrupt-as-thread plus fibers (https://www.drone-os.com/) | MIT OR Apache-2.0; yes | 2022-11-20 | design reference | no |
| lilos | Cortex-M (https://github.com/cbiffle/lilos/blob/main/os/Cargo.toml) | single-stack async; stable (https://crates.io/crates/lilos) | MPL-2.0 (https://github.com/cbiffle/lilos); yes | 2025-05-15 | reuse components (timeouts, sync) | no |
| MnemOS / maitake | RISC-V, x86, wasm (https://github.com/tosc-rs/mnemos/tree/main/platforms); nightly | async kernel; maitake timer wheel and maitake-sync wait queues (https://github.com/hawkw/mycelium/tree/main/maitake, https://crates.io/crates/maitake-sync) | MIT OR Apache-2.0, maitake MIT; yes | maitake-sync 0.3.0 2026-03-13 | reuse components: maitake timer wheel and wait queues | no |
| R3 | Armv7-A port in A32 with MMU, SP804 and GIC, no FPU save (https://github.com/r3-os/r3/blob/r3%400.2.4/src/r3_port_arm/src/lib.md); one `r3_kernel::System` per core in `smp_rp_pico` (https://github.com/r3-os/r3/tree/r3%400.2.4/examples/smp_rp_pico/src) | preemptive kernel modelled on µITRON 4.0 and TOPPERS 3rd generation (https://github.com/r3-os/r3/blob/r3%400.2.4/src/r3_core/src/kernel/error.rs); tasks, semaphores, event groups, mutexes, timers; static objects only (https://github.com/r3-os/r3); nightly (https://github.com/r3-os/r3/blob/r3%400.2.4/rust-toolchain.toml) | MIT OR Apache-2.0 (https://crates.io/crates/r3); yes | archived, last commit 2023-03-30 | reuse components: A32 context switch, timeout heap, µITRON-style semantics | yes (static) |
| tornado-os | RISC-V, Sv39 paging (https://github.com/HUST-OS/tornado-os) | async research kernel | Apache-2.0 OR MulanPSL-2.0; yes (Apache) | archived 2022, last commit 2021-08-18 | not useful | no |
| zephyr-rust | maps Cortex-R to `thumbv7r-zephyr-eabi` and sets `RUSTC_BOOTSTRAP=1` (https://github.com/tylerwhall/zephyr-rust/blob/master/CMakeLists.txt) | Zephyr threads | Apache-2.0; yes | active, 2026-10-12 (https://github.com/tylerwhall/zephyr-rust/commits/master) | design reference (Zephyr route through Rust) | yes (C) |

### 3.3 TOPPERS and other µITRON implementations

| Project | ARMv7-R / multicore | µITRON fit | License / AGPL | Maintenance | Fit |
| --- | --- | --- | --- | --- | --- |
| TOPPERS/ASP3 (+dcre) | no Cortex-R port (https://www.toppers.jp/asp3-kernel.html) | µITRON 4.0-derived 3rd generation; the dcre extension has `acre_*`/`del_*` only, no fixed-ID `cre_*` (https://www.toppers.jp/download.cgi/asp3-3.7.2.tar.gz, `doc/extension.txt`) | current TOPPERS license (https://www.toppers.jp/license.html). Clause (4) indemnifies holders against user claims. **[INFERENCE]** Under a strict reading this is a further restriction under AGPL §7, even though the TOPPERS FAQ calls the license GPL-compatible (https://www.toppers.jp/faq/faq_ct12.html) | 3.7.2 2026-05-18 | design reference |
| TOPPERS/FMP3 | Cortex-R5F "Kria R5" package (https://www.toppers.jp/fmp3-e-download.html); one shared binary across cores (https://www.toppers.jp/fmp3-kernel.html) | ASP3 plus multiprocessor; static objects | current TOPPERS license | 3.4.0 2026-07-28 | design reference: current ARMv7-R µITRON-family port layer |
| TOPPERS/FDMP | ARM MPCore, no Cortex-R (https://www.toppers.jp/fdmp-kernel.html) | function-distributed multiprocessor µITRON with cross-processor calls, close to our AMP shape | old TOPPERS license or GPLv2-only (https://www.toppers.jp/license.html#old-toppers-license) | 2007 | design reference for cross-domain calls |
| TOPPERS/FI4 | SH3 only (https://www.toppers.jp/fi4-kernel.html) | full-set µITRON 4.0 with `cre_*`/`acre_*`/`del_*` (https://www.toppers.jp/download.cgi/fi4-1.0.2.zip) | old TOPPERS license | 2006 | design reference: semantic oracle for dynamic creation |
| TOPPERS/JSP, HRP2/3 | no ARMv7-R (https://www.toppers.jp/jsp-kernel.html, https://www.toppers.jp/hrp3-kernel.html) | standard profile (static) / protection extension | old / current license | 2026 releases | design reference (HRP: MPU from static config) |
| HOS-V4a | `arm_v7r` and `cortex_r5` ports, built without `-mthumb` (https://github.com/ryuz/hos-v4a/blob/master/kernel/build/arm/arm_v7r/gcc/Makefile); no SMP code found | µITRON 4.0; fixed-ID `cre_*` for tsk, sem, flg, dtq, mtx, cyc; `acre_*` only for mbx, mpf, isr, with no `del_mbx`/`del_mpf` (https://github.com/ryuz/hos-v4a/tree/master/kernel/source/object) | Project HOS license, permissive, explicitly allows GPL relicensing (https://github.com/ryuz/hos-v4a/blob/master/document/licence.txt); yes | 2023-07-29 | reuse components: ARMv7-R exception and context code; partial semantic reference for fixed-ID creation |
| eCos µITRON | µITRON 3.02 (https://ecos.sourceware.org/docs-latest/ref/compat-uitron-microitron-api.html); hardware list has no Cortex-R (https://ecos.sourceware.org/hardware.html) | | GPL-2.0-or-later with exception (https://ecos.sourceware.org/license-overview.html) | 2015 | design reference |
| NORTi | full-set µITRON 4.0 plus extensions (http://www.mispo.co.jp/document/no4guide.pdf) | non-X-M1 predecessor of the FF API base, per fffw | commercial (https://www.mispo.co.jp/products/NORTi/Pro) | | documentation reference only |

No Rust-native kernel on either list has an ARMv7-R port. embassy has an ARMv7-R async executor. R3 (Armv7-A, A32) is the shortest Rust porting distance. rt, FreeRTOS, NuttX, Zephyr, HOS-V4a and FMP3 already run on ARMv7-R in C.

## 4. Options for #156

Maintainer decision (2026-11-02): option (b), a mainstream Rust base plus a shrinking shim, unless both gaps in §4.7 and §4.8 stay open, in which case the fallback is our own RTOS built from components of modern Rust projects (the maintainer's "(c)", which corresponds to (a) or c1 below). §4.1-4.6 keep the side-by-side comparison as background. §4.7-4.10 answer the two feasibility questions and give the recommendation.

### 4.1 Inputs common to every option

- **Two layers.** (1) An owned core per domain: scheduler, context switch, timers, IRQ and cross-core notification. (2) A compatibility shim that implements the 97-slot stock ABI, the direct entries and the raw C/T/descriptor view for retained callers. It shrinks as callers migrate [Map A.5; Draft:67,78].
- **Shim requirements from X-M1 evidence:**
  - fixed caller-chosen IDs with encoded class/domain;
  - creation at runtime with -41 on duplicates;
  - i-variant (interrupt-context) calls;
  - timeouts including −1 forever;
  - whole-word flag semantics;
  - -50 polling failures;
  - two roots;
  - the critical pair and extensions;
  - raw C+8/C+20 and stack fill pattern for `0x725b74`;
  - the tick pointer for `0x7257a8..0x725940` [§1.2-1.4].
- **Blocking-thread constraint.** While stock code runs, its tasks block inside kernel waits (flag, semaphore, queue receive, delay) in the middle of a C call chain on their own stacks, and resume through saved SP/PC and exception-return frames [§1.4]. The shim therefore needs preemptive threads with per-task stacks and blocking wait queues. An async-only executor (embassy, RTIC, lilos, maitake) cannot suspend a stock C frame by itself.
  - Projects that supply such threads: FreeRTOS, rt, Zephyr, NuttX, HOS-V4a, R3 (static), bern and Ariel OS (both after an ARMv7-R port).
  - **[INFERENCE]** Combining an async core with threads means a thread layer next to the executor, as Ariel OS already does (https://raw.githubusercontent.com/ariel-os/ariel-os/main/book/src/multithreading.md):
    - each stock task is a preemptive thread;
    - owned async code runs on one executor thread per domain;
    - shim wait queues accept both a blocked thread and a registered waker, so either side can wake the other.
- **Whatever the base, the shim is ours.** No surveyed kernel provides fixed-ID dynamic creation with X-M1's encoded IDs, dual roots and raw layout. HOS-V4a and FI4 come closest semantically.

### 4.2 Option (a): owned kernel modelled on µITRON semantics

- **Shape:** an AGPL Rust kernel per domain whose native object model is the µITRON-like one recovered in §2. The shim is a thin ABI adapter over native objects. Owned code uses typed Rust wrappers over the same kernel.
- **Reuse (licence-compatible):**
  - A32 context switch and timeout heap from R3 (MIT/Apache);
  - ARMv7-R exception and context code from HOS-V4a (permissive);
  - timer wheel and wait queues from maitake or maitake-sync (MIT);
  - ARMv7-R port details from FreeRTOS ARM_CRx_No_GIC (MIT) or NuttX (Apache) as references.
- **Strengths:**
  - The shim needs almost no translation.
  - Stock semantics are the native semantics, which helps comparison at retained seams [Draft:78].
  - Native C/T layout compatibility is possible where raw readers remain.
  - It suits the per-family milestone plan [Draft:69-76].
- **Costs and risks:**
  - We own the scheduler, timers and SMP-free cross-core paths indefinitely.
  - µITRON-isms (TA_* attributes, encoded IDs) can leak into owned APIs unless typed wrappers stay the only owned-code interface.
- **End state:** once no stock caller remains, the shim and raw layout are deleted. A µITRON-shaped kernel stays, either reduced to the subset owned code uses or replaced later.

### 4.3 Option (b): mainstream base plus shrinking shim

Candidates, ranked by fit for dual Cortex-R4F AMP (one kernel per core, shared memory, MPU only), porting effort and license:

| Rank | Base | AMP fit | ARMv7-R effort | License | Notes |
| --- | --- | --- | --- | --- | --- |
| 1 | FreeRTOS kernel | documented one-instance-per-core AMP (URL in §3.1) | ports exist (No_GIC, MPU); write IRQ-controller glue | MIT | C kernel. The Rust wrapper is thin and Cortex-M-tested. Runtime creation exists, but the shim must map fixed encoded IDs to handles. Most widely supported. |
| 2 | rt | no multicore; two images **[INFERENCE]** | armv7-r port in tree with several non-GIC VIC backends; a VIC backend for this SoC may be new | Apache-2.0 | Static objects only, so the shim needs pre-sized pools for 120 tasks and the objects behind the 97 slots. Small and maintained. Build uses cc/bindgen, which has to be brought into Bazel. |
| 3 | Zephyr | OpenAMP IPC; SMP needs a GIC | Cortex-R4 supported | Apache-2.0 | Heaviest. west/CMake conflicts with `docs/principles.md:29`. The Rust bindings refuse Cortex-R today. |
| 4 | embassy (+ thread layer) | one executor per core **[INFERENCE]** | thread executor builds for armv7r today; interrupt executor needs a GIC | MIT OR Apache-2.0 | Rust-native and most active. Supplies no blocking threads, so it needs a thread layer from (a), Ariel OS or R3 beside it. |
| 5 | Ariel OS | SMP only; run as two instances **[INFERENCE]** | new ARMv7-R arch module for threads; ports are about 11 KB (§3.1) | MIT OR Apache-2.0 | Already combines preemptive threads with embassy, the pattern §4.1 needs. |
| 6 | NuttX | RPTUN/OpenAMP | armv7-r in tree | Apache-2.0 | POSIX C kernel; no armv7r Rust target. |

- **Strengths:** an upstream community maintains the scheduler and port. Owned code gets mainstream APIs.
- **Costs and risks:**
  - The shim is a full emulation layer of µITRON-like semantics on foreign objects: ID tables, whole-word flags, attribute 4 clear, -50 polling and i-variants.
  - The raw C/T view becomes a maintained mirror.
  - The C-kernel candidates (1-3, 6) bring a C dependency, and vendoring and build integration land under Bazel.
- **End state:** with no stock caller left, the shim, mirror and root publication are deleted. The mainstream base stays, and owned code uses its native API or embassy.

### 4.4 Option (c): mixes

- **c1. Owned thread kernel plus embassy.** A small owned thread kernel per domain (option (a) scope limited to threads, wait queues, timeouts and context switch) hosts the shim. Owned code is async on embassy, in one thread per domain.
  - Leaves embassy as the long-term owned API, and the thread kernel can shrink to what stock still needs.
  - Embassy's interrupt executor depends on the GIC question (§1.4).
- **c2. FreeRTOS or rt as the thread substrate plus embassy.** Same split, with the thread layer borrowed rather than owned. The cost is a C kernel kept for as long as any blocking thread remains.
- **End state for c1/c2:** threads remain only if owned code still needs blocking contexts. Otherwise the thread layer goes with the last stock caller and each domain runs executor plus interrupts.

### 4.5 Comparison

| Criterion | (a) owned µITRON-shaped | (b) mainstream + shim | (c1) owned threads + embassy | (c2) borrowed threads + embassy |
| --- | --- | --- | --- | --- |
| Shim translation effort | lowest | highest | low (shim shapes the thread layer) | high |
| Long-term maintenance owned | highest | lowest | medium | low-medium |
| Rust-native | yes | no for ranks 1-3, 6 | yes | partly |
| AGPL combination | clean | clean (MIT/Apache) | clean | clean |
| Raw C/T compatibility | native | mirror | native | mirror |
| Fit to existing per-family plan [Draft:69-76] | direct | via shim families | direct | via shim families |
| Depends on GIC question | no | Zephyr SMP, embassy interrupt executor | interrupt executor only | interrupt executor only |

### 4.6 Checks that shift the choice

**[INFERENCE]** Each of these narrows the options:

1. Identify the interrupt controller from X-M1 evidence (GIC or not; §1.4). This decides embassy's interrupt executor, Zephyr SMP and which rt backend applies.
2. Admit the status-register instructions in the extent tool and test the 30 high-tier predictions (§2.5). If the µITRON map holds, (a) and c1 gain a ready test generator and the shim surface is bounded and named. If it fails, all options face the same open recovery.
3. Enumerate the cross-domain operations: encoded class versus current core, and peer notification through `0x433e4` [Draft:24,61]. Any slot that blocks across domains needs FDMP-like remote calls, which no surveyed base provides.
4. Recover the wait-queue order (TA_TFIFO 0 in create packets suggests FIFO), the timeout unit (the elapsed adapter's ×10 [Draft:48]) and i-variant context rules. These are the semantics an emulation shim in (b) or c2 has to reproduce exactly.
5. Enumerate every reader of raw C/T/descriptor data beyond `0x725b74` and the tick consumers [Draft:46,50]. A large reader set favours (a) or c1, where the layout is native.
6. Build trial: compile an embassy executor with the aarch32 crates under Bazel with the pinned 1.98.1 toolchain. Done; see §4.11.

### 4.7 Gap 1: embassy on Cortex-R4F (ARMv7-R, A32, VFPv3-D16, MPU only, vendor interrupt controller, AMP)

Already available upstream, all MIT OR Apache-2.0:

- **Executor.**
  - embassy-executor `platform-aarch32` covers "Cortex-A/R and other ARMv7-A/R and ARMv8-A/R cores in 32-bit mode". Embassy CI builds it for `armv7r-none-eabi` with `executor-thread` (https://github.com/embassy-rs/embassy/blob/592a8c2e1f727f483d238dd01c0862e1e0c51c73/embassy-executor/Cargo.toml).
  - The thread executor polls, then executes `WFE`; its pender executes `SEV`.
  - The interrupt executor pends GICv1/GICv2 SGIs through `GICD_SGIR` (https://github.com/embassy-rs/embassy/blob/592a8c2e1f727f483d238dd01c0862e1e0c51c73/embassy-executor/src/platform/aarch32.rs).
  - The platform is documented as "Only usable on single-core chips" unless the event signal is wired between cores, and not for use inside an RTOS thread: "you must use RTOS primitives to sleep/notify the thread instead" (same Cargo.toml).
  - A custom pender can be registered through the `Pender` trait and `pender_impl!`, which exports symbol `__pender` (https://github.com/embassy-rs/embassy/blob/592a8c2e1f727f483d238dd01c0862e1e0c51c73/embassy-executor/src/pender.rs).
- **Dual-core template.** `examples/mps3-an536` runs one embassy executor per core in a single image on two Cortex-R52 cores (`fizzbuzz-smp.rs` with `kmain`/`kmain2`) (https://github.com/embassy-rs/embassy/tree/592a8c2e1f727f483d238dd01c0862e1e0c51c73/examples/mps3-an536).
  - It implements `embassy_time_driver::Driver` (`now`, `schedule_wake`) with one timer queue per core, and uses `aarch32-cpu` `critical-section-multi-core`.
  - It depends on the ARMv8-R generic timer and GICv3, which a Cortex-R4 does not have [INFERENCE].
- **Arch and runtime crates.** `aarch32-cpu` 0.4.0 and `aarch32-rt` 0.5.0 from rust-embedded target Armv7-R "like the Arm Cortex-R5", with MSRV 1.93 (https://github.com/rust-embedded/aarch32, commit b73d6de, 2026-10-18).
  - `aarch32-cpu` provides PMSAv7 MPU (`pmsav7.rs`), cache and system registers, plus single-core and multi-core critical sections.
  - The multi-core critical section masks IRQ, then spins on a global LDREX/STREX word keyed by `core_id()`, which reads MPIDR (https://github.com/rust-embedded/aarch32/blob/b73d6de1d2c7d9143cea981a711da11ff3ebfe90/aarch32-cpu/src/critical_section.rs, https://github.com/rust-embedded/aarch32/blob/b73d6de1d2c7d9143cea981a711da11ff3ebfe90/aarch32-cpu/src/asmv7.rs).
  - `aarch32-rt` provides:
    - the A32 vector table;
    - per-mode stacks for `_num_cores` cores;
    - trampolines for SVC, IRQ, undefined and abort;
    - `svc-stack-interrupt` "if you are writing an RTOS";
    - `eabi-fpu`, which enables the FPU on soft-float targets and saves D0-D7, FPSCR and FPEXC around interrupts. That matches our soft-float `armv7r-none-eabi` toolchain.

    (https://github.com/rust-embedded/aarch32/blob/b73d6de1d2c7d9143cea981a711da11ff3ebfe90/aarch32-rt/src/lib.rs)
  - Its `versatileab` examples run `armv7r-none-eabi` on a QEMU Cortex-R5F with a non-GIC PL190 VIC through `pl190-vic` (https://github.com/rust-embedded/aarch32/tree/b73d6de1d2c7d9143cea981a711da11ff3ebfe90/examples/versatileab). This is the pattern for a vendor interrupt controller.
- **Not found:** any embassy HAL, time driver or interrupt glue for a Cortex-R4 SoC. A web search found none (https://docs.embassy.dev/embassy-time/git/default/index.html lists only MCU HALs).

#### 4.7.1 ti-hercules-bsp (https://github.com/paoloteti/ti-hercules-bsp)

- **What it is:** a TMS570 BSP, last commit 44210274 on 2020-12-04, 231 commits since 2018-04-07. It is licensed MIT OR Apache-2.0 (`COPYRIGHT`), except `src/dabort.s`, which is Texas Instruments BSD-3-clause.
- **Toolchain:** it needs a 2020 nightly (`#![feature(llvm_asm, global_asm, naked_functions)]`; `llvm_asm!` has since been removed from Rust) and targets big-endian `armebv7r` (`README.md`). X-M1 runs little-endian [Inventory:498].
- **Core-level pieces:**
  - lock-step-safe register initialisation of all modes (`syscore.rs:init_core_registers`);
  - mode stack setup (`cpustack.s`);
  - VFP enable through CPACR and FPEXC (`syscore.rs:vfp_enable`);
  - ATCM/BTCM ECC and VIC-port enables in ACTLR/SCTLR;
  - PMU event-bus export;
  - Cortex-R4 errata 57/66 through `silicon-cr4`, which ships prebuilt `.a` blobs beside `asm.s` (https://github.com/paoloteti/silicon-cr4).
- **Missing:** `arch/cortex-r4/src/mpu.rs` is empty. There is no critical section, cache code, executor, or RTIC/embassy integration.
- **TI-specific:** the vector table (`intvect.rs` `ldr pc,[pc,#-0x1b0]` targets the VIM `IRQVECREG`), VIM, RTI timer and watchdog, ESM, PBIST, eFuse, PLL, flash, and the ESM/ECC data-abort handler.
- **Effect on estimates:** under a week saved. `aarch32-rt` and `aarch32-cpu` already provide maintained, stable equivalents of every core-level piece except the errata workarounds and lock-step register initialisation. Those are worth reading as references [INFERENCE]. The repository does not establish whether X-M1's cores are configured lock-step or need errata 57/66; the fffw lead that they run separately is non-X-M1.
- It changes nothing for Hubris, which needs kernel arch work this BSP does not touch.

#### 4.7.2 Work we would write, with effort estimates

Effort is in engineer-weeks of implementation once the cited X-M1 laws are known. It excludes law recovery itself. All estimates are **[INFERENCE]**.

| Piece | Work | Weeks | Blocking X-M1 evidence |
| --- | --- | --- | --- |
| Build integration | Done for the published crates by the §4.11 trial: embassy-executor 0.10.0, aarch32-cpu 0.4.0 and aarch32-rt 0.5.0 build and link under Bazel through the existing Nix/Cargo genrule pattern, with host proc-macros and build scripts. Remaining: embassy-sync and embassy-time crates (same pattern), and moving to unreleased `platform-aarch32` once released or through a pinned Git dependency with `importCargoLock` output hashes. | <0.5 | none |
| Phase-1 coexistence | Run `raw::Executor` inside one stock task per domain. The custom pender sets a dedicated flag through the task/interrupt flag-set slots (`+0x88/+0x8c`); idle waits on `+0x9c` with timeout −1. This follows the embassy rule for running inside an RTOS thread. | ~1 | full lower ABI of flag set/wait and their numeric errors [Draft:27] |
| Time driver | 64-bit `now()` from the reload-minus-count relation at `0xfff91004/0xfff91000`, wrap extension, alarm IRQ for `schedule_wake`, one queue per domain as in the mps3-an536 template. | 1-2 | divider/frequency, width, wrap, compare or reload IRQ, and whether the timer is per core or shared [Draft:57] |
| Interrupt glue | IRQ entry and dispatch on the controllers at `0xfffb3000/0xfffb4000`: own selected sources and forward the rest to stock handlers until the stock vector `0x40204/0xd0204` is retired; ack/EOI; optional software-pend for an interrupt-mode executor (the GIC-only built-in executor does not apply unless the controller proves to be a GIC). | 2-3 | routing, arbitration, EOI, nesting, software pend [Draft:56] |
| Critical sections | Per-domain IRQ masking (`critical-section-single-core`) is enough for embassy state private to a domain. Any cross-domain shared state needs a lock keyed by the CP14 discriminator used at `+0x1618` [Manifest], not MPIDR. Retained stock code also takes stock locks. | 1 | exclusive-monitor law across cores [Draft:59]; physical meaning of the CP14 identity |
| Cross-core wake | Shared-memory channel plus peer notification IRQ, the mechanism stock activation uses through `0x433e4/0xd33e4`. `SEV`/`WFE` across cores is not established (stock idle uses `WFI` [Draft:44]). | 1-2 | notification registers, ordering, acknowledgement [Draft:61] |
| Runtime crate | Keep the stock startup and vectors at first. Adopt `aarch32-rt` (vectors, stacks, VFP context) only when the platform owns the vectors. | 0-2 | SVC0 and restore routes [Draft:41-42] |

**Subtotal for the embassy core: about 6-12 weeks.**

#### 4.7.3 Coexisting with the shim's blocking thread layer

Embassy does not supply preemptive threads with per-task stacks and blocking waits, which the shim needs once the stock kernel is retired (§4.1).

- **Phase 1 (stock kernel retained):** the stock kernel is the thread layer, and embassy is one stock task per domain, as above.
- **Phase 2 (stock kernel retired):** a thread layer provides:
  - priority-preemptive threads for stock tasks, with their own stacks and saved SP/PC;
  - shim wait queues whose entries are either a blocked thread or an embassy `Waker`;
  - an executor thread per domain whose pender unparks it;
  - ISR-side shim calls (the i-variants) that can ready a thread or wake a waker.
- Ariel OS already combines preemptive threads with embassy executors (https://raw.githubusercontent.com/ariel-os/ariel-os/main/book/src/multithreading.md), but has no ARMv7-R thread port (https://github.com/ariel-os/ariel-os/tree/main/src/ariel-os-threads/src/arch). Porting it is about 4-6 weeks **[INFERENCE]**.
- Alternatively, write a minimal owned thread layer using the R3 `r3_port_arm` A32 context switch (MIT/Apache, https://github.com/r3-os/r3/blob/r3%400.2.4/src/r3_port_arm/src/threading/imp.rs) and `aarch32-rt` trampolines, in about 6-10 weeks **[INFERENCE]**.
- The µITRON-like object semantics on top (§2, §4.1) are shim work in either case.

**Verdict:** gap 1 closes. The upstream pieces exist on stable Rust under compatible licenses, and the remaining work is board glue whose inputs are the device-law gaps the manifest already lists.

### 4.8 Gap 2: Hubris on Cortex-R

- **Upstream position:** the FAQ says "Cortex-R has the right memory protection model for Hubris, but an entirely different interrupt model" (https://github.com/oxidecomputer/hubris/blob/a9e139664f9131de2714800fa75c16152d3ca3fb/FAQ.mkdn?plain=1#L197). No Cortex-R fork or port was found (web search; the upstream kernel arch directory contains only `arm_m.rs`, https://github.com/oxidecomputer/hubris/tree/a9e139664f9131de2714800fa75c16152d3ca3fb/sys/kern/src/arch).
- **Cortex-M-specific parts (commit a9e13966):**
  - `sys/kern/src/arch/arm_m.rs`, 2,015 lines:
    - `SVCall` entry, which relies on hardware stacking, PSP and EXC_RETURN;
    - `PendSV` for deferred context switches from ISRs;
    - `SysTick` as the kernel timebase;
    - NVIC for `enable_irq`, `disable_irq`, `irq_status` and `pend_software_irq`;
    - SCB for fault priorities, with CFSR/MMFAR/BFAR fault decoding;
    - ARMv6/7-M and ARMv8-M MPU register programming;
    - lazy FP stacking.
  - `sys/userlib/src/arch/arm_m.rs`, 1,558 lines of syscall stubs.
  - Kernel and task link scripts under `build/`.
  - The architecture-independent kernel reaches all of this through the `Arch` trait in `sys/kern/src/arch.rs` (116 lines), which bounds the port (https://github.com/oxidecomputer/hubris/tree/a9e139664f9131de2714800fa75c16152d3ca3fb/sys/kern/src).
  - Builds go through `cargo xtask dist app.toml` (https://github.com/oxidecomputer/hubris/blob/a9e139664f9131de2714800fa75c16152d3ca3fb/README.mkdn). The toolchain is pinned to 1.95.0 with Thumb targets only (https://github.com/oxidecomputer/hubris/blob/a9e139664f9131de2714800fa75c16152d3ca3fb/rust-toolchain.toml).
- **What a Cortex-R port involves [INFERENCE]:**
  - SVC entry in A32 with banked SVC registers and manual save of the user-mode r4-r11, SP, LR and SPSR into the task save area, because there is no hardware stacking;
  - deferred switching at IRQ exit or through a software-pended source on the vendor controller, because there is no PendSV;
  - a vendor-timer timebase;
  - vendor-controller IRQ operations;
  - PMSAv7 regions through CP15 c6 instead of memory-mapped MPU registers. The region attribute fields correspond closely, and the Cortex-R4 implements 8 or 12 regions (https://support.arm.com/documentation/ddi0363/g/Memory-Protection-Unit/About-the-MPU/Memory-regions; TI lists 8, 12 or 16 regions across Hercules parts, https://www.ti.com/content/dam/videos/external-videos/en-us/3/3816841626001/5799997315001.mp4/subassets/hercules_cortexr4_system_architecture_mpu-production.pdf);
  - abort-vector fault decoding from DFSR/DFAR/IFSR/IFAR;
  - explicit VFP save;
  - userlib stubs for A32 or `thumbv7r`;
  - link scripts and the xtask app model reproduced under Bazel.
  - Estimate: 6-10 weeks for arch and userlib, 4-8 weeks for build integration, then permanent fork maintenance, because upstream does not target Cortex-R.
- **Fit to two cores:** "Behavior in multicore situations is not yet defined" (https://hubris.oxide.computer/reference/). That means two independent images, plus a custom bridge task for cross-domain IPC over shared memory and the notification IRQ.
- **Fit to the shim:**
  - **Task model.** "Tasks can't be created or destroyed", and all tasks are declared in `app.toml` (https://hubris.oxide.computer/reference/). Stock code creates tasks at runtime with entry, stack, priority and size taken from the creation packet, up to 120 records per domain [Draft:23; Map A.2].
  - **Object model.** Hubris kernel objects are IPC and notifications only, so semaphores, flags, queues, pools and timeouts would live in a server task, and every stock service call would become an IPC round trip.
  - **Privilege.** Stock image4 code masks interrupts itself (critical pair `0x7235d8/0x7236b0` with IRQ dispatch and a private prior mask [Inventory §8.8; Draft:29]). **[INFERENCE]** Under the ARMv7-A/R architecture, MSR writes to the CPSR mask bits are ignored in User mode (https://developer.arm.com/documentation/ddi0406/latest), so stock code in an unprivileged Hubris task would lose its critical sections silently.
  - **Consequence.** Hosting stock tasks therefore needs privileged execution plus either kernel changes for dynamic tasks or one privileged legacy task with its own internal thread scheduler. The latter is option (c) inside Hubris, and it gives up Hubris's isolation for the largest body of code.
- **Total:** about 10-18 weeks before shim work, plus a design conflict that kernel changes could remove only by abandoning Hubris's static model. All figures are **[INFERENCE]**.

**Verdict:** gap 2 can be closed for owned tasks only. It does not close for a shim that has to host stock tasks.

### 4.9 X-M1 evidence needed by either path

These are already listed as gaps in the manifest draft:

1. Interrupt controller laws at `0xfffb3000/0xfffb4000`: routing, arbitration, EOI, nesting, software pend and per-core banks [Draft:56,58].
2. Timer laws at `0xfff91000..`: divider and frequency, width, wrap, IRQ fan-out, deadlines [Draft:57].
3. Cross-core notification registers and ordering [Draft:61].
4. Exclusive-monitor behaviour across cores [Draft:59].

Not in the evidence set and also needed:

- whether `SEV`/`WFE` reach the peer core;
- MPU region count and the stock MPU configuration;
- whether stock tasks use VFP, which decides whether VFP is saved per thread **[INFERENCE]**.

### 4.10 Recommendation

**[INFERENCE]** Take **(b) on embassy**. Gap 1 closes with maintained, stable, MIT/Apache upstream pieces (§4.7), and the remaining board glue depends on device laws the project must recover anyway. Do not take **(b) on Hubris**: its static, unprivileged, IPC-only task model cannot host stock tasks without the kernel changes listed in §4.8, and it would be a permanent Cortex-R fork.

One condition belongs with the choice. Embassy covers the owned core, not the shim's thread substrate. The phase-2 thread layer (§4.7.3) is a component-level build under any option: an Ariel OS thread port or a minimal owned layer. Fall back to the maintainer's (c) only if the timer or interrupt-controller laws rule out an embassy time driver or wake path. Embassy needs only a monotonic counter with an alarm IRQ and an IRQ-driven wake, so that is unlikely.

Order of work:

1. Done: the build trial (§4.11) links an embassy executor with the aarch32 crates under Bazel with rustc 1.98.1.
2. Recover the timer and interrupt-controller laws.
3. Done in the emulator: phase 1, an embassy executor inside one stock task per domain (§4.12).
4. Choose the phase-2 thread layer after the µITRON slot tests in §2.5 bound the shim.

### 4.11 Build trial: embassy executor under the pinned toolchain

`//replacement/embassy_trial:embassy-trial` builds and links a minimal executor image for `armv7r-none-eabi` with `target-cpu=cortex-r4f`, the project's pinned soft-float target. It is host-only; the image is not executed and its layout (`replacement/embassy_trial/memory.x`) is not an X-M1 placement.

- **Crates:**
  - embassy-executor 0.10.0 with `platform-cortex-ar` and `executor-thread`, which is the published name of the aarch32 platform (https://crates.io/crates/embassy-executor/0.10.0);
  - aarch32-cpu 0.4.0 with `critical-section-single-core`;
  - aarch32-rt 0.5.0 with `eabi-fpu`.
  - `Cargo.lock` locks 62 packages. Published embassy-executor 0.10.0 still depends on aarch32-cpu 0.1, so two aarch32-cpu versions are compiled.
- **Build path:** the genrule runs `nix-build` on `embassy-trial.nix`, which imports `Cargo.lock` with `importCargoLock` and runs `cargo build --frozen --target armv7r-none-eabi` with `RUSTC` set to the `xm1-rust.nix` wrapper. This is the pattern `replacement/heap_replay` already uses for rlsf, talc and buddy. No `-Z build-std` and no `RUSTC_BOOTSTRAP` is needed: the wrapper's sysroot already carries the target `core` and `compiler_builtins` and the host std that proc-macros and build scripts link against [`replacement/toolchain/xm1-rust.nix`]. The derivation exports the staticlib, aarch32-rt's generated `link.x` and the list of host proc-macros; `xm1_rust_elf` links them with the package's `memory.x`.
- **Proc-macros handled:** nine host proc-macro libraries were built and loaded by the pinned rustc: `aarch32_rt_macros`, `bitbybit` (two versions), `darling_macro`, `document_features`, `embassy_executor_macros`, `num_enum_derive`, `rustversion` and `thiserror_impl`. The `#[embassy_executor::task]` and `#[aarch32_rt::entry]` expansions are in the linked image. Build scripts (`arm-targets` cfg selection, aarch32-rt's `link.x`) ran unchanged.
- **Result:**
  - The ELF links with entry `_start` at `0x218`.
  - Sections are `.vector_table` 64 bytes, `.text` 3,988 bytes and `.rodata` 352 bytes, and the flat image is 4,404 bytes.
  - The disassembly is A32 and contains the executor's `WFE` idle loop, the pender's `SEV`, LDREX/STREX run-queue updates, and the aarch32-rt exception trampolines saving D0-D7, FPSCR and FPEXC.
  - The first source revision failed to compile: in 0.10.0 a task function returns `Result<SpawnToken, SpawnError>`. It was corrected before the successful build.
- **Single-crate rule:** `xm1_rust_staticlib` passes one crate root to `rustc` with no `--extern`, no host compilation and no build-script step [`replacement/toolchain/rust.bzl:52-92`], so it cannot express this graph by itself. No rule extension is needed, because the Nix/Cargo genrule covers third-party graphs.
- **Smallest optional change:** a shared `xm1_cargo_staticlib(name, nix_file, srcs, outs)` macro would remove the duplicated genrule in `replacement/heap_replay/replay.bzl` and here. An owned single-crate root that must call into the vendored graph directly would instead need an `externs` attribute on `xm1_rust_staticlib`: `--extern NAME=PATH` and `-L dependency=DIR` over rlibs exported by the same derivation. Nothing needs that yet.
- **Command:** `devenv shell -- bazel --batch --output_base=... build --jobs=2 //replacement/embassy_trial:embassy-trial`. Its genrule runs `nix-build` locally and is tagged `manual`, like the heap replay subjects.
- **Licenses:** the `license` fields of the 62 vendored packages are MIT (19), MIT OR Apache-2.0 in its spellings (38), BSD-3-Clause OR MIT OR Apache-2.0 (2), Unlicense OR MIT (2) and (MIT OR Apache-2.0) AND Unicode-3.0 (1, unicode-ident), all combinable with AGPL-3.0. Their notices travel with any conveyed artifact; this trial is not conveyed.

### 4.12 Phase 1: embassy inside one stock task per domain (emulated)

`replacement/embassy_platform` is the phase-1 platform: embassy-executor 0.10.0 `raw::Executor` with no platform feature, embassy-time 0.5 at 3 MHz over an owned driver, and the stock kernel as the thread layer. It is installed as a RAM-push experiment (`experiments/embassy_phase1`), because RAM push is the only owned-code path that has run on the camera (E-RAM-PUSH-INSTALL-150) and it installs after the kernel is running. Experiments gained `prelinked.json`: a Bazel-built relocatable object, gc'd from the hook handler and the exported records, replaces `lib.rs` (`host/firmware/build-experiment-ram.sh`). The demo object takes 0x3ea0 of the 0x5000-byte code window.

- **Host tasks:** the hook in front of the stock modal dispatcher `0x00bfb028` runs once from task context with IRQ unmasked. It fills B's stack with the id byte and cleans the payload's D-cache lines.
  - **B (class 2, id 119):** inside the stock critical pair `0x7235d8/0x7236b0` the hook calls B's root `+0x80`, the eventflag create, for flag 119 with `{attribute 0, pattern 0}`, then the task create `+0x0` with `{flags 2, argument 1, entry, priority 14, 2 KiB stack}`. That is the order of stock task create `0x725dd8`, which creates the flag with the task's own id and passes attribute 4 unless descriptor word +0x18 is `0xa5f0a5f0` (E-RTOS-SLOT-MANIFEST-V5-150). After leaving the pair it activates B through the executing core's `+0xc`.
  - **A (class 1):** the hook calls the stock allocator `0x728804(0, 1, {entry, priority 14, 2 KiB stack size, stack base, argument 0})` as its four plain-image callers do. The allocator fills the stack with byte 0 and cleans it. It tries A `+0x0` with flags 2 on ids from cursor `0x40a0fc34` (initially 120) down to 105, moving on when one fails, stores the cursor below the id it took and activates the task through the executing core's `+0xc`. It returns the id, or -34 when the pool is empty, else the failing status. The hook records that return and publishes A's encoded id for the pender.
  - Priority 14 is one above the idle tasks 102/103 (15), whose WFI loop never blocks; at 15 the hosts never ran.
- **Task ids:**
  - **Census:** on the stock route, root `+0x0` and `+0x80` received ids up to 104 only (A: 74 calls, B: 10 calls; `private-data/scratch/EmbassyPhase1/census-creation-ids.json` from E-RTOS-SLOT-CENSUS-STOCK-150).
  - **Eventflags:** the allocator calls no `+0x80`, so A has no eventflag record, like the stock tasks it creates. B's task-119 flag is created but never used by the host. In the census no eventflag slot (`+0x80..+0x9c`, `+0xd0`) on either domain received id 119; B saw only `+0x80` (ids up to 103) and one `+0x9c` (id 93) (`private-data/scratch/EmbassyAlloc/census-flag-ids.json`). The stock queue destructor `0x724f08(n)` overwrites task n-1's flag record through `+0xd0` (E-IMAGE4-QUEUE-DESTRUCTOR-POWEROFF-150), so a B call with n = 120 would reach task 119's flag; only A calls with 8 and 41 are observed, and no static search for B callers passing 120 was made.
  - **Wrapper call sites:** none of the 67 static call sites of `0x725dd8` passes an id above 104. Every id is a constant or comes from a static table; the only runtime case is a worker table bounded to 88..92, or 0..4 after its cleanup.
  - **Class 1:** ids 105..120 belong to the allocator `0x728804`, so A takes one from it instead of a fixed id. A then holds one of the allocator's 16 ids for the session.
  - **Class 2:** no traced path creates 119 in class 2.
  - **Not established:** whether the allocator runs on other routes, any indirect caller of the allocator or the wrapper, and the word at `0x96bd28` that a class-3 lookup would use.
  - **Other checks:** the class table `0x7cad9c` and the stack check `0x69cc18` cover ids 1..104.
- **Waker:** the task wakeup queue. The host loop polls, takes the next deadline from the integrated timer queue and sleeps through `+0x28` with that deadline rounded up to whole ticks, or -1 when there is none. The pender sets a `pending` flag when the host task itself is the caller, and otherwise calls `+0x2c` from a task or `+0x30` from a handler. The one-deep queued wakeup closes the check-then-sleep race. Eventflags (`+0x88/+0x8c/+0x9c`) stay unused until their lower tuples are recovered.
- **Time:** `now = 30000 × tick cell 0x400c0800 (extended to 64 bits) + sub-tick from [0xfff91000]`. The tick cell is read before and after the counter and the read retried while the two differ, and each domain clamps the result to be monotonic while counting how often and by how much the raw reading fell back. The counting direction is a build feature (`counter-up` for the camera and the default FujiMod model; `counter-down` is the mismatch control). At entry each host measures it, and a mismatch drops that domain to tick-only time. No timer register is written.
- **Critical sections:** `critical-section` over the stock CPU-lock slots `+0x130/+0x134` and `+0x138/+0x13c`, selected by `+0x148`, restoring the prior CPSR.I. The two cores' data caches are not coherent, so each domain's executor state, task storage and stack sit in 32-byte-aligned padded blocks written by one core only, and cross-core words (the demonstration record, the doorbell) go through the uncached alias.
- **Interrupts:** `kernel::define_interrupt/enable_interrupt/disable_interrupt` over `+0x160/+0x158/+0x15c`; phase 1 does not use them.

**Emulated result (E-EMBASSY-PHASE1-EMULATED-150).** One run of the demonstration and one of each control on the stock DAT, cardless FujiMod RAM-push route under the up-counting default model, with the modal75 FILM SIMULATION trajectory, all at main `82a8af1b`. The demonstration object is the `counter-up` build `8888b6d1…`. The reviewed checker `replacement/embassy_platform/check-demo.jq` accepts all four runs. They link their named objects with the same prelinked source digests, trajectory, hooks and builder identity, and they start from one checkpoint key (two cold, two restored).

The current host entry point is `//replacement/embassy_platform:check-demo`. It accepts embedded observations and verified observation-document/trace-sidecar references. Wake events are streamed into the same demonstration and control predicates.

```sh
devenv shell -- host-build run --jobs=2 //replacement/embassy_platform:check-demo -- \
  --demo DEMO_RECEIPT --no-executors NO_EXECUTORS_RECEIPT \
  --no-pender-kernel NO_PENDER_RECEIPT --counter-down COUNTER_DOWN_RECEIPT \
  --objects OBJECT_DIGESTS_FILE
```

- **Demonstration:**
  - Install, route and unload pass with every step matched and no forbidden fetch.
  - The allocator returned id 120 for A, B's three statuses are 0, and both host tasks entered.
  - Both domains measured the counter counting up and kept sub-tick time, with no clock regression.
  - By handback each 100 ms ticker had run 34 iterations over a 331-tick span, and both kept running after unload.
  - The latest wake was 53,180 counts (about 18 ms) on A and 39,718 on B, none later than two ticks. No kernel wake came early.
  - A rang B's doorbell 3 times and B received all 3 through the kernel wakeup. The observer traced 4 `+0x2c` calls with B's id 0x00020077, and the worst delivery latency was 42 counts.
  - The observer traced 39 `+0x28` calls on core 0 and 43 on core 1. The stock route makes none (E-RTOS-SLOT-CENSUS-STOCK-150).
  - Stack high-water was 304 bytes on each domain; A's is a lower bound, since the allocator fills its stack with zeros.
- **No executors:** the same hook and route with nothing created produce the same four captures as the demonstration (PNG and source-plane digests).
- **No pender kernel:** the tickers keep running, and B receives none of its 3 doorbells, with no traced `+0x2c` for B.
- **Counter down:** both domains measure the up-counting model, fall back to tick-only time and keep their timers on schedule, and B still receives every doorbell.

**Limits:**

- These are emulated coexistence demonstrations on a model with no caches, not kernel or silicon qualification.
- B's timeouts on hardware depend on line 32 reaching core 1 (laws U4).
- Wake resolution is one tick.
- The host tasks keep running after `ram-unload`, and A holds one of the allocator's 16 class-1 ids for the session.
- Before any camera run, the payload must be the `counter-up` build, and the push and run need the owner's go. Core 1's instruction cache over the payload needs nothing beyond the install: the shared installer runs stock `0x006b0c3c` and `0x0061c490` on core 1 over the whole image, then `peer_cache_action` on core 1 ends in DSB/ICIALLU/DSB/ISB, and core 1 repeats ICIALLU after GO (`menu/ram_push_installer.c:199-206,239-289`; E-RAM-PUSH-INSTALL-150).

### 4.13 Owned thread layer and stock-ABI shim: design input from the decomp

Source: x70view `decomp/xm1-coverage` at `55436fc85c0560dd252b0410af6f2427fe198cd3`, `decomp/firmwares/xm1-150/kernel0` (loader A) and `kernel1` (loader B). Kernel0 and kernel1 are the same C apart from the domain constant and the differences listed in private-data/scratch/RtosDecomp/crosscore/design.md §1.4. Facts below come from byte-exact functions unless marked "(stock disassembly)" (no C body) or "(NONMATCHING, confirmed)" (a draft checked against the stock disassembly of both loaders). Contracts per slot are in manifest v6 (E-RTOS-SLOT-MANIFEST-V5-150). Object layouts are in the [manifest draft](rtos-platform-manifest-draft.md#kernel-object-layouts-from-the-decomp).

**One scheduler with two run queues.**
- Each kernel keeps two context records (0x84 bytes; A 0x400545cc, B 0x400e458c), one per domain. `kernel_initialize_owner_contexts` (A 0x43ac0 / B 0xd3a7c) builds them after the startup barrier (order from NONMATCHING `kernel_start`, confirmed) from the shared configuration entries at 0x400c0818 (A) and 0x400c0888 (B).
- The remote record points straight into the other kernel's private RAM: ready lists, bitmap, selected and running cells, dispatch-enabled word, timer heap, clock words, task records and lock words.
- Any service given a class-2 id from A code therefore links tasks into B's ready queue, timer heap or object queues itself.
- So one domain's scheduler can be owned alone only if those structures keep their stock addresses and semantics. The clean cut is to own both domains' schedulers together.

**Locks.**
- Each domain has a scheduler lock (A 0x400545c0 / B 0x400e4580) and an object lock (A 0x400545c4 / B 0x400e4584).
- Both are taken through acquire A 0x439d8 / B 0xd398c (stock disassembly), which spins with IRQ masked until it holds the word and always returns 0. Every "acquire failed, retry" branch in the C is unreachable. Release (A 0x43a14 / B 0xd39c8, stock disassembly) is DMB, store 0, DSB.
- Lock order is global lock, then object lock, then the waiter's scheduler lock (selected by T+0x20).
- Paths that must take a lock out of order drop and retake, and use the T+0x24/+0x28/+0x2c handshake flags to detect that another path finished the job.

**Global lock 0x400c0804.**
- The IRQ vector of each core (asm, stock disassembly A 0x40258..0x4030c) takes it for the whole handler, with tag 0x11 (core 0) or 0x12 (core 1), and nests on its own tag. The image4 critical pair takes it with tags 0x21/0x22 through `+0x161c..+0x1624`, whose trylock and unlock (A 0x427d4 / 0x427fc) are NONMATCHING, confirmed. `+0x161c` operates on the lock word in r0; 0 selects 0x400c0804.
- [INFERENCE, pending cross-core exclusive-monitor behaviour, laws U12] Every interrupt handler of both cores and every image4 critical section is therefore serialized. A holder in task context must keep IRQ masked on its own core until release, or an IRQ on that core spins forever with IRQ masked.
- Owned handlers installed through `+0x160` run inside this lock.

**Cross-core make-ready.**
- The waker takes the target domain's scheduler lock and inserts the task itself.
- When the target's selected cell changed and the target has dispatch enabled, it raises IRQ 120 on the target core (controller bank 0xfffb3000 or 0xfffb4000, offset 0xcc) before releasing the lock. A local target returns without a doorbell.
- The receiver's handler (A 0x40b00 / B 0xd0b00, stock disassembly) clears the request and sets the delayed-dispatch word. The outermost IRQ exit then switches if the selected cell differs from the running cell.
- All decisions are the sender's; a doorbell that finds nothing to do costs one IRQ.
- Stock uses this path in both directions. In the census, most cross-domain calls are data-queue sends, plus wakeups from B to A and activations from A to B (E-RTOS-SLOT-CENSUS-STOCK-150).

**Selected-cell race.**
- Either core writes a domain's selected cell (A 0x40054768 / B 0x400e4728), always under that domain's scheduler lock. The owning core's dispatcher and IRQ-return path read it without the lock.
- While the other core changes things, the running task keeps running until its doorbell arrives. Every service that blocks or acts on the running task re-checks the running task's ready bit under the lock, and on a clear bit unmasks IRQ so the pending doorbell preempts first.
- [INFERENCE] No barrier separates the sender's scheduler stores from the doorbell store. Whether the receiver can observe the doorbell first depends on memory attributes that are not recovered. An owned layer should issue DMB after the scheduler stores and DSB before the doorbell.

**Hazards an owned layer must not inherit.**
- The stock dispatcher's empty-selection branch uses r4..r6 that only its C entries load. An outermost IRQ exit with a pending reschedule request, an empty selected cell and dispatch enabled reaches it with the interrupted task's registers, writes 1 to an arbitrary address and loads an arbitrary mode. While the stock vector and dispatcher run, each domain needs one ready, never-blocking task; stock uses idle tasks 102/103.
- [INFERENCE from exact code] The tick pops a timer entry without clearing its index, and the timeout callback drops the scheduler lock with IRQ unmasked. A waker in that window removes the popped entry again, deleting another heap entry or underflowing the count. The zero-token repair owns both tick and remove: invalidate the popped entry before its callback, make removal of index zero a no-op, and invalidate an entry removed normally. Stock removal decrements the count even for index zero, so changing only the pop is insufficient. The callback's wait-result word is not the membership token. Source: x70view `55436fc85c0560dd252b0410af6f2427fe198cd3`, both kernels' `src/time/timer_tick.c` and `src/time/timer_heap.c` (E-RTOS-TIMER-MEMBERSHIP-SOURCE-150). The owned host/native repair and remaining firmware integration obligations are recorded in §4.15.
- Deleting an active cyclic handler (`+0x10c`) makes the tick loop and call address 0; a zero period makes the tick loop on its handler.
- The interrupt-context eventflag set `+0x8c` readies a satisfied waiter through the flag's domain rather than the waiter's own.

**Tick.** Only core 0 programs the timer; both cores define and enable IRQ 32 (NONMATCHING `kernel_start_tick_timer`, confirmed against A 0x40b5c..0x40c54 / B 0xd0b5c..0xd0c4c). Whether line 32 reaches core 1 on hardware is open (laws U4). The stock ISR (A 0x40b24 / B 0xd0b24) reaches the tick body only through the published `+0x104` word, so republishing that word redirects the stock tick. Each domain processes only its own heap, 10 clock units per tick (about 10 ms at the observed 99.88 Hz, E-XM1-TIMER-IRQ-LAWS-150).

**Slot classes for the shim** (per domain, the same for A and B; private-data/scratch/RtosDecomp/crosscore/classification.tsv; reviewed):

| Class | Slots | Reason |
| --- | --- | --- |
| Keep stock-exact | `+0x0`, `+0x4`, `+0x161c`, `+0x1620`, `+0x1624` | Stock code reads the task records raw (stack check `0x725b74`, class table `0x7cad9c`, root descriptors); the global lock and its tags are shared with both IRQ vectors and the critical pair |
| Own first | `+0x60`, `+0xfc/+0x100` as a pair, `+0x128`, `+0x12c`, `+0x130..+0x13c`, `+0x148..+0x154`, `+0x164`, `+0x168`, `+0x16c`, `+0x1618` (17) | CPU-local, or lock-free reads of stock words whose layout the owned body keeps. `+0x16c` also has direct callers (image4 0x72b00c/0x72b068, loader A 0x416a8) that bypass the slot |
| Depends on owning something first (75) | All task, synchronization, queue, pool, time, cyclic and rotation services, plus dispatch control `+0x140/+0x144` and the IRQ slots `+0x158..+0x160` | Units: both schedulers together; timers with the tick; the IRQ vectors with the controller programming; each object family in both domains. Eventflags and mailboxes move together, because `+0xd0` on mailbox n writes flag n-1's record |

**Consequences for the plan.**
- The phase-1 embassy hosts (§4.12) already use exactly these paths: B's pender wakeup from A is the cross-core make-ready, and each host's sleep inserts into its own heap.
- The 17 own-first slots are owned in both domains, as Rust bodies linked in place at the stock bodies (`replacement/rtos_own_first`) and staged as a standalone leaf (E-RTOS-OWN-FIRST-STAGE-150).
  - In place is required. Five of the slots have direct `bl` callers that bypass the root word in each loader (+0x148 and +0x128 from +0x16c, +0x16c from 0x416a8/0xd16a8, nine writers each into +0x164 and +0x168), and image4 calls A's +0x16c on either core.
  - The native-case oracle matches stock on all 6,584 cases per loader, with all 72 controls holding (E-RTOS-OWN-FIRST-ORACLE-150).
  - The from-reset natural pair qualifies both roles over about 661,000 slot calls and gives equal route and menu states. The extent readback and the per-call semantics each refuse their guest control (E-RTOS-OWN-FIRST-NATURAL-150).
  - Stock task resume never restores a task's A bit and takes F from the current CPSR (0x4034c-0x40364, 0x437c0-0x437d8), so a preempted slot call can return with A changed.
  - The leaf does not compose with the startup_artifact pieces yet: their footprints cover the extents, and three witnesses bind the stock scratch registers.
  - The next step is the scheduler of both domains together, behind the keep-exact task-record and global-lock slots. It keeps the stock structures at their stock addresses, so retained object, timer, vector and task-record code keeps working unchanged. It is built in four steps, each qualified alone: S0 (runner extensions, a cross-core exclusive-monitor probe, a shared layout crate), S1 (the nine scheduler helpers behind their stock entries), S2 (dispatcher and IRQ 120 doorbell), S3 (the scheduler-only services). Owned prepare adds a DMB between its T+0x18/+0x1c stores and the ready insert, and owned notify adds a DSB before the doorbell.
- Owned code may assume no failed lock acquisition, but it must preserve the global-lock tags and the one-ready-task rule while any stock vector remains.

**Intentional differences from stock (S2, approved).** Each is a typed redesign item. Its native case shows stock's defective result and the owned fixed result as an expected difference, keyed by the id. A case or natural call is tagged with an id only when the trigger holds; untagged cases must match stock.

| Id | Stock | Owned | Trigger | How a retained dependence would show |
| --- | --- | --- | --- | --- |
| `null-selection-own-registers` | The null-selection branch (A 0x436ec..0x43724) stores 1 and 0 through r6 and loads CPSR from r4/r5. Only 0x43640/0x43688 preload them, so an entry from 0x4375c stores through the interrupted task's r6 and takes the mode in its r4/r5 | The null branch loads the context-word address, the idle stack and the SVC modes itself; 0x43640/0x43688 drop the r4-r6 preloads | Selected cell 0 at select, entered from 0x4375c (outermost IRQ exit, reschedule requested, dispatch enabled) | A stray store of 1 or a non-SVC mode in stock only. Elsewhere only r4-r6 differ at a dispatch, and no next instruction reads them (the stubs pop r4-r11, the trampoline sets r0, lr and CPSR). A retained reader would show as a task reading r4-r6 at its first entry |
| `null-loop-releases-leaked-global-lock` | An IRQ in the null loop's unmask window sees context word 1, runs nested, takes 0x400c0804 with tag 0x11/0x12 and never releases it, because stock releases only on the outermost exit (0x4030c). The other core's IRQs and the image4 0x21/0x22 sections spin until this core's next outermost exit | After each window, with IRQ masked and the loop's context word 1, if 0x400c0804 holds the build's tag (A 0x11, B 0x12), issue a DMB and release it through stock unlock 0x427fc/0xd27fc | Selected cell 0, and 0x400c0804 holding this core's IRQ tag after a window with context word 1 | 0x400c0804 still holds the tag at the next loop iteration in stock and is 0 in owned. The other core's vector spins in its observe loop (0x40258..0x4029c) in stock and takes the trylock path in owned |
| `remote-terminate-restarts` | +0x18 from the other core on a running task with a queued activation sets T+0x30 = 1 and prepares a restart. The running core resumes the old frame when selected == running at 0x4375c, because T+0x30 is tested only afterwards at 0x43784. Or 0x43640 saves the live SP/PC over the prepared T+0x18/+0x1c | 0x4375c keeps the dispatch-disabled tail, then tests T+0x30 before the selected cell and dispatches without saving. 0x43640 skips the save when T+0x30 = 1. 0x43668 and 0x437a4 stay at their stock addresses | At 0x4375c: dispatch enabled, T+0x30 = 1 and selected == running. At 0x43640: T+0x30 = 1. Evaluated on the loaded values | The task's next execution resumes at its interrupted PC in stock and enters the trampoline 0x43728 in owned. Residual, on the object side and no worse than stock: a blocking call the stale frame makes before the doorbell moves the prepared task into an object queue. The restart then waits on that object, the first signal is consumed by the dead wait, and on an untimed wait that is never satisfied the activation is lost |

**Merged scheduler native qualification.** The S1, S2, S3a, S3b and S3c implementations are merged into one tree. Rerunning the S1 oracle passes 5,086 cases per loader and all 46 controls (E-RTOS-SCHED-S1-ORACLE-150). S2 passes 560 cases per loader, including 436 exact comparisons and the three approved expected-difference groups: 72 `null-selection-own-registers`, four `null-loop-releases-leaked-global-lock` and 48 `remote-terminate-restarts`; all 22 control groups per loader hold (E-RTOS-SCHED-S2-ORACLE-150). S3a passes 3,271 cases per loader with all 42 control groups across both loaders (E-RTOS-SCHED-S3A-ORACLE-150). S3b passes 664 cases per service per loader, 7,968 owned comparisons total, with every equivalent matching and every incorrect-result mutant refused (E-RTOS-SCHED-S3B-ORACLE-150). These are isolated native boundaries, not the full combined scheduler natural-pair gate.

**Natural scheduler cutover qualification (E-RTOS-SCHED-CUTOVER-NATURAL-150).** The from-reset stock-10 and stage-1 candidate-3 roles qualify with the same observer executable, SHA256 `69d5cb454ff143ece4387f9896648824402e4535b587543bbc7fa8a1e8da64f9`. The candidate DAT is `47c95e9ca3e7f3881fb1c04c6cc698f716206cc1286ef621d96a68394822369f`. The shooting-menu and packaged FILM SIMULATION route/menu signatures match. Stock completes 1,054,071 modeled observations in 842,322,166 aggregate entered instructions; owned completes 1,068,808 in 833,936,664. Both complete all entered observations, with no pending calls, byte-guard fault, untracked continuation or machine/observer error. All completed classes satisfy their stock contracts, with the approved S2 redesign rules retained. No real semantic or route mismatch remains, so incremental-subset localization was not needed. Counts and instruction interleavings are recorded, not required to be equal.

**First-entry finding.** The stock trampoline loads argument C+4 and PC C+8 through configuration `0x4005bf24`; the target word is the low spelling `0x1215480`, not an alias. Both per-core RAM maps expose the low and `0x40000000` spellings of the backing. The attempted load-to-PC at `0x43758` takes a permission prefetch abort at `0xc` before a target instruction point can fire. The stock diagnostic retains IFSR 13, SPSR `0x53`, IFAR/`LR_abt-4` equal to C+8, unchanged r0-r12/task/core and exactly one faulting-core instruction since the load. The matched pair each qualify one such attempted-transfer witness and a later same-task/core task-context service entry. This is not a claim that a target-body instruction executed at that boundary. Other exception transitions refuse, and proof-to-abort correspondence is bijective.

**Effect attribution.** Task continuations remain armed in both RAM spellings. Captured writes retain their actual core, PC, SP, CPSR, task, kernel context and core instruction count; bank attempts reconcile one-for-one with accepted controller writes. Invocation effects satisfy the model, while every captured foreign/IRQ write remains in raw replay to the next resource snapshot. Same-core foreign effects require the validated save/resume envelope. CPSR.A is excluded only across that task/core's observed C frame restoration; native no-IRQ comparisons remain strict. The earlier stock-9 receipt is retained as an unqualified attribution diagnostic.

| Role | Core | Entered instructions | Cross-domain inserts | Ringing notifies | Bank writes | IRQ 120 entries |
| --- | --- | --- | --- | --- | --- | --- |
| stock | 0 | 632,958,198 | 749 | 747 | 1,486 | 735 |
| stock | 1 | 209,363,968 | 739 | 735 | 1,482 | 747 |
| owned | 0 | 620,237,853 | 765 | 762 | 1,520 | 754 |
| owned | 1 | 213,698,811 | 756 | 754 | 1,516 | 762 |

The PC watches at `0x436ec`, `0xd36ec` and image5 direct-ring site `0xec8014` are zero on both cores in both roles. IRQ 120 entry watches are `0x40b00` on core 0 and `0xd0b00` on core 1, with the counts above; the opposite-core entry watches are zero. Each of the three approved S2 redesign triggers has zero qualified calls in both roles. This route does not exercise those repairs.

**Guest controls.** All six frozen control roles are independently recognized by the fresh consumer. The three restore controls fail extent readback. Silent-notify's first remote A notify is at 6,472,686; the enclosing activation entered at 6,472,579 also fails `BankWrites`. Wrong-predicate's first B/id1 notify at 12,862,224 returns 1 without a bank write, violating B's normal `ReturnValue` contract. Its enclosing wake entered at 12,862,088 and takes the induced save at 12,862,242 with reason -19, resume at 12,862,267 and return at 12,862,278. Its baseline refusal is `ProcessorState`, not an ignored A-bit difference. Control-only adjudication derives the defective unsigned `id <= 1` predicate from the frozen words and rechecks the full same-domain service, including frame, flags, reason, writes and completion. Independent earlier faults remain refusals; production qualification uses the stock predicate. Wake-no-insert fails `WrittenWords` at its first wake, 29,529,704.

| Control | Recognized refusal |
| --- | --- |
| restore-s1 | Readback |
| restore-s2 | Readback |
| restore-s3 | Readback |
| silent-notify | BankWrites, with the first missing notify and its enclosing activation bound |
| wrong-predicate | First B/id1 ReturnValue defect and precisely modeled enclosing ProcessorState consequence |
| wake-no-insert | WrittenWords |

**Per-entry completed observations.** The table includes all 86 planned entries; 48 are reached per role and the other 38 have zero counts. Dispatcher checkpoints and terminal discards count as completions under the observer contract. Zero means unobserved, not retired. The full machine-readable counts, per-core watches, redesign tags and control verdicts are in `private-data/scratch/SchedCutover/natural-cutover-summary.json`, alongside the complete lossless raw receipts and independent qualification/comparison outputs.

Paired entry addresses are listed in stock / owned order.

| Semantic | A entry | B entry | A stock | A owned | B stock | B owned |
| --- | --- | --- | --- | --- | --- | --- |
| helper/acquire | `0x439d8` | `0xd398c` | 334,227 | 338,346 | 7,813 | 7,979 |
| helper/release | `0x43a14` | `0xd39c8` | 334,227 | 338,346 | 7,813 | 7,979 |
| helper/find | `0x43c48` | `0xd3c04` | 43,387 | 44,168 | 765 | 781 |
| helper/insert | `0x43ca4` | `0xd3c60` | 38,994 | 39,647 | 741 | 758 |
| helper/remove | `0x43d20` | `0xd3cdc` | 38,983 | 39,635 | 749 | 765 |
| helper/reset | `0x43dd0` | `0xd3d8c` | 121 | 121 | 120 | 120 |
| helper/prepare | `0x43f70` | `0xd3f2c` | 82 | 82 | 2 | 2 |
| helper/notify | `0x44018` | `0xd3fd4` | 19,409 | 19,750 | 735 | 754 |
| helper/requeue | `0x44044` | `0xd4004` | 14,846 | 15,076 | 24 | 24 |
| helper/rotate | `0x44134` | `0xd40f4` | 687 | 657 | 0 | 0 |
| irq/doorbell | `0x40b00` | `0xd0b00` | 735 | 754 | 747 | 762 |
| dispatch/save | `0x43640` | `0xd3640` | 40,470 | 41,111 | 749 | 765 |
| dispatch/resume | `0x43668` | `0xd3668` | 40,398 | 41,040 | 741 | 757 |
| dispatch/no-save | `0x43688` | `0xd3688` | 2 | 2 | 1 | 1 |
| dispatch/select | `0x436ac` / `0x436a4` | `0xd36ac` / `0xd36a4` | 58,367 | 59,369 | 1,497 | 1,528 |
| dispatch/first-entry | `0x43728` | `0xd3728` | 74 | 74 | 10 | 10 |
| dispatch/irq-return | `0x4375c` | `0xd375c` | 17,895 | 18,256 | 747 | 762 |
| dispatch/irq-resume | `0x437a4` | `0xd37a4` | 17,895 | 18,255 | 746 | 761 |
| dispatch/irq-tail | `0x437a8` | `0xd37a8` | 17,895 | 18,255 | 746 | 761 |
| service/exit-task | `0x453f0` | `0xd53b0` | 1 | 1 | 0 | 0 |
| service/rotate-ready | `0x4c28c` | `0xdc24c` | 687 | 657 | 0 | 0 |
| service/interrupt-rotate-ready | `0x4c36c` | `0xdc32c` | 0 | 0 | 0 | 0 |
| service/core-rotate-ready | `0x4c434` | `0xdc3f4` | 0 | 0 | 0 | 0 |
| service/interrupt-core-rotate-ready | `0x4c530` | `0xdc4f0` | 0 | 0 | 0 | 0 |
| service/disable-dispatch | `0x4c9dc` | `0xdc99c` | 220 | 223 | 0 | 0 |
| service/enable-dispatch | `0x4cac0` | `0xdca80` | 220 | 223 | 0 | 0 |
| service/activate-task | `0x44f2c` | `0xd4eec` | 82 | 82 | 2 | 2 |
| service/interrupt-activate-task | `0x450f0` | `0xd50b0` | 0 | 0 | 0 | 0 |
| service/cancel-activation | `0x45270` | `0xd5230` | 0 | 0 | 0 | 0 |
| service/wake-task | `0x46004` | `0xd5fc4` | 1,216 | 1,180 | 12 | 12 |
| service/interrupt-wake-task | `0x46200` | `0xd61c0` | 469 | 456 | 0 | 0 |
| service/cancel-wakeup | `0x46398` | `0xd6358` | 0 | 0 | 0 | 0 |
| service/deliver-task-exception | `0x441b4` | `0xd4174` | 0 | 0 | 0 | 0 |
| service/deliver-task-exception-entry | `0x44288` | `0xd4248` | 0 | 0 | 0 | 0 |
| service/get-task-priority | `0x45b40` | `0xd5b00` | 6,025 | 6,111 | 0 | 0 |
| service/sleep-task | `0x45cd4` | `0xd5c94` | 1,697 | 1,648 | 0 | 0 |
| service/suspend-task | `0x46a00` | `0xd69c0` | 0 | 0 | 0 | 0 |
| service/resume-task | `0x46c80` | `0xd6c40` | 0 | 0 | 0 | 0 |
| service/force-resume-task | `0x46e38` | `0xd6df8` | 0 | 0 | 0 | 0 |
| service/raise-task-exception | `0x46fcc` | `0xd6f8c` | 0 | 0 | 0 | 0 |
| service/interrupt-raise-task-exception | `0x471b0` | `0xd7170` | 0 | 0 | 0 | 0 |
| service/disable-task-exception | `0x47354` | `0xd7314` | 0 | 0 | 0 | 0 |
| service/enable-task-exception | `0x47428` | `0xd73e8` | 0 | 0 | 0 | 0 |

The qualification is finite emulated evidence for this route and these contracts. It does not establish hardware/cache ordering, unused-entry behavior, general stack capacity or composition with the separate startup-artifact pieces. No camera was used.

### 4.14 Owned eventflag and mailbox family: the recovered `+0xd4` body and the family design

Sources: x70view `55436fc85c0560dd252b0410af6f2427fe198cd3`, `decomp/firmwares/xm1-150/kernel0` (A) and `kernel1` (B); the pinned loaders `private-data/analysis/x-m1-1.50/decode/loader-40000.{bin,dis}` (A) and `loader-d0000.{bin,dis}` (B); manifest v6 (E-RTOS-SLOT-MANIFEST-V5-150) for the other 13 slots of the family; census-v1 (E-RTOS-SLOT-CENSUS-STOCK-150) for reach; E-IMAGE4-QUEUE-DESTRUCTOR-POWEROFF-150 for the destructor. **Exact** = a byte-exact decomp function. **Confirmed** = a NONMATCHING draft or a bodyless declaration checked instruction by instruction against both loaders' stock disassembly. **[INFERENCE]** = design conclusion, not an observation. The NONMATCHING `mailbox_send` C was read only as a hypothesis; every fact in §4.14.2 and §4.14.3 was re-derived from the bytes.

#### 4.14.1 The two families are one ownership unit

The record tables are laid out so that one family's writes land in the other's records, in both domains:

| Record | A | B | Stride | Count |
| --- | --- | --- | --- | --- |
| mailbox configuration | `0x4005d16c` | `0x400ed12c` | 12 | 1 |
| eventflag configuration | `0x4005d178` | `0x400ed138` | 12 | 120 |
| eventflag control block | `0x400624b4` | `0x400f2474` | 16 | 120 |
| mailbox control block | `0x40062c34` | `0x400f2bf4` | 20 | 1 |
| eventflag wait record | `0x40055fdc` | `0x400e5f9c` | 24 | 120 |
| eventflag timer entry | `0x40055a3c` | `0x400e59fc` | 12 | 120 |
| mailbox wait record | `0x4005783c` | `0x400e77fc` | 16 | 120 |
| mailbox timer entry | `0x40057fbc` | `0x400e7f7c` | 12 | 120 |

- `0x4005d16c + 12 = 0x4005d178`, so **mailbox configuration n is eventflag configuration n-1**. The flag table ends at `0x4005d178 + 120*12 = 0x4005d718`, the domain's timer heap, so flag id 121 writes the heap.
- `0x400624b4 + 120*16 = 0x40062c34`, so eventflag control block 121 is mailbox control block 1. Mailbox control block 2 is `0x40062c48`, past the one-entry table and inside the boot stack below its top `0x40063460`.
- Counts are loader constants, mailbox 1 and eventflag 120 (`[0x4dab4]`/`[0x4dac4]` in A, `[0xdda78]`/`[0xdda88]` in B), read by `mailbox_initialize` A `0x4a9bc` / B `0xda97c` and `event_flag_initialize`, published into the shared configuration at `0x400c0818`/`0x400c0888` and copied into each context record's `+0x54/+0x58` (flags) and `+0x64/+0x68` (mailboxes).
- `kernel_delete_mailbox` (`+0xd0`, exact) indexes `uxth(r0)-1` with no class, range or existence check, writes `{2, 32, 0}` to that configuration record and a self-link plus `+0xc = 0` to that control block. Image4's queue destructor `0x724f08(n)` calls it with its **data-queue** id `(0x7cad9c[n-1] << 16) | n`, so on the observed POWER OFF route `n = 8` and `n = 41` rewrite A eventflag configurations 7 and 40 from `{4, 0, 1}` to `{2, 0x20, 0}` (E-IMAGE4-QUEUE-DESTRUCTOR-POWEROFF-150). The data queue is never deleted, because `+0xa4` is not called.
- External callers use ids only: task create `0x725dd8` calls `+0x80` with the task's own id, task delete `0x72612c` calls `+0x84`, the destructor calls `+0xd0`, and the flag wrapper `0x724480` calls `+0x9c`. The message constructor `0x724cdc`, send `0x72503c` and receive `0x725318` use the data-queue `+0xa0`/`+0xac`/`+0xc8` and pool `+0xe4`/`+0xf8` families instead; no stock wrapper calls `+0xd4`, `+0xd8`, `+0xdc` or `+0xe0`.
- A class-2 id passed from A code resolves to B's tables through B's context record, so A code writes B's records. That is the scheduler's cross-domain rule again, and it makes the family one unit in both domains.

#### 4.14.2 `+0xd4`: byte extent

| Domain | Target | Start | Bytes | End | SHA-256 | Fence |
| --- | --- | --- | --- | --- | --- | --- |
| A | `+0xd4` | `0x4aa44` | 772 | `0x4ad48` | `7f49745856203fe1beee899c193344fb140a1832269b9e451f4272914143f1a8` | `0x4ad48` |
| B | `+0xd4` | `0xdaa04` | 772 | `0xdad08` | `aa5d81982986f242cbe97a5a92951793722c27d5f41fd149153ceabde6697e42` | `0xdad08` |

Both hashes equal the `function-ranges.json` row hash at the pinned commit (`FUN_0004aa44`, `FUN_000daa04`, 772 bytes), recomputed here from our loaders at offsets `0xaa44` and `0xaa04`; the loader section hashes are the pinned `21366fff7cebc617…` and `8fb0c322a22443a3…`. The fence is the next published entry, and it coincides with the measured end.

Boundary proof:

- 193 four-byte instructions exactly fill each window; no literal pool, no Thumb.
- The window's only entry point in a linear decode of the loader's own bytes is `push {r4-r11, lr}` at `0x4aa44` / `0xdaa04`: the two published neighbours end before it, and no direct branch anywhere in that loader targets an instruction inside the window. The scan establishes no computed, restored or aliased entry, so an indirect arrival inside the window stays unresolved; the extent claims the byte range, not whole-entry closure.
- One exit in the same linear decode: the only PC load is `pop {r4-r11, pc}` at `0x4ad18` / `0xdacd8`, and `sub sp, r11, #32` immediately precedes it on every path.
- Every conditional and unconditional branch target encoded in the window lies inside it; the last instruction is `b 0x4acfc` at `0x4ad44` / `b 0xdacbc` at `0xdad04`.
- `0x4ad48` / `0xdad08` opens the next function with its own frame (`push {r3-r11, lr}`), the `+0xd8` receive body.
- A and B differ in exactly 10 of 193 encodings: four `movw`/`movt` pairs (`0x400545a4→0x400e4564`, `0x400545cc→0x400e458c` twice, `0x400545c8→0x400e4588`) and two `bl` displacements (`0x44018→0xd3fd4`, `-0x6c28` against `-0x6c24`; `0x43640→0xd3640`). The `bl 0x44568→0xd4528` and all eight indirect calls encode identically, because the bodies are exactly `0x8ffc0` apart. There is no behavioural difference between the domains.
- The body makes eight indirect calls through a context record: object-lock acquire/release `[record+0x18]`/`[record+0x1c]` at `0x4ab7c`, `0x4abe8`, `0x4ac50`, `0x4ad24`, `0x4ad40`, and scheduler-lock acquire/release `[record+0xc]`/`[record+0x10]` at `0x4abd4`, `0x4ac44`, `0x4ad34`.

Why the manifest still refuses the extent: `extent.status` is `unknown` with `refusal: "PC load is not proven to be the incoming return at 4ad18; premise uses: [4ab7c=unexposed-caller-frame-isolation, 4ad24=unexposed-caller-frame-isolation]"` (B: `dacd8`, `dab3c`, `dace4`). StockInventory cannot prove that an indirect call through a function pointer held in a context record returns to its caller, so the saved-LR slot may have been clobbered before the `pop`. The refusal is a tool premise, not a byte fact.

Exact remaining prerequisite, one of:

1. A byte-exact `mailbox_send` in x70view at a pinned commit. The current marker is `/* NONMATCHING: 54 instructions: block order and a spilled copy of the mailbox pointer */`. The row that would then enter `replacement/media_permit/rtos_decomp_extents.json` is `{domain: A, start: 0x4aa44, bytes: 772, sha256: 7f497458…, name: mailbox_send, path: decomp/firmwares/xm1-150/kernel0/src/sync/mailbox_send.c}` plus its B twin, and the contract's `decomp` entry for `0xd4` must flip `exact` to true.
2. A reviewed `ReturnProvenance` premise that an indirect call through a context record's `+0x18`/`+0x1c` or `+0xc`/`+0x10` returns to its caller. It must admit the construct, not the two named sites, because `0x4abd4`, `0x4ac44`, `0x4ad34` and `0x4ad40` are the same thing.
3. A reviewed byte-boundary extent source independent of the return proof, in the shape of the existing `rtos_function_boundaries.json` rows, with `{boundaryDomain: A, boundaryStart: 0x4ad48}` and the basis text of the proof above.

Until one lands the extent stays `unknown`, and no slot contract may cite it as `decomp-exact`. The engineering extent is settled, so the F1-F3 stages may use it as a region.

The same method settles the other refused extent, which belongs to the data-queue family rather than this one: `+0xc0` is A `[0x49f2c, 0x4a2f0)` and B `[0xd9eec, 0xda2b0)`, 964 bytes, SHA-256 `e45bfbf917a5053c…` and `6ec7e61cc9ecf064…`, 241 instructions, one entry point in the same linear scan, one `pop {r4-r11, pc}` at `0x4a2cc` / `0xda28c`, every encoded branch target internal and only `bl` transfers out. It is recorded here only so that family starts from the same evidence; its refusal rests on the same unproved-return class, with more premise kinds.

#### 4.14.3 `+0xd4`: the contract

Signature `int mailbox_send(int id, struct message *message)`; `r0 = (class << 16) | n`, `r1` = message pointer; returns 0. Admission, in stock order and all before any lock:

1. context word `A 0x400545a4 / B 0x400e4564` nonzero → `-25`.
2. `CPSR.I` set → `-25`.
3. `class = r0 asr 16`; `(class-1) as u16 <= 1` selects class 1 or 2 (record `0x400545cc`/`0x400e458c` + `(class-1)*132`); class 0 selects the executing record `*0x400545c8`/`*0x400e4588`; anything else `-18`.
4. `n = r0 as i16`; `n <= 0` or `n > record+0x68` → `-18`. The bound is 1 in both domains.
5. `config = *(cb+0x8)`; `config+0x8` (exists) zero → `-42`.
6. Attribute bit1 set and (`message+0x4 as i32 <= 0` or `> config+0x4`) → `-17`.

The dispatch-enabled word is **not** read: a send proceeds with dispatch disabled, unlike every receive form.

With a queued receiver (`cb+0x0 != cb`):

1. Mask IRQ (I only) and take the object lock `record+0x14` through `[record+0x18]`.
2. `T = cb+0x0`; `dom' = 0x400545cc/0x400e458c + (T+0x20 - 1)*132`.
3. `*(T+0x14) + 0xc := message`, **before** the scheduler lock.
4. Take `dom'+0x8` through `[dom'+0xc]`.
5. Unlink: `cb+0x0 := T+0x0`; `(T+0x0)+0x4 := cb`. `cb+0x4` stays; it equals `cb` when `T` was the only waiter.
6. `0x44568/0xd4528(dom', T)`: cancel `*(T+0x14)` as a timer in `dom'` when nonzero, `*(T+0x14) := 0`, then reset or ready-insert per `T+0x24`, `T+0x28`, `T+0x2c` and `T+0xc` bit2.
7. Only when step 6 returned nonzero, `0x44018/0xd3fd4(dom'+0x0)` while both locks are still held. Nonzero means the target is local and its selection changed with dispatch enabled; zero means a remote doorbell was rung.
8. Release `dom'+0x10`, then release `record+0x1c`. A zero step 6 clears IRQ and returns 0 with no dispatch. Otherwise a nonzero step 7 runs `-63` into the dispatcher `0x43640/0xd3640`, whose result `0` overwrites it, and a zero step 7 returns 0.
9. Every exit clears `CPSR.I`.

With no receiver:

- Attribute bit1: insert into the list at `cb+0xc` before the first entry whose `+0x4` is strictly greater, so equal priorities keep send order. `cb+0x10` is not written.
- Otherwise: `message+0x0 := 0`; when `cb+0xc != 0` then `(cb+0x10)+0x0 := message`, else `cb+0xc := message`; then `cb+0x10 := message`.
- Release the object lock, clear IRQ, return 0. No dispatch and no doorbell.

Lifetime:

- The message is caller-owned. On the queued path the kernel overwrites `message+0x0` and keeps the pointer until a receive returns it or a delete drops it. On the receiver path only `T+0x14`+0xc is written, so `message+0x0` keeps its value.
- `message+0x4` is read only under attribute bit1, and it is compared as a signed word, so 0 and negatives are refused.
- The pointer crosses domains unchanged: a class-2 send from A code hands it to a B receiver.
- Nothing validates `message`, `message+0x4` or the wait record; a bad pointer is a caller fault.
- A `+0xd0` on the same mailbox sets `cb+0xc := 0` and returns no message to anyone.
- The `acquire != 0` retry branches in the body are dead: the installed acquire spins and returns 0 (manifest v6 correction).

#### 4.14.4 The other 13 slots the design must cover

| Slot | A | B | Operation | Admission before the object lock | Under the lock | Errors | Order |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `0x80` | `0x4d39c` | `0xdd360` | flag create | none: no class, range or context check, only an I/F mask | `{attr, initial, 1}` to the configuration, self-link the queue, `pattern := config+0x4` | `-41` | n/a |
| `0x84` | `0x4d434` | `0xdd3f8` | flag delete | none | `{0, 0, 0}`, self-link, `pattern := 0`; a queued waiter is stranded | none | n/a |
| `0x88` | `0x4813c` | `0xd80fc` | flag set, task | `-25` context/I, `-18` class/index, `-42` | object lock, then the waiter's `T+0x20` scheduler lock; OR, match, `wait+0x14 := pattern`, clear the whole word when attr bit2, unlink, release, notify | `-25`, `-18`, `-42` | one waiter |
| `0x8c` | `0x4839c` | `0xd835c` | flag set, handler | `-25` unless handler context with I clear; `-18`, `-42` | as `0x88`, but the release uses the **flag's** domain record | `-25`, `-18`, `-42` | one waiter |
| `0x90` | `0x485c0` | `0xd8580` | flag clear | `-25`, `-18`, `-42` | `pattern &= mask` | `-25`, `-18`, `-42` | none |
| `0x94` | `0x486f8` | `0xd86b8` | flag wait | `-25` context/I/dispatch, `-18`, `-17` for pattern 0 or a mode bit, `-42`, then `-28` under the lock | match → `*result := pattern`; else block untimed | `-25`, `-18`, `-17`, `-42`, `-28`, `-49` | one waiter |
| `0x98` | `0x489ac` | `0xd896c` | flag poll | `-25`, `-18`, `-17`, `-42`, `-28` | match → 0 with `*result`; else `-50` | plus `-50` | one waiter |
| `0x9c` | `0x48b30` | `0xd8af0` | flag timed wait | as `0x94`, plus `-17` for timeout `< -1`; `-50` under the lock when timeout 0 and no match | match → 0; else block with the timer in the **waiter's** heap | `-25`, `-18`, `-17`, `-42`, `-28`, `-50`, `-49` | one waiter |
| `0xcc` | `0x4d5bc` | `0xdd580` | mailbox create | none, only an I/F mask | `{attr, limit, 1}`, self-link, `head := 0`; `tail` untouched | `-41` | n/a |
| `0xd0` | `0x4d654` | `0xdd618` | mailbox delete | none, only an I/F mask | `{2, 32, 0}`, self-link, `head := 0`; `tail` untouched | none | n/a |
| `0xd4` | `0x4aa44` | `0xdaa04` | mailbox send | §4.14.3 | §4.14.3 | `-25`, `-18`, `-42`, `-17` | messages by attr bit1 |
| `0xd8` | `0x4ad48` | `0xdad08` | mailbox receive | `-25` context/I/dispatch, `-18`, `-42` | head → pop; empty → block untimed | `-25`, `-18`, `-42`, `-49` | receivers by attr bit0 |
| `0xdc` | `0x4af9c` | `0xdaf5c` | mailbox poll receive | `-25` context/I, `-18`, `-42` | head → pop; empty → `-50` | `-25`, `-18`, `-42`, `-50` | n/a |
| `0xe0` | `0x4b0e4` | `0xdb0a4` | mailbox timed receive | as `0xd8`, plus `-17` for timeout `< -1` and `-50` for timeout 0 on an empty list | head → pop; else block with a timer | `-25`, `-18`, `-17`, `-42`, `-50`, `-49` | receivers by attr bit0 |

- **Waiting order.** `FUN_000448e8` appends at the tail when `*queue->config & 1` is clear, and otherwise inserts before the first waiter whose `T+0xd` is strictly greater, so equal priorities queue behind existing waiters. `queue->config` is the family's own configuration record, so bit0 orders waiting tasks and bit1 orders queued messages; owned code must not conflate the two.
- **Timeouts.** The unit is 10 per tick, the deadline is `*own record+0x40 + timeout`, and the entry goes into the waiting task's own heap even when the object belongs to the other domain. Expiry completes the wait with `-50`, a forced release (`+0x38`/`+0x3c`) or termination with `-49`, and a satisfied wake with 0 plus, for flags, the pattern from `wait+0x14`.
- **Context.** The four blocking forms (`0x94`, `0x9c`, `0xd8`, `0xe0`) require the dispatch-enabled word; `0x88`, `0x90`, `0x98`, `0xd4` and `0xdc` do not. The handler form `0x8c` requires handler context and rejects task context.

#### 4.14.5 Design

**(a) Artifact and increments.** One artifact, `//replacement/rtos_objects`, crate `xm1_rtos_objects`, one `xm1_rust_staticlib` build in F0 (domain-agnostic until the first entry lands, then one per domain), the `rtos_layout` views, one host-test target and one isolated `xm1_rust_elf` stage per increment. No per-slot crate, target or gate: the family is one artifact with one cutover.

- **F0, layout and decode.** Implemented in `replacement/rtos_objects` (crate `xm1_rtos_objects`) over the views added to `replacement/rtos_layout`: typed ids and slots, the record views, the admission order per service, the match predicate, the queue-order policies and the timeout encoding. `EncodedId` appears only in `resolve_flag`, `resolve_mailbox` and `own_*_slot`; every other owned entry point takes `FlagId` or `MailboxId`. The host tests pin the admission order by which refusal wins when several inputs are wrong, against the exact C of `event_flag_check`, `kernel_create_flag`, `kernel_delete_flag`, `kernel_create_mailbox`, `kernel_delete_mailbox`, `mailbox_initialize`, `event_flag_initialize` and `FUN_000448e8`. The policy touches no guest memory. 53 host tests pass (35 in the new crate, 18 in `rtos_layout`, five of them new), the armv7r staticlib builds, and a throwaway consumer caller outside the crate printed the seven expected lines for a cross-domain class-2 id, class 0 from a B entry domain, a class-1 mailbox from B, an admitted priority and three refusals including a negative priority under a negative limit. The first host test run failed one fixture, not the implementation: the dispatch-word case used the zero pattern that `+0x98` refuses with -17, and the fixture, not the policy, was corrected. That caller and its target were removed after the receipt (E-RTOS-OBJECTS-F0-HOST-150).
- **F1a, record-only bodies.** `0x80` and `0xcc`: an I/F mask and a record write, with no lock, no scheduler and no timer. The mask pair is the stock I+F one (A `0x42560` / B `0xd2560`, restored by A `0x42568` / B `0xd2568`), not the owned I-only slots `+0x164`/`+0x168`, so the mask stays a retained seam. Implemented in source as `create_flag` and `create_mailbox` over a `Machine` seam (word load and store, the mask pair, `dmb`), taking a resolved `FlagId`/`MailboxId` and typed create records: `FlagAttributes`, `MailboxAttributes`, `PriorityLimit` and `Pattern` are transparent and carry any bit pattern the stock body would store, because the create validates nothing. Each refuses an existing record with -41 before any write, writes the configuration words, self-links the control block, copies the configuration's initial word into the flag pattern, and writes the mailbox `tail` 0, which stock leaves to the first FIFO append. The table is the id's domain, which `admit` set to the entry domain, and `queue.config` stays with the initialiser.
  **Publication order differs deliberately.** Stock sets `exists = 1` before it initialises the control block, so a racing reader can see a published record with an uninitialised queue. The owned create writes the marker last, after a `dmb`; the difference and its risk are in the differences table, and the barrier's sufficiency is static inference because the memory attributes of both kernels' private RAM are unrecovered.
  **Scope.** This increment covers serialized creation with no live waiters. Retained table aliases, cross-core serialization, re-create/timer lifetime and the final publication barrier remain unresolved. The local I+F mask does not exclude the other core. `no-waiters-on-recreate` remains a caller obligation. Source `d51aa66c` passes 42 object and 18 layout host cases and the external consumer's four creation/refusal outcomes; post-removal `73e2c4ef` builds the staticlib and retains both suite passes. The generic bodies are instantiated by the host caller, not an A32 firmware adapter. Native and system qualification remain separate (E-RTOS-OBJECTS-F0-HOST-150).
  Current integration `9d527c7d` builds the staticlib and passes the object/layout suites and research contracts. Its model checks restored caller interrupt state and masked writes rather than counting mask calls. The explicitly reused qualified consumer again produces all four outcomes. The terminal and packet are `private-data/scratch/Issue117Integration/rtos-f1a-host-1/results.json` and `review-packet.json`.
- **F1b, object-lock-only bodies.** `0x90`, `0x98`, `0xdc`: the object lock and a pattern or head read, with no scheduler lock and no timer. Implemented in source as `clear_flag`, `poll_flag` and `poll_receive` over an `ObjectLock` seam: the `Machine` words plus `with_object_lock(lock, body)`, which brackets the section with the I-only mask and the object domain's `+0x14` lock through `+0x18`/`+0x1c`. The scope owns the acquire and release, so no held token is handed out and there is no second lock argument. The lock and the table are the object domain's, so a class-2 id from an A body reads B's record and B's lock while the entry domain supplies only `admit`.
  **Inward types.** `poll_flag` takes a `WantedMask` whose constructor refuses zero with -17, the rule `admit` already applies, so the body cannot be called with the mask that `MatchMode::All` would satisfy trivially. Both polls take an `OutWord`, a typed guest word the body writes through the memory seam at the stock point: the reported pattern before the clear-on-match store, and the head before the head is advanced.
  **Alias edge and two stock defects.** `+0xdc` stores the head through the caller's word before loading the message's next link (A `0x4b0a8`/`0x4b0ac`, B `0xdb068`/`0xdb06c`). `event_flag_check` stores the pattern (A `0x48114`, B `0xd80d4`), then loads the control block's configuration pointer (`0x48118`/`0xd80d8`) and attributes (`0x4811c`/`0xd80dc`). An output at the message link makes stock self-cycle the queue; an output at the attributes can suppress the clear. The owned bodies capture the successful read before the store.
  **Scope.** Source `35ad56c4` builds the staticlib and passes 55 object and 18 layout host cases. The seven-outcome consumer built and executed at `471aac9c` is explicitly reused, not relabeled as current-source evidence. Immutable proof and current integration remain separate. These generic bodies have no qualified A32 instantiation, native execution or firmware retirement. Scheduler, wakeup, timer, teardown, retained-alias closure and physical cross-core exclusion remain open (E-RTOS-OBJECTS-F0-HOST-150).
- **F2, task-context wake bodies.** `0x88` and `0xd4`: object lock, then the waiter's scheduler lock, then the owned release, notify and dispatcher. Implemented in source as `set_flag` and `send` over a `Wake` seam whose primitives are the owned S1 and S2 bodies: `with_scheduler_lock`, `waiter_domain` (T+0x20), `reset_task`, `prepare`, `insert` and `notify`, with `dispatch` as the S2 entry. The wait release is owned: `wake.rs` implements the `0x44568` algorithm itself over those primitives, including the timer cancel, the T+0x24/+0x28/+0x2c handshake, the suspended short-circuit and the ready insert, and takes only `kernel_remove_timer` (A `0x44ca8` / B `0xd4c68`) as a retained stock seam.
  **Host and emission.** At `18a00b71`, both object/layout host targets pass and the external consumer asserts all twelve declared pattern, readiness, running, notification, message and FIFO outcomes. Both strict stages fit the original regions and reject unplaced sections. The standalone S1 byte-identity check passes at `r24`; S2 builds, but no historical byte baseline was found. Standalone identity does not assert identity of every padding byte in the F2 composition. The six stage binaries and native layout facts are retained in `private-data/scratch/RtosObjectsF2/baseline/`.
  **Native subset.** At `2ef9a144`, all 44 comparisons pass across the two domains. Current admission and comparison reuse all 88 completed stock/owned observations. Returned calls check specified results, caller SP, r4-r11 and caller-frame bytes. Task-arrival cases check selected PC/SP, thread identity, FPEXC, CPSR control, live suspended-frame bounds, the resume entry, r7-r11 coherence and continuation code ownership. The service-local r4-r6 words are not direct-S2 test seeds. Evidence: `private-data/scratch/RtosObjectsF2/r34/review-packet.json` (E-RTOS-OBJECTS-F2-NATIVE-SUBSET-150).
  **Recovered ordering.** The first native run exposed the adapter's incorrect I+F mask; the F2 adapter now masks only I. Both roles ring the exact peer bank while holding the waiter's scheduler lock and complete a DSB after unlocking. The owned notifier additionally issues its qualified pre-ring DSB; stock's notifier has no such instruction. The checker retains this distinction instead of imposing the owned helper's instruction sequence on stock.
  **Publication counterexample.** Lock validation requires an acquisition DMB before protected writes and a distinct release DMB after those writes but before unlocking; compiler-private frame stores are excluded. The synthetic `controls/drop-release-dmb.hs` replaces the unique DMB in owned A's fixed release slot with a NOP. The old checker accepted this missing-barrier image; the corrected checker rejects all thirteen affected calls while normal observations still pass. Both corrected comparisons reuse the completed raw observations. The current packet binds the generator, control image and before/after verdicts.
  **Limits.** The timer seam records owner/token arguments, not heap removal. Task arrival does not prove resumption of the suspended service or execution of a remote receiver. The direct release cases entered as SVC handler callbacks do not qualify the IRQ entry or the F3 handler-set body. Full native, integrated-system and hardware qualification remain separate.
  **Order and conditionals.** `set_flag` ORs the bits, reports the pre-clear pattern into the wait record, applies clear-on-match, unlinks the waiter, and dispatches only when the release changed the selection and the notify is local. `send` stores the message into the receiver's wait record before the scheduler lock, unlinks the receiver, and on the empty path appends FIFO or inserts by ascending priority.
  **F2 scope.** Handler set, blocking waits and teardown are not part of the F2 native subset.
- **F3, blocking bodies and the handler set.** Implemented as a host increment in `block.rs` and `wake.rs`: flag wait (`0x94`/`0x9c`), mailbox receive (`0xd8`/`0xe0`), explicit resume and handler set (`0x8c`). The caller owns the dispatcher seam after the pending operation releases its locks. Family-specific pending tokens retain the task, wait record and original output; premature checks return ownership and successful completion consumes it. A zero word in an armed untimed wait is not success while the task still has its wait bit. Successful resume captures the family value before writing output; `-49` and `-50` leave output untouched. Corrupt completion words return the token for diagnosis.
  **Timer and handler contract.** A timed wait stores its distinct per-task flag/mailbox timer pointer in the wait block, writes `kernel_timeout_task` and the task argument into that entry, and inserts at the executing context's clock plus the timeout with wrapping arithmetic. Queue ordering remains the object's bit-0 FIFO/priority policy. The handler set releases through the waiter's domain, correcting stock's flag-domain defect, and requests delayed local dispatch rather than the task dispatcher.
  **Exercised host proof.** At `d356fd36`, all 81 object and 18 layout cases pass. A separate public-API caller retains a pending token across a premature check, then completes flag and mailbox wake/resume with the original output and released lock/I state. The I-only output scope is separate from F1a's I+F creation scope; both initially clear and set F states are checked at the output store and after restoration. Both strict F2 stages rebuild byte-identically to their retained baseline, without rerunning completed firmware observations (E-RTOS-OBJECTS-F3-HOST-150).
  **Limits.** This is not an emitted F3 body or a firmware context-switch proof. Native placement/ABI, the actual dispatcher continuation and IRQ entry remain open. Timer insertion/removal and the timeout callback's lock-retry are retained seams; F3 does not fix the stock entry-clearing hazard while those firmware bodies remain retained (§4.14.5f). The owned timer heap has separate host evidence only.
- **F4, teardown bodies.** Host `delete_flag` (`0x84`) and `delete_mailbox` (`0xd0`) are implemented in `teardown.rs`. They unlink every queued waiter under its scheduler-domain lock, cancel its distinct timer, publish the released `-49` completion before readiness, and retain remote notification ordering. Local dispatch waits until object reset and unlock. Flag reset writes `{0,0,0}`, a self-linked queue and zero pattern; mailbox reset writes `{2,32,0}`, a self-linked queue and zero head/tail. A typed allocation-free `MessageDrain` transfers the queued caller-owned handles in delivery order without freeing or copying messages. Stock strands waiters; that defect is corrected rather than reproduced.
  **Exercised host proof.** At `ec3e28a6`, all 87 object and 18 layout cases pass. The multi-receiver case combines an untimed A receiver and timed B receiver on one mailbox, checks both released outputs, B's timer cancellation, both ready lists, local dispatch, the remote doorbell and reset state. A separately compiled public caller deletes and recreates a flag after released resume, and drains two mailbox messages before recreation. Both strict F2 stages remain byte-identical to their retained baseline; completed firmware was not rerun (E-RTOS-OBJECTS-F4-HOST-150).
  **Limits.** The raw `-49` word does not distinguish forced release from delete. Cross-core create/delete serialization remains open: F1a creation does not take the teardown's object lock. Existing lifecycle preconditions remain required. Timer/tick/cancellation consumers are retained firmware seams, and emitted teardown placement/ABI, full native family qualification and the integrated cutover remain open.
- **Cutover.** Both domains in one DAT, one integrated comparison and one technical review, as for S1-S3.

**(b) Typed identities.** Owned code carries domain, task and object identity in the type, and the stock `i32` status exists only at the entries and in the stock-ABI adapter:

```rust
pub struct EncodedId(i32);                      // the stock r0 word, boundary only
pub struct EntryDomain(Domain);                 // the loader whose body runs, not the core
pub struct FlagSlot(u16);                       // 1..=the flag count word, like TaskSlot
pub struct MailboxSlot(u16);                    // 1..=the mailbox count word
pub struct FlagId { pub domain: Domain, pub slot: FlagSlot }
pub struct MailboxId { pub domain: Domain, pub slot: MailboxSlot }
pub struct Pattern(u32);
pub struct WantedMask(Pattern);                 // nonzero requested bits
pub struct OutWord(GuestAddress);               // writable stock-edge result word
pub struct KeepMask(u32);
pub enum MatchMode { All, Any }                 // mode bit 0
pub enum Timeout { Forever, Poll, Units(u32) }
pub enum WaitOutcome<T> { Satisfied(T), Released, Deleted, TimedOut }
pub enum ObjectStatus { Context, InvalidClass, InvalidIndex, AlreadyExists, NotCreated, Param, QueueOverflow }
```

`resolve_flag` and `resolve_mailbox` are the only constructors of `FlagId` and `MailboxId`. They take the entry domain, the raw word and the live count, which the entry reads from the **selected context record** (`+0x58`/`+0x68`), never from the configuration table, because the record is what stock compares against. `EntryDomain` is the loader whose body runs, not the executing core: a loader-A slot runs on either core, and class 0, the create tables and the delete tables all follow the entry domain. `class_zero_is_the_entry_domain`, `a_class_two_id_from_an_a_body_names_b` and `create_names_the_entry_domain_whatever_the_class` are the counterexamples. Raw layout stays at named consumers only: the 14 entry bodies, the image4 wrappers `0x724480`, `0x72503c`, `0x725318`, `0x724cdc`, `0x724f08`, `0x725dd8`, `0x72612c` (ids and the two `Message` words), the other domain's retained kernel, and the two initialisers.

`EncodedId` exists only at the stock ABI boundary, where the raw word arrives:

```rust
fn resolve_flag(entry: EntryDomain, id: EncodedId, count: u32) -> Result<FlagId, ObjectStatus>;
fn resolve_mailbox(entry: EntryDomain, id: EncodedId, count: u32) -> Result<MailboxId, ObjectStatus>;
fn own_flag_slot(id: EncodedId, count: u32) -> Result<FlagSlot, ObjectStatus>;      // class ignored
fn own_mailbox_slot(id: EncodedId, count: u32) -> Result<MailboxSlot, ObjectStatus>;

fn create_flag(m: &M, id: FlagId, create: FlagCreate) -> Result<FlagId, ObjectStatus>;
fn delete_flag(m: &M, id: FlagId) -> Result<(), ObjectStatus>;
fn set_flag(m: &M, id: FlagId, bits: Pattern, from: Context) -> Result<(), ObjectStatus>;
fn clear_flag(m: &M, id: FlagId, keep: KeepMask) -> Result<(), ObjectStatus>;
fn wait_flag(m: &M, id: FlagId, want: WantedMask, mode: MatchMode,
             out: OutWord, timeout: Timeout) -> Result<WaitOutcome<Pattern>, ObjectStatus>;
fn poll_flag(m: &M, id: FlagId, want: WantedMask, mode: MatchMode,
             out: OutWord) -> Result<WaitOutcome<Pattern>, ObjectStatus>;

fn create_mailbox(m: &M, id: MailboxId, create: MailboxCreate) -> Result<MailboxId, ObjectStatus>;
fn delete_mailbox(m: &M, id: MailboxId) -> Result<MessageDrain, ObjectStatus>;
fn send(m: &M, id: MailboxId, message: Message) -> Result<(), ObjectStatus>;
fn receive(m: &M, id: MailboxId, out: OutWord,
           timeout: Timeout) -> Result<WaitOutcome<Message>, ObjectStatus>;
fn poll_receive(m: &M, id: MailboxId, out: OutWord) -> Result<WaitOutcome<Message>, ObjectStatus>;
```

Create, clear, poll, task-context set/send, blocking begin/resume, handler set and teardown have host implementations. The code block shows the completed-call target interface; current blocking host APIs return typed pending continuations around the caller's dispatcher seam. Emitted bodies and native/system qualification remain separate from these generic host implementations.

`Err` carries admission (`Context`, `InvalidClass`, `InvalidIndex`, `AlreadyExists`, `NotCreated`, `Param`, `QueueOverflow`); `Ok` carries the wait outcome, so `TimedOut` covers both the poll's `-50` and expiry's `-50`. The entries module maps typed results to stock `i32` statuses (`Released` and `Deleted` both map to `-49`). The current resume can report only `Released` for that word. `MessageDrain` is the host teardown result: messages still queued when the mailbox is deleted, in delivery order.

**(c) Record views to add to `replacement/rtos_layout`.**

```rust
pub struct WaitBlock(GuestAddress);      // +0x00 result/timer entry, +0x04 queue, +0x08 owner
pub struct WaitQueue(GuestAddress);      // +0x00 next, +0x04 prev, +0x08 config
pub struct FlagConfig(GuestAddress);     // +0x00 attributes, +0x04 initial, +0x08 exists; 0x0c
pub struct FlagControl(GuestAddress);    // queue, +0x0c pattern; 0x10
pub struct FlagWait(GuestAddress);       // base, +0x0c pattern, +0x10 mode, +0x14 result; 0x18
pub struct MailboxConfig(GuestAddress);  // +0x00 attributes, +0x04 limit, +0x08 exists; 0x0c
pub struct MailboxControl(GuestAddress); // queue, +0x0c head, +0x10 tail; 0x14
pub struct Message(GuestAddress);        // +0x00 next, +0x04 priority; 0x08
```

Add the eight table accessors on `DomainWords` (`flag_config`, `flag_control`, `flag_wait`, `flag_timer`, `mailbox_config`, `mailbox_control`, `mailbox_wait`, `mailbox_timer`) with the addresses of §4.14.1, and the attribute constants `PRIORITY_ORDER = 0x1`, `MESSAGE_PRIORITY = 0x2`, `CLEAR_ON_MATCH = 0x4`.

**(d) Admission.** One ordered function per direction, returning `Result<_, ObjectStatus>` in the stock order, with the dispatch-enabled check as an explicit parameter because only the blocking forms read it. The four create/delete bodies use a separate resolver that keeps the stock "ignore the class half and index `uxth(id)-1`" behaviour **plus** a range check, which is the intentional difference below. The message priority is decoded at the boundary into `MessagePriority`: the raw word and the record's limit are both read as signed, so a negative word is refused whatever the limit says. `a_negative_priority_is_refused_whatever_the_limit` is the counterexample, and it fails under an unsigned comparison when the limit word is also negative.

**(e) Lock order, including the retained paths.** The nested order is global (`0x400c0804`, held by a whole IRQ handler and by the image4 critical pair), then object lock, then scheduler lock. Every retained path that needs the opposite role holds nothing while it changes rank:

| Path | Nested order | Handshake |
| --- | --- | --- |
| family services `+0x88`, `+0x8c`, `+0x94`, `+0x9c`, `+0xd4`, `+0xd8`, `+0xe0` | object (object's domain) → scheduler (waiter's domain), released in reverse | none |
| `+0x18` terminate | scheduler → **released** → object (`task->wait->owner`) → scheduler | `T+0x24` |
| `+0x38`/`+0x3c` release wait | scheduler → **released** → object → scheduler | `T+0x2c` |
| `+0x1c` change priority | scheduler → **released** → object → scheduler | `T+0x28` |
| tick `+0x104` → `kernel_timeout_task` (A `0x44318` / B `0xd42d8`) | the tick's scheduler lock → **released** → object (`wait->owner`) → scheduler | `T+0x2c` |
| `+0x40` suspend | scheduler only | none |
| cyclic handler through `cyclic_expire` | no kernel lock (the tick releases it around the handler) | none |

The four task bodies and the timeout callback are exact C at the pinned commit. Each sets its handshake word before dropping the scheduler lock and re-checks it after retaking both, so a waker that completed the wait in the window makes it return early instead of unlinking twice. The two-lock graph is therefore acyclic by rank: object before scheduler whenever both are held, and nothing held when the roles swap. This covers the retained paths that act on owned waiters, not only the family's. IRQ is masked by the entry and cleared on every exit; the owned acquire and release are S1's, on the same words.

**(f) Wait, timeout, cancellation.** `block` performs the stock sequence: `T+0xc := 0x32`, remove from ready, `T+0x14 := wait`, `wait.result := 0`, insert by family order, `wait.owner := object domain owner`, `wait.queue := queue`. The timer goes into the waiter's own heap through the stock `kernel_insert_timer` (A `0x44c60` / B `0xd4c20`) with the stock deadline, and a wake or a cancel removes it through the stock `kernel_remove_timer` (A `0x44ca8` / B `0xd4c68`); both are seams until the timer family is owned. The zero-token repair belongs to the TIMER increment's tick-and-remove boundary (§4.13), not this family's blocking bodies. F3 does not claim to close that race while either body remains stock. Cancellation keeps the `T+0x24`/`T+0x28`/`T+0x2c` handshake, because `+0x18` terminate, `+0x38`/`+0x3c` release and `+0x1c` priority change are still stock and act on owned waiters.

**(g) Wake selection.** `wake(waiter)` resolves the record from `T+0x20`, the **waiter's** domain, for the ready insert, the timer cancel and the notify; the object's domain supplies only its object lock. `WakeMode::Task` dispatches through the owned dispatcher when the insert reports a selection change and the target's dispatch-enabled word is nonzero; `WakeMode::Handler` sets the delayed-dispatch word instead. Owned notify issues `DSB` before the doorbell store.

**(h) Teardown.** `delete_flag` and `delete_mailbox` first release every queued waiter as `Deleted`, cancelling each timer in the waiter's own heap; `delete_mailbox` then drains `cb+0xc` into a `MessageDrain`. The record is reset to the stock values (`{0,0,0}` and `{2,32,0}`, self-links, pattern 0, head 0) so a raw reader sees what stock leaves, `tail` is set to 0, and the id is range-checked before any write. The drained messages are caller-owned memory that `send` retained; the stock ABI has no way to return them, so that seam discards the pointers exactly as stock does, and nothing is leaked by the kernel because it never owned them. A caller that needs them back uses the typed delete.

**(i) Intentional differences.**

| Id | Stock | Owned | Trigger | How a retained dependence would show |
| --- | --- | --- | --- | --- |
| `object-id-out-of-range` | the four create/delete bodies (`+0x80`, `+0x84`, `+0xcc`, `+0xd0`) index `uxth(id)-1` and write outside the tables; the eight service bodies already refuse with `-18` | `InvalidIndex`, no write | a create or delete id outside `1..=count` | a stock-role change at `0x4005d178+12(id-2)`, `0x40062c34+20(id-1)` or the timer heap with no owned counterpart |
| `queue-destructor-misdirected-delete` | `0x724f08` calls `+0xd0` with a data-queue id; flag `n-1` becomes `{2,0x20,0}` and the data queue stays created | the caller is corrected to `+0xa4`; until then `+0xd0` refuses with `-18` | `0x724f08(n)`, `n >= 2` | the destructor's own report: code 27 to `0x9121b8` and the `0x40a0fc3c` counter in the owned role, with flag records 7 and 40 unchanged |
| `delete-stranded-waiter` | delete self-links the queue; a queued waiter stays blocked with dangling links | every waiter is unlinked, its timer cancelled and completed `Deleted` (`-49` at the ABI) | delete with a queued waiter | the stock role never returns from the wait; the owned role returns `-49` |
| `delete-discards-queued-messages` | delete sets `cb+0xc := 0`; the kernel forgets the queued pointers, which stay caller-owned | the typed delete returns a `MessageDrain`; the stock-ABI delete discards the pointers as stock does, with no counter | delete with queued messages | none at the ABI: both roles leave the message memory allocated and the kernel's list empty. Only a caller that expects the pointers back through the kernel differs, and there is no demonstrated leak because `send` retains caller-owned memory |
| `mailbox-tail-uninitialised` | init and create leave `cb+0x10` | written 0 by the F1a create | any create | none reachable, because `head == 0` gates every read of `tail` |
| `receive-reads-next-after-output` | `+0xdc` stores the head through the caller's word and then loads the message's next link, so an output word at that link makes the queue self-cycle | the successor is read before the output write, so the message is popped | an output word aliasing the head message's next link | a subsequent receive returns the same message in stock and times out in owned; no route observes such a caller |
| `poll-reads-attributes-after-output` | `event_flag_check` stores the pattern through the caller's word and then reads the creation attributes, so an output word at the attributes changes the clear decision | the clear decision is read before the output write | an output word aliasing the creation attributes | a subsequent matching poll matches again in stock and times out in owned; no route observes such a caller |
| `create-publishes-last` | the create sets `exists = 1` before it self-links the control block, so a racing reader can see a published record with an uninitialised queue | the owned create writes the configuration and control-block words, issues `dmb`, and only then sets `exists = 1` | any concurrent reader: the other core, a retained body, or a copied table pointer | a stock-role reader that observes `exists = 1` with a stale queue differs; no such reader is observed on the census route, and the barrier's sufficiency depends on the unrecovered memory attributes |
| `handler-set-waiter-domain` | `+0x8c` releases a satisfied waiter through the **flag's** domain record: ready insert into the wrong list, timer cancel in the wrong heap | the waiter's domain from `T+0x20` | handler-context set on a flag whose waiter is in the other domain | the flag's domain bitmap and selected cell change in stock and the waiter's in owned; the doorbell goes to the waiter's core in both |
| `timeout-entry-not-cleared` | the stock tick pops without clearing `entry->index` or `wait->result`; a waker in the window removes slot 1 or underflows the count | cleared before the callback, **only once the tick `+0x104` is owned**; this family cannot repair it | timer expiry racing a wake | heap slot 1 removed or count underflow in stock. F3 depends on the stock tick and inherits the stock behaviour until the TIMER increment owns the pop |

**(j) Retained seams and removal conditions.** The task create/delete and global-lock seams stay exactly as they are.

| Seam | Address | Retained because | Removal condition |
| --- | --- | --- | --- |
| task create `+0x0`, task delete `+0x4` | A `0x4d094`, `0x4d1a4`; B `0xdd054`, `0xdd168` | unchanged by this family; their wrappers `0x725dd8` and `0x72612c` call `+0x80` and `+0x84` | task family cutover, when those wrappers migrate |
| global lock `+0x161c`, `+0x1620`, `+0x1624` | `0x400c0804`, tags `0x11`/`0x12` and `0x21`/`0x22` | unchanged; shared with both vectors and the image4 critical pair | both vectors and the critical pair owned |
| terminate `+0x18`, release wait `+0x38`/`+0x3c`, priority change `+0x1c` | A `0x454f0`, `0x46534`, `0x467b4`, `0x4582c`; B `0xd54b0`, `0xd64f4`, `0xd6774`, `0xd57ec` | stock bodies unlink and release an **owned** waiter, so the wait block, the circular queue links, `T+0x0c/+0x14/+0x20`, `T+0x24/+0x28/+0x2c` and the timer entry keep their stock shape | those four slots owned |
| object and scheduler lock records and function pointers | record `+0x14/+0x18/+0x1c` and `+0x8/+0xc/+0x10`; words A `0x400545c0`/`0x400545c4`, B `0x400e4580`/`0x400e4584` | any stock service still takes them | no stock service takes them |
| object tables at stock addresses | §4.14.1 | the other domain's retained bodies reach them through ids, and the initialisers publish them | both domains' family bodies, both initialisers and the `0x724f08` caller owned |
| delayed-dispatch word and doorbell | A `0x400546d8`, B `0x400e4698`; bank offset `0xcc` | the stock IRQ exit consumes the word and the handler clears the request | owned IRQ exit and doorbell |
| timer insert and remove | A `0x44c60`, `0x44ca8`; B `0xd4c20`, `0xd4c68` | a timed wait arms its entry and a wake or cancel removes it through the stock heap code, because the family does not own the heap | the TIMER increment owns the heap |
| tick `+0x104` and `kernel_timeout_task` | A `0x44d8c`, `0x44318`; B `0xd4d4c`, `0xd42d8` | the stock tick pops the family's entries and the callback completes the wait with `-50`; the entry-clearing repair needs this seam owned | the TIMER increment |
| per-task-slot wait and timer indexing | one outstanding wait per task | a stock task can hold only one; stock terminate and release address it by `T+0x14` | the stock wait consumers above are owned |
| waiter scheduler lock | record `+0x8` through `+0xc`/`+0x10` | the wake takes it under the object lock; S1 owns the word | no stock path takes it |
| wait release and timer cancel | the `0x44568` algorithm in `wake.rs`; `kernel_remove_timer` A `0x44ca8` / B `0xd4c68` | the release is owned over the S1 primitives; only the heap removal is the stock body | the TIMER increment owns the heap |
| notify and dispatcher entry | A `0x44018` / B `0xd3fd4`, A `0x43640` / B `0xd3640` | both are owned S1 and S2 bodies, called through the seam | none for the bodies; the doorbell register stays a device seam |
| `Message` words `{next, priority}` at the image4 wrappers | `0x724cdc`, `0x72503c`, `0x725318` | caller-owned messages built and consumed by image4 | the image4 message family is owned |

#### 4.14.6 Scenario matrix

Every row is a native case or a natural observation the increment must be gated on. `Stock` and `Owned` are the discriminating expectations; a row with a difference id is an expected difference, keyed by that id. The census counts quoted below are discovery provenance from census-v1 (E-RTOS-SLOT-CENSUS-STOCK-150), not fixed qualification counts; a qualification states the totals its own receipt observed.

| Id | Kind | Setup | Discriminating observation | Stock | Owned |
| --- | --- | --- | --- | --- | --- |
| N1 | native | receiver blocked in `+0xd8`; sender calls `+0xd4` | sender r0, receiver r0 and `*r1`, `message+0x0` | 0; 0; the pointer; untouched | equal |
| N2 | native | empty mailbox, attr 0; send A then B; receive twice | receive order and the two link words | A then B; `A+0x0 = B`, `B+0x0 = 0` | equal |
| N3 | native | attr 2, priorities 3, 1, 3 | receive order | 1, 3, 3 | equal |
| N4 | native | attr 2, priority 0 and 33 | r0 and the object-lock word | `-17`, word untouched | equal |
| N5 | native | `+0xd0(0x10002)`; then `+0x80(121)` | r0 and the words written | `0` with flag 0's record `{2,0x20,0}` and `0x40062c48` self-linked; `0` with the timer-heap word and mailbox control block 1 overwritten | `-18` with no write in both (`object-id-out-of-range`) |
| N6 | native | exists word 0 | r0 | `-42` | equal |
| N7 | native | dispatch-enabled word 0 | r0 for `+0xd4` and for `+0xd8` | 0; `-25` | equal |
| N8 | native | handler context, then `CPSR.I` set | r0 | `-25`; `-25` | equal |
| N9 | native | flag pattern `0x30`; waiter `0x10`, mode All | `*result`, pattern | 0, `0x30`, pattern `0x30` | equal |
| N10 | native | flag pattern `0x30`; waiter `0x50`, mode All, then mode Any | r0 and `*result` | `-50` with nothing written; then 0 with `0x30` | equal |
| N11 | native | two `+0x94` waiters on one flag | second r0 | `-28` | equal |
| N12 | native | attr 4, pattern `0x30`, waiter `0x10` | `*result`, pattern | `0x30`; 0 (whole word) | equal |
| N13 | native | pattern `0x31`, `+0x90(0x11)` | pattern | `0x11` | equal |
| N14 | native | empty mailbox; timeout 0, then expiry, then `+0x38` | r0 | `-50`; `-50`; `-49` | equal |
| N15 | native | delete with a queued receiver | whether the waiter returns, and r0 | never returns | `-49` (`delete-stranded-waiter`) |
| N16 | native | delete with two queued messages | whether the kernel still reaches the pointers, and the caller's own memory | `cb+0xc := 0`; the pointers become unreachable through the kernel, the memory is untouched | the typed delete returns both; the ABI delete discards them as stock does (`delete-discards-queued-messages`) |
| N17 | native | flag in B, waiter in A, set by a task in A | which domain's bitmap and selected cell change | A (the waiter's) | equal |
| N18 | native | as N17, set from handler context | same | B (the flag's) | A (`handler-set-waiter-domain`) |
| N19 | native | `+0xd0` with `0x10008` and `0x10029` | A flag records 7 and 40, the mailbox record, the error report | `{4,0,1}→{2,0x20,0}`, 0 | unchanged, `-18` (`queue-destructor-misdirected-delete`) |
| B1 | behavior | the existing cold-start and FILM SIMULATION route | the 3,551 family entries of the census and their results (discovery counts) | 3,549 matched returns, all 0; A `+0x80` 74, `+0x84` 1, `+0x88` 714, `+0x8c` 722, `+0x90` 592, `+0x9c` 1,437; B `+0x80` 10, `+0x9c` 1; the two unmatched entries are flag timed waits still blocked at the end | equal, with the two expected differences above untriggered |
| B2 | behavior | a route that sends and receives a mailbox message | message delivery and the ready transition | **not observed**: `+0xd4`/`+0xd8`/`+0xdc`/`+0xe0` have 0 calls in both domains | must be covered by N1-N8 until a route exists |
| E1 | effect | N1 | lock words taken, in order, and the bank write | object lock `0x400545c4`/`0x400e4584` then the waiter's scheduler lock; one `0xcc` bank write only when remote and the selection changed | equal |
| E2 | effect | N1 with a remote waiter | ordering primitives | none between the scheduler stores and the doorbell | `DMB` then `DSB`; not an equality case |
| E3 | effect | N1 with a local, higher-priority receiver | the service's r0 | 0, with `-63` loaded and overwritten by the dispatcher | 0 |
| L1 | lifetime | N1 then a second receive | the returned pointer, `message+0x0` | same pointer; `+0x0` rewritten only when it was queued | equal |
| L2 | lifetime | two successive `+0x9c` on one task | wait and timer record identity | the same two records, indexed by the task slot | equal |
| L3 | lifetime | N15 with a timed `+0xe0` waiter | the waiter's heap entry after the delete, and its r0 | still armed, fires later and returns `-50` | cancelled by the release, which returns `-49` at once (`delete-stranded-waiter`) |
| L4 | lifetime | create, delete, create | second create's r0 and the record bytes | `-41` unless the delete ran; the delete leaves `{0,0,0}` / `{2,32,0}` | equal |
| L5 | lifetime | class-2 send from A to a B receiver | the pointer value at the receiver | unchanged | equal |

#### 4.14.7 What is not established, and the lease request

- No natural route exercises the mailbox half: `+0xd4`, `+0xd8`, `+0xdc` and `+0xe0` have zero calls in the census-v1 discovery route in both domains, and the only observed stock writer of any of these records is the destructor's misdirected `+0xd0` on POWER OFF. The mailbox half's stock reference is therefore native-case only, plus that route.
- No cross-domain flag waiter was observed: all 3,551 family entries of that census carry the calling root's own class, so `handler-set-waiter-domain` has no natural trigger. The correction rests on the code, and the `same-domain-waiter` precondition stays recorded until a route produces one.
- No blocking path ends in `-50` or `-28` on the existing route: every reached timed wait returns 0, so timeouts, queue overflow and forced release are native-case facts.
- The family inherits the scheduler's open hardware laws unchanged: exclusive-monitor coherence of the lock words (U12), the memory attributes of both kernels' private RAM and of the controller banks, and the doorbell's priority against the tick.
- F0 ran as a host-only increment: the staticlib build, both host test targets, the consumer caller, and the manifest v7 regeneration with its schema validation and v6-to-v7 delta. The observed v7 delta is the eight domain/slot precondition changes plus provenance: the slot-contract input's bytes and hash, and four `inputPath` spellings that are absolute in v6 and relative in v7 for the same bytes and hashes. No extent or contract field moved.
- F0's policy qualification and F1a's creation qualification are host-only. F1a adds serialized create bodies with publication last and a zero mailbox tail; the I+F mask remains a stock seam. The A32 staticlib build does not instantiate these generic bodies or qualify an emitted firmware body. No contract, extent or precondition changes, and `+0xd4` remains an unknown extent.
- F1a is a record write behind the stock I/F mask and needs no guest run. The first guest-facing gate is F2's native subset, and the destructor difference needs one POWER OFF re-run with the owned `+0xd0` staged, which belongs to the F4 cutover.

### 4.15 Owned timer heap membership: host increment

`replacement/rtos_timer` is one allocation-free artifact for initialization, insert, remove and tick. It uses the canonical guest/context views and typed entry identity, membership, capacity, callback, argument and clock values. Private sift operations move nodes and update their membership indices; they are not separate public gates.

**Ownership contract.** A pop invalidates the entry's index before invoking its callback. A normal removal invalidates the removed entry, reindexes a replacement and skips movement when removing the final occupied slot. Removing a detached entry returns without reading or changing heap state. This implements the joint pop/remove zero-token repair of §4.13 in the host core, not in the currently retained firmware bodies. The generic timer never treats a task's wait-result word as membership.

The tick admits only handler context with I clear, masks I and acquires the scheduler lock before touching the heap. Callback entry and return hold that lock; callbacks may temporarily release and reacquire it themselves. Only after the callback returns does the tick release, clear/set I and reacquire. It re-resolves the current heap each iteration and reloads base, clock and count, preserves a rearmed entry and advances the final clock by 10 with wrapping arithmetic. Corrupt-count failure releases the lock and clears I.

**Exercised host/native increment.** Fifteen host cases cover membership repair, callback rearming, capacity boundaries, interrupt cleanup and a callback-selected new heap. Both capacity constructors share const validation; invalid values cannot enter through safe Rust. The role-grounded arena oracle passes 40 stock/owned comparisons and 108 controls through cached native execution with current input admission and comparison. Initialization resets time offset, base and count, sets tick to 10 and publishes the owned arena base at config `+0x20` with count/base/tick at `+0x24..+0x2c`; the stock owner-context initializer copies the heap word to context `+0x34` unchanged. A owns `[0x40a5a200,0x40a5aa00)` and B owns `[0x40a5aa00,0x40a5b200)`, each 0x800 bytes, 32-byte aligned and disjoint. The oracle compares the relocated owned node bytes with the stock nodes, masks only the bounded heap-pointer difference, and refuses the old stock base, the other arena and boundary overlap; a consumer-visible control mutates the owned entry pointer. CPSR control bits, callee-saved registers, SP and declared-memory preservation remain checked. The context-switch fixture aliases the two lock words; it does not prove independent-lock ownership. The arena source and native-fixture review is clear. The full composed host test passes all 153 checks after grouping assertions without changing coverage, compiler flags or the 6 GiB limit. Evidence: `private-data/scratch/MainTimerArena/review-packet.json` (E-RTOS-TIMER-ARENA-HOST-150) and `private-data/scratch/ComposedTestGroups/host-proof.json`.

**Capacity boundary.** Host fixtures supply capacities 8 and 32; the owned arenas keep the logical capacity 184. The recovered declaration of `g_timer_heap` is unsized, so 184 is not a recovered stock allocation; it is the owned arena's policy bound, and 184*8 = 0x5c0 fits each 0x800-byte arena. The 120-task and 64-cyclic counts do not establish a stock array size. The fixture's 185th insertion is a boundary counterexample, not a demonstrated reachable stock defect. The task-storage helper uses the reduced `0x4a000` allocatable bound with a remaining-capacity check under the admitted cursor-domain invariant `pool_base <= cursor <= pool_base + 0x4a000`; the guard fits its 204-byte retained window and the linked image builds at `0xcc`. A retained record above the new bound is refused by host admission; the invariant proves a current-boot record base cannot exceed the allocatable end.

Reset zeroes the heap gap as part of kernel BSS: A `[0x4004dae0,0x40062c54)`, B `[0x400ddaa0,0x400f2c14)`. Both calls use byte count `0x15174`; the zero routine converts bytes to words. This establishes initialized backing, not exclusive timer ownership. Source and byte provenance are recorded in `private-data/scratch/RtosTimerAbi/storage-provenance.json`.

**Carrier and retained seams.** The emitted A32 artifact fits initialization, insert, remove, tick and all private bodies in A `[0x44a50,0x44f2c)` and B `[0xd4a10,0xd4eec)`, 1244 bytes per domain. Linker assertions bind every entry to its original address, refuse code outside the module and forbid writable data; unused bytes are UDF. No `0x50000` scratch-code region remains. The reserved arena has host admission and native-subset evidence, not an accepted system cutover. Runtime storage ownership, real callback and wait-result integration, cyclic lifecycle and independent-lock ownership remain open. Cutover still needs caller closure, complete subsystem native obligations and one integrated comparison through the same technical review cycle. No full-native, system or hardware timer qualification is claimed.

**Arena memory-law scope.** The boot records program the high task-pool address through Normal non-cacheable shareable region 1; the low addresses selected by stock `BIC #0x40000000` use Normal write-back, no-write-allocate region 0. This decode follows [Arm DDI 0363E, tables 4-33 and 7-3](https://documentation-service.arm.com/static/5e8e1efa88295d1e18d36cd8), not a cache or physical-alias observation. Phase 0's store-only clear at `0x69ce18`-`0x69ce30` includes the reserved tail. A no-write-allocate store miss does not establish dirty residency. The stock task wrapper fills each allocated stack through its low address and then calls clean/invalidate at `0x725f70`-`0x725f78`; an unallocated reserved tail receives no such per-stack maintenance. Earlier cached residency, actual alias/cache behavior and the unbound dynamic restore-range writer remain open. The reduced owned allocation bound must be the live bound; the retained stock bound still admits the reserved tail.

### 4.16 Owned semaphore family: entries, carriers and retained seams

`replacement/rtos_semaphore` (crate `xm1_rtos_semaphore`, `no_std`, no statics) owns the seven semaphore slots of both loaders: create `+0x64`, delete `+0x68`, signal `+0x6c`, interrupt signal `+0x70`, wait `+0x74`, poll `+0x78` and timed wait `+0x7c`. Admission, count and maximum, waiter order, blocking, timeout arming and the wake release are typed values; ids, counts, timeouts, outcomes and the queue order are distinct types. The canonical tables are `rtos_layout` views: `SemaphoreSlot`, `SemaphoreConfig`, `SemaphoreControl`, the shared `WaitBlock` and `TaskDefinition::slot()`.

**Carriers.** `//replacement/rtos_semaphore:semaphore-a` and `:semaphore-b` link the bodies in place at the stock extents: create `0x4d2a8`/`0xdd26c` (0x90), delete `0x4d338`/`0xdd2fc` (0x64), signal `0x47604`/`0xd75c4` (0x240), interrupt signal `0x47844`/`0xd7804` (0x210), wait `0x47a54`/`0xd7a14` (0x240), poll `0x47c94`/`0xd7c54` (0x13c) and timed wait `0x47dd0`/`0xd7d90` (0x274). Each entry is asserted at its stock origin and every body end inside its own extent; the delete, admission, signal, block and create bodies are placed in the slack of those same extents, and the link refuses unplaced text or data.

**Retained seams and removal conditions.** The bodies call the stock lock acquire/release (`0x439d8`/`0x43a14`, B `0xd398c`/`0xd39c8`), notify (`0x44018`/`0xd3fd4`), dispatcher save (`0x43640`/`0xd3640`), mask pair (`0x42560`/`0x42568`, B `0xd2560`/`0xd2568`), wait release (`0x44568`/`0xd4528`), wait cancel (`0x44744`/`0xd4704`) and the wait enqueue pair (`0x44800`/`0x448e4`, B `0xd47c0`/`0xd48a4`). Each seam is removed when its own unit lands: the scheduler seams with the S1/S2 cutover, the mask pair with the CPU/IRQ backend, and the wait release, cancel and enqueue pair when every object family owns its queueing. The host model implements the release and enqueue tails over the owned S1 helpers.

**Intentional differences.** `semaphore-create-publishes-last` writes the count before the exists word and issues a DMB between them; stock publishes exists before the count. Trigger: a concurrent access to the same id during create. `semaphore-create-range-refused` returns `-18` for `uxth(id)` outside 1..=250; stock indexes the creation and control tables out of bounds, which in the stock role writes the adjacent cyclic-configuration and task-definition records. `semaphore-delete-releases-waiters` takes the object lock, releases each queued waiter with `-49` and readies it before resetting the record; stock resets the record and leaves the waiters linked with `exists` clear. `semaphore-delete-serialized` also takes the object lock, which stock's mask-only delete does not.

**Exercised.** `//replacement/rtos_semaphore:semaphore-host-tests` passes 21 host cases: record publication, count and maximum, overflow without a count write, FIFO and priority queue order, signal with own, peer and handler waiters, timer removal on wake, zero/negative/positive timeouts, blocking and resume, delete with and without waiters, and the admission precedence table. `//replacement/rtos_semaphore:family-suite` passes all 62 stock/owned observations in both domains through `cachedObservedBatch`: create (fresh, existing, out-of-range), delete (plain, with waiters, handler-context deferral), signal (below maximum, at maximum, own, peer and handler waiters, armed waiter, task and handler admission refusals), wait (count, blocking, dispatch-disabled, signal return, delete return, F-set return), poll (count, empty, dispatch-disabled) and timed wait (zero timeout, negative, armed, expiry, timeout return). The case functions carry the retained closure the bodies reach: mask save/restore tails, notify and raise, the dispatcher save/resume/select entries, the wait release and cancel tails, both enqueue helpers, the timer sift and, for the timeout return, the timeout callback and tick. `kernel_find_ready`'s nibble table is read from the pinned loader windows: literal A `0x4daa0` (window `0x4da80`) and B `0xdda64` (window `0xdda40`), both inside their loader prefixes. Two fixture/edge corrections came from the raw observations: the B semaphore creation table is `0x400edf98`, not the A-derived `0x400edfd8`, and the enqueue helpers take the owner domain number, not the context record address.

**Delete dispatch mask and handler deferral.** The owned delete restores the saved F bit while keeping I set, calls the dispatcher, and only after the dispatch returns restores the caller's saved I|F. The dispatcher's switch therefore never runs with IRQ unmasked, matching S2's requirement; the host regression asserts the recorded dispatch mask is `0x80`. A handler-context delete cannot use that path: it would save the handler stack as the task continuation before IRQ teardown. `delete-handler-defers` (both domains) queues a higher-priority local waiter, enters with the handler-context word set, and the owned role cancels the waiter with `-49`, readies it, sets the reschedule word and returns without dispatching; the stock role resets the record, leaves the waiter queued and returns. The deferral helper is a separate entry in the signal body's slack, so the delete body stays inside its shared extent. The arrival CPSR expectation is `0x93` for every arrival, and the selected task's frame carries that control.

**Resumed caller and saved mask.** `wait-signal-return`, `wait-signal-return-f`, `timed-timeout-return` and `wait-delete-return` (both domains) declare the caller and waker stacks through `kernel.stack_contexts`. The caller enters the stock or owned wait/timed-wait at the stock entry and blocks; the dispatcher selects a second task on its own declared stack, whose stub calls the stock signal, delete or timer tick (the tick then calls the stock timeout callback under the scheduler lock) and the stock release, cancel and dispatcher. The caller resumes through the saved dispatcher frame into the wait epilogue and returns to the case exit. The waker stub records the CPSR at its first instruction, which is the caller's dispatch CPSR: the case asserts I set with F preserved (`0x93`, and `0xD3` for the F-set variant) and the resumed caller's control (`0x13`/`0x53`), `r0` (`0`, `-50`, `-49`), SP and r4-r11. The retained tick returns with I clear and a fallback dispatch follows a returned signal or delete, so the driver masks I and records its own CPSR immediately before the dispatcher call; the timeout and stock-delete roles assert that word is masked too. The stock delete strands the waiter, so its role is a declared 20,000-instruction no-return expectation and the owned role is the resumed `-49` counterexample. The comparator enforces the declared arrival PC and SP, the returned caller SP, r4-r11 and control, and the recorded mask; the cached batch's adapter expectations are not the gate.

**Not yet established.** The resumed-caller cases use a synthetic waker caller and the timer removal entry is a recording stub; the timer heap's capacity and the tick's entry-clearing hazard remain retained. The handler deferral covers delete; create remains a mask-only record write. No system, live-placement or hardware qualification is claimed.

**Cutover staging and typed observation (host).** `//replacement/rtos_semaphore:stage-semaphore` admits the six qualified scheduler/own-first groups, stages the base candidate, then patches the two semaphore carrier extents per domain in place. The canonical DAT stages from `43d67afa...` and the qualified base `47c95e9c...` to `854c4d85...`; the loader regions carry the carrier bytes and every change is confined to the eight extents plus the container checksum. The stager emits `semaphore-regions.json`, the typed catalog the shared observation engine consumes. The engine carries the seven semaphore services through `SchedulerPairCatalog`, `SchedulerPairPlan` and `SchedulerPairDecode`; the observer captures the config, control, wait queue and timer records under the family queue bound; `SchedulerPairSemaphore` validates the seven operations' status, effect, timer and continuation rules with a partial replay over the captured words, and `checkPendingSemaphore` validates a still-blocked wait on the shared terminal-memory `Pending`. The layout, semaphore, scheduler-pair and stage tests pass. The checker applies the service admission precedence before any operation effect, reads the timed-wait timeout from `r1`, returns an immediate `-50` for a zero timeout with no save or timer, derives poll from the entry count, returns `-43` at overflow, validates the saved continuation rather than the resume-entry PC, associates timer inserts, removals and callback consumption through the bounded streaming witnesses, proves the pending caller's queue membership with reciprocal links and stack bounds, replays byte stores into their containing word, and derives the release state from the suspension and termination obligations. `//emulation:scheduler-pair-semaphore-tests` exercises the false accepts and refusals through the streaming qualification path. The natural stock/owned plans (`plan-stock2.json`/`plan-owned2.json`, 100 entries including the 14 semaphore entries) were run under the 8G emulator cap, but both stock attempts were killed by host memory (exit 137, empty output) while concurrent reviewed runs held both slots at a host load above 17; no semaphore natural observation was obtained. The 62 cached native comparisons remain the behavior and lifetime oracle; the natural comparison is the remaining path, a host resource kill rather than a semantic refusal.

**Staging verification.** The observer, stager and host tests produced these complete wrapper verdict lines, persisted in `private-data/scratch/RtosSemaphore/final-observer-proof.log` and `final-observer-host.log`:

```text
host-build verdict {"verdict":"passed","resource_failure":null,"command":"build","exit_code":0,"unit":"xm1-host-build-3992163.scope","unit_result":"success","memory_max":"6G","lock_wait_seconds":0.000,"wall_seconds":186.916,"scope_cpu_seconds":16.639,"scope_memory_peak_bytes":743653376,"nix_daemon_cpu_seconds":355.786,"nix_daemon_oom_kills":0}
host-build verdict {"verdict":"passed","resource_failure":null,"command":"build","exit_code":0,"unit":"xm1-host-build-3990740.scope","unit_result":"success","memory_max":"6G","lock_wait_seconds":0.000,"wall_seconds":21.970,"scope_cpu_seconds":40.685,"scope_memory_peak_bytes":1198645248,"nix_daemon_cpu_seconds":0.002,"nix_daemon_oom_kills":0}
host-build verdict {"verdict":"passed","resource_failure":null,"command":"test","exit_code":0,"unit":"xm1-host-build-4185596.scope","unit_result":"success","memory_max":"6G","lock_wait_seconds":8.009,"wall_seconds":6.739,"scope_cpu_seconds":30.842,"scope_memory_peak_bytes":1231122432,"nix_daemon_cpu_seconds":0.001,"nix_daemon_oom_kills":0}
host-build verdict {"verdict":"passed","resource_failure":null,"command":"run","exit_code":0,"unit":"xm1-host-build-3943432.scope","unit_result":"success","memory_max":"6G","lock_wait_seconds":261.282,"wall_seconds":3.052,"scope_cpu_seconds":22.684,"scope_memory_peak_bytes":667574272,"nix_daemon_cpu_seconds":0.000,"nix_daemon_oom_kills":0}
```

The staged candidate is `private-data/scratch/RtosSemaphore/cutover-5/candidate.DAT`, SHA-256 `854c4d8501a9c5b4b912ef1f6e04d1fe483fabc8b7a2a86e3b0bb869cc61e0cb`, 38,405,148 bytes; `plan-stock2.json`/`plan-owned2.json` carry the 100-entry plans and the contract records the source, base, carrier and per-region digests.


**Verification.** The final committed tree produced these complete wrapper verdict lines, persisted with the packet under `private-data/scratch/RtosSemaphore/final-host-test.log`, `final-build.log` and `final-run.log`:

```text
host-build verdict {"verdict":"passed","resource_failure":null,"command":"test","exit_code":0,"unit":"xm1-host-build-3298731.scope","unit_result":"success","memory_max":"6G","lock_wait_seconds":9.009,"wall_seconds":1.900,"scope_cpu_seconds":11.615,"scope_memory_peak_bytes":395673600,"nix_daemon_cpu_seconds":0.000,"nix_daemon_oom_kills":0}
host-build verdict {"verdict":"passed","resource_failure":null,"command":"build","exit_code":0,"unit":"xm1-host-build-3294826.scope","unit_result":"success","memory_max":"6G","lock_wait_seconds":0.000,"wall_seconds":10.372,"scope_cpu_seconds":27.720,"scope_memory_peak_bytes":1009913856,"nix_daemon_cpu_seconds":0.022,"nix_daemon_oom_kills":0}
host-build verdict {"verdict":"passed","resource_failure":null,"command":"run","exit_code":0,"unit":"xm1-host-build-3296119.scope","unit_result":"success","memory_max":"6G","lock_wait_seconds":3.004,"wall_seconds":2.715,"scope_cpu_seconds":20.214,"scope_memory_peak_bytes":661061632,"nix_daemon_cpu_seconds":0.009,"nix_daemon_oom_kills":0}
```

The run's terminal JSON at `private-data/scratch/RtosSemaphore/results-return.json` reports `failures: 0` over 62 cases, with both roles `BatchReused` in both domains. The wrapper verdict describes the Bazel command; the app result is the terminal JSON.

**Checker verification (review fixes).** The three host checker targets and the rebuilt observer produced these complete wrapper verdict lines, persisted in `private-data/scratch/RtosSemaphore/final-checker-proof.log`:

```text
host-build verdict {"verdict":"passed","resource_failure":null,"command":"test","exit_code":0,"unit":"xm1-host-build-570630.scope","unit_result":"success","memory_max":"6G","lock_wait_seconds":94.118,"wall_seconds":4.828,"scope_cpu_seconds":24.740,"scope_memory_peak_bytes":984465408,"nix_daemon_cpu_seconds":4.806,"nix_daemon_oom_kills":0}
host-build verdict {"verdict":"passed","resource_failure":null,"command":"build","exit_code":0,"unit":"xm1-host-build-564410.scope","unit_result":"success","memory_max":"6G","lock_wait_seconds":6.006,"wall_seconds":195.006,"scope_cpu_seconds":15.196,"scope_memory_peak_bytes":646217728,"nix_daemon_cpu_seconds":620.313,"nix_daemon_oom_kills":0}
```

`//emulation:scheduler-pair-semaphore-tests`, `//emulation:scheduler-pair-tests` and `//emulation:scheduler-pair-family-tests` pass; the 62 cached native comparisons were not rerun and remain the behavior and lifetime oracle.


```sh
devenv shell -- host-build --output_base="$HOME/.cache/bazel/latent-image-RtosSemaphore" test --jobs=2 --local_test_jobs=1 //replacement/rtos_semaphore:semaphore-host-tests
devenv shell -- host-build --output_base="$HOME/.cache/bazel/latent-image-RtosSemaphore" build --jobs=2 //replacement/rtos_semaphore:semaphore-a //replacement/rtos_semaphore:semaphore-b
XM1_EMULATOR_SLOTS=1 devenv shell -- host-build --output_base="$HOME/.cache/bazel/latent-image-RtosSemaphore" run --jobs=2 //replacement/rtos_semaphore:family-suite -- --cache-root NEW_DIRECTORY --layout bazel-bin/replacement/rtos_semaphore/layout.json --loader-a private-data/analysis/x-m1-1.50/decode/loader-40000.bin --loader-b private-data/analysis/x-m1-1.50/decode/loader-d0000.bin --owned-a-bin bazel-bin/replacement/rtos_semaphore/semaphore-a.bin --owned-b-bin bazel-bin/replacement/rtos_semaphore/semaphore-b.bin --results NEW_JSON
```




### 4.17 Owned data-queue family: blocking returns and the timer seam

`replacement/rtos_data_queue` (crate `xm1_rtos_data_queue`, `no_std`, no statics) owns the eleven stock data-queue entries of both loaders behind their stock thunks: create `+0xa0`, delete `+0xa4`, send `+0xa8`, send-poll `+0xac`, send-poll-from-handler `+0xb0`, send-timeout `+0xb4`, force-send `+0xb8`, force-send-from-handler `+0xbc`, receive `+0xc0`, receive-poll `+0xc4` and receive-timeout `+0xc8`. Ids, slots, payloads, ring indices, wait records, timeouts, deadlines and outcomes are distinct types; `rtos_layout` supplies every table and record view. The [family summary](../replacement/rtos.md#cached-family-oracles) and the [send contract](../replacement/media_permit/rtos_slot_contracts.json) carry the per-entry arguments, results, errors, context rules and divergences.

The qualified native subset is 52 comparisons (26 per domain, 12 blocking) and 36 controls. Each of the four timed blocking rows maps the role's timer heap and clock, arms the per-task entry through the real stock insert (A `0x44c60` / B `0xd4c20`), sets the clock to the deadline, runs the domain tick (A `0x44d8c` / B `0xd4d4c`), records the callback boundary and calls the stock `kernel_timeout_task` (A `0x44318` / B `0xd42d8`). The stock role uses the stock heap at A `0x4005d718` / B `0x400ed6d8` (count A `0x40054778` / B `0x400e4738`); the owned role uses the owned timer arena at A `0x40a5a200` / B `0x40a5aa00` and overlays the owned timer module at the stock extents, so insertion, consumption and detachment happen in each role's own heap layout. At the callback the record shows `I` masked, the scheduler lock held, the timer count already zero, the base and tick unchanged and one invocation; the stock timer leaves the popped entry index linked while the owned timer detaches it, and the rows check the per-role index instead of requiring leaked-state parity. Blocking retries reopen the caller's `I` after the attempt released its locks and re-mask for the next enqueue, matching the stock clear-`I` retry; a host pending-IRQ test delivers the wake only when the retry reopens `I`, and the callback makes the retry progress; the test asserts the blocked receive, its output and the restored caller `I`, and the old no-unmask behavior fails it. Controls feed the baseline and each mutant through the production comparator; a control holds only when the unmodified pair is accepted, the mutant is refused and removing the specific expectation accepts the mutant. An unaffected-membership control mutates an unrelated timer index in one role and the region compare refuses it; the wake callback counter is the installed wrapper's own count. The two wake rows arm and record the membership, then remove it through the real remove path (A `0x44ca8` / B `0xd4c68`); the callback is not invoked. Controls independently refuse an unarmed membership, a consumed count that is not zero, a callback with `I` clear, a callback without the scheduler lock, a tick that leaves the lock held and a wake that invokes the callback. The caller returns through the real save/select/resume dispatcher with its own `I` mask, result, output, queue, ready-list and peer-kernel bytes. Under the corrected executor (`e27e506b8e106d4764f91e5c045e0a24b72e33cf6e046efa94d06e696ccfd633`) all 52 comparisons and 36 controls pass with zero differences in packet `private-data/scratch/QueueTimerLifetime/native-3` (layout SHA-256 `72d0881dd2149fa5a9c0e8197ba025a42d9945cce49db1aa5adc6f3be97b654e`, owned A/B `0f3652845f7406dd362edb9a7c955f5ed16379530aace8ff2fde9f89485e3b29`/`93328404504dc97a5cf9f545c075d7ab11d371516c3bccff84ab5f061ed5effb`, timer A/B `7729666d239a81c2791e7750d7a60aeaa42b2d64d826d97ea62ec4e4492455ec`/`5f44ddaeee9ad002c32aab14053caa99f0c5bacf0e97ee2211c5a10c8eb0633c`), the changed owned adapter executed in packet `native-2` and the final packet reuses all four completed batches (E-RTOS-DATA-QUEUE-F2-NATIVE-150). The pre-fix packets native-14, native-15, native-18 and native-19 stay frozen and scoped to their own runners.

Two stock defects are repaired rather than copied. Delete releases every queued waiter with `-49` and a handler-context teardown defers its dispatch through the reschedule word instead of switching stacks inside the handler. The stock timer leaves a popped or removed entry's index linked; the owned timer detaches it before the callback and on removal, and the timed rows check the per-role index. Blocking send requires task context: the send contract's `task-context-caller` precondition is unchecked by the stock body, and blocking from a handler would dispatch from the ISR stack, so admission refuses it with `-25`; the named counterexample is `a_handler_send_is_refused`. The timeout callback remains the stock `kernel_timeout_task` at A `0x44318` / B `0xd42d8`, and the owned role overlays the owned timer module at the stock extents; the family does not own that callback. Live placement, system cutover and hardware are not qualified, and case-local scratch `0x50000` is not live.
### 4.18 Owned fixed-pool family: host and emitted-ABI increment

`replacement/rtos_fixed_pool` is one allocation-free artifact for the six fixed-pool slots and the `+0x170` extension. It uses family-named views added to `replacement/rtos_layout`: `FixedPoolSlot` (122), `FixedPoolConfig`, `FixedPoolControl`, `FixedPoolWait`, the four table accessors and the default-pool placeholder. The typed entries are `+0xe4` create, `+0xe8` delete, `+0xec` blocking get, `+0xf0` poll, `+0xf4` timed get, `+0xf8` release and `+0x170` unchecked poll. `EncodedId` exists only at admission; the bodies take `PoolId` and `BlockAddress`.

**Recovered behavior.** Admission keeps the stock order: context word, `CPSR.I`, dispatch-enabled for blocking forms, class, index, timeout for `+0xf4`, then the exists word. Create and delete ignore the class half and index `uxth(id)`. The allocator pops the free list before the bump frontier and advances by the rounded stride. A release with a waiter writes the block into the head waiter's `FixedPoolWait+0x0c`, unlinks it, completes the wait through the stock `0x44568` helper and notifies the waiter's home domain; otherwise it pushes the block onto the free list. FIFO/priority order is the creation attribute bit 0 and is applied by the retained stock enqueue helpers. Timed waits arm the per-task `g_fixed_pool_timers` entry through the stock `0x449cc` path; the stock timeout callback keeps the `-50` completion. The output word is written only on success, including after a resumed blocking get; `-49` and `-50` leave it untouched.

**Proved corrections.** Create stores `begin + stride*count` rather than `begin + size*count`, so a non-multiple-of-4 block size hands out the full requested count. Create and delete range-check `uxth(id)` before indexing. Delete releases every queued waiter with `-49` under the object lock instead of leaving it stranded. Release refuses a block above the bump frontier or already on the free list, in addition to stock's range/stride checks. `+0x170` reads the exists word and returns `-42` for a deleted or never-created pool instead of handing out the shared default placeholder. The create publishes the control block and `dmb` before the exists marker. Triggers and retained risks are the create/delete callers that relied on the out-of-range write, the deleted-pool placeholder alias, and a caller that releases a block it does not own.

**Shims and removal conditions.** The emitted adapter keeps stock raw-record/scheduler/timer seams: lock acquire/release `0x439d8/0x43a14`, notify `0x44018`, dispatch `0x43640`, I/F mask `0x42560/0x42568`, I save/restore `0x42574/0x4257c`, wait release/cancel `0x44568/0x44744`, wait enqueue/timed enqueue `0x44800/0x448e4`, and `__aeabi_uidivmod` `0x4da6c` (B `0xdda30`). Each is removed when the owned scheduler/lock, wait-queue, timer or CPU-mask entry replaces the corresponding stock body and the owned timer family owns the heap callback. Until then the adapter calls the stock address; no shim reimplements a retained body.

**Emitted ABI and native status.** The seven entries are four-byte branches at their stock addresses. Each owned body is a whole function in its own `.text.fp_body_*` input section, so the aggregate linker can place it in an entry-free carrier run without slicing; the largest is `admit` at 588 bytes (A and B), under the 672-byte union bound. The adapter gates its panic handler on `not(feature="component")`, so that link emits none. Their Rust bodies are placed at `0x50000` in the stage image; that placement is native-only scratch, not an admitted live address. `native/SuiteMain.hs` derives each flat-image origin from the emitted `.symbols` map, checks every stub and body symbol, and runs the stock and owned roles through `cachedObservedBatch`. The host target passes 23 cases. Both stage bins build. The native matrix passes all 88 rows (44 cases per domain, 0 failures): creation/refusal, bump/free-list/exhaustion, blocking and timed caller arrival with declared caller/target stack contexts, timer arming and cancellation, waiter handoff in FIFO and priority order, cross-domain notification, release boundary and duplicate-ownership refusals, delete/recreate, the `+0x170` deleted/never-created refusals, and resumed-caller rows. The B tick is entered at its prologue `0xd4d4c`, not the `0xd4d8c` retry loop. In `get-resume-wake` and `timed-resume-wake` the first task suspends in the service, a second task at `0x54000` releases the held block (or cancels the armed timer), the dispatcher switches back, and the original caller returns to `0x958f00` with `r0=0`, `sp=0x959800`, `r4..r11` preserved, CPSR control `0x13` and the delivered block in the caller's output word. `timed-resume-expiry` drives the retained tick in both domains, consumes the heap entry (`timer_count` zero), and returns `-50` with the output untouched. `get-resume-delete` has the second task delete the pool: the stock role stops at the driver, while the owned delete releases the suspended waiter, which returns `-49` with the output untouched. The matrix also checks returned-caller SP, callee-saved registers, CPSR control, caller-frame bytes, and arrival PC/SP; the suspended task's private saved SP is normalized out of the state comparison. Delete dispatches with the saved F bit and I masked, so the delete-waiter arrival CPSR is `0x93`, not `0xd3`. The allocator stores the output word before advancing the bump frontier, so an output word aliasing `control.next_free` cannot repeat a block; host counterexamples cover get, poll and `+0x170`. The B fixture maps the shared lowest-bit tables at `0x000dda64`/`0x0004daa0`; the B timer bodies are `0x000d4a80`/`0x000d4b44`/`0x000d4c20`/`0x000d4c68` and the B notify body includes its return at `0x000d4000`. A handler-context delete uses the delayed-dispatch word rather than saving the handler stack as a task continuation. This is native-fixture evidence only; no full-native, system or hardware fixed-pool claim is made.

**Frame scope.** The reviewed emitted call chains have these bounds, excluding asynchronous exception nesting. Retained callees contribute to the total; returned SP preservation is not a frame-demand proof.

| Service | Maximum owned nesting | Including examined retained callees |
| --- | ---: | ---: |
| Create | 96 bytes | 96 bytes |
| Delete | 120 bytes | 196 bytes |
| Blocking get | 160 bytes | 212 bytes |
| Timed get | 160 bytes | 240 bytes |
| Poll | 104 bytes | 104 bytes |
| Release | 136 bytes | 196 bytes |
| Unchecked poll | 112 bytes | 112 bytes |

A suspended get retains 188 bytes; dispatcher transfer adds temporary register/TLS scratch. The frozen family comparator passes 88 rows, while the generic native judge reports 176 failed role expectations under its default memory expectations. Those are not 176 passed judgments. Code, runner, case and raw-observation identities are retained under `private-data/scratch/RtosFixedPoolsSections/`; aggregate placement and system qualification remain separate.

### 4.19 Task-record create/delete root slots

`replacement/rtos_task_records` owns task create `+0x0` and task delete `+0x4` in both loaders. The typed core validates slot, priority, stack rounding, existence, liveness and the `T+0x8` binding; the ABI adapter keeps the stock I/F mask pair, ignores the class half and writes the executing loader's tables. Nine named differences cover invalid ids and ranges, create over a live record, stale bindings, delete of a live or missing record, and `exists` publication last after a DMB.

The 15 task-record cases and integrated layout, semaphore and fixed-pool host suites pass. The emitted adapter and host API share the same typed operations and normalized `Priority`; no separate raw-status implementation remains. The existing `cachedObservedBatch` oracle passes 31 cases per loader and 466 controls, with zero failures, reusing the current stock observations and executing changed owned code. Four emitted create/delete smoke cases return and match. The create group needs `0x168` bytes and delete `0x108`, so the native-only `0x50000`/`0x50200` artifact is not admitted live. The same technical review clears this source/native-fixture increment, not system or hardware behavior. Contract, retained callers, stock-defect triggers and evidence `E-RTOS-TASK-RECORD-HOST-150`: [task-record create/delete root slots](rtos-task-records.md).

### 4.20 Owned global-lock root extensions: host increment

`replacement/rtos_global_lock` owns the three root extensions `+0x161c` (single-try acquire), `+0x1620` (release) and `+0x1624` (observe), plus their atomic helper closure in both loaders. The root veneers select the lock word; the A32 helpers implement the exclusive pair and committed-store barrier. S1 scheduler/object locks and the exception vectors remain separate components.

**Contract.** `r0` is the lock word and `0` selects `0x400c0804`; `r1` is the stamp try stores unchanged. Try returns `0` after a committed exclusive store of `r1` into a word that read `0`, followed by a DMB, and `-1` at once on a nonzero word; a lost exclusive store repeats the pair. Release returns `0` after an exclusive store of `0` into a nonzero word, followed by a DMB, and `-1` on a free word; any nonzero value is cleared, without an owner check. Observe returns one plain load, with no exclusive access and no barrier. No operation reads CPSR or changes its control bits. Retained IRQ vectors use `(core index + 1) | 0x10` (0x11/0x12), while the image4 critical pair uses `(core index + 1) | 0x20` (0x21/0x22). Those callers and their tag construction remain outside this increment.

**Stock bytes.** The slot bodies at A `0x412cc`/B `0xd12cc` are 21 words and the atomic helpers at A `0x427d4`/B `0xd27d4` are 23 words; the words are identical across domains, with domain-relative branch immediates and `bx lr` in the atomics. The oracle refuses a pinned-loader slice that differs from the recorded words. The entry extent is bounded by the next published extension and the next function, not by a symbol name.

**Carrier.** The Rust veneers at the stock slot extents A `0x412cc`/`0x412e8`/`0x41304` and B `0xd12cc`/`0xd12e8`/`0xd1304` (0x1c each) apply the `r0` rule and tail-branch to narrow A32 helpers at A `0x427d4`/`0x427fc`/`0x42828` and B `0xd27d4`/`0xd27fc`/`0xd2828` (0x28/0x2c/0x8). The try and release helpers run `ldrex`, refuse or `strex`, retry on a lost store and take `dmb sy` only on the committed path; observe is `ldr r0, [r0]` and `bx lr`, byte-identical to stock. All six extents are their stock extents; the flat images are byte-identical (`bd667654…a594`) and the ELFs differ only in address. Linker assertions bind each symbol to its extent origin, refuse overflow and any unplaced text or writable data.

**Exercised host/native increment.** Sixteen host cases cover default and explicit words, free and held try, free and held release, plain observe, zero and arbitrary stamps and the local monitor's lost-store retry, including a retry that finds a holder. The oracle passes 64 comparisons, 1024 checks and 288 controls through cached native execution with current input admission and comparison. The convention cohort enters the helpers directly at A `0x427d4`/`0x427fc`/`0x42828` and B `0xd27d4`/`0xd27fc`/`0xd2828` with `r0=0x400c0804` and the IRQ stamps `0x11`/`0x12`. Each case checks `r0`, both lock words, CPSR control bits, `r4`-`r11`, SP and ordered writes/DMBs. Result, CPSR.I, word-byte, DMB and stored-value mutations refuse.

**Staging.** `stage-global-lock` requires the complete `0x1564`-byte flat image, present bytes for every admitted symbol, each symbol at its extent origin, the recorded stock words and zero gap bytes. It patches the six extents, confines changes to those extents and the main checksum, verifies the staged loaders and restores the original DAT. Controls refuse a root-only `0x54`-byte truncation, a gap byte, a moved symbol, an out-of-extent symbol and swapped loaders. Stock extent hashes are derived before helper installation; critical-pair and task-storage admission must run against those pre-installation bytes. Composition over the qualified scheduler candidate is a test base, not a granted cross-family carrier union.

**Limits.** The runner's single-core local monitor cannot lose a reservation between the exclusive pair, so retry remains host-model evidence; cross-core exclusive-monitor behavior, alias and cache laws remain open (law U12). The owned helpers preserve the ABI consumed by the retained IRQ vectors and image4 critical pair. Native helper cases and a staged candidate do not qualify those callers, vector integration, a cross-family carrier union, full native, system or hardware behavior.

### 4.21 Owned cyclic handler family: lifecycle, expiry and reentrancy

`replacement/rtos_cyclic` (crate `xm1_rtos_cyclic`, `no_std`, no statics) owns configure, schedule, expire, initialize, start `+0x110`, stop `+0x114`, create `+0x108` and delete `+0x10c` in both loaders. The typed core uses `CyclicSlot`, `CyclicConfig` and `CyclicControl` views added to `rtos_layout`, `PeriodWord`/`CyclicPeriod`, `CyclicAttributes`, `CyclicHandler`, `CyclicArgument`, `Expiry` and `CyclicStatus`. Create/delete ignore the class half and index the executing loader's 64-entry creation table; start/stop resolve class 0/1/2 to the selected context and validate the signed index against that context's count.

Corrections against the stock bodies are explicit. Create and start refuse period 0 with `E_PAR`; boot initialize leaves a zero-period record stopped; expire with a zero period delivers one callback, clears `active` and removes the entry instead of re-arming at the same expiry. Delete and stop always remove the timer entry and clear `active` before zeroing the creation record, so an active delete cannot leave a null handler or a looping tick. Expire captures the handler and argument before releasing the scheduler lock, so a concurrent delete cannot turn an in-flight expiry into a call to address 0. The re-arm happens before the callback and every reentrant start/stop/delete/create takes the same scheduler lock, so a later registration is not cleared or re-armed by the old expiry; no generation word is needed.

The timer join is named, not implicit. Start/stop/delete call the owned timer entries at A `0x44c60`/`0x44ca8` and B `0xd4c20`/`0xd4c68`; the stage treats an entry that is still linked before insert or not detached after remove as `E_SYS`, so the stock tick's non-detaching pop is refused rather than supported. Expiry runs through the owned timer tick, which detaches the popped entry before the callback. The subsystem oracle loads the owned timer image at its admitted A/B carriers beside the cyclic image.

Entry veneers sit at the stock addresses: A configure `0x4be3c`, schedule `0x4be84`, expire `0x4beb0`, initialize `0x4bf3c`, start `0x4bfcc`, stop `0x4c13c`, create `0x4d818`, delete `0x4d8a8`; B `0xdbdfc`, `0xdbe44`, `0xdbe70`, `0xdbefc`, `0xdbf8c`, `0xdc0fc`, `0xdd7dc`, `0xdd86c`. Entry bodies stay in the stock spans. The core bodies do not fit those spans, so the emitted artifact places them in a native-only scratch window at A `0x4c28c..0x4c94c` and B `0xdc24c..0xdc90c`, overwriting stock functions in that window. Live placement of the core bodies is open and is not claimed.

The host target passes 20 cyclic cases plus the added layout cases. The subsystem oracle uses `cachedObservedBatch` for 24 cases per domain (48 comparisons) and 162 controls; all 48 stock and owned cases pass their checks, all 162 controls hold and the terminal result has 0 failures. The fixture uses the stock timer words `g_timer_heap`/`g_timer_count`/`g_timer_base`/`g_tick_now`, with `g_cyclic_count` at A `0x4dacc` and B `0xdda90`; the receipt records the masked CPSR control. Configure publishes from the boot count word before context publication (`configure-unpublished`), and create defers the five request reads behind slot, mask and existence admission (`create-unreadable-existing` returns `-41` without an access). The zero-period Repair rows assert the complete creation record, and the expiry, delete and recreate rows assert the recorded callback argument, lock word and masked CPSR, with a receipt-argument control. Native acceptance is for this synthetic native subset only; system integration, hardware, live core placement, firmware heap ownership and independent scheduler-lock ownership remain open.

### 4.22 Owned IRQ control and handler registration: host and native increment

`replacement/rtos_irq_control` is one allocation-free artifact for the three callable root slots `+0x158`, `+0x15c` and `+0x160` in both loader domains. The typed contract is `replacement/rtos_irq_control/irq-control-contract.json`; sources are the v6 slot contracts in `replacement/media_permit/rtos_slot_contracts.json` and the pinned loader disassembly at x70view `55436fc85c0560dd252b0410af6f2427fe198cd3`.

**Boundary.** The services take a typed interrupt line `0..=127`, a validated registration and a table view. The table is 128 entries of 24 bytes at A `0x400538fc` / B `0x400e38bc`. The slot domain selects the table; CP14 `c2,c0,0` equal to `0x4003` selects controller bank `0xfffb3000`, any other value `0xfffb4000`. Enable and disable return 0 or -5; define returns 0, -17 for a refused non-null packet, and 0 for a null packet even when the line is above 127. Define applies `uxth` to its first argument before validation.

**Ordering.** Enable masks I, reads the table handler, refuses the default empty handler with no controller write, then reads the group enable word, writes `~0` to the clear word, read-modify-writes `+0x60` and `+0x100`, read-modify-writes `+0x180` and `+0x140` from the table mode selected by table `+0xc`, writes the handler and priority to `+0x400 + 4*line` and `+0x800 + 4*line`, writes the line bit to `+0x120` and the saved enable word OR the bit to `+0x80`, and restores I. Disable follows the stock clear sequence and writes the empty handler to `+0x400 + 4*line`; it leaves the registration table untouched. Define masks I and F, and its configure step masks I around the five table stores. The host oracle checks these sequences event by event, including the mask points and every refusal.

**Correction.** Enable and disable compare only `r0 > 127` in stock, so a negative number indexes below the table and controller bases. The typed boundary refuses it before any backend access and the A32 entry returns -5. Named counterexamples include -1, -2, -127, -128 and `i32::MIN`; no census id is negative. Define keeps the stock `uxth` interpretation of the low 16 bits before line validation.

**Verification.** `//replacement/rtos_irq_control:irq-host-tests` runs 18 host tests over a recording backend: mask/table/controller order, no-write refusals, both domains and banks, I/F preservation, define replacement, disable/enable republish and negative boundaries. The native suite passes 40 cases: 32 stock/owned matches cover define success, null reset, invalid packet, line 128, `0xffff`, `uxth` line `0x10000` and enable/disable refusal paths. Eight owned-only negative-boundary cases cover enable/disable corrections and define's unchanged null/packet handling. Entries remain at the stock addresses with the shared body at native-only scratch `0x50000`.

**What is not exercised.** Enable and disable success read and write controller words outside the native device allowlist, which admits only four bank words of size 4 and refuses bank reads. No owned full-system run was made; the stock bank sequence is recorded against the FujiMod controller model in [the timer and interrupt laws](xm1-timer-interrupt-laws.md) sections 4.4 and 7. The success branch is host-qualified only. A live placement still needs an admitted carrier for the body, and the full-system owned run needs that carrier or a RAM-push experiment. Vector, IRQ and SVC entry and exit, delivery, nesting, arbitration, the physical controller backend and hardware laws remain outside this IRQ-control increment. The global-lock helper ABI is a separate owned component; its integration with retained callers and the vector is not qualified here.

### 4.23 Owned task lifecycle services, S3d

`replacement/rtos_scheduler/src/s3d.rs` owns terminate `+0x18`, priority change `+0x1c`, timed sleep `+0x28`, wait release `+0x38`, handler wait release `+0x3c` and delay `+0x4c` in both domains. One `Machine`-generic core holds the six bodies; `stage/s3d_abi.rs` maps memory, IRQ state, lock function pointers and retained stock helper calls. Ids parse into a class-selected record and a signed one-based task number, keeping the executing domain distinct from the target record. Error precedence follows context, class/number, `C+0x24`, current task and locked state.

Terminate and the two release entries implement the `T+0x24`/`T+0x2c` cross-object handshakes: drop the record lock, take the owner object lock, retake the record lock and abandon the operation when the peer cleared the pending flag. Priority change does the same with `T+0x28` and reorders a waiter only when object attribute bit 0 selects priority order. Release applies pending priority, clears cancellation, resets a terminated waiter, and readies or leaves it suspended. Timed sleep and delay arm per-slot timers through the retained heap. Terminate restores the entry I state on the stock concurrent-completion path that leaked a masked IRQ state.

**Verification.** Thirty-five host behavior/effect/lifetime cases pass. An independent timer consumer seeds the stock 8-byte node with entry pointer at `+4`, and the 12-byte entry with callback/argument at `+4`/`+8`. Its uncancelled baseline expires, changes the result and readies the task; after release or termination removes the node, the same consumer produces no effect or duplicate ready entry. Membership uses the heap node, not an overwritten wait receipt. The former 12-byte node stride and `+8` pointer offset were corrected.

The native oracle passes 20 cases per domain with stock/owned comparison and independent role contracts. Returned R0, caller SP/R4-R11, CPSR control, writable/device memory and non-stack write multisets are checked. Each role must return its expected R0, including zero, or stop at its exact dispatcher address with the declared switch reason. Guards, budgets, unknown/unsupported execution, matching refusals, wrong zero returns and correct-PC/wrong-reason stops fail the production predicate. Both loader/helper closures are admitted at actual boundaries, including requeue A `0x44044`, release A `0x44744..0x44800` / B `0xd4704..0xd47c4`, and the independent early-B delta `0x8ffbc` rather than the later `0x8ffc0`. The interior `0x447d4` instruction is not a function entry.

The timed-sleep and delay cases execute a synthetic save/wake/return stub and return -50 and 0 to the original caller with stack, preserved registers and control restored. Notification passes the context id, including local id zero, rather than the record pointer. Local higher-priority dispatch reaches the local switch; both cross-domain directions ring the target IRQ120 and preserve the required lock, result, timer and ready-queue effects.

**Placement.** Six four-byte veneers retain the stock entries. Delay's `0x170` body is grounded at `0x46e4c`/`0xd6e0c` inside its extent. Other whole sections require terminate `0x3ac`, priority `0x440`, timed sleep `0x1e4`, release `0x454`, handler release `0x42c` and timer membership `0x48`, totaling `0x1298` relocatable bytes. The native fixture's `0x50000`/`0xe0000` addresses are not live carriers.

Synthetic single-core states and the test stub do not qualify cross-core concurrency, cache ordering, task/IRQ stack capacity, natural reach or the full dispatcher. Full-native, composed scheduler/platform, system and hardware qualification remain separate.

