# RTOS kernel startup: contract and fixed-bank form

Part of [#156](https://github.com/skeet70/latent-image/issues/156) for [#117](https://github.com/skeet70/latent-image/issues/117). Sources: x70view `55436fc85c0560dd252b0410af6f2427fe198cd3`, `decomp/firmwares/xm1-150/kernel0/src/` (`core/kernel_start.c`, `core/kernel_publication.c`, `core/context_initialize.c`, `object/object_setup.c`, `sched/task_initialize.c`, `sched/task_setup.c`, `sync/{semaphore,event_flag,data_queue,mailbox,fixed_pool}_initialize.c`, `cpu/core_sync.c`, `cpu/core_sync_wait.c`, `native_recovery/native_globals.c`), `symbols/kernel{0,1}.txt`, and the pinned loaders `loader-40000` (A) and `loader-d0000` (B). Every address, constant, store order and caller below was read from the loader disassembly of both domains (`private-data/analysis/x-m1-1.50/decode/loader-{40000,d0000}.dis`); where a decomp name or comment disagrees with the bytes, the bytes are used.

## Extents

| Entry | A | B | Bytes A / B |
| --- | --- | --- | --- |
| `kernel_publish_interfaces` | `[0x40430,0x409ac)` | `[0xd0430,0xd09ac)` | 1,404 / 1,404 |
| `kernel_start` (literal pool to `0x40b00`) | `[0x409ac,0x40b00)` | `[0xd09ac,0xd0b00)` | 340 / 340 |
| `kernel_start_tick_timer` | `[0x40b5c,0x40c58)` | `[0xd0b5c,0xd0c58)` | 252 / 252 |
| core configuration, decomp `FUN_00043a58` | `[0x43a58,0x43ac0)` | `[0xd3a0c,0xd3a7c)` | 104 / 112 |
| `kernel_initialize_owner_contexts` | `[0x43ac0,0x43c48)` | `[0xd3a7c,0xd3c04)` | 392 / 392 |
| `kernel_initialize_tasks` | `[0x43e18,0x43f70)` | `[0xd3dd4,0xd3f2c)` | 344 / 344 |
| `kernel_autostart_tasks` | `[0x43f9c,0x44018)` | `[0xd3f58,0xd3fd4)` | 124 / 124 |
| `semaphore_initialize` | `[0x47580,0x47604)` | `[0xd7540,0xd75c4)` | 132 / 132 |
| `event_flag_initialize` | `[0x48044,0x480cc)` | `[0xd8004,0xd808c)` | 136 / 136 |
| `data_queue_initialize` | `[0x48e38,0x48edc)` | `[0xd8df8,0xd8e9c)` | 164 / 164 |
| `mailbox_initialize` | `[0x4a9bc,0x4aa44)` | `[0xda97c,0xdaa04)` | 136 / 136 |
| `fixed_pool_initialize` | `[0x4b38c,0x4b41c)` | `[0xdb34c,0xdb3dc)` | 144 / 144 |
| `kernel_initialize_objects` (literal pool to `0x4d094`) | `[0x4cea0,0x4d094)` | `[0xdce60,0xdd054)` | 500 / 500 |

The group is 4,172 bytes in A and 4,180 in B. Apart from code and RAM addresses the two loaders differ only in the core configuration and owner contexts (below). Contiguous extents form eleven carriers per domain: publish plus `kernel_start`, and configuration plus owner contexts, are one carrier each.

Outside the group, and called at their stock addresses: the core-sync routines (`0x43834` sets `g_interrupt_context`, `0x4386c` resets the sync block, `kernel_wait_core_sync` `0x438a8`, the rendezvous `0x43934`; `cpu/core_sync*.c`), the IRQ initializer wrapper `0x43a3c`/`0xd39f0`, the VFP enable `0x424f8`, the IRQ-120 registration `0x40d98`, the exception-hook setter `0x40c6c`, the empty hooks `0x4ce78`/`0xdce38` and `0x4ccbc`/`0xdcc7c`, `kernel_patch_vectors` `0x4ccd0`/`0xdcc90`, `syslog_initialize` `0x41354`, `syslog_core0_record` `0x415a0`, `kernel_initialize_timers` `0x44a50`/`0xd4a10`, `cyclic_configure` `0x4be3c`/`0xdbdfc`, `cyclic_initialize` `0x4bf3c`/`0xdbefc`, `kernel_zero_words` `0x434f0`, `kernel_core_index` `0x427b8`, the dispatcher `0x43688`, `kernel_reset_task` `0x43dd0`/`0xd3d8c` and the make-ready helper `0x43f70`/`0xd3f2c`. The last two are also called by the task lifecycle services. The tick callback `0x40b24` and the IRQ-120 callback `0x40b00` are interrupt-time code, registered here but not part of startup. B entries are A + `0x90000` unless listed.

## Entries and callers

Every caller is a direct `b`/`bl` in the loader of the same domain; no group entry is a root-table slot or a literal word in either loader.

| Entry | Called by (A; B the same offsets) |
| --- | --- |
| `kernel_start(phase)` | the reset path's `b` at `0x400c4` / `0xd00c4`, with R0 the phase routine |
| publish | `kernel_start` `0x409dc` (core 0), `0x40aac` (core 1) |
| configuration, `kernel_initialize_objects`, owner contexts, tick timer | `kernel_start` on both paths |
| `kernel_autostart_tasks` | `kernel_start` `0x40a70` |
| `kernel_initialize_tasks` and the five object initializers | `kernel_initialize_objects` `0x4d058..0x4d06c` |

## `kernel_start`

`kernel_core_index()` (CP14 `c2,c0,0` equal to `0x4003` gives 0) selects the path once.

- Core 0: `kernel_zero_words(0x400a0800, 0x20100)`, publish, `syslog_initialize(0)`, `0x43834`, `0x43a3c`, `0x424f8`, `0x40d98`, `phase(0)`, store 1 to `0x400a00c4`, `dsb sy`, `0x4386c`, configuration, `kernel_initialize_timers`, `kernel_initialize_objects`, `0x4ce78`, rendezvous `0x43934`, owner contexts, tick timer, `phase(1)`, `phase(3)`.
- Core 1: poll `0x400a00c4` until nonzero, 100 `nop`s between polls; then publish, `syslog_initialize(0)`, `0x43834`, `0x43a3c`, `0x424f8`, `0x40d98`, `kernel_wait_core_sync`, configuration, `kernel_initialize_timers`, `kernel_initialize_objects`, `0x4ce78`, rendezvous, owner contexts, tick timer, `phase(4)`.
- Both: `r = phase(2)`, `0x40c6c(r)` (stores the exception hook `0x4004f114` / `0x400df0d4`), `kernel_autostart_tasks`, `cyclic_initialize`, `syslog_initialize(1)`, store 1 to `g_kernel_started` (`0x4005927c` / `0x400e923c`), dispatcher `0x43688(-10)`, `syslog_core0_record(1, 0, 0)`, then `b .`.

## Interface publication

`kernel_publish_interfaces` fills the 1,419-word root of its domain (A `0x4004dae0`, B `0x400ddaa0`) in this order: the 93 service words `+0x000..+0x170`, the primary tick-count pointer `0x400c0800` at `+0x174`, the adapters `0x412bc`, `0x412cc`, `0x412e8`, `0x41304` (B `0xd12bc..`) at `+0x1618..+0x1624`, the initial stack top (`0x40063460` / `0x400f3420`) at `+0x1628`, then for each of 120 descriptors at `+0x178 + 44*i`: the id `i+1`, eight pointers `0x4005beac + 40*i + 4*k` (B `0x400ebe6c`), and two pointers `0x4005ff0c + 56*i + 24` and `+28` (B `0x400efecc`). It reads nothing. `+0x104` is the tick slot the IRQ-32 callback calls and `+0x158..+0x160` the IRQ slots of the [interrupt contract](rtos-irq-cache-services.md); image4's phase-0 installer copies `0x40010` (the root address) to `0x96bd20` and every image4 kernel call goes through these words.

## What each initializer writes

Addresses are A; B is A + `0x8ffc0` for kernel RAM. Shared words (`0x400a....`, `0x400c....`) are the same in both. Stores are listed in execution order.

- **Core configuration** (A `0x43a58`, B `0xd3a0c`): with `cfg` the own record of the shared table at `0x400c0818` (A `cfg = 0x400c0818`, owner 1; B `0x400c0888`, owner 2): `g_kernel_config` (`0x400546d4`) = `cfg`; `cfg+0` = owner; scheduler lock word `0x400545c0` = 0; `cfg+4` = `0x400545c0`; object lock word `0x400545c4` = 0; `cfg+8` = `0x400545c4`; `g_current_context` (`0x400545c8`) = own context (A `0x400545cc`, B `0x400e4610`, the second record).
- **Owner contexts** (A `0x43ac0`, B `0xd3a7c`): for owner 1 then 2, context `0x400545cc + 0x84*(owner-1)` from config `0x400c0818 + 0x70*(owner-1)`: `+0` = 0 for the own owner (A 1, B 2), else `cfg+0`; `+4` = owner; `+0xc`/`+0x18` = acquire `0x439d8` (B `0xd398c`); `+0x10`/`+0x1c` = release `0x43a14` (B `0xd39c8`); `+0x14` = `cfg+8`; `+8` = `cfg+4`; then `+0x20..+0x80` = `cfg+0xc..+0x6c` in ascending order. The stock body loads pairs with `ldm` and stores them with `stm`; each store is a word. A and B differ only in the owner compared against and the lock routine addresses.
- **Tick timer** (A `0x40b5c`): `0x400c0800` = 0; `kernel_define_irq(32, {0, 0x40b24, 1, 1, 1})` with the descriptor on the stack; on core 0 only, read the counter `0xfff91000` twice: `reload = 30000 - (c1 % 1875) * 30000 / 1875`, `0x400c0800 = c2 / 1875`, control `0xfff91008` = 0, reload `0xfff91004` = `reload`, 100 `nop`s, control = `0x23`, reload = 30000. Then, on both cores, the `+0x158` slot routine `0x41108(32)`.
- **Tasks** (A `0x43e18`): `0x40054768` = 0, `0x40054760` = 0, `0x400546d8` = 0, `0x40054764` = 1; the 16 ready heads `0x400546dc + 8p` point to themselves (`+0` then `+4`); `0x4005475c` = 0. For `i` below the loader word `0x4dabc` (120, re-read each iteration): task `0x4005ff0c + 56i`: `+8` = definition `0x4005beac + 40i`; byte `+0xe` &= `0xfe`; `+0x20` = 1 (B 2); `+0x28`, `+0x24`, `+0x2c`, `+0x30` = 0; `kernel_reset_task(task)`. Then in `*g_kernel_config`: `+0xc` = `0x400546dc`, `+0x10` = `0x4005475c`, `+0x14` = `0x40054760`, `+0x18` = `0x40054768`, `+0x1c` = `0x40054764`, `+0x30` = `0x4005ff0c`, `+0x34` = the count, `+0x68` = `0x43728` (B `0xd3728`).
- **Autostart** (A `0x43f9c`): for `i` below the count word: `id = 0x400622d4[i]`, task `0x4005ff0c + 56*(id-1)` (not range-checked); if the word at the task's definition has bit 1, `0x43f70(*g_current_context, task)`.
- **Objects** (A `0x4cea0`): for each of the 120 definitions `0x4005beac + 40i`: `{0, i, 0, 15, 4, 0x40059280, 0, 0, i+1, 0}`, then slot id `0x400622d4[i]` = `i+1`; 250 semaphore records `0x4005dfd8`: `{0, 1, 1, 0}`; 120 flag records `0x4005d178`: `{0, 0, 0}`; 120 data-queue records `0x4005988c`: `{0, 1, 0x40059284, 0}`; the mailbox record `0x4005d16c`: `{2, 32, 0}`; 122 fixed-pool records `0x4006194c`: `{0, 4, 0x40059288, 0x4005928c, 0}`; 64 cyclic records `0x4005ef78`: words `+4..+0x14` = 0, the attribute word `+0` kept; 64 three-word records `0x4005dcd8` = 0; the boot vector patch `0x4005f578` = `{0, 0, 0}`. Then tasks, the five object initializers, `cyclic_configure`, `0x4ccbc`, `kernel_patch_vectors`. With the zeroed patch record and the loader count 1, `kernel_patch_vectors` stores 0 through `g_vector_slots[0]` into the loader word `0x403a0` (B `0xd03a0`).
- **Object initializers**: each reads its count from the loader (pool `0x4dab0`, mailbox `+4`, semaphore `+8`, data queue `+0x10`, flag `+0x14`; B `0xdda74..`) once, and for each index writes the control block `cb`: `cb+0` = `cb+4` = `cb`, `cb+8` = its config record, then semaphore and flag `cb+0xc` = config `+4`; data queue `cb+0xc` = `cb+0x10` = `cb+0xc`, `cb+0x14..+0x1c` = 0; mailbox `cb+0xc` = 0; fixed pool `cb+0xc` = config `+8`, `cb+0x10` = 0. Then `*g_kernel_config` gets the control table and count at `+0x38/+0x3c` (semaphores `0x4005af0c`, 16 B), `+0x40/+0x44` (flags `0x400624b4`, 16 B, config stride 12), `+0x48/+0x4c` (data queues `0x4005a00c`, 32 B), `+0x50/+0x54` (mailboxes `0x40062c34`, 20 B, config stride 12), `+0x58/+0x5c` (fixed pools `0x4005f584`, 20 B, config stride 20).

## Ordering across cores

- Core 1 starts nothing until core 0 has cleared the syslog area `[0x400a0800,0x400c0900)` (which includes the tick count, the core-sync block `0x400c0808..` and both config records), published its root and run `phase(0)`; core 0 then stores the release word and issues `dsb sy` before resetting the core-sync block.
- `0x4386c` writes the magic `0x87654321` at `0x400c080c`; core 1's `kernel_wait_core_sync` waits for it before its own configuration, so core 1's config record is written after core 0's clear.
- Each core writes only its own config record (configuration and the five object initializers through `g_kernel_config`, tasks, timers, cyclic configure). Owner contexts reads both records, so it runs after the rendezvous: A sets `0x400c0810` and waits until `0x400c0810` and `0x400c0814` are both 1, then stores the magic at `0x400c0808` and clears `0x400c080c`; B sets `0x400c0814` and waits for the magic at `0x400c0808`.
- Each core's root table, contexts, task and object records are in its own RAM; both cores write the shared tick count `0x400c0800` (zero, then core 0 the start count).

## Declared differences

- Retained routines are called through a register (`blx`) at their stock addresses; the stock code uses `bl`. Stack frames differ and the tick-timer descriptor may be read from the bank's read-only data instead of the stack; stack bytes are not part of the contract.
- The owned tick timer computes the first reload as `30000 - 16 * (c1 % 1875)`, equal to stock's expression for every 32-bit count.
- After the dispatcher returns, the bank `kernel_start` tail-calls `syslog_core0_record` and spins in its export; stock calls it and spins in place.
- B's publication repeats loader B's 93 service words in the bank; `replacement/interface_initializer` publishes the same words from the loader-prefix image and is not reusable from a bank crate.

## Fixed-bank form

`//replacement/rtos_kernel_init:kernel-bank-{a,b}` are the rlibs a bank root links: `stage/abi.rs` with feature `bank` exports `xm1_rtos_kernel_<entry>_{a,b}` for `publish_interfaces`, `start`, `start_tick_timer`, `configure_core`, `initialize_owner_contexts`, `initialize_tasks`, `autostart_tasks`, `initialize_semaphores`, `initialize_event_flags`, `initialize_data_queues`, `initialize_mailboxes`, `initialize_fixed_pools` and `initialize_objects`. The semantics are in `src/lib.rs`, generic over a `Machine` that supplies guest words, retained calls, the phase routine, `dsb` and `nop`; the host tests in `src/tests.rs` drive it. Loader A's publication is `replacement/primary_initializer/src.rs`, compiled into the bank with its panic handler excluded; its own image is unchanged. Group entries call each other directly, so a bank `kernel_start` reaches the bank bodies without veneers.

`:kernel-bank-probe-{a,b}` link the same bodies at the fixed body origin `0x00ba0020` behind a four-byte `b` veneer at each of the 13 stock entries, the rest of each carrier `udf`-filled; `-split` writes one binary per carrier and `-bank.bin`. The probe does not replace `primary_initializer`'s J5 piece at `0x40430` or `loader_orchestration`'s `kernel_start` carrier `[0x409ac,0x40b00)` in a startup package; a bank image takes the bank bodies or those pieces, not both. No RTOS platform carrier intersects the group.

## Native comparison

`//replacement/rtos_kernel_init:kernel-bank-cohort` runs each case on the stock loader and on the loader with the probe carriers written over it and the bank body mapped at `0x00ba0020`. Both roles run with the VFP enable `0x424f8`'s first word replaced by `bx lr`, because the runner refuses its CPACR write. Kernel RAM (`0x4004d000` / `0x400dd000`, 220 KiB), the shared window `[0x400a0000,0x400c1000)` and the controller banks start from a distinct word at every address, with bit 1 clear (`cyclic_initialize` schedules a cyclic whose attribute has it set); the timer words are memory, not a timer model.

Cases per domain: publication; `kernel_start` on core 0, stopped at the rendezvous entry, and on core 1, stopped at the dispatcher entry with the release word, the core-sync magic and the peer flag preset; configuration; owner contexts; tasks; autostart over 120 slot ids in stride-7 order with every fifth definition autostarting; the five object initializers; `kernel_initialize_objects`; the tick timer on core 1 and on core 0 for counter values 0, 1, 1874, 1875, 123456789 and `0xffffffff`. The phase routine records each phase in a log region and returns `0x5100 + phase`.

One comparator judges every case and every mutant: the outcome (return, or the stop address), callee-saved R4–R11 and SP after a return, R0 at the dispatcher stop, CPSR control bits, the final bytes of kernel RAM, the shared window, the timer, the controller banks, the vector-slot word and the phase log, and, where the runner retains the whole access log, the ordered writes into those regions and the ordered device accesses and barriers. The runner keeps 4,096 log rows; the two `kernel_start` cases and `kernel_initialize_objects` exceed that, so for them write order and barriers are not observed and the comparison is the final state.

## Open

- Linking into the default bank waits for Root to land on main, and the from-reset pair needs a bank image.
- Core 0's path after the rendezvous (owner contexts, tick timer, `phase(1)`, `phase(3)`) is compared through the single-entry cases, not inside `kernel_start`: a single-core case cannot pass the rendezvous after core 0 has reset the sync block.
- The `kernel_start` and object-initialization cases compare final state only, under the runner's log cap.
