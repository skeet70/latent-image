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
- Per domain, 27 slots are partially described and 70 are unknown. The extent tool derives 23 bodies and refuses 74; 68 of the refusals hit unadmitted status-register instructions [Draft:17].
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
| -25 | nonzero context or IRQ masked | activate `+0xc`, current-task `+0x16c` | Manifest; Draft:24,29 |
| -41 | C+36 already nonzero / eventflag record +8 already nonzero | create `+0`, per-task eventflag create `+0x80` | Manifest; Draft:23 |
| -42 | creation record absent | activate | Manifest |
| -43 | already pending activation | activate | Manifest |
| -50 | finite queue full/empty, flag-wait wrapper special case | `+0x9c`, `+0xac`, `+0xb0`, `+0xc8` | Manifest; Draft:28 |
| -63 | cleanup `0x72612c(0)` result if lower operations return | image4 | Map A.1; Inventory:1633 |
| -64 | stack-check failure | image4 `0x725b74` path | Draft:46; Inventory:1635 |
| -60, -62 | message-wrapper returns; sibling `os_error.h` names them MBX_INVALID_LENGTH and MBX_FULL (non-X-M1) | image4 message wrappers | `E-ITRON-VOCAB-150` |
| -1 | try-lock `0x427d4` / unlock `0x427fc` | loader helpers | Map A.1 |

Objects are 120 C/T task records per kernel, per-task eventflag records (12 B) and nodes (16 B), scheduler domains (132 B), ready lists and bitmaps, resources/semaphores (97 constructed by root104), mailbox/message objects with pools, and timer records of 24 bytes [Map A.2; Draft:23-29,43; E-RTOS-SLOT-MANIFEST-V5-150].

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
| per-task eventflag create `+0x80` `{attribute 0 or 4, pattern 0}`, duplicate -41; attribute 4 clears the whole word [Manifest v5 `0x80`; Draft:27] | T_CFLG `{flgatr, iflgptn}`; TA_WSGL 0x00, TA_CLR 0x04; cre_flg E_OBJ [spec] | |
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

1. **Encoded IDs and dual domains.** IDs are `(class<<16)|id`, the kernel rejects a bad class with -18, and there are two roots plus extensions `0x1618..0x1624` (identity and critical lock retry/release/observe) [Draft:23-24,29]. µITRON 4.0 is uniprocessor. Multiprocessor µITRON exists only as later extensions such as TOPPERS FDMP/FMP (https://www.toppers.jp/fdmp-kernel.html, https://www.toppers.jp/fmp-kernel.html).
2. **Current-task query.** `+0x16c` refuses masked or non-task context with -25, and the image4 wrapper converts failure to 0 [Map A.1]. µITRON get_tid returns TSK_NONE (= 0) from non-task contexts [spec get_tid]. NORTi `vget_tid()` returns the ID directly [no4guide.pdf]. The wrapper restores the µITRON-visible result.
3. **IRQ save/restore with mask** (`0x164/0x168`) follows the NORTi `vdis_psw/vset_psw` shape, not µITRON 4.0.
4. **Non-standard slots and layout.** `0x170` buffer acquire sits outside the family order [Manifest]. The tick pointer, 120 raw descriptors and stack-top word inside the root, and their external raw-layout readers, are not part of any µITRON API [Draft:46-48].
5. **Critical pair.** The pair keeps a private prior mask in r10, and a release after failed acquisition clears a peer lock. That is stock behaviour, not a lock guarantee [Draft:59; Map A.1].
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
  - **B (class 2, id 119):** inside the stock critical pair `0x7235d8/0x7236b0` the hook calls B's root `+0x80`, the per-task eventflag create, with `{attribute 0, pattern 0}`, then the task create `+0x0` with `{flags 2, argument 1, entry, priority 14, 2 KiB stack}`. That is the order of stock task create `0x725dd8`, which passes attribute 4 unless descriptor word +0x18 is `0xa5f0a5f0` (E-RTOS-SLOT-MANIFEST-V5-150). After leaving the pair it activates B through the executing core's `+0xc`.
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

**Emulated result (E-EMBASSY-PHASE1-EMULATED-150).** One run of the demonstration and one of each control on the stock DAT, cardless FujiMod RAM-push route, with the modal75 FILM SIMULATION trajectory, all at `ea90d676`, when both hosts used id 119 and FujiMod counted down. The reviewed checker `replacement/embassy_platform/check-demo.jq` accepts all four runs:

- **Demonstration:**
  - Install, route and unload pass with every step matched and no forbidden fetch.
  - All six creation and activation statuses are 0, and both host tasks entered.
  - Both domains measured the counter counting down and kept sub-tick time, with no clock regression.
  - By handback each 100 ms ticker had run 33 iterations over a 321-tick span, and both kept running after unload.
  - The latest wake was 68,757 counts (about 23 ms) on A and 32,347 on B, and A had two wakes later than two ticks. No kernel wake came early.
  - A rang B's doorbell 3 times and B received all 3 through the kernel wakeup. The observer traced 4 `+0x2c` calls with B's id 0x00020077, and the worst delivery latency was 2,861 counts (about 1 ms).
  - The observer traced 39 `+0x28` calls on core 0 and 43 on core 1. The stock route makes none (E-RTOS-SLOT-CENSUS-STOCK-150).
  - Stack high-water was 304 bytes on each domain.
- **No executors:** the same hook and route with nothing created produce the same four captures as the demonstration (PNG and source-plane digests).
- **No pender kernel:** the tickers keep running, and B receives none of its doorbells, with no traced `+0x2c` for B.
- **Counter up:** both domains detect the down-counting model, fall back to tick-only time, and keep their timers on schedule.

Two earlier inputs are preserved as failures:

- The first stopped in FujiMod at the hook's byte exclusive (`ldrexb/strexb`; "R-profile nonword exclusive execution is not implemented"). The start flag is now a word atomic.
- The second passed the route, but neither host task ran at priority 15.

**Allocator-created A (E-EMBASSY-ALLOC-A-EMULATED-150).** With A created through `0x728804`, one run of the `counter-up` object `8888b6d1…` (experiment `embassy_phase1_counter_up`, on the down-counting model) passes install, route and unload with every step matched. The allocator returned id 120 and A entered. Both domains fell back to tick-only time, A and B ran 33 and 34 ticker iterations by handback, and B received all 3 doorbells through `+0x2c`. After the run that control became `embassy_phase1_counter_down` for FujiMod's up-counting default model, under which the demonstration and its controls have not yet run.

**Limits:**

- These are emulated coexistence demonstrations on a model with no caches, not kernel or silicon qualification.
- B's timeouts on hardware depend on line 32 reaching core 1 (laws U4).
- Wake resolution is one tick.
- The host tasks keep running after `ram-unload`, and A holds one of the allocator's 16 class-1 ids for the session.
- Before any camera run, the payload must be the `counter-up` build, and the push and run need the owner's go. Core 1's instruction cache over the payload needs nothing beyond the install: the shared installer runs stock `0x006b0c3c` and `0x0061c490` on core 1 over the whole image, then `peer_cache_action` on core 1 ends in DSB/ICIALLU/DSB/ISB, and core 1 repeats ICIALLU after GO (`menu/ram_push_installer.c:199-206,239-289`; E-RAM-PUSH-INSTALL-150).
