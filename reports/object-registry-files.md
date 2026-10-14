# The object registry's five names: readers, triggers and gates (X-M1 1.50, host-only)

The record check at `0x00d91988` pairs six object formats with five canonical names: `0x3002` with `\AUTO_ACT.SCR`, `0x3801` with `\DSCF0000.JPG`, `0x3800` and `0xb103` with `\DSCF0000.RAF`, `0xb802` with `\FPUPDATE.DAT` and `0xc802` with `\XFUPDATE.DAT`. [The auto-act report](auto-act-script.md) covers `\AUTO_ACT.SCR`. This report answers the same question for the other four: what reads them, when, behind which gate, under which validation, and whether any is a route to what the project wants.

**Headline result: none of the four is read.** No firmware path opens a file named `\FPUPDATE.DAT`, `\XFUPDATE.DAT`, `\DSCF0000.JPG` or `\DSCF0000.RAF`; the names exist as format labels for records that the media/object service keeps about the card. `\FPUPDATE.DAT` is the one name the camera's card inventory looks for, and finding it only registers a record. The update path reads a different name (`A:\FWUP0002.DAT` on this body), and the sibling statement that `FPUPDATE.DAT` names this body's update file is refuted (section 5). The standing no-recovery-entry position survives (section 6).

Inputs (byte-read, hashes rehashed 2026-10-09): id4 `id4-0061c400.bin` SHA-256 `f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16`, id5 `id5-00bf1a00.bin` SHA-256 `3cd501c07533938ae4d9347616986fd1649005a588c36b5abf05e36ef4fd892a`, plain image `plain-01100000.bin` SHA-256 `ee211bdaad39005b4f72e3443a76ca194905381a967cab787d6a4ef14e2d889d`, boot records `decode/boot-40048.bin` SHA-256 `c2da900dcd3f193b05b5ae968dfcb87723571462eafd84c0fed8467f66f7dff3` and `update/boot-50048-reextract.bin` SHA-256 `2f1bf3f0e483b892d2890599679c1b515cfc1be8b8c27cbb06d4d913e428abb4`. Labels: **byte-read** for bytes taken from those inputs, **static-inference** for conclusions drawn from them, **reported** for the sibling sources S17/S22/S23, **unproved** where the named fact is not established.

## 1. What the check is (byte-read, static-inference)

The check runs from `0xd91988` to `0xd91e48`. It takes a first argument in `r0` and a 60-byte record pointer in `r1`: it rejects `r0` above `[0x146a650]` and a null record, uppercases the record's 28-byte name field unless `r0 == 2`, requires `[record+0x20] == 0`, then dispatches on the format at `+0x1c` (instructions `0xd91a04`-`0xd91af0`):

| format at `+0x1c` | compared name (strncmp `0x72232c`, 14 bytes) | continuation on a match |
| --- | --- | --- |
| `0x3801` | `\DSCF0000.JPG` (`0xd920a8`) | `0xd91ce0` |
| `0x3002` | `\AUTO_ACT.SCR` (`0xd92020`) | `0xd91b48` |
| `0x3800`, `0xb103` | `\DSCF0000.RAF` (`0xd920b8`) | `0xd91ce0` |
| `0xb802` | `\FPUPDATE.DAT` (`0xd91fe8`) | `0xd91b48` |
| `0xc802` | `\XFUPDATE.DAT` (`0xd92098`) | `0xd91b48` |

A mismatch returns `0x80000000` for `0x3002`, `0xb802` and `0xc802`: those three formats are enforced to their canonical names. For the image formats a mismatch is not an error by itself: the mismatch path at `0xd91d00` looks the name up (`0xd8f038`) and returns `0x80000000` only when the name is already registered, otherwise it falls into the same generic find-or-register continuation as a match on the script formats. The `0xd91b48` continuation looks the record's name up (`0xd8f038`), allocates an index when absent (`0xd8d4d8`), copies the record to `0x46b5612c + index*60`, stamps the date `20060101120000` (`0xd91fc8`) when `+0x28` is unset, and inserts the name into the index hash (`0xd8dc70`). The `0xd91ce0` continuation checks `[0x146a648] <= 0x270e`, copies the record to the global `0x146a6d0` and posts `0xd830f0(0xd90734)`.

The check is entry 19 (`0xd92114`) of the 21-entry function-pointer table at `0xd920c8`, which the module's init `0xd90968` installs as the active ops table by calling `0xd8cd70(0xd920c8)` at `0xd90a90` (the pointer lands at `0x146a2ec`). Every call of it is indirect through that table at `+0x4c`; only two sites do so, `0xd8a73c` and `0xd8af18`, and both pass a 60-byte record assembled on the stack from module state: the format from `[ops+0x138]`, a word from `[ops+0x15c]`, the size from `[ops+0x13c]` and the name from `[ops+0x1ac]`.

