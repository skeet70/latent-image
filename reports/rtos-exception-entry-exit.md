# X-M1 1.50 exception entry and exit (#156)

Ledger: `E-RTOS-EXCEPTION-ENTRY-EXIT-150`. Scope: the loader exception entries, the common dispatcher, acknowledge and end-of-interrupt ordering, the IRQ nesting lifetime and the scheduler handoff in both X-M1 1.50 loaders. The image4 abort installation, the retained diagnostic hooks, the shutdown poll, halt and the VFP owner stay stock.

## 1. Owned extents

Each loader's bytes were read from its own listing, not derived from the other.

| Block | A | B | bytes |
|---|---|---|---|
| vector bodies and IRQ entry/exit | `0x40100..0x40368` | `0xd0100..0xd0368` | 0x268 |
| fault record in retained-literal slack | `0x403a0..0x40410` | `0xd03a0..0xd0410` | 0x70 |
| common dispatcher | `0x40cac..0x40d50` | `0xd0cac..0xd0d50` | 0xa4 |
| ack/EOI wrappers and FIQ hook | `0x40d50..0x40d98` | `0xd0d50..0xd0d98` | 0x48 |
| Rust decision bodies | `0x430a8..0x432f8` | `0xd30a8..0xd32f8` | 0x250 |

The vector bodies `0x40100..0x40368` and the CP14 identity `0x427b8..0x4286c` are byte-identical between loaders. The dispatcher differs in six words (`0x40cbc`, `0x40cc0`, `0x40cf8`, `0x40cfc`, `0x40d18`, `0x40d1c`), acknowledge and end-of-interrupt in thirteen words each (the handler-table, active-slot and unhandled-counter immediates), the dispatch tail in one word (`0x437bc`) and the first transfer's idle stack in one (`0x436f4`). A blanket "byte-identical bodies" reading is wrong for those blocks.

The loader install at `0x40020..0x40084` still writes the seven target cells `+0x24..+0x3c` in the order undefined, SVC, prefetch, data, reserved, IRQ, FIQ and never writes `+0x20`. The retained literal pool at `0x40370` (IRQ stack top `0x70002000`) and `0x40410..0x40428` (dispatcher `0x40cac`, FIQ hook `0x40d70`, counter, lock, wrappers, dispatch flag) is read by the carrier and is not patched.

## 2. Carrier and frames

`stage/carrier.S` keeps every banked and mode-changing operation. The IRQ entry runs in SVC mode on the task stack, saves `r0-r3/r12/lr/pc` and the interrupted `spsr` as the 8-word frame `{spsr, r0, r1, r2, r3, r12, lr, pc}`, and for the first entry moves to `0x70002000` and stores the frame pointer at `0x70001ffc`. Nested entries keep the frame on the task stack. The handler runs with I clear and F as the interrupted context had it; acknowledge and end-of-interrupt run with I set. The fault vectors save the 16-word frame `{r0..r12, lr, spsr, r12, lr}` on the current mode stack and tail-call the dispatcher at `0x40cac`. The return restores the SPSR F bit from the current context and uses `ldm ...^`.

The image4 abort handlers at `0x61c73c` and `0x61c7f0` are installed by the image4 patch at vector slots `0x2c`/`0x30`; both tail-call `0x40cac` with kind 3 or 4 (literal `0x61c8c4 = 0x40cac`). The carrier keeps that entry ABI. The hook cells `0x4004f10c` (resume gate), `0x4004f110` (FIQ), `0x4004f114` (exception) and the 17-word record at `0x4004f118` are read and published unchanged; the dispatcher calls the exception hook as `hook(kind, frame->pc)` and the FIQ hook as `hook(frame->pc)`. The shutdown poll `0x40df8`, halt `0x410fc` and VFP owner `0x42420` remain stock.

## 3. Decisions

`src/lib.rs` holds the decisions over a `Machine`:

- `lock_acquire`: observe `0x400c0804`; a free word tries the exclusive pair with tag `0x11` (CP14 identity `0x4003`) or `0x12`, retrying the pair on a lost store; a word holding the own tag proceeds; a foreign tag re-observes.
- `acknowledge`: select `0xfffb3000` or `0xfffb4000` by identity, read `+0x200`; zero or `0x42840` increments `0x400544fc` and logs code 6; otherwise scan `+0x230..+0x23c` for the first nonzero group and its highest bit, set TPIDRURW bit 31, and store `{+0x1c0, line}` at `0x40054500 + 8*priority`. No pending bit is cleared.
- `end_of_interrupt`: re-derive the line from `+0x230`, clear its `+0x120` pending bit, clear TPIDRURW bit 31 only when the nesting counter is 1, clear the priority slot to `{0, -1}`, and write zero to `+0x200`.
- `exit`: compute `counter - 1`; release the lock only when it is zero; decrement; consume `0x400546d8` and report dispatch only at zero. The release precedes the decrement and the flag consumption.
- `record_fault`: publish the kind and 16 frame words, log `(kind, pc)`, and return the published hook for the carrier to call.

## 4. Host evidence

`//replacement/rtos_exception:exception-host-tests` passes 15 behavioral cases: outer acquire/acknowledge, nested own-tag re-entry, nested exit keeping the lock and flag, outer release-before-decrement-before-dispatch order, outer return, the premature-unmask counterexample (counter 1 at unmask leaves the lock held), acknowledge not clearing pending, unhandled code 6, end-of-interrupt re-deriving a changed line, TPIDRURW only at depth one, lost-try retry, secondary tag 18, fault record/log/hook, absent hooks, and the FIQ one-argument hook. `//replacement/rtos_exception:exception-artifact-tests` checks both linked images' entry symbols and first instructions against the stock extents.

## 5. Staging and the full-system attempt

`//replacement/rtos_exception/stage:stage-exception` admits the canonical DAT and both loaders, checks every entry symbol and extent, patches only the owned extents, rebuilds the container, verifies the outside bytes and restores the source. On the canonical 1.50 DAT it produced a 38,405,148-byte candidate (SHA-256 `87ca8798e1b9e61fa65b01c5cd6878ab208aacff501869738de29b851170d979`) with both patched loaders.

The intended full-system scenario is `//emulation:idle-tasks --role owned` with `--loader-a`/`--loader-b` set to the staged loaders, so the real IRQ 32/120 delivery, frame, acknowledge, end-of-interrupt and return run through the owned vectors. The attempted run refused at the observer's first-wrapper source check (`installed source differs at first wrapper`) before any IRQ boundary. The observer binary used was the 07:04 SDK build; the later diagnostic edit did not reach it because the external `lab_fujimod_xm1` repository kept its earlier source copy, and the run was not repeated. The candidate DAT and the staged loaders are retained under `private-data/scratch/rtos_exception/stage1/`; the run output is `private-data/scratch/rtos_exception/run1`. No IRQ delivery, nesting or dispatch boundary is claimed.

## 6. Limits

Physical arbitration, priority, active-bitmap and nesting semantics, the `+0x200` read/EOI relatch, WFI with I set, the exclusive monitor across cores, the vector-base alias and the counter-equals-one startup window remain unqualified. The native-case runner refuses SPSR, banked and mode-changing MSR forms, so it cannot qualify the carrier. FIQ and the abort stacks are uninitialized by the loaders. The owned decisions call the retained helpers at their stock addresses; aggregate composition may replace those bytes with the owned global-lock helpers under the unchanged ABI.
