# Special label ids 0x83c-0x83e: which accessor resolves a list label, and what the harness shows (1.50)

Host-only work on the pinned official 1.50 images and the bounded native runner. No camera, no USB device and no storage is touched. This note settles the first of the two checks [the reach report](named-recipe-reach.md) leaves open for design D3: whether a list row carrying string id 0x83d or 0x83e draws its inline text.
Evidence labels: **byte-read** for bytes read from a pinned artifact, **static-inference** for a conclusion drawn from those bytes, **emulated** for native-runner results, **inherited** for a named record reused without re-derivation, **unproved** for a named missing fact.
Markdown paragraphs are one line each.

## 1. The accessor a list label goes through

A labelled list row's string ids are loaded from the node's labels array (node +0x24) by two id5 accessors: `FUN_00bf9360(node, row, field)` for a row index and `FUN_00bf93e0(node, key, value, field)` for a matching value, both returning one of the four ids of the 16-byte label quadruple (byte-read: `0x00bf9360` at 0x7960 selects `labels + 16*row + 4*field`; `0x00bf93e0` walks node +0x20 for the value then reads node +0x24).
The plain image calls the row getter four times and the value getter 55 times; one labelled-list row drawer, `FUN_01163f2c`, loads the name field (index 3) with the value getter and hands it to `FUN_01112e78` for drawing and `FUN_0111258c` for measuring (static-inference; its decompiled body loads fields 0, 2 and 3 and passes field 3 to both).
Both halves resolve text through one accessor (static-inference over byte-read call sites):

| Stage | Draw | Measure |
| --- | --- | --- |
| Row drawer | `FUN_01112e78` | `FUN_0111258c` |
| Wrapper | `FUN_01112380` or `FUN_0111262c` (via `FUN_01112e78`) | `FUN_01178538` (via `FUN_0111258c`) |
| Text resolution | `FUN_01179da0` at 0x0117a124 | `FUN_01178f6c` at 0x01179028 and `FUN_0117933c` |
| Text pointer | `FUN_01179b0c` | `FUN_01179b0c` |

`FUN_01179b0c` is therefore **the accessor a list label goes through**. Its dispatch: ids at or below 0x83d go to the record locator `FUN_0117dfd8`, glyph ids in `0xff000000 | n`, `n <= 0x8c5`, to `FUN_0117e15c`, id `0xfe000000` to a font table read, and everything else returns zero (byte-read of 0x01179b0c-0x01179b84).
The locator returns, for id in `{0x83c, 0x83d, 0x83e}`, the inline record base `0x37b784 + 66*(id - 0x83c)` and ignores its record index; for ids at or below 0x83b it reads the language table entry through the entry-pointer accessor `FUN_0117deb0` and returns the text pointer of record `index` (byte-read of 0x0117dfd8-0x0117e048).
The entry-pointer accessor is the accessor the reach report's tension names: it returns `*(0x01178aac) + 8*id` for id at or below 0x83d and zero above, so for a special id it lands past the 0x83c-entry table rather than on the inline record (byte-read).

Two consequences follow with the metric family's own special cases, and the harness checks both:

- The metric reader `FUN_0117dee0` branches on `id > 0x83b` before touching the entry-pointer accessor: it writes the constant 100 to both metric outputs for ids 0x83c and 0x83d and zero for larger ids (byte-read of 0x0117def4-0x0117df14 and 0x0117df58-0x0117df6c).
- The draw wrapper's text branch is guarded by `id < 0x83e`, and the line query `FUN_01178ce0` and the line layout `FUN_01178f6c` carry the same guard, so 0x83e is excluded before any record or table byte is touched (byte-read of 0x01124c8, 0x01178cf4-0x01178d00 and 0x01178fb8-0x01178fcc).

## 2. Native-runner results

Ten stock cases under `private-data/analysis/x-m1-1.50/native-case/label-resolve/`, all passing with empty `mismatches` and `runner.status` `returned` (emulated). Each case maps the real plain-image code, the real record bytes from image 2 and, for the control, the real English language slot; every expectation was derived from the decode before the run.