Nothing in the check or its two callers opens a file. It validates a descriptor against the format table and files it.

## 2. The card inventory, and the one name it looks for (byte-read, static-inference)

The init tail after the table registration (`0xd90a94`-`0xd90b48`) starts a chain of file-service requests through `0xd83010(callback, context 0x146a5d4, directory, name)`. The context is filled by the service with `<volume letter>:<path>` and the record name is copied from the context tail: `0x146a5d6` for ordinary formats, `0x146a5db` for `0x3002` (five characters into the path, i.e. past `\DCAA`):

1. directory `<volume>:\`, name `FPUPDATE.DAT` (the literal word at `0xd90b54` points at `0xd91fe9`, the bare name), callback `0xd90c6c`.
2. `0xd90c6c`, on a successful find, creates a record of format `0xb802` through `0xd8ef8c(0xb802, 0)` and requests directory `<volume>:\DCAA` (`0xd92030`) with name `AUTO_ACT.SCR` (`0xd90d14` -> `0xd92021`), callback `0xd90d18`.
3. `0xd90d18`, on success, creates a `0x3002` record and requests `<volume>:\FGPS` (`0xd92040`) with pattern `*.log` (`0xd92038`), callback `0xd90b58`.
4. `0xd90b58` requests `<volume>:\MISC` (`0xd92008`) with name `AUTPRINT.MRK` (`0xd90bc8` -> `0xd91ff9`), callback `0xd90bcc`, and the chain continues with `*.JPG` (`0xd92010`) and `*.MOV` (`0xd92018`) over the `<volume>:\DCIM` buffer built at `0xd9114c`, then `*.RAF`/`*.MPO`/`*.AVI` (`0xd92070`/`0xd92078`/`0xd92080`), `*.WAV` (`0xd92048`), and the mark and discovery names `\HDISCVRY.DPS` (`0xd92088`) and `\AUTXFER.MRK` (`0xd92061`).

The whole chain runs when the module's init finds a volume: the init reads the active volume letter through `0xd8312c` (id4 `0x71f708`) and, when it is non-zero, sets `[0x146a650] = 1` and starts the chain. No mode, no strap and no adjustment byte gates it, in contrast to the auto-act script, whose loader sits behind message `0x3302` and enable byte `0xa2`.

**Exactly one of the five canonical names appears in this chain: `FPUPDATE.DAT`.** `\XFUPDATE.DAT`, `\DSCF0000.JPG` and `\DSCF0000.RAF` are referenced only by the check; a reference scan of id5 and id4 for all five strings and their bare forms finds the check for all five and this inventory step for `FPUPDATE.DAT` alone.

## 3. What a registered record does (byte-read)

Records live at `0x46b5612c + index*60` with the count at `0x146a65c` and a cap of `0x2af0`; names map to indices through the hash at `0x23462e0`. Resolution to a path is `0xd8e70c(mode, index, op)`: it builds `<volume letter>:<name>` and, for format `0x3002` only, inserts `\DCAA` before the name (`0xd8e868` copies from `0xd91f90`), then dispatches `0xd82dec(0xd8e6c8, path, op)`. That dispatch stores a callback and calls id4 `0x71c648(0xd82cd4, path, op)`, the same open the updater uses (`0xdba2c8` family) and the auto-act loader uses (`0xed1de0`).

So a record is a file the service can open by path with an operation code taken from the requester's state (`op` 0, 1 or 2 selected from the state field at `0xd8e8cc`-`0xd8e928`). The check itself never opens anything, and neither does the inventory; both only build the record table. Whether one of those three operation codes is a write is unproved by this pass.

## 4. Per name

| name | format(s) | reader | trigger and gates | validation | route value |
| --- | --- | --- | --- | --- | --- |
| `\FPUPDATE.DAT` | `0xb802` | none. The only two references are the check and the inventory's name argument; no open of this name exists in id4, id5 or the plain image | card inventory step 1, at module init when a volume letter is present; no enable byte, mode or strap | the name-format check applies to descriptors entering through the ops table; the inventory's own create path (`0xd8ef8c`) writes the name the service reported | not a film-sim install route and not a recovery entry; a card file of this name alone triggers nothing beyond a record |
| `\XFUPDATE.DAT` | `0xc802` | none; referenced only by the check | none found: it is not in the inventory chain | same check | none |
| `\DSCF0000.JPG` | `0x3801` | the still-image class. After a match the record goes to `0x146a6d0` and `0xd830f0(0xd90734)`; `0xd90734` appends `.JPG` (`0xd91fc0`) or `.RAF` (`0xd91fb8`) to the entry name, looks it up, and notifies; the image formats then feed the picture path (id5 `0xd93768`/`0xd93a40` branch on `0x3801`/`0x3808`/`0x3800`/`0xb103`/`0x300d`) | card scans for `*.JPG`, the create path, and the camera's own capture | the canonical name `\DSCF0000.JPG` selects the image-notify continuation; any other name still registers through the generic path, so the canonical form classifies rather than enforces; whether `0000` is the first file's number or a template base is unproved | the camera's ordinary still path; the camera writes these files itself, and a counterpart-driven write through the same service is unproved |
| `\DSCF0000.RAF` | `0x3800` and `0xb103` | same as JPG, with `.RAF`; the table walk at `0xd91ec8` rewrites `0xb103` records to `0x3800` | the `*.RAF` scan and the capture path | same | the RAW still path |
| `\AUTO_ACT.SCR` | `0x3002` | the loader `0xed1de0`, behind DCAA message `0x3302` | enable byte `0xa2 == 2`; dead as shipped | 1 MiB cap, no lower bound; script grammar | studied in [the auto-act report](auto-act-script.md); no memory write |

The five names are not the body's whole file vocabulary. The same region names more files: `A:\MISC\AUTPRINT.MRK`, `A:\FGPS\*.log`, `A:\FFDB\FFDB.DAT` (`0xdfe420` with `A:\FFDB` at `0xdfe414`), `A:\DCAA\auto_act.scr` (`0xed3ac4`), `C:\IMFIDX10\LX30A.S3` (`0xdba31c`, [the S-record route](srecord-route.md)), the host exchange files `Z:\HREQUEST.DPS`/`Z:\HRSPONSE.DPS`/`Z:\DREQUEST.DPS`/`Z:\DRSPONSE.DPS` (`0xdf8244` onward), and the three updater templates below.

## 5. `FPUPDATE.DAT` and `XFUPDATE.DAT` against the update path (byte-read, static-inference)

The body's update path derives its file name, it does not use either registry name. The name builder `0xdb971c(output, selector)` holds three 17-byte templates and copies out the selector-th one (`selector*17` at `0xd999c`):

- selector 0: `A:\FWUP0001.DAT` (`0xdba2c8`), with the four characters at positions 7-10 replaced by adjustment-cache bytes read through `0x69f8fc` at keys `0x1c0`-`0x1c3`; those bytes read ASCII `0002` on this body, so the name is `A:\FWUP0002.DAT` (E-CONFIG-OFFSET-NAMES, inherited).
- selector 1: `A:\XFUPDATE.DAT` (`0xdba2dc`), the four characters at 7-10 (the literal placeholder `DATE`) replaced by adjustment bytes at keys `0x3e02a`-`0x3e02d` when all four are non-zero, else by `01`/`02`/`03` chosen by matching the 6-byte codes at `0xdba304`/`0xdba30c`/`0xdba314` (`LX201B`, `LX202A`, `LX204A`), else left as `DATE`. Assembled names are `A:\XFUP<code>.DAT`.
- selector 2: `A:\XFTCDATE.DAT` (`0xdba2f0`), the four characters replaced by state bytes at `0x95ca68+0x44`-`+0x47`. Assembled names are `A:\XFTC<code>.DAT`.

The three read branches pass selector 0 (body, `0xec88c8` -> `0xdb99c8` at `0xec8950`), 1 (the report's lens branch, `0xec8ad4` -> `0xec8b5c`) and 2 (other, `0xec8cf0` -> `0xec8d78`). The parser `0xdb9cd4` builds the same name itself from the selector and opens it through id4 `0x71c648`. The XFUP family is therefore a **non-body selector's** file name built by the same updater code, with the per-selector policy differences [UPDATE-01](update-validation-and-boot.md) already records (header type and version comparison); it is not the body route and no evidence puts selector 1 or 2 in reach for a body install.

One format tie exists in the other direction. The update module's status reporter `0xecfa9c` has a case `0x1733` at `0xecfd04` that compares `[0x15c20c4+0x238]` with `0xb802` and posts event `0x877`; that word is the third word of the object at `[0x15c20c4+8]`, copied by `0xecfda0` into `+0x230`/`+0x234`/`+0x238`, and the in-memory update mode takes source and length from `+0x230`/`+0x234`. So `0xb802` is the format this build expects of an update source, and the registry's label for it is `\FPUPDATE.DAT`. That tie is static-inference over byte-read, and it creates no reader: the update path's own file opens use the templates above, its in-memory producer stays unproved exactly as UPDATE-01 records, and the media requests the update module itself issues (`0xecddd4`/`0xecde4c`/`0xecdeb0` -> `0xd8317c`/`0xd831e8`/`0xd83248`, DCAA opcode `0x3406` with callbacks `0xed2d8`/`0xed324`/`0xed370`) are a file-service interface, not a container reader.

The sibling claim that `FPUPDATE.DAT` is the firmware file (S17, X100F 2.12) and that the update utility ships `AUTO_ACT.SCR` next to it (S23) is **refuted as a statement about this body's update name**: the X-M1 1.50 body selector derives and opens `A:\FWUP0002.DAT`, and the only `FPUPDATE` strings in the decoded images are the registry name and the inventory argument. The shared part is the format code, not the file name.

## 6. Does the no-recovery-entry position survive

Yes. Nothing reads `FPUPDATE.DAT` or `XFUPDATE.DAT`; the loader the project wants does not exist for either name, and the update path's own name is `FWUP0002.DAT` for the body. The card inventory's only act on `FPUPDATE.DAT` is a record in a RAM table, with no gate to arm and no content read, so a card file alone triggers nothing and no adjustment byte or mode would change that. For recovery the position is unchanged: no early loader, no weaker validation and no bypass of the identity and version gates follows from these names, and the one door that stays open, a producer for the in-memory update source of format `0xb802`, is the door UPDATE-01 already leaves unproved. No new state-changing byte is proposed here, so no independent review is needed.

## 7. Refuted, and what stays unproved

Refuted: `FPUPDATE.DAT` as this body's update-file name (S17/S23 lead); `\XFUPDATE.DAT` as a card-triggered route; the reading that the registry's five names are the body's whole file vocabulary.

Unproved: whether any producer exists for the in-memory update source and whether it can carry a `0xb802` object; the meaning of the four-character codes that fill the XFUP/XFTC templates (the adjustment keys `0x3e02a`-`0x3e02d` are not mapped on this body); whether the `0000` base of the still names is the first file's number or a template base; the semantics of the check's first argument and of the table walk at `0xd91ec8`; whether one of the three file-service operation codes is a write; the client/server split of the object service, whose ops table the module installs on itself at `0x146a2ec`.

## 8. Reproduction

All commands read host files only. The reference scans use `private-data/analysis/x-m1-1.50/tools/refs-route` (built from `jig-reset/refs.c`, which prints literal-word, branch and movw/movt references for a list of addresses) and `private-data/analysis/x-m1-1.50/update/natw-tool`.

```sh
# byte-read: the name table and the inventory arguments
xxd -s 0x1a05c0 -l 224 private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin
xxd -s 0x1c88c8 -l 64  private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin

