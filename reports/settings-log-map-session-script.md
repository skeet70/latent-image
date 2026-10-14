# Settings-byte mapping session at the bench: ten settings, one per power cycle (1.50)

Status: the owner ran this session on 2026-10-09; the filled note sheet is section 5 and the [byte bindings](settings-byte-map-150.md) cover all ten acts. The instructions remain the next session's bench procedure. It changes no firmware, no calibration and no persistent state beyond the listed settings, which the owner sets himself through the ordinary menus. Nothing here authorizes a run: the owner's go under [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs) covers it.

The session binds the stored bytes of each listed control inside the area-2 settings record. Each act changes one setting and ends with an ordinary POWER/OFF, which commits one record; the difference between consecutive records is then what that act changed. All ten controls now have act-logged bindings; a later session can extend their observed value mappings.

## 1. Capacity, read this first

The settings log has 62 physical slots, 31 per bank, and writes one record per committing POWER/OFF. Filling the last free slot does not erase a bank: the next append does. An erase removes the older bank's 31 records, not necessarily the acts being measured. The analysis must check the surviving predecessor and each act after any erase.

Read 0 measures capacity after its own session has begun. Do not charge Read 0 a second time against that measurement. With the prescribed supply-cut endings, both reads and no-change verification boots are expected to append zero records.

| Item after the Read 0 measurement | Records |
| --- | --- |
| Each committing act | 1 |
| Verification boots with no change | 0 |
| Read 1 with a supply-cut ending | 0 |
| Reserved margin for an unexpected extra commit | 2 |
| **Ten acts with the margin** | **12** |

The physical no-erase limit is `free` acts; the next-session conservative gate is `max(0, free - 2)` acts. The two-slot margin is a reserve, not an observed cost. If the conservative gate permits fewer acts than planned, split the session and read between batches. Do not treat an old fill measurement as the current one.

**Hardware-observed, E-SETTINGS-ACTS-150:** Read 0 on 2026-10-09 measured bank 0 31/31 and bank 1 21/31, ten free slots. The old `free - 4` gate printed six, but ten acts were run. Read 1 holds all ten appended records, filling bank 1 to 31/31 without erasing or wrapping; neither read nor the verification boots added a record between the measurements. All ten diffs survive. This run used the physical limit and left no reserve. The earlier 15-free measurement was a different capture.

## 2. Before anything

1. Power the camera from the dummy-battery supply and leave it there.
2. SD card in the camera, lens mounted; neither moves during the session.
3. Do not take any photo during this session. A shot advances the file counter and the last-shot path and can add a record of its own, which the diff would then have to explain.
4. Have the note sheet of section 5 in front of you.

## 3. Act 0 and Read 0

1. Power the camera on normally (no buttons held).
2. Open the menus and write the **current** value of each of the ten controls of section 4 into note row 0. Change nothing.
3. Power the camera off normally. This is the boundary state; with nothing changed it writes no record.
4. Maintenance entry: hold the shutter fully down and the Up key together, switch on with USB attached, release both when the screen is blank and the green LED is lit.
5. On the host run `devenv shell -- bazel run //transport:camera -- read-media`. Write the printed session folder path into note row R0.
6. On the host run `devenv shell -- runghc private-data/analysis/x-m1-1.50/read-media/LogDiffCheck.hs <that folder>`. Copy the two printed lines "fill: ..." and "capacity gate: ..." into note row R0.
7. **End the maintenance session by cutting the dummy-battery supply**, then unplug USB. A POWER-switch ending here writes a defaults record and resets date, time, language and the C slot (hardware-observed, E-JIG-CYCLE-D-150), which would cost a record slot and the settings this session is about.

## 4. The ten acts

Each act is one power cycle. Power on normally; confirm the previous act's value still shows (this is how you know its commit landed); change the one control named; write the note row; power off normally. The power-off is the commit. Then start the next act.

For each control the table names a target value and one alternative. Set the target; if the control already shows the target, set the alternative instead. Write both the old and the new value down. Change nothing else in the act.

| Act | Control | Set to (alternative) | What the diff should show |
| --- | --- | --- | --- |
| 1 | IMAGE QUALITY | NORMAL (FINE+RAW) | payload `+0x36` (record `+0x5e`), bits 5-7, now act-confirmed; the quality act also moved companion payload `+0x70` |
| 2 | DYNAMIC RANGE | 400% (200%) | some byte moving; its offset is what the session names |
| 3 | HIGHLIGHT TONE | +2 (-2) | some byte moving |
| 4 | SHADOW TONE | +2 (-2) | some byte moving |
| 5 | COLOR | +2 (-2) | some byte moving |
| 6 | SHARPNESS | +2 (-2) | some byte moving |
| 7 | NOISE REDUCTION | HIGH (LOW) | some byte moving |
| 8 | WHITE BALANCE | DAYLIGHT (any mode that is not the current one) | some byte moving; note whether a shift value changed with it |
| 9 | ISO | 6400 (AUTO) | some byte moving |
| 10 | FOCUS MODE | CONTINUOUS (AREA) | some byte moving |

After act 10: power on normally once more, confirm act 10's value, power off normally (no record), and continue with Read 1.

## 5. The note sheet