| Case | Entry | Registers in | r0 out | Other observed |
| --- | --- | --- | --- | --- |
| `label-resolve-83c` | `0x01179b0c` | r0 = 0x83c | `0x0037b784` | lr 0x01179b24, 25 instructions |
| `label-resolve-83d` | `0x01179b0c` | r0 = 0x83d | `0x0037b7c6` | lr 0x01179b24, 25 instructions |
| `label-resolve-83e` | `0x01179b0c` | r0 = 0x83e | `0x00000000` | lr unchanged at the exit sentinel, no nested call, 20 instructions |
| `label-resolve-48b` | `0x01179b0c` | r0 = 0x48b | `0x003a6978` | control: the `<f06f>/STANDARD` text pointer from the English slot, 40 instructions |
| `label-metrics-83c` | `0x0117dee0` | r0 = 0x83c | 0x83c (unchanged) | both metric outputs become 0x0064, 14 instructions |
| `label-metrics-83d` | `0x0117dee0` | r0 = 0x83d | 0x83d (unchanged) | both metric outputs become 0x0064, 14 instructions |
| `label-metrics-83e` | `0x0117dee0` | r0 = 0x83e | 0x83e (unchanged) | both metric outputs become 0x0000, 14 instructions |
| `label-entryptr-83c` | `0x0117deb0` | r0 = 0x83c | `0x003a3934` | the first byte past the 0x83c-entry table |
| `label-entryptr-83d` | `0x0117deb0` | r0 = 0x83d | `0x003a393c` | eight bytes past the table end, not the inline record |
| `label-entryptr-83e` | `0x0117deb0` | r0 = 0x83e | `0x00000000` | the class boundary from the other side |

Three of the results are negative proofs by case construction, not by argument: `label-resolve-83e` maps the locator's code region but declares only the accessor as a function, so a call to the locator would fail the case as an unknown call; the metric cases declare only the metric function, map no language table and map no table-pointer word, so a table access would fail as a memory guard and a call to the entry-pointer accessor as an unknown call; and the `0x83e` entry-pointer case maps no table-pointer word at all.
The control ties the call to a real menu name: id 0x48b resolves to 0x003a6978, the UTF-16 `<f06f>/STANDARD` name of the first film-simulation row, through the real English slot and the stock table word 0x0039f754 at plain 0x01178aac.

The three ids differ in content (0x83c holds `? A M ?`, 0x83d 24 glyphs, 0x83e zeros) and the runs show that content does not decide the path: every class comparison is on the id before a record byte is read. The resolution of 0x83c and 0x83d is arithmetic on the id alone, the metric outputs are the fixed constant 100 for both, and 0x83e is refused by the same comparisons that admit the other two. An id that resolves because its record happens to hold plausible bytes is therefore not the mechanism here, and the mechanism is what a name write relies on.

What the harness cannot do, stated plainly: the draw wrapper and the text draw itself are outside the runner's admitted instruction set. `FUN_01179da0` executes SXTH, LDRSH and SMULL, `FUN_01112380` executes SXTH on the path that calls it, and the line layout `FUN_01178f6c` executes LDRSB; the runner refuses all of these (byte-read of the disassembly; the runner's refusal is the documented halfword and extend forms). So the run establishes what the renderer's text resolution returns, which is the id-to-pointer step every draw and measure path uses, and not the pixel path behind it. Widening admission for it needs hardware traces and a named experiment under phase 3's rule.

## 3. What writing a name takes

One name fits, on id 0x83d, and the write is 56 bytes in place.
The record layout is byte-read: 66 bytes per id, first 56 bytes an inline UTF-16 buffer, five layout halfwords at +56, +58, +60, +62 and +64. The 0x83d record at RAM 0x37b7c6 carries 24 glyphs in the 0xe1xx range, and its layout reads `{20, 20, 30, 36, 48}`; the normal per-id layout table at RAM 0x130000 + 10*id carries the same five fields with the same trailing three for ordinary menu ids (0x489, 0x48a, 0x48b, 0x48c and 0x83b all read `{*, *, 30, 36, 48}`), so a name drawn through 0x83d inherits the ordinary item box and no layout edit is needed for the one name. A 27-character name plus a zero terminator fits the 56-byte buffer; the text loops stop at a zero halfword (byte-read of the draw's and layout's character loops).
The bytes live in image 2's decode at RAM 0x00130000's extent, so the RAM offsets are the inline buffers at 0x37b784 (id 0x83c), 0x37b7c6 (id 0x83d) and 0x37b808 (id 0x83e). In the pinned container `FWUP0002.DAT` the same bytes are at file offsets 0x00eeb9e0, 0x00eeba22 and 0x00eeba64, stored as the byte complement of RAM (byte-read: 0xeeba22 reads `c0 ff af 1e ...`, the complement of `3f 00 50 e1 ...`; 0xeeba64 reads all `ff`, the complement of the zeroed 0x83e buffer; the mapping is container offset 0x00ca025c plus the record offset in image 2). The three locations lie inside the main checksummed range, so a container write must recompute the main checksum word, and the current rebuild tool has no image-2 record operation (`rebuild-update replace-record4` covers image 4 only, inherited).

## 4. Two names do not fit, and what survives

