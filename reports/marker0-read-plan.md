# Marker-0 read plan: the three lifecycle observations

Revision note (2026-10-11): four amendments from the probe-routes review (implementation accepted with no findings) - the two journal allowances stated separately, the derivation command's umask 077 and the reader's file rules, the rehearse forms and their scope, and the duplicate-address rejection outcome.

Revision note (2026-10-11, second): the phase-1 capture falsified window 0's pure struct-pointer premise and the plan re-derives phase 2 from the pinned-byte binding (section Window-0 binding): the window is the two-entry class descriptor table plus adjacent runtime objects, phase-2 windows are derived only for the class descriptor pair, deduplicated, and the remaining word classes take the unresolved discipline. The capture and the cursor sample stay the observation-1 input. P1 correction on recheck: the method-word spans are the pinned loader code (the descriptors are loader-published), with both loaders' publication values and the address/alias mapping and host-side resolution procedure recorded, and the exact derived window set for the captured pair is now in the plan (`0x00a0f86c`/4, `0x4004db40`/32, `0x4004dc20`/40, `0x400ddb00`/32, `0x400ddbe0`/40).

Prepared 2026-10-10 for the owner to run on XM1-body-01 (official 1.50), prepared work only. **Execution is blocked on the owner's go in chat and on one independent review of the new bytes** (`probe-routes`), under [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs). This is **H2 content, not H1**: every operation here enables adjustment index `0xf7` and restores it, and those are adjustment-cache writes. The gate is instrument-required rather than incidental: every routine of opcode `0x200001` acts only while `0xf7` is nonzero (the service README), so the RAM reads this plan needs cannot execute without the write pair. The plan therefore carries H2 writes and read-only content, and the earlier H1 no-write characterisation is withdrawn.

The plan settles item (e) of the placement work (RECIPE-PLACEMENT-01): the marker-0 block (ids 98-164 and 169 of the namespace table at `0x0093e6f4`) cannot be called free until the three lifecycle facts close. The prior capture (E-PROBE-OWNERSHIP-RUN-150) showed the table's markers (1 for ids 1-97 and 165-168, 0 for 98-164 and 169) and refuted the 33-id free list at the observable level; this plan does not re-litigate that.

## The three observations

| # | Observation | Target | Address | Length | What it settles |
| --- | --- | --- | --- | ---: | --- |
| 1 | Driver method implementations | the class descriptor table and its descriptors' method words | phase 1 `0x0096bd20`, 256 B; phase 2 per distinct class descriptor word `p`: `p+0x60`, 32 B and `p+0x140`, 40 B | 256 + adaptive | The `+0x64` check and `+0x7c` wait implementation addresses, whose marker-0 branch decides refuse, create or route |
| 2 | Id lifecycles after the captures | the namespace table, bracketed | `0x0093e600`, `0x0093e700`, `0x0093e800`, `0x0093e900` (256 B each) | 1024 per capture | Observed endpoint stability or change between the first and last captures; intervening lifecycles and candidate collision stay unresolved either way |
| 3 | Creation-batch timing | the allocation-search cursor cell | `0x00a0f86c`, 4 B, sampled in BOTH route phases | 4 per sample | The cursor's endpoint behavior across the route work; the batch's position relative to the hook is not established by this channel (see the residual) |

The cursor cell at `0x40a0f86c` (canonical `0x00a0f86c`) is written by two search families and read by both. The descending allocation-search family (`0x724840` region) rejects values at or below 168 and stores id-1 on success, so a search starting at 170 stores 169 and stored values run at 168 and above - values above 168 are valid outcomes of this family, not a domain mismatch. The zero-bounded family (`0x729264`-`0x7292e8`) touches the same cell and operates in the range from its own start down toward zero. Creation writes lower values (id-1 for created ids) and destruction moves the cell upward. Captured values are classified by the domains of these paths: 168 and above belong to the descending search family, values in the creation/destruction range below 168 belong to the other families' domain, and a value matching no path's domain is out-of-domain and takes the unresolved verdict below.

## Schedule: the executable order

The bracket is explicit: the first ownership capture opens it and the final ownership capture closes it, with the route work inside.