# reference scan: the five names and their bare forms over id5 and id4
cd private-data/analysis/x-m1-1.50
./tools/refs-route lz/id5-00bf1a00.bin 0xbf1a00 /tmp/refs-names.txt 0xd91f90 0xd91f98 \
  0xd91fa0 0xd91fa8 0xd91fb0 0xd91fb8 0xd91fc0 0xd91fd8 0xd91fe8 0xd91ff8 0xd92008 \
  0xd92010 0xd92018 0xd92020 0xd92030 0xd92038 0xd92040 0xd92048 0xd92050 0xd92058 \
  0xd92060 0xd92070 0xd92078 0xd92080 0xd92088 0xd92098 0xd920a8 0xd920b8 0xd91fe9 \
  0xd92021 0xd91ff9 0xd91fd9 0xd92059 0xd92089 0xd92099
./tools/refs-route lz/id4-0061c400.bin 0x61c400 /tmp/refs-names4.txt 0xd91988 0xd920c8 \
  0xd91fe8 0xd92098 0xd920a8 0xd920b8 0xd92020 0x46b5612c 0xdba2c8 0xdba2dc

# listings: check 0xd91988..0xd91e48, inventory 0xd90a94..0xd91880, ops install 0xd90968
./update/natw-tool slice lz/id5.dis 0xd91988 0xd91e48
./update/natw-tool slice lz/id5.dis 0xd90a94 0xd90d20
./update/natw-tool slice lz/id5.dis 0xd8e70c 0xd8e960
./update/natw-tool slice lz/id5.dis 0xecfa9c 0xecfd30
./update/natw-tool slice lz/id5.dis 0xdb971c 0xdb99c8

# string inventories: every file-name-like string in id5, id4 and the plain image
strings -t x -n 6 private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin | awk '$0 ~ /DAT|SCR|MRK|DPS|\.S3/'
strings -t x -n 5 private-data/analysis/x-m1-1.50/lz/id5-00bf1a00.bin | awk 'strtonum("0x"$1)>=0x1fc900 && strtonum("0x"$1)<=0x1fcb00'
```

Notes on method: the DAT payload is complemented, so string scans of `FWUP0002.DAT` itself find nothing; the three decoded images and the two boot records are the search scope. `natw-tool` was used for listing extraction, and the collection and grouping of the reference scans ran in the session kernel, not as checked-in tooling.