The second compiled name has no id. Id 0x83e returns zero from the text accessor, zero from both metric outputs and zero from the entry-pointer accessor, and the draw wrapper's own text branch admits only ids below 0x83e, so its record is unreachable from every one of the four entry points a label passes (emulated for the first three, byte-read for the fourth). Its layout halfwords are also all zero, so even an admitted path would get a zero box.
Alternatives a second name could use, with the verdict each gets here:

1. **The blank id 0x83c.** The record holds `? A M ?` and its layout is `{1, 1, 30, 36, 48}`. 0x83c is the id the node map uses wherever a label is blank (for example every label of the unlabelled nine-row list is 0x83c, inherited), so writing a name into it would show that name on every blank label of every surface. Rejected for D3.
2. **An in-table id below 0x83c whose text is unused.** This is the only route that keeps two names without a code change, and it is not established: a label id must be free on every surface that reads the string table, and no whole-image label-reference census exists. The film-simulation arrays alone name 0x489-0x4b2, so the search space is the rest of the table and the census is the work.
3. **A patched accessor family.** Extending the class bound from 0x83d to 0x83e needs edits in `FUN_01179b0c`, `FUN_0117df70`, `FUN_0117dee0` and the draw wrapper's compare, plus a layout entry for the new id. That is the D4 shape, whose fixed 328 bytes exceed the free tail even after the widened-window acceptance of 2026-10-09 (E-FILMSIM-PREFIX-WIDENING-150) and which has no home in image 2 or the code images.
4. **A third row that shows a stock label.** A third recipe row is possible on a free conversion-table code (`0xf`), but it would show whatever id the values/labels arrays give it, which is the numbered-row fallback and not a name.

The id-value census is bounded and reported as such: aligned 4-byte words, `movw`/`movt` pairs and branch targets were scanned across id4, id5, plain, id2, both loader copies, both boot records and the English slot. The special records' own addresses (`0x37b784`) are named by exactly six `movw`/`movt` sites, all in the plain special-class family (the five layout accessors at 0x0117dc30, 0x0117dc9c, 0x0117dd08, 0x0117dd70, 0x0117ddd8 and the locator at 0x0117e010), so no other code names the record base. The id *values* 0x83d and 0x83e appear as data words at id4 0x7cd958 (a pair table whose readers at 0x6cc730 and 0x6cc964 read numeric pairs), id4 0x7def54 and 0x7e9970 (inside float-shaped tables), and id5 0x100d8ac through 0x100d8b8 and 0x10a5664 (a table of adjacent id pairs). None of them is a node values or labels array, and no node pointer names any of them (byte-read of the node map and of the pair tables). Classifying those five tables' consumers is **unproved**, so they stay a residual for a rewrite decision rather than an accepted exclusion.

## 5. Residuals

- Whether any of the five data tables holding the id values is read by a live surface that would then draw 0x83d's or 0x83e's text. The pair tables read as numeric pairs at 0x6cc730 and 0x6cc964 make a label meaning unlikely, but they are not classified here.
- Whether the 66-byte record's inline buffer is read by any path other than `FUN_01179b0c`'s special branch. The record base is named only by the family's six sites, but a base-plus-index path from image 2's own data (a pointer word at RAM 0x37bab8 points to 0x37b84c, just past the 0x83e record) is not explained.
- Whether the menu would draw a 30x36 box name legibly. The box matches every ordinary menu id's layout, but no pixel reference output exists for this body.

## 6. Reproduction

```sh
R=private-data/analysis/x-m1-1.50
devenv shell -- bazel build //emulation:native-case //emulation:native-runner
for c in label-resolve-83c label-resolve-83d label-resolve-83e label-resolve-48b \
         label-metrics-83c label-metrics-83d label-metrics-83e \
         label-entryptr-83c label-entryptr-83d label-entryptr-83e; do
  devenv shell -- bazel-bin/emulation/native-case --runner bazel-bin/emulation/native-runner \
    "$R/native-case/label-resolve/$c.json"
done

# the record bytes and their container images
od -A x -t x1 -v -j $((0x24b784)) -N $((0xc6)) "$R/filmsim/id2-00130000.bin"
od -A x -t x1 -v -j $((0xeeb9e0)) -N $((0x96)) private-data/firmware/x-m1-1.50/FWUP0002.DAT

# the layout table entry for an ordinary id, and a special record's layout halfwords
od -A n -t u2 -v -j $((0x48b*10)) -N 10 "$R/filmsim/id2-00130000.bin"
od -A x -t x2 -v -j $((0x24b7b8)) -N 30 "$R/filmsim/id2-00130000.bin"
```