| Order | Command | Output | Transfers |
| --- | --- | --- | ---: |
| 1 | `probe-ownership` | `OUT1-probe-ownership` | 68 |
| 2 | `probe-routes fixed` | `OUT3a-probe-routes-phase1` | 32 |
| 3 | derivation, then `probe-routes derived PLAN_JSON` | `OUT3b-probe-routes-phase2` | 24 + 8k |
| 4 | `probe-ownership` | `OUT2-probe-ownership` | 68 |

```sh
devenv shell -- bazel run //transport:camera -- probe-ownership "$PWD/private-data/sessions/OUT1-probe-ownership"
devenv shell -- bazel run //transport:camera -- probe-routes fixed "$PWD/private-data/sessions/OUT3a-probe-routes-phase1"
(umask 077 && od -A n -t x4 -j 0 -N 8 -v "$PWD/private-data/sessions/OUT3a-probe-routes-phase1/windows/0096bd20.bin" | \
  awk 'BEGIN{n=0; printf "{\"windows\":[{\"index\":0,\"address\":\"0x00a0f86c\",\"length\":4,\"purpose\":\"allocation-search cursor cell\"}"} {for(i=1;i<=NF;i++){p=strtonum("0x"$i); if(p!=0 && p!=0x00a0f86c && p+0x60!=0x00a0f86c && p+0x140!=0x00a0f86c && !seen[p]++){printf ",{\"index\":%d,\"address\":\"0x%08x\",\"length\":32,\"purpose\":\"class descriptor +0x60 method fields\"},{\"index\":%d,\"address\":\"0x%08x\",\"length\":40,\"purpose\":\"class descriptor +0x140 method fields\"}", 2*n+1, p+0x60, 2*n+2, p+0x140; n++}}} END{printf "]}"}' \
  > "$PWD/private-data/read-plans/marker0-routes-phase2.json")
devenv shell -- bazel run //transport:camera -- probe-routes derived "$PWD/private-data/read-plans/marker0-routes-phase2.json" "$PWD/private-data/sessions/OUT3b-probe-routes-phase2"
devenv shell -- bazel run //transport:camera -- probe-ownership "$PWD/private-data/sessions/OUT2-probe-ownership"
```

Phase 2's capture input is `OUT3a-probe-routes-phase1/windows/0096bd20.bin`: the derivation reads the class descriptor pair at the window's first two words, derives one window pair per distinct descriptor word (deduplicated, cursor collisions skipped), and writes the derived window list for the phase-2 run. Its output is the distinct `OUT3b-probe-routes-phase2` directory; phase-2 is not a repeat of phase 1. Aggregate across the four runs: 68 + 32 + (24 + 8k) + 68 = 192 + 8k transfers for `k` distinct descriptor words, and `k` is at most 2 by the binding, so at the captured pair (k = 2) the aggregate is 208 transfers and the phase-2 run is 40. The journal allowances differ per operation: `probe-ownership` uses 8192 per window per journal file; `probe-routes` uses `Read.journalCapFor`, 65536 + 4096 per window per journal file, capped at 8 MiB.

## Phase-2 invocation contract

`transport/Main.hs` parses operations explicitly and has no implicit phase selection, so the prepared operation has two CLI forms:

