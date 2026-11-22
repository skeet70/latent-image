# Capture save path (X-M1 1.50)

What runs after a still JPEG is encoded until its file is closed on the card, and the boundary the owned DCF numbering takes. Decomp pin: x70view `55436fc85c0560dd252b0410af6f2427fe198cd3`. Addresses are X-M1 1.50 (DAT `43d67afa…cd1490`, image 4 `f2e2acab…dcff16`). Tags: **bytes** (read from the image-4/5 disassembly), **C** (decomp C used as a reading aid, not confirmed), **open**.

## Sequence

| Step | Code | Task / mailbox | Evidence |
| --- | --- | --- | --- |
| 1. Exif block | `metadata_write_exif_header` `0x011db098` (overlay 46) writes SOI, APP1, TIFF header, IFD0, Exif IFD, FUJIFILM maker note, Interop, optional GPS and IFD1 into `image_work.header_00`; `image_relocate_jpeg_payload` `0x011d2778` moves the scan behind it and `metadata_finish_exif` `0x011db188` patches the APP1 and thumbnail lengths | image encoder, queue `0x014ddb74`, message `0x2b00` | C |
| 2. Scatter list | `image_build_scatter_list` `0x011d2adc` fills the file descriptor at `frame + 0x27d0 + 0x150*bank`: `[Exif block, scan, trailer, {0,0}]` | record controller (image 5) through `0x00e51b9c` | C; call site bytes |
| 3. Save facade | `0x00e518c0` sets the descriptor kind, reads the numbering snapshot through `0x7101e4`, records the current task at `0x0155ccb8` and calls `0x011d5600(1, descriptor, 0x00e4e5dc)` (kind 3: `0x011d5688`), then waits on flag `0x10` | record controller | bytes `0xe51a68..0xe51b08`; owned in `replacement/save` (`save-facade-150.json`) |
| 4. File machine | `0x011d5600` posts `0x2b01` to `0x014ddbfc`; `file_open_output` `0x011d5aac` builds the name through `0x011d59f4 -> 0x0070fbfc`, appends `.JPG` (`0x011d5d8c`) and opens through `0x009968dc`, a copy of image-4 `0x71c648` | message-registry worker 1 (overlay 46) | C |
| 5. Open | `0x71c648` posts `0x2c2d` to mediamsg `0x65`, routed as `0x2c17` to the media task `0x1c`; worker `0x7115a4` creates the folder through `0x716184` when absent and opens the file in mode `w`; `0x71f2a8` registers it (`0x71565c`) in a context slot | media task | owned file service (E-FILE-SERVICE-WORKERS-150) |
| 6. Writes | `file_open_completed` `0x011d5540` writes buffer 0, `file_advance_write` `0x011d5400` each following buffer on every `0x2c38` result (posts `0x2c19`, worker `0x7116e4`) | file machine / media task | C |
| 7. Close | terminator: `0x71c7bc` posts `0x2c1a`; worker `0x711768` closes and, for a mode-1/3 slot with status 0, calls `0x71141c` | media task | bytes `0x7117b4..0x7117c4` |
| 8. Numbering after close | `0x71141c` advances the slot group's last-used pair, persists it and creates the next folder | media task | bytes |
| 9. Completion | `stream_signal_phase` posts `0x2b16`; `file_complete_path` `0x011d536c` copies the path to `0x014ddba8` and calls `0x00e4e5dc`, which sets `0x0154d970` and signals `0x10` to the task at `0x0155ccb8` | file machine, then record controller | C |

The card I/O (steps 5 to 7) is the owned file service. The machine tables `0x011d5d44`/`0x011d5dac` and `registry_queue_ids[1]` (`0x007d088c`) are not byte-read (**open**).

## DCF naming and the number store

Name: `L:\DCIM\NNN_SSSSS\PPPPFFFF` (no extension), written by `0x70fbfc` into the caller's buffer. Folder 100..999; file 0001..9999; a folder's last digit selects its thousand block of file numbers. Prefix: the four context bytes (`DSCF`), configuration bytes `0x142/0x143` or `0x144/0x145` plus a two-digit sequence (modes 1/2 from `0x70f790`), or valid caller text. Folder suffix: the group's `_FUJI` or valid caller text.

Number store, all in the media context at `*(0x910204)` (bytes):

| Offset | Field |
| --- | --- |
| `+0x0` | four-byte prefix |
| `+16*i+4`, `+6` | directory/file halfwords of written-file slot `i` |
| `+0x1fc` | written-file count |
| `+0x2168`, `+0x216c` | highest folder on the card and its highest file |
| `+0x21b8` | highest folder of group 0 |
| `+0x21d0+16*g` | group `g`: count, last directory, last file, five-byte suffix at `+0xa` |
| `+0x2210` | selected-group mask |
| `+0x2218+n` | group of folder `n` |
| `+0x2600` | group count |
| `+0x2604+4*n` | first cluster of folder `n`, zero when absent |
| `+0x127d4` | two-digit sequence |
| `+0x16bd0`, `+0x16bd4` | sequence latch (0 idle, 2 armed, 1 consumed) and prefix mode |
| `+0x16c64` | sequence at the last write |
| `+0x16c6c` | FRAME NO. CONT flag |

The card scan (`0x719de0` with `0x713894`, `0x7166cc`) fills the catalog fields. The persisted pair is not in the record store: `0x715a2c` writes it through `0x6a802c` (directory) and `0x6a8008` (file) into the settings profile header at `0x9c281c` (C: synced by `0x6a6ddc` through `0x6a59b8`), and `0x7159f8` reads it back through `0x6a8040`/`0x6a801c`. FRAME NO. reaches `0x70faa0` from image-5 `0x00ec589c` (C): CONT persists every group's pair, RENEW resets the profile through `0x6a8050`.