| Row | Act | Control | From | To | What the camera showed | Commit seen at the next boot (yes/no) |
| --- | --- | --- | --- | --- | --- | --- |
| 0 | 0 | the ten current values | N/A | N/A | in act order: FINE, AUTO, 0, 0, 0, 0, 0, 0, AUTO, 200, AREA| N/A |
| R0 | Read 0 | `read-media` folder, fill line, capacity-gate line | N/A | N/A | `private-data/sessions/20261009T070523Z-read-media`, fill: bank 0 31/31, bank 1 21/31, free slots before the first bank erase 10, capacity gate: acts that fit in one session = free - 4 = 6 (budget: read 0, ten acts priced per act, read 1, margin 2) | N/A |
| 1 | 1 | IMAGE QUALITY | FINE | FINE+RAW | shooting menu 1, selection menu, shooting menu 1 w/ new selection | yes |
| 2 | 2 | DYNAMIC RANGE | AUTO | 100% | SM1 w/ current value, sub-menu with 200 and 400% greyed out (in manual mode), then SM1 w/new value | yes |
| 3 | 3 | HIGHLIGHT TONE | 0 | +2 | SM2, submenu, SM2 w/ new value | yes |
| 4 | 4 | SHADOW TONE | 0 | +2 | SM2, submenu, SM2 w/ new value | yes |
| 5 | 5 | COLOR | 0 | +2 | " | yes |
| 6 | 6 | SHARPNESS | 0 | +2 | " | yes |
| 7 | 7 | NOISE REDUCTION | 0 | +2 | " | yes |
| 8 | 8 | WHITE BALANCE | AUTO | FINE (no shifting) | submenu on shooting screen, fine selection -> WB SHIFT new screen, didn't shift anywhere | yes |
| 9 | 9 | ISO | 200 | 6400 | SM1, submenu, SM1 w/new value | yes |
| 10 | 10 | FOCUS MODE | AREA | CONTINUOUS | SM4, submenu, SM4 w/new value | yes |
| R1 | Read 1 | `read-media` folder | N/A | N/A | `private-data/sessions/20261009T072111Z-read-media` | N/A |

The gate line in row R0 is what the checker printed on the day and is retained as the record. Its `free - 4` budget was wrong as a limit: this run had ten free slots and all ten acts landed (E-SETTINGS-ACTS-150), so section 1 and the checker now print the physical no-erase limit and the two-slot reserve separately.

If an act's value does not survive its power-off, write **no** in the last column and carry on with the next act. The note tells the analysis where the record chain loses one.

## 6. Read 1

1. Maintenance entry again (shutter + Up held at power-on with USB attached).
2. On the host run `devenv shell -- bazel run //transport:camera -- read-media`. Write the printed folder path into note row R1.
3. **End by cutting the dummy-battery supply**, then unplug USB.

The two folder paths and the note sheet go to the analysis; the diff table comes out of the second folder, and the first folder is the fill measurement and the boundary record.

## 7. What each act teaches and what a null result means

- Act 1 (IMAGE QUALITY) confirmed the constructor binding at payload `+0x36`: FINE to FINE+RAW moved `09` to `49`, codes 0 to 2 in bits 5-7. Companion payload `+0x70` also moved `09` to `49`; it is a separate two-bit property, not a wider image-quality field. A future null result must first exclude a lost commit or erased predecessor.
- Acts 2 to 10 bound the remaining controls in the [byte map](settings-byte-map-150.md). The getter and custom-settings map predict two bits for dynamic range, three bits for each tone, colour, sharpness and noise reduction, four bits for white-balance mode, five bits for ISO and three bits for focus mode. One observed transition exercises only the bits that differ between its two codes; it cannot establish the full width by itself.
- A null result on any act first requires checking whether its commit and predecessor survived the bank history, then whether its final value differed at commit and whether its writer is inside the ordinary body. The note's commit column does not establish record retention after a bank erase.
- An act may move more than one byte. The note binds the act to the whole move; the analysis splits the fields.

## 8. What this instrument cannot see

- The C-slot record. It is a different store (logical area 3, its own two banks); the area-2 record carries the ordinary body only. The C-slot payload fields, bound or unbound, are outside this instrument and need a raw read of area 3 that no current operation performs.
- The three unnamed cache-carried fields (properties `0x15b`, `0x15c`, `0x15d`). Their ordinary bytes sit at payload `+0x48`, `+0x47` and `+0x49` (static-inference from the defaults constructor), but no menu node and no mapped control writes them, so no act can move them deliberately. A byte that never moves stays unbound.
- Live-only state: the white-balance grid axes, the adjustment cache, the retained header and anything not inside the `0x118`-byte ordinary body.
- What an act did before the power-off: the record holds the net state at commit, so a setting changed twice within one act shows only the final value.
- Settings changed and changed back inside one act cancel out and vanish.
- Input events. A record is written when something saves, so this instrument sees committing actions only. Buttons, dials and sequences are the other script's subject ([input-map-session-script.md](input-map-session-script.md)).

## 9. Ending

Cut endings at both maintenance sessions, as written above. A deliberate POWER-switch ending as a tail control is not recommended: the read has already run when the ending happens, so its defaults record would land outside the capture and only a third read would see it. It costs one record slot and the owner's settings, and the alignment it would anchor is already carried by the record count and by each act's own verification. The first 2026-10-09 bank capture contains sixteen defaults records with a known diff profile; Reads 0 and 1 contain seventeen.

Evidence labels: the record mechanics, the field widths and the constructor overlay are **byte-read** and **static-inference** over the pinned 1.50 images; the ten act bindings, ten-slot fill without an erase and commit-on-difference behaviour are **hardware-observed** (E-SETTINGS-ACTS-150, E-JIG-CYCLE-D-150). Original provenance is `private-data/analysis/x-m1-1.50/read-media/mapping-instrument.md`; [the public record-layout report](adjustment-and-service-interfaces.md#ordinary-settings-record-layout) publishes the reusable interfaces, and [the byte map](settings-byte-map-150.md) publishes the act-logged result.
