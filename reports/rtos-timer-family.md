# RTOS timer family: contract and fixed-bank form

Part of [#156](https://github.com/skeet70/latent-image/issues/156). Sources: x70view `55436fc85c0560dd252b0410af6f2427fe198cd3`, `decomp/firmwares/xm1-150/kernel{0,1}/src/time/` (`timer_heap.c`, `timer_initialize.c`, `timer_tick.c`, all exact), the pinned loaders `loader-40000` (A) and `loader-d0000` (B), and the slot contracts in [`rtos_slot_contracts.json`](../replacement/media_permit/rtos_slot_contracts.json). Every caller below was read from the loader disassembly of both domains. The owned core and its declared differences are in [platform report §4.15](rtos-platform-options.md#415-owned-timer-heap-membership-host-increment); this report fixes the family boundary for the fixed bank.

## Services

| Service | A | B | Entered by |
| --- | --- | --- | --- |
| `kernel_initialize_timers` | `0x44a50` | `0xd4a10` | `kernel_start` A `0x40a18`, `0x40ad0` / B `0xd0a18`, `0xd0ad0` |
| `kernel_insert_timer(context, entry, expiry)` | `0x44c60` | `0xd4c20` | timed-wait arm A `0x44560` / B `0xd4520` (`FUN_000445cc`), `kernel_delay_task` A `0x46f94` / B `0xd6f54`, `cyclic_schedule` A `0x4bea4` / B `0xdbe64` |
| `kernel_remove_timer(context, entry)` | `0x44ca8` | `0xd4c68` | wait release A `0x44588` / B `0xd4548` (`FUN_00044650`), forced release A `0x44718` / B `0xd46d8` (`FUN_000447e4`), `cyclic_start` A `0x4c0e8` / B `0xdc0a8`, `cyclic_stop` A `0x4c258` / B `0xdc218` |
| `kernel_timer_tick`, root `+0x104` | `0x44d8c` | `0xd4d4c` | interrupt-32 callback A `0x40b24` / B `0xd0b24` through its own published `+0x104` |
| `kernel_timer_sift_up` | `0x44ac0` | `0xd4a80` | insert and remove only |
| `kernel_timer_sift_down` | `0x44b84` | `0xd4b44` | remove and tick only |

All callers are direct `bl` to the stock entry addresses, except the tick, which is reached only through the published slot. No image-4 code calls a timer entry. The sift helpers have no caller outside the family, so the family's external surface is four entries per domain.

The family is one ownership unit: the four entries and both sift helpers of a domain share the node array and the entry index protocol, and removing an entry that a stock sift has moved needs the same index convention on both sides.

## Records and raw consumers

- Configuration `+0x20..+0x2c` (A `*0x400546d4`, B `*0x400e4694`): the initializer publishes the node array, count, base and tick words. The stock context initializer (`core/context_initialize.c`) copies them unchanged to context `+0x34..+0x40`; every later reader goes through the context.
- Words: count A `0x40054778` / B `0x400e4738`, base A `0x4005476c` / B `0x400e472c`, tick A `0x40054770` / B `0x400e4730`, time offset A `0x40054774` / B `0x400e4734`. Set time `+0xfc` and get time `+0x100` (own-first, owned) read base and offset raw. The timed services read `*context+0x40` to arm `expiry = tick + timeout`.
- Entry `{index, callback, argument}`: callers own the storage (`g_sleep_timers`, `g_wait_timers`, the per-object timer arrays and cyclic records). Wait release and forced release read the index word and skip removal when it is zero, so the index is a raw-consumed membership token, not private state.
- Cross-domain: cyclic start/stop select the other domain's context by class and insert into or remove from its heap with this domain's code; those entries carry the owner domain's `cyclic_expire` and are serviced by the owner's tick.
- Callbacks: the tick runs `kernel_timeout_task` (A `0x44318`), the delay completion (A `0x44634`) and `cyclic_expire` (A `0x4beb0`), each entered with the scheduler lock held and I set.

Reset zeroes the words as kernel BSS. The owned initializer publishes the owned arena (A `[0x40a5a200,0x40a5aa00)`, B the next `0x800`) instead of `g_timer_heap`; the stock array stays unused.

## Tick interaction

The interrupt-32 vector holds the global interrupt lock `0x400c0804` for the whole callback. The tick requires handler context with I clear, masks I, takes the executing domain's scheduler lock through the context's lock record, pops every due node, calls its callback with the lock held, then releases, briefly unmasks, remasks and reacquires before the next pop. It ends with `base = tick`, `tick += 10` (clock units, 10 per tick) and returns 0, or `-25` when entered from a task or with I set.

## Declared differences

The owned bodies keep the three corrections of §4.15: a popped or removed entry's index is written 0 (stock leaves its last slot), a linked entry is refused on insert without a write, and the tick releases the lock record it acquired rather than the post-callback context's. The 184-node arena bound refuses the 185th insertion. Each is a role-specific expectation in the cohorts, not a masked difference.

## Fixed-bank form

`replacement/rtos_timer/stage/abi.rs` with feature `bank` exports the four bodies as `xm1_rtos_timer_{initialize,insert,remove,tick}_{a,b}` and emits no stock-address veneer; the sift helpers, the post-removal repair, `finish` and the lock release are private to each domain's crate. I is set and cleared with `cpsid i`/`cpsie i`, the form the bank's stack admission accepts. The bodies reference only absolute kernel words and function pointers read from the context, so they import nothing from the loaders. `//replacement/rtos_timer:timer-bank-{a,b}` are the rlibs a bank root links.

In a bank image each stock entry keeps a four-byte veneer, a branch to its domain's bank body, and the rest of the timer carrier (`[0x44a50,0x44f2c)`, `[0xd4a10,0xd4eec)`) is fill. The other families keep calling the stock entry addresses.

`//replacement/rtos_timer:timer-bank-probe-{a,b}` link the same bodies at the fixed body origin `0x00ba0020` behind those veneers, with the carrier filled, and `-split` extracts the carrier and bank binaries. `//replacement/rtos_timer/oracle:timer-oracle` maps the bank binary as one more owned region when given `--bank-{a,b}-{bin,symbols}` (`:timer-bank-oracle` binds them). Cases, checks, mutants and controls are the existing timer cohort's; only the code mapping changes. The result is E-RTOS-TIMER-BANK-NATIVE-150.

## Open

- Bank placement and the from-reset pair need joint bank/loader admission, which is not on main.
- The cross-domain cyclic insert and remove are not exercised by a native case; they reach the same entry code with the other context.
- Core-1 instruction visibility of the bank is the physical limit recorded for the flag/mailbox bank.