Next-name rules (bytes, `0x70fcb4..0x71003c`), from the last-used pair `(d, f)` that `0x716324` derives:

- `d%10 == f/1000`: `f+1`; at a multiple of 1000, or with the latch set and sequence above 99, a new folder.
- otherwise: `f+1`, or `f+2` at a multiple of 1000; past 9999, file 1 in a new folder. When the thousands digit then exceeds `d%10` (or after the wrap), the folder advances to the next one whose last digit matches. When it is below, the file restarts at the folder's block, or a new folder when the sequence is exhausted.
- A new folder is one past `max(d, highest folder)`; at 1000 the entry reports `0x726c74(2, 0x51)` and returns 6.

`0x716324` takes the group's pair, falls back to `(100, 0)` on an empty card or to the highest folder's block end, merges the persisted pair when CONT is on (replacing it with the highest folder when the persisted folder belongs to another group), and for group 0 moves to the highest card folder's block end when that folder is not the group's own.

## EXIF assembly

The writer is overlay-46 code `0x011c0004..0x011cf908` entered at `0x011db098`; 53 functions, 17,504 bytes, of which 16 functions (12,968 bytes) are NONMATCHING drafts, including all four serializers and both tag filters. Its inputs are five `image_work` blocks (`metadata_42c`, `metadata_5d8`, `metadata_91c` maker note, `raw_header_f224` GPS, `raw_tail_f2c4` IFD1) and the one-shot identity at `0x014e9d98`. The IFD slot callbacks have no X-M1 body in the decomp, and `0x011c36f8..0x011c6e30` is one undivided range. `recbfCreateExifHead`/`recbfCreateExifTag` (image 5 `0xea8492`/`0xea84c2`) are trace-section names, not code.

## Owned boundary

The largest coherent boundary that needs no camera evidence is the image-4 DCF numbering: ten entries that choose and persist folder/file numbers and create folders, with every card, profile and formatting effect submitted to the same retained services.

| Entry | Extent owned | Role |
| --- | --- | --- |
| `0x70f790` | `[0x70f790,0x70f8b8)` with `0x70f840` | prefix mode; name-text check |
| `0x70faa0` | `[0x70faa0,0x7101e4)` with `0x70fb68`, `0x70fbfc` | FRAME NO. apply; last-used of the selected group; next name |
| `0x71141c` | `[0x71141c,0x7115a4)` | numbering after a written file closes |
| `0x7159f8` | `[0x7159f8,0x715a58)` with `0x715a2c` | persisted pair read/write |
| `0x716184` | `[0x716184,0x71657c)` with `0x716324` | folder creation; last-used pair |

Retained services: media status `0x71ddf0`, current drive `0x71f708`, group index `0x715f0c`, event report `0x726c74`, drive prefix `0x712d08`, copy `0x70e6c4`, decimal `0x70eaa0`, configuration byte `0x69f8fc`, profile `0x6a8040`/`0x6a801c`/`0x6a802c`/`0x6a8008`/`0x6a8050`, snapshot `0x712b4c`, fill `0x43550`, folder digits `0x722218`, copy `0x722054`, directory open `0x701e18`, entry cluster `0x713460`, path groups `0x7160b8`, status translation `0x71d450`.

References into the five extents (bytes, the staging census of E-DCF-NUMBERING-CARD-PAIR-150 over image 4, image 5, the plain image, both loaders, image 2 and image 3): 36 A32 `bl`, all to entries. Image 4 calls `0x70fb68` (6), `0x71141c` (2) and `0x716184` (1); image 5 calls `0x70fbfc` (4), `0x70f790` (2), `0x70fb68` (2) and `0x70faa0` (1); the overlays call `0x70fbfc` (10), `0x70f790` (5) and `0x70fb68` (3). No pointer word, literal, MOVW/MOVT pair, ADR or Thumb branch names an entry or an interior address; four aligned data words whose values fall inside the extents (image 4 `0x7bdfd0`, `0x7dfd50`, `0x7ea76c`, image 5 `0xf580bc`) sit in tables of like values and are loaded by no PC-relative instruction.

Not owned here: the overlay-46 file machine (steps 4, 6, 9) and the EXIF writer need an overlay carrier that main does not have; the EXIF writer also needs its serializers and callbacks recovered from bytes. The snapshot query `0x7101e4` and refresh `0x712b4c`, and the card scan, stay stock.

## Emulated route

No emulated capture reaches save: the capture route stops at the second geometry filter's store at `0xfff30010` ([#139](https://github.com/skeet70/latent-image/issues/139)) before any save caller runs (E-SAVE-LIFECYCLE-NATURAL-PREFIX-150). The numbering is qualified natively, per entry, against the stock bytes ([replacement/README.md](../replacement/README.md#dcf-numbering)).

The card routes of the file service (playback and PROTECT with the three-JPEG card) reach one entry from outside: `0x70fb68`, called from image 5 `0xed5908` (404 times on each route) and from image 4 `0x710634`, `0x710728`, `0x710790`, `0x712cdc` and `0x71fd14`. In stock it runs `0x716324` and `0x7159f8`. The staged owned image is equal to stock on both routes and the `profile_ignored` control, which skips the persisted-pair read in `0x716324`, is different (E-DCF-NUMBERING-CARD-PAIR-150). Neither route saves a file or creates a folder, so the other seven entries stay native-only.
