# Settings-log instrument: capacity, act record and limits (1.50)

Status: the area-2 stored-settings record as an instrument. It accounts for record selection, capacity and bank erasure in any `read-media` capture, and it records the ten act-logged bindings that [the byte map](settings-byte-map-150.md) analyses. It changes no firmware, calibration or persistent state. Nothing here authorizes a run: camera work follows [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs), and a later batch that extends the observed value mappings is priced by section 1.

## 1. Capacity, read this first

The settings log has 62 physical slots, 31 per bank, and writes one record per committing POWER/OFF. Filling the last free slot does not erase a bank: the next append does. An erase removes the older bank's 31 records, not necessarily the acts being measured. The analysis must check the surviving predecessor and each act after any erase.

Read 0 measures capacity after its own session has begun. Do not charge Read 0 a second time against that measurement. With supply-cut endings, both reads and no-change verification boots are expected to append zero records.

| Item after the Read 0 measurement | Records |
| --- | --- |
| Each committing act | 1 |
| Verification boots with no change | 0 |
| Read 1 with a supply-cut ending | 0 |
| Reserved margin for an unexpected extra commit | 2 |
| **Ten acts with the margin** | **12** |

The physical no-erase limit is `free` acts; the next-session conservative gate is `max(0, free - 2)` acts. The two-slot margin is a reserve, not an observed cost. If the conservative gate permits fewer acts than planned, split the session and read between batches. Do not treat an old fill measurement as the current one. The append-and-erase chain and its observed fill are in [the byte map section 2](settings-byte-map-150.md#2-capacity-and-the-boundary).

**Hardware-observed, E-SETTINGS-ACTS-150:** Read 0 on 2026-10-09 measured bank 0 31/31 and bank 1 21/31, ten free slots. The old `free - 4` gate printed six, but ten acts were run. Read 1 holds all ten appended records, filling bank 1 to 31/31 without erasing or wrapping; neither read nor the verification boots added a record between the measurements. All ten diffs survive. This run used the physical limit and left no reserve.

## 2. What this instrument cannot see

- The C-slot record. It is a different store (logical area 3, its own two banks); the area-2 record carries the ordinary body only. The C-slot payload fields, bound or unbound, are outside this instrument and need a raw read of area 3 that no current operation performs.
- The three unnamed cache-carried fields (properties `0x15b`, `0x15c`, `0x15d`). Their ordinary bytes sit at payload `+0x48`, `+0x47` and `+0x49` (static-inference from the defaults constructor), but no menu node and no mapped control writes them, so no act can move them deliberately. A byte that never moves stays unbound.
- Live-only state: the white-balance grid axes, the adjustment cache, the retained header and anything not inside the `0x118`-byte ordinary body.
- What an act did before the power-off: the record holds the net state at commit, so a setting changed twice within one act shows only the final value, and a setting changed and changed back cancels out.
- Input events. A record is written when something saves, so this instrument sees committing actions only. Buttons, dials and sequences are the [input map's](input-map-150.md) subject.

## 3. Null results and the ending

A null result on any act first requires checking whether its commit and predecessor survived the bank history, then whether its final value differed at commit and whether its writer is inside the ordinary body. The note sheet's commit column does not establish record retention after a bank erase. An act may move more than one byte; the act binds the whole move and the analysis splits the fields.

End maintenance sessions by cutting the dummy-battery supply. A POWER-switch ending writes a defaults record and resets date, time, language and the C slot (E-JIG-CYCLE-D-150), which would cost a record slot and the settings under measurement. A deliberate POWER-switch tail control is not recommended: the read has already run when the ending happens, so its defaults record would land outside the capture, and the alignment it would anchor is already carried by the record count and by each act's own verification.

## 4. Reproduction

Both commands read host files only. `read-media` captures the two area-2 banks and their journals; the checker prints the fill and the capacity gate from a capture folder.

```sh
devenv shell -- bazel run //transport:camera -- read-media
devenv shell -- runghc private-data/analysis/x-m1-1.50/read-media/LogDiffCheck.hs <session-folder>
```

## 5. The note sheet

**Hardware-observed:** the filled note sheet records 2026-10-09, X-M1 1.50, Read 0 at `private-data/sessions/20261009T070523Z-read-media` and Read 1 at `private-data/sessions/20261009T072111Z-read-media`. Each act changed one control and ended with an ordinary POWER/OFF, which commits one record; every act's value was seen again at the next boot, so every commit landed.

| Act | Control | From -> To |
| ---: | --- | --- |
| 1 | IMAGE QUALITY | FINE -> FINE+RAW |
| 2 | DYNAMIC RANGE | AUTO -> 100% |
| 3 | HIGHLIGHT TONE | 0 -> +2 |
| 4 | SHADOW TONE | 0 -> +2 |
| 5 | COLOR | 0 -> +2 |
| 6 | SHARPNESS | 0 -> +2 |
| 7 | NOISE REDUCTION | 0 -> +2 |
| 8 | WHITE BALANCE | AUTO -> FINE, no shift |
| 9 | ISO | 200 -> 6400 |
| 10 | FOCUS MODE | AREA -> CONTINUOUS |

The per-act byte, bit and width results are in [the byte map](settings-byte-map-150.md#3-per-act-byte-and-bit-map). Act 1 also moved companion payload `+0x70` and act 9 moved `+0x5e`; both are separate two-bit properties, not wider copies of quality or ISO. One observed transition exercises only the bits that differ between its two codes, so no act establishes a field's full width by itself.

Evidence labels: the record mechanics, the field widths and the constructor overlay are **byte-read** and **static-inference** over the pinned 1.50 images; the ten act bindings, the ten-slot fill without an erase and commit-on-difference behaviour are **hardware-observed** (E-SETTINGS-ACTS-150, E-JIG-CYCLE-D-150). [The public record-layout report](adjustment-and-service-interfaces.md#ordinary-settings-record-layout) publishes the reusable interfaces and [the byte map](settings-byte-map-150.md) publishes the act-logged result.
