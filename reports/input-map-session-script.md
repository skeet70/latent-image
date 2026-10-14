# Press-mapping session at the bench: what to press and what to write down (1.50)

Status: run by the owner on 2026-10-09; the filled note sheet is below and the [physical input findings](input-map-150.md) record integrity, alignment and E-PRESS-MAP-150. The script now includes his sub-command-dial correction and added main-command rotation act. It commits nothing and changes no setting; the ending still matters (step 2.22) because a POWER-switch ending of a maintenance session writes a defaults record and resets date, time, language and the C slot (E-JIG-CYCLE-D-150, hardware-observed). The only host command is the reviewed `read-log` operation; no new camera bytes are involved. Nothing here authorizes another run: the owner's go under [the camera-run procedure](../spec/07-governance-and-handoff.md#3-camera-runs) covers one.

The session binds each physical control to the code the camera publishes in the console-log ring. Ordinary buttons emit a press/release pair, rotations emit pulses, the flash emits a pop-up state, and mode changes have a scanned record plus a forwarded copy. Shutter presses and releases use different sources; startup also publishes held states. Decode type, thread and emitter signature together as [the findings](input-map-150.md#2-main-correction-the-ring-has-several-input-sources) describe. `read-log` reads the ring; the firmware's key table names the codes.

## 1. Before anything

1. Power the camera from the dummy-battery supply. Do not move the supply switch during the session.
2. SD card in the camera. Lens mounted. Both stay in place for the whole session.
3. USB cable attached to the host; the host is on at the repository.
4. Turn the mode dial to AUTO. Write the start position in note row 19 if it is not AUTO.
5. Camera off.

## 2. The session

Every act below is one physical act plus one line in the note sheet of section 3. Do the acts in order, one at a time, and write the line before touching the next control. A press is one clean press and release. Between acts, wait five seconds; the pause is what separates the acts in the ring.

| Step | Do exactly this | What the ring should hold afterwards |
| --- | --- | --- |
| 2.0 | Note sheet open. Write the date, the body, and "dial at AUTO" (or your start position). | nothing yet |
| 2.1 | Maintenance entry: hold the shutter fully down and the Up key together, switch the power on, keep both held until the screen is blank and the green LED is lit, then release both and wait ten seconds. | boot-time condition entries naming POWER, ADAPTER, LENS_DET, USB and the dial position, then release entries for UP, S1 and S2 |
| 2.2 | Press **OK** (center of the D-pad) once. | one press and one release entry, code `0x0001` (the key table names it EXE_M) |
| 2.3 | Press **UP** once. | code `0x0002` |
| 2.4 | Press **DOWN** once. | code `0x0003` |
| 2.5 | Press **LEFT** once. | code `0x0004` |
| 2.6 | Press **RIGHT** once. | code `0x0005` |
| 2.7 | Press **DISP/BACK** once. | code `0x0009` (DISP_B) |
| 2.8 | Press **PLAY** once. | code `0x000b` (K_PLAY) |
| 2.9 | Press **Q** once. | code `0x0032` (K_Q) |
| 2.10 | Press **Fn** once. | code `0x0028` (FUNCTION) |
| 2.11 | Press the **sub-command dial straight down** once. | code `0x002b` (KEY_CMDL_PUSH), one press/release pair |
| 2.12 | Turn the **sub-command dial one click in the direction you use to raise a value**. | one `0x002d` pulse (KEY_CMDL_RIGHT); call this direction A in the note |
| 2.13 | Turn the **sub-command dial one click in the other direction**. | one `0x002c` pulse (KEY_CMDL_LEFT); call this direction B in the note |
| 2.14 | **Shutter half-press**: press the shutter gently to the first click and release. | code `0x0007` (S1): press from thread `0x00`, release from keytimer |
| 2.15 | **Shutter full-press**: press the shutter past the click and release. | S1 `0x0007` and S2 `0x0008` press/release pairs, four records across two sources |
| 2.16 | Press **OK** and keep it held for five full seconds, then release. | one press entry and one release entry; the owner-run act emitted a 583-tick gap and no repeat |
| 2.17 | Push the **flash pop-up lever** and return it. | code `0x001c` (POP); the owner-run act produced one event and no release |
| 2.18 | Press the **MOVIE button** once. | code `0x0034` (KEY_MOVIE_REC), one press/release pair |
| 2.19 | **Mode dial walk**: turn the dial one click at a time from AUTO through every position around the dial and back to AUTO. Pause three seconds at each position and write the position down as you land. | one keytimer entry per changed position plus a usbcont copy; `0x0302` for AUTO and one `0x03xx` code per other position |
| 2.20 | Turn the **main command dial one tick in direction A, then one tick in direction B**. Record both directions. | one `0x001e` pulse for the recorded A and one `0x001d` pulse for B; no releases |
| 2.21 | **The read.** On the host run `devenv shell -- bazel run //transport:camera -- read-log`. Wait for its result line, then write the printed output folder path into note row 21. | the ring as it stood when the read passed over it |
| 2.22 | **The ending.** Cut the dummy-battery supply. Do not touch the POWER switch: a POWER-switch ending here writes a defaults record and resets date, time, language and the C slot. Unplug the USB cable after power is gone. | nothing in the preceding capture; the ring is RAM |

Do not touch during the session: the POWER switch, the lens release, the card door, the USB plug at the camera, and anything on the hot shoe.

## 3. The note sheet

One row per act. "What the camera showed" is the screen and LED as you saw them; in maintenance mode the expected answer is blank screen and steady green LED for every press, because the keys reach no action there (E-SERVICE-SHOOT-01, static-inference). Any change of screen or LED during a press is worth recording as a finding.

10/9/26 6:36 AM UTC, X-M1 1.50, dial at AUTO

| Row | Step | Control | Time | What the camera showed |
| --- | --- | --- | --- | --- |
| 1 | 2.1 | entry combo: shutter + Up held at power-on | 12:37 | black screen, green LED on |
| 2 | 2.2 | OK | 12:38 | black screen, green LED on |
| 3 | 2.3 | UP | 12:38 | black screen, green LED on |
| 4 | 2.4 | DOWN | 12:39 | black screen, green LED on |
| 5 | 2.5 | LEFT | 12:39 | black screen, green LED on |
| 6 | 2.6 | RIGHT | 12:39 | black screen, green LED on |
| 7 | 2.7 | DISP/BACK | 12:39 | black screen, green LED on |
| 8 | 2.8 | PLAY | 12:40 | black screen, green LED on |
| 9 | 2.9 | Q | 12:40 | " |
| 10 | 2.10 | Fn | 12:40 | " |
| 11 | 2.11 | sub-command dial push | 12:41 | " |
| 12 | 2.12 | sub-command dial, direction A | 12:41 | " |
| 13 | 2.13 | sub-command dial, direction B | 12:41 | " |
| 14 | 2.14 | shutter half-press | 12:41 | " |
| 15 | 2.15 | shutter full-press | 12:42 | " |
| 16 | 2.16 | OK held five seconds | 12:42 | " |
| 17 | 2.17 | flash pop-up lever | 12:43 | ", but mechanical flash popped up |
| 18 | 2.18 | MOVIE button (or "no such control") | 12:43 | black screen, green LED on |
| 19 | 2.19 | mode dial, each position in order | 12:44 | " the whole time. Order was auto -> adv. -> SP -> action -> landscape -> portrait -> C -> M -> A -> S -> P -> SR+ -> back to auto |
| 20 | 2.20 | command dial one tick A then one tick B | 12:48 | black screen, green LED on |
| 21 | 2.21 | host `read-log`, output folder written here | 12:50 | `private-data/sessions/20261009T064945Z-read-log` |
| 22 | 2.22 | ending: supply cut (confirm: not the POWER switch) | 12:52 | power off |

## 4. Capacity and what the ring holds

The ring is the store at `0x400a0800`: 8,188 entries of sixteen bytes, cleared at every boot (E-LOG-STORE-150, static-inference; E-LOG-PERSIST-150, hardware-observed). Acts and the read must therefore be in one boot, and they are: everything above is one maintenance session.

- Ordinary button taps cost two records; a full shutter act costs four, rotations one per direction, POP one, and each mode change two across keytimer and usbcont. The named control acts in the owner-run capture contribute 59 input publications.
- The boot and the `read-log` run also occupy the ring. This capture holds 1,219 contiguous occupied slots with no wrap; the heaviest earlier observed boot, twenty-one dump sessions, held 7,602.
- Order matters against the cap: controls first (2.1 to 2.20), the read last (2.21). At capacity the writer wraps to entry 0 and overwrites the oldest records.
- The read reads the store as it passes over it, so anything pressed after 2.21 is outside this capture. Do not press anything between the read and the cut.

## 5. What the ring shows and what it cannot

- The ring records input publications, their order and paired duration by tick, with startup-held states and forwarded copies distinguished from physical edges.
- A control producing no event in one source is not absent from the ring. A control producing no event in the whole capture is not thereby proved absent on the body.
- D-pad, OK, DISP, PLAY, Q, Fn, MOVIE and sub-command push pair in keytimer. Shutter press forms are in thread `0x00`, releases in keytimer; entry states and POWER press forms also appear in syscont/initcont. Held OK does not repeat in the measured act. The earlier keytimer-only release-only classification is withdrawn (E-PRESS-MAP-150).
- The ring carries no settings values and no effect: in maintenance mode a press does nothing, so this session binds names to codes only. What a control does in ordinary operation is a different instrument (the settings session script).
- Entries written before the boot clear are gone, but the resulting held entry-combo state is published in syscont before keytimer records its releases. The ring therefore contains press-form states as well as release edges.

## 6. Ending, once more

The session ends with the supply cut of step 2.22. A POWER-switch ending would write a defaults record, reset the owner's settings and still be invisible to the preceding read. The note sheet records a supply cut, not a firmware clean-exit operation.

Evidence labels: the ring layout and firmware code names are **byte-read** and **static-inference**; the 2026-10-09 acts and captured bytes are **hardware-observed** (E-PRESS-MAP-150, [findings](input-map-150.md)). The ending behaviour is **hardware-observed** (E-JIG-CYCLE-D-150); the inertness of keys in maintenance mode is **static-inference** (E-SERVICE-SHOOT-150). Private provenance `private-data/analysis/x-m1-1.50/read-media/mapping-instrument.md` retains the earlier thread-scoped mining and corrected limits; [the console trace instrument](console-trace-instrument.md) publishes the record format and its decoding limits.