- `probe-routes fixed OUTDIR` - phase 1: the two fixed windows (window 0's 256 B capture and the cursor cell 4 B).
- `probe-routes derived PLAN_JSON OUTDIR` - phase 2: the window list read from a JSON plan file.

The derived list's path is `private-data/read-plans/marker0-routes-phase2.json`. Its format is a window list object mirroring the read-plan shape with RAM addresses: `{"windows":[{"index":N,"address":"0xXXXXXXXX","length":N,"purpose":"string"}]}`, starting with the cursor cell window and followed by one `+0x60` (32 B) and one `+0x140` (40 B) window per distinct class descriptor word from the binding.

The derivation command (schedule step 3, run between commands 2 and 3) reads the phase-1 record and writes the plan under `umask 077`, so the file it creates or recreates carries no group or other permission bits.

The complete phase-2 command, with the phase-1 capture as its input:

```sh
devenv shell -- bazel run //transport:camera -- probe-routes derived "$PWD/private-data/read-plans/marker0-routes-phase2.json" "$PWD/private-data/sessions/OUT3b-probe-routes-phase2"
```

The reader's plan-file rules are absolute path, owned by the running user, a regular file, and no group or other permission bits. A standalone redirection under a conventional umask creates a file the reader rejects, which is why the schedule's derivation runs inside `umask 077`.

The operation rejects duplicate addresses before any USB traffic: repeated window addresses in the plan file, a `+0x60`/`+0x140` pair colliding with another pair, or any derived window colliding with the cursor address stops phase 2. The revised derivation deduplicates descriptor words and skips cursor collisions, so the captured pair produces no duplicates; the rejection remains the backstop for a malformed or colliding plan file. A rejection leaves the five-step bracket incomplete. That outcome is not a completed lifecycle comparison: the captures that finished stand alone and no combined verdict is derivable from them.

## Transfer spec

The `0x200001` function-2 primitive takes one bounded request per read: a 256-byte window takes two 128-byte requests; the 4-byte, 32-byte and 40-byte windows each take one request at their own length (the `probe-straps` 4-byte reads are the same shape). Four transfers per read request plus the gate/session operations give the totals above: `probe-ownership` is 12 requests and 68 transfers as documented; `probe-routes` phase 1 is 3 requests (two for the 256-byte window, one for the 4-byte cell) and 32 transfers; phase 2 is `1 + 2k` requests (the cell plus one request per method window) and `24 + 8k` transfers for `k` distinct class descriptor words, at most 2 by the binding - 5 requests and 40 transfers at the captured pair.

## Operation probe-routes (new bytes, review required)

Same gate enable, readback and restore sequence as `probe-ownership` (the `0xf7` index write to 1 through `0x1a0002` function 3, the config-window readback requiring `0xf7` == 1, the reads, the `0xf7` restore to 0 and the readback requiring 0 before C4) - the writes that make this H2 and that no read-only variant can avoid while `0x200001` is gated on `0xf7`.

| Window | Address | Length | Requests | Contents |
| --- | --- | ---: | ---: | --- |
| 0 | `0x0096bd20` | 256 | 2 | the class descriptor table and adjacent objects: the descriptor pair at idx 0/1 and 62 words of the four other classes (see the Window-0 binding section) |
| 1 | `0x00a0f86c` | 4 | 1 | the allocation-search cursor cell (also sampled in phase 2) |

Phase 2 windows: the cursor cell `0x00a0f86c`, 4 B (1 request), plus per distinct class descriptor word `p`: `p+0x60`, 32 B (covers `+0x64` check and `+0x7c` wait) and `p+0x140`, 40 B (covers `+0x148` release and `+0x164` create), one request each.

## Offline rehearse

The two rehearse forms are the offline mocked end of this plan: `rehearse probe-routes fixed` and `rehearse probe-routes derived PLAN_JSON`. Their scope is synthetic address-based payloads: they validate the transport mechanics (request shapes, transfer counts, journal bounds, the derived-list reader and its file rules, the duplicate-address rejection) and the capture mechanics (window records written per phase), not the pointer or method semantics. A passing rehearse says nothing about what the captured words mean; that reading happens host-side from the real capture afterwards.

## Window-0 binding, against the pinned bytes

The phase-1 capture falsified the pure struct-pointer reading of window 0; the capture at `private-data/sessions/OUT3a-probe-routes-phase1/windows/0096bd20.bin` stays the observation-1 input. The pinned bytes bind the window as the class descriptor table at `0x96bd20` plus adjacent runtime objects, not a 64-entry vtable. Byte-read basis: the id4 readers index `0x96bd20` by a class value - the selector call through `*(table[0] + 0x1618)` at `0x7256d0`, `0x7257b4`, `0x7257ec`, `0x72586c`, or a caller-carried class argument at `0x7255f8`/`0x725734` - then dereference method slots at `+0x10c`/`+0x110`/`+0x114`/`+0x164`/`+0x168`/`+0x174`; the marker-0 handlers fold `id | marker<<16` through the same table into method slots at `+0x64`/`+0x68`/`+0x7c`/`+0x148`/`+0x164` (E-PLACEMENT-MARKER0-LIMIT-150); ids 1..104 map to one of two classes through `0x7cad9c` (byte-read, reports/auto-act-script.md), so the class index domain is two entries; and `0x725a04` fills the two words from the loader words `0x40010`/`0xd0010` (byte-read, reports/service-query-and-jig-reset.md).

The window's 64 words hold five classes (idx = word offset from `0x96bd20` in units of 4):

| Class | Words (idx: value) | Binding |
| --- | --- | --- |
| class descriptor pair (struct pointers) | 0: `0x4004dae0`, 1: `0x400ddaa0` | the loader-published per-CPU service tables, read as `table[class]` then a method slot; the only words the pinned readers dereference as struct pointers |
| code addresses | 12: `0x0069bde4`, 13: `0x0069c9f8`, 18: `0x00ec554c`, 34 and 40: `0x006d38e8`, 37: `0x006d3954`, 43: `0x006d4700`, 46: `0x006d377c` | value class; no pinned reader establishes struct-pointer use at these slots |
| intra-region pointers | 16: `0x0096bd64`, 38: `0x0096bdbc`, 44: `0x0096bdd4` | point into the window itself (`+0x44`, `+0x9c`, `+0xb4`); value class only |
| packed words | 8: `0x09091900`, 14: `0x22023200` | the message-record field shape `{u16 id, u8 source, u8 destination}` by value; semantics unresolved |
| small integers | 11: `0xa`, 15: `0x8`, 19/21/25/28: `0x1`, 22: `0x6`, 30/31: `0x5` | scalar fields; semantics unresolved |

Phase 2 derives windows only for the class descriptor pair, deduplicated: one `+0x60` (32 B) and one `+0x140` (40 B) window per distinct descriptor word, covering the marker-0 method fields (`+0x64`/`+0x68`/`+0x7c` and `+0x148`/`+0x164`). At the captured pair the derived windows are, in order: the cursor cell `0x00a0f86c`/4, `0x4004db40`/32 and `0x4004dc20`/40 (`0x4004dae0` + `0x60` and + `0x140`), `0x400ddb00`/32 and `0x400ddbe0`/40 (`0x400ddaa0` + `0x60` and + `0x140`). The other four classes get no windows and take the decision table's unresolved discipline.

Address and alias mapping: the captured descriptor words are the `0x40000000`-alias forms (`0x4004dae0`/`0x400ddaa0`) of the canonical loader RAM addresses `0x4dae0` (loader-40000) and `0xddaa0` (loader-d0000), one table per loader, and the method words are published canonical (`0x0004xxxx`/`0x000dxxxx`). Host-side resolution of a method word: clear the `0x40000000` alias bit if set, then resolve against the pinned images - `decode/loader-40000.bin` at `0x40000`, `decode/loader-d0000.bin` at `0xd0000`, and the pinned id4/id5 decodes - and read the implementation bytes from the matching pinned image slice.

## Expected record shapes

- `probe-ownership` window `0x0093e600`: the namespace table starts at `+0xf4`; the id words run `0x0093e6f4` (id 1) to `0x0093e998` (id 169) at stride 4. Expected words: 0, 1 or 2 per the marker semantics (the prior capture showed 0 and 1 only). The four windows are 256 B each and tile `0x0093e600`-`0x0093e9ff`.
- `probe-ownership` window `0x00829a00`: the heap allocation-break cell at `+8` (`0x00829a08`), one word (`0x00a7f000`-class values; the prior capture read `0x00a7f000`). This is the heap cursor, distinct from the `0x00a0f86c` id cursor.
- `probe-routes` window 0: 64 words bound by the Window-0 binding section: the class descriptor pair at idx 0/1 (the loader-published service tables in their `0x40000000`-alias forms, see the alias mapping) and 62 words of adjacent-object fields in the four other classes (code addresses, intra-region pointers, packed message-shaped words, small integers). A null word marks an unwritten slot of whichever object owns it.
- `probe-routes` window 1 and the phase-2 cell window: one word each; classified by the two families' domains on the same cell: 168 and above are valid descending-search outcomes (a 170 start stores 169), values below 168 fall in the zero-bounded and creation/destruction families' domain, and a value matching no path's domain is out-of-domain and unresolved. The captured phase-1 sample `0xdd` (221) is in the descending family's domain (id-1 from a 222 start), a valid outcome and not a domain mismatch.
- Phase-2 method windows (per class descriptor word): the `+0x64`/`+0x68`/`+0x7c`/`+0x148`/`+0x164` words; the descriptors are loader-published, so the expected values are method code addresses in the pinned loader code spans (or 0), not the id4/id5 spans. `loader-40000` publishes `+0x64` = `0x0004d2a8` and `+0x68` = `0x0004d338` (`0x4056c`-`0x40580`), `+0x7c` = `0x00047dd0` (`0x405b4`-`0x405bc`), `+0x148` = `0x0004cbd8` (`0x40818`-`0x40820`) and `+0x164` = `0x00041140` (`0x4086c`-`0x40874`); `loader-d0000`'s counterparts are `+0x64` = `0x000dd26c` and `+0x68` = `0x000dd2fc` (`0xd056c`-`0xd0580`), `+0x7c` = `0x000d7d90` (`0xd05b4`-`0xd05bc`), `+0x148` = `0x000dcb98` (`0xd0818`-`0xd0820`) and `+0x164` = `0x000d1140` (`0xd086c`-`0xd0874`). The implementations at those addresses are read host-side from the pinned loader images afterwards; the capture only yields the addresses.

## Decision table

Endpoint language is deliberate: equal endpoints are observed endpoint stability, not proof of absence of an intervening destroy-and-recreate cycle. Intervening lifecycles and candidate collision are unresolved in every row, including create-on-demand. No row produces a positive free-at-hook verdict while the hook-time observation is named absent.

| Observation | Capture outcome | Consequence for the free-id question |
| --- | --- | --- |
| 2 | Candidate-block marker words (ids 98-164 and 169) change between OUT1 and OUT2 | The stores update one id word at a time, so the changed ids are stated. Each changed candidate id is an observed endpoint change at that id and is unsafe to claim at that id. Each unchanged candidate id stays UNRESOLVED (not approved and not disproven). Approval is withheld for the unproven candidates; a single id's change does not make the whole block unsafe to claim. Intervening lifecycles and candidate collision unresolved. |
| 2 | Marker words outside the candidate block (ids 1-97, 165-168) differ but all candidate words are unchanged | An unrelated id word change: lifecycle activity outside the block. UNRESOLVED for the free-id question (not-established-safe); never a proven unsafe-to-claim verdict. |
| 2 | Marker words unchanged but words outside the marker-word range (`0x0093e6f4`-`0x0093e997`) differ | The comparison covers only the 169 marker words; other table-region words are out of scope. UNRESOLVED. |
| 2 | Marker endpoints identical between OUT1 and OUT2 | Observed endpoint stability over the bracket. Intervening destroy-and-recreate is not excluded and later search activity is not excluded; candidate collision unresolved. No positive verdict. |
| 1 | Window-0 words in the four non-descriptor classes (code addresses, intra-region pointers, packed words, small integers) | Out-of-binding evidence at those slots: the pinned readers do not establish struct-pointer use for them and no windows are derived. UNRESOLVED (not-established-safe) for the free-id question; never unsafe-to-claim and never a positive verdict. |
| 1 | A class descriptor word is 0 or non-canonical | Runtime-state-dependent evidence: the pair is populated later than the capture or holds a non-pointer value at capture time. UNRESOLVED; no verdict. |
| 1 | Method words resolve to code addresses and the implementations show marker-0 = refuse | The claim path fails on marker-0 ids; no id in the block is claimable; the typed extension has no usable resource id until a created-but-unused id is proven (none is, today). Intervening lifecycles and candidate collision unresolved. |
| 1 | Implementations show marker-0 = create-on-demand | The claim creates the routing state; conditional at best. Endpoint stability of the table is required and even then intervening lifecycles and candidate collision stay unresolved; the create-on-demand marker may collide with the creation family's own class values. No positive verdict. |
| 1 | Implementations show marker-0 = route elsewhere (default owner) | The claim routes to another owner; collision risk is decided by that owner's behavior, read host-side from the implementation. Intervening lifecycles and candidate collision unresolved. |
| 1 | Method words are 0 or unresolvable (class descriptor slots unpopulated at capture) | Runtime-state-dependent evidence: the descriptors are populated later than the capture or by a path this channel does not reach. Unresolved; no verdict. |
| 3 | Cursor endpoints stable across both route phases | Consistent with no search activity during the route work; stable snapshots do not exclude later search activity and do not date any earlier activity. UNRESOLVED for the batch's position. |
| 3 | Cursor endpoints differ between the route phases, with the values in the descending family's domain (168 and above) | Search-family activity during the route work; consistent with a lifecycle in progress. UNRESOLVED (not-established-safe); never a proven unsafe-to-claim verdict on its own. |
| 3 | Cursor endpoints differ with values in the creation/destruction range below 168 | Creation or destruction touched the cell. UNRESOLVED (not-established-safe) on its own; combined with a marker change at a candidate id it supports unsafe-to-claim at that id via the marker row. |
| 3 | A cursor sample matches no path's domain | Out-of-domain: the cell's value does not match the pinned cursor semantics (a different writer or an unmodelled domain). Unresolved; no verdict. |

## Combined outcomes and precedence

The final verdict is the weakest of the resolved rows, with this precedence: (1) per-id marker-word changes decide unsafe-to-claim at the changed candidate ids only - the changed ids are stated, approval is withheld for the unproven candidates, and unchanged candidate ids stay UNRESOLVED (not approved and not disproven); unrelated id-word changes, cursor-only changes and out-of-range table-word changes map to UNRESOLVED (not-established-safe) and never reach unsafe-to-claim on their own; (2) absent any candidate change, a marker-0 = refuse result decides not-claimable; (3) absent both, the create-on-demand and route-elsewhere rows give conditional results that remain conditional - intervening lifecycles and candidate collision are unresolved and no positive free-at-hook verdict issues while the hook-time observation is absent. Missing evidence (a run that did not complete), partial evidence (a window with an unexpected reply), mixed evidence (conflicting rows), out-of-domain values and runtime-state-dependent evidence (unpopulated vtables, method words unresolvable) all produce UNRESOLVED as the combined verdict. The combined verdict vocabulary is: unsafe-to-claim, not-claimable, conditional, unresolved. There is no free-at-hook verdict in the vocabulary while the residual below stands.

Residual no read-only run settles: a capture AT the hook's firing point. The hook runs at the image-5 entry, before any service session exists, so this channel cannot date activity before OUT1; stable snapshots also do not exclude later search activity. The exact hook-time table state needs an instrumented hook or an in-boot read path; that is outside this plan's scope and is named here rather than inferred.

## What the owner does

1. Note the current date, time, language and one C-slot value; a POWER/OFF jig exit resets them as usual (E-JIG-OBSERVED). Use a fully charged battery or the dummy-battery supply, keep the card free of firmware files, and have the USB cable connected.
2. Power off, then enter jig mode as usual with USB connected (shutter and up held at power on). Entry needs the host attached (E-JIG-ENTRY-REQUIRES-USB).
3. Run the executable sequence above in one jig entry: the four service commands with the derivation step between the route phases. Each command opens and closes its own service session. The phase-2 window list is derived from phase 1's output between commands 2 and 3. If a run stops, the captures that finished stand on their own and the bracket is closed at the last completed capture.
4. Exit jig mode. Ending by cutting the dummy-battery supply preserved every watched setting in two no-command jig sessions (E-JIG-POWERCUT-PRESERVES-150); with these read-only-content commands the same is expected but unobserved, so note which ending was used. The POWER/OFF ending resets date, time, language and the C slot.
5. Power-cycle normally, take a shot and play it back, and report in chat: the output directories and result lines, which exit was used, and whether the settings survived.

## Review class

**H2**: the `0xf7` enable and restore pair is an adjustment-cache write and the `0x200001` reads are gated on it, so no operation here is write-free. Runs 1 and 4 are the existing `probe-ownership` operation (H2 content, no new operation bytes). Runs 2 and 3 are `probe-routes` (H2 content, new operation bytes) and need one independent review of the new bytes before their first live run. Execution is blocked on the owner's go and on that review. The original operation criterion for these instruments was no state-changing bytes; this plan carries the gate writes because the read primitive cannot execute without them, so running it requires an explicit owner scope revision of that criterion first - stated here as a prerequisite, not assumed. The captures are supporting observations for the placement work; none alone clears lifetime ownership, and the combined verdict cannot read free-at-hook while the hook-time observation is absent.
