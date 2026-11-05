# X-M1 1.50 stock record-service contract

## Evidence and notation

This is a static contract for image4 `[0x6ac2b4,0x6ae114)`. **byte-read** denotes A32 instructions in `private-data/analysis/x-m1-1.50/lz/id4.dis`; citations are inclusive instruction addresses. **exact-C** denotes a definition marked `exact` in `private-data/scratch/xm1-decomp/index/55436fc85c0560dd252b0410af6f2427fe198cd3-31e27c0e12fe.tsv`, with C under `private-data/scratch/StorageDesign/decomp/decomp/firmwares/xm1-150/app/settings/`. **open** identifies a limitation, not an inferred implementation requirement. NONMATCHING C was used as a reading aid; the claims below about those bodies cite instructions. No dynamic execution was performed.

**byte-read**: The outer initialized word is `0x89f8b0`; the five wrapper objects start at `0x89f8b4` with stride 12: `{active, banks, slot_pointer}`. The inner initialized word is `0x89f8f0`, cache enabled is `0x89f8f4`, and five slot objects start at `0x89f8f8` with stride `0x48` (`0x6ac42c..0x6ac458`, `0x6ad180..0x6ad22c`, `0x6ad054..0x6ad0a0`). A wrapper pointer is an allocated object identity, not a kind number; allocation uses the first inactive entry.

**byte-read**: Slot word offsets are `+0 active`, `+4 kind`, `+8 bank index`, `+0xc bank-count query result`, `+0x10 pages-per-bank query result`, `+0x14 bytes-per-page query result`, `+0x18 record size S`, `+0x1c records in this bank N`, `+0x20 sequence offset Q`, `+0x24 records per page P`, `+0x28 prepare`, `+0x2c prepare context`, `+0x30 check`, `+0x34 check context`, `+0x38 position`, `+0x3c page`, `+0x40 byte offset`, `+0x44 buffer` (`0x6ad1dc..0x6ad22c`, `0x6acf44..0x6acf9c`). The structure-header names `block_count` and `reserved_14` are misleading; the instructions establish their meanings.

**byte-read**: Status constants are signed 32-bit `-14 = ~13`, `-30 = ~29`, `-255 = ~254`, and zero. Unless a section specifies an output write, a failing branch leaves caller outputs unchanged. Pointer validity is assumed: these entries generally dereference wrapper/output pointers without null validation (for example `0x6ac350..0x6ac35c`, `0x6ac4f4..0x6ac514`).

## Geometry and raw-service boundary

**byte-read**: `record_query_block_count(kind,&B)` returns two for every kind 0 through 4; `record_query_pages_per_block(kind,&H)` returns one. Both require cache enabled, returning `-14` otherwise, and return `-30` for an unsigned kind greater than four, without writing the output (`0x6ada80..0x6adabc`, `0x6adac0..0x6adafc`). Thus the wrapper `blocks` field is the bank count B=2, not the registration's record-count argument.

**byte-read**: `record_query_kind_region_count(kind,&L)` obtains a byte capacity from the per-kind region query for bank zero. Its calls are kind0 `0x6aaa7c`, kind1 `0x6aa8ac`, kind2 `0x6aafdc`, kind3 `0x6ab254`, kind4 `0x6aac4c`; it ignores their returns, writes the local capacity, then returns zero. Disabled returns `-14`; kind greater than five returns `-30`; kind five writes zero and succeeds (`0x6adb00..0x6adbe0`). The locals start at address zero/capacity `0xffffffff`, so a failed backend query can expose that sentinel (`0x6adb10..0x6adb1c`).

**byte-read**: The descriptors encode one 128 KiB block per bank for kinds0..3; kind4 divides its two-block descriptor into equal one-block banks. Consequently L=`0x20000`, H=1, P=`floor(L/S)`, and registration makes N=`floor(r2/B)` for each bank (`0x6ac364..0x6ac37c`, `0x6ad110..0x6ad158`; descriptors `0x7c8cf0..0x7c8d4c`, `0x7c8d68..0x7c8d70`; kind4 split `0x6aac78..0x6aaca4`). Odd counts lose one record in the aggregate; there is no ceiling division.

| Kind | Stock registration S / total r2 / Q | N per bank | P per physical block | Logical pool banks | Fixed program length | Evidence |
| --- | --- | ---: | ---: | --- | ---: | --- |
| 0 | `0x60 / 0x1fc / 0` | 254 | 1365 | `0x3d4,0x3d5` | `0x60` | **byte-read** `0x727e5c..0x727eac`, `0x7c8d08..0x7c8d1c`, `0x6adf34..0x6adf64` |
| 1 | `0x1000 / 0x20 / 0` | 16 | 32 | `0x3d2,0x3d3` | `0x1000` | **byte-read** `0x727c08..0x727c58`, `0x7c8cf0..0x7c8d04`, `0x6ade6c..0x6ade9c` |
| 2 | `0x200 / 0x3e / 0` | 31 | 256 | `0x3d6,0x3d7` | `0x200` | **byte-read** `0x6a75a4..0x6a75f4`, `0x7c8d20..0x7c8d34`, `0x6adf6c..0x6adf9c` |
| 3 | `0x34 / 0xfe / 0` | 127 | 2520 | `0x3d8,0x3d9` | `0x600` | **byte-read** `0x6a8abc..0x6a8b0c`, `0x7c8d38..0x7c8d4c`, `0x6adeac..0x6adedc` |
| 4 | `0x600 / 0x14 / 0` | 10 | 85 | `0x3e8,0x3e9` | `0x600` | **byte-read** `0x6b02d4..0x6b0324`, `0x7c8d68..0x7c8d70`, `0x6adee4..0x6adf14` |

**byte-read**: Kind3's `0x600` program is not its `0x34` record size. The read preparation copies only S bytes into the kind buffer, append modifies S bytes, and program sends `0x600` bytes starting there. Bytes `[S,0x600)` retain prior RAM contents, initially `0xff`. Adjacent on-flash bytes can therefore be part of a program request even though the caller supplied only one 52-byte record (`0x6add70..0x6add98`, `0x6ad498..0x6ad500`, `0x6aded4..0x6aded8`, `0x6ad9e4..0x6ad9f4`). This is part of the stock trace, not an implied safe NAND programming law.

**byte-read**: Let D be the descriptor bank's logical pool start. The per-kind region query's address is `((D+0x7c80)<<17) mod 2^32`, equivalently `(D-0x380)<<17` for these banks. Thus cache addresses use pool-relative blocks, not absolute logical block D (`0x6aa8e4..0x6aa938`, `0x6aaab4..0x6aab08`, `0x6ab014..0x6ab068`, `0x6ab28c..0x6ab2e0`, `0x6aac78..0x6aaca4`).

**byte-read**: For a program at bank-local offset O, the per-kind write calls `0x6a8c94(r0=D-0x380+(O>>17), r1=O&0x1ffff, r2=fixed_length, r3=kind_buffer)`. Kind4 selects D as descriptor-start plus bank times half the descriptor block count (`0x6aa9ec..0x6aaa00`, `0x6aabbc..0x6aabd0`, `0x6ab11c..0x6ab130`, `0x6ab394..0x6ab3a8`, `0x6aad04..0x6aad30`). For these stock geometries O is position times S, always within the selected bank.

**byte-read**: Erase dispatch ultimately calls `0x6a8c10(D-0x380, descriptor_block_count)` for kinds0..3; kind4 calls with half its two-block count and corresponding bank start. Erasing a bank therefore requests one pool-relative raw block (`0x6aaa3c..0x6aaa74`, `0x6aac0c..0x6aac44`, `0x6ab214..0x6ab24c`, `0x6aad68..0x6aad84`, `0x6ab48c..0x6ab4c4`).

**byte-read**: Cache misses invoke `0x6aad94(aligned_pool_relative_byte_address, cache_buffer)`; its terminal dispatch is `0x6a8d40(address>>17,1,cache_buffer)`, a complete 128 KiB block read (`0x6ad910..0x6ad930`, `0x6aad94..0x6aadd0`, `0x6aafc8..0x6aafd0`). Raw physical remapping is below this contract. The supplied known backend chain reaches NAND request entries read `0x6fb904`, write `0x6fb7c8`, erase `0x6fb57c`; their physical-device completion semantics are outside the replaced span.

## Nine client entries

### `0x6ac2b4`: open service

**exact-C**, `record_service_open.c:6..14`; **byte-read** `0x6ac2b4..0x6ac2f0`: r0 is forwarded to inner initialization; already-open outer state returns `-14`. An inner failure is mapped to `-14`; success sets outer initialized=1 and returns zero. It writes no client handle or caller buffer and performs no raw-service request. The argument is not consumed by the cache initializer's instructions.

### `0x6ac328`: register wrapper

**byte-read** `0x6ac328..0x6ac3b8`: ABI is `(r0 kind, r1 S, r2 total_count, r3 Q, [entry_sp] prepare, [entry_sp+4] prepare_context, [entry_sp+8] check, [entry_sp+12] check_context, [entry_sp+16] client_word_pointer)`. It asks B, computes signed total_count/B, and opens bank zero with that N and the callbacks. The query return is ignored.

**byte-read** `0x6ac348..0x6ac360`, `0x6ac3bc..0x6ac474`: unopened service writes zero to the client word and returns `-14`. Inner-open failure returns `-30`, leaving the client word unchanged. An unsigned total_count greater than `0x7ffe` closes the newly allocated slot and returns `-30`. Otherwise it claims the first inactive wrapper, stores `{1,B,slot}`, writes its address to the client word, and returns zero. Wrapper exhaustion closes the slot and returns `-14`, leaving the client word unchanged. Registration does not initialize the slot's cursor or buffer fields beyond the fields explicitly stored by inner-open. No raw read, write, or erase occurs.

### `0x6ac478`: close wrapper

**exact-C**, `record_recovered_wrappers.c:4..11`; **byte-read** `0x6ac478..0x6ac4c4`: r0 is wrapper pointer. Void operation; if outer service is off or wrapper inactive, it returns without change. Otherwise it closes a nonnull slot, clears wrapper.slot and wrapper.active. It does not clear the client's word containing the wrapper pointer. Release performs no flush, and no persistent effect occurs.

### `0x6ac4c8`: read latest valid record

**byte-read** `0x6ac4c8..0x6ac57c`: ABI `(wrapper, optional_u16_recovery_count, buffer_pointer_output)`. The third argument receives a pointer into the per-kind staging buffer, not a copy into caller storage. Outer-off, inactive wrapper, or null slot writes a null pointer and returns `-14`. Slot-query failure writes null and returns `-30`. Later failure branches generally do not initialize the output pointer.

**byte-read** `0x6ac57c..0x6ac690`, `0x6ac7c4..0x6ac7d0`: obtain slot configuration, define T=N*B and W=`0x8000-T`, then visit banks cyclically from the currently selected index, for up to B+1 candidate examinations. Each bank calls find-valid, which supplies record position p, number skipped s, and buffer. The first valid candidate becomes best. For candidate sequence c and best b, choose candidate when `c<=b && signed(b-c)>=T`, or when `c>b && signed(c-b)<=W`; otherwise stop scanning with the previous best. Equality enters the first arm and stops for positive T. A selected best retains its bank, p, s, and sequence. After each continued iteration it closes and reopens the next bank `(index+1)%B`. A reopen failure returns `-255`. If all examinations yield no candidate, return `-255`. These thresholds are the stock cross-bank wrap comparator; they are not a conventional fixed half-range comparison.

**byte-read** `0x6ac694..0x6ac744`: if the optional halfword output is nonnull, write the selected candidate's skipped count s, truncated to 16 bits. If its p equals N-1, temporarily open the following bank and call find-valid there; if that call fails and its returned skipped count is nonzero, write the same saved s again. Reopen failure returns `-255`. The optional halfword is a recovery/skipped-record count, not the sequence number.

**byte-read** `0x6ac748..0x6ac7bc`: close the current slot, reopen the best bank, then return find-valid(slot,NULL,NULL,buffer_pointer_output)'s status. The selected bank is re-searched, so callbacks run again. Reopen failure returns `-255`. The final find-valid can return `0`, `-14`, or `-255`; output behavior is that helper's behavior. This entry reads flash through the cache but makes no erase/program request. Wrapper.slot can change during scanning, even on failure.

### `0x6ac7d4`: append request and rotation

**byte-read** `0x6ac7d4..0x6ac87c`: ABI `(wrapper, initial_sequence, caller_record_pointer)`. Invalid outer state/wrapper/slot returns `-14`; slot-query failure returns `-255`. It calls read-latest with no recovery-count output. On success, seed becomes `(latest_sequence+1)&0x7fff`; it first tries inner append in the bank selected by read-latest. Success returns zero.

**byte-read** `0x6ac880..0x6ac978`: if read-latest fails, choose bank zero and preserve the caller's initial sequence. If the first append after successful read-latest fails, choose `(original queried bank index+1)%B`. The index used here is the one queried before read-latest, not re-queried from its selected slot. Close the current slot, open the chosen bank, and attempt append without erasing it first. On open failure return `-255`. If this append fails, call erase-slot on this bank, ignore its return, and attempt append again. Final append success returns zero; failure returns `-255`. Hence a full destination is erased before reuse, but a destination with room is appended without erase. It never erases both banks merely because both are full.

**exact-C**, `record_cursor.c:27..45`; **byte-read** `0x6ad434..0x6ad514`: inner append recomputes sequence from the last physically written record when its bank is nonempty, even if that record fails the check callback. Thus the wrapper's seed matters only when the destination bank is empty. Caller record bytes are copied except for the replaced sequence halfword, then prepare(buffer,context) runs, then fixed-length program runs. The caller input buffer is not written by stock service code. Raw program failures are swallowed by the program step and append still returns zero.

### `0x6ac984`: erase all banks and re-register

**exact-C**, `record_wrapper_rotation.c:6..59`; **byte-read** `0x6ac984..0x6acab4`: r0 wrapper; invalid wrapper/service/slot returns `-14`, failed slot-query returns `-255`. If B<=0 return zero. Starting at the current bank, repeat B times: erase-slot, ignore its status; close; reopen `(index+1)%B` with saved geometry and callbacks into wrapper.slot. Reopen failure returns `-255` immediately. On completion return zero. The client word remains the wrapper pointer; only wrapper.slot is replaced. Ordered erase calls therefore follow current-bank cyclic order. There is no record append or sequence reset stored separately.

### `0x6acab8`: collect records, sort banks, compact caller buffer

**byte-read** `0x6acab8..0x6acb68`: ABI `(wrapper, caller_buffer, optional_byte_count_output)`. Invalid outer state/wrapper/slot returns `-14`; slot-query failure returns `-255`. It calculates bank chunk size C=N*S. The caller must supply storage for B*C bytes; the API has no capacity argument.

**byte-read** `0x6acb78..0x6acc34`: for each bank in cyclic order, call read-all into its C-byte caller chunk, then mark unused record sequence halfwords `0xffff` at record-size strides. The rest of each unused record's bytes are left as supplied by the caller. Close and reopen the next bank after every chunk, including the final chunk. A read-all or reopen failure returns `-255`, preserving partial caller-buffer writes and leaving optional byte count unchanged.

**byte-read** `0x6acc38..0x6accec`: compare each bank chunk's first sequence halfword as an unsigned number and byte-swap complete C-byte chunks when the later chunk's first sequence is smaller. This orders banks numerically by their first record sequence; it does not apply the read-latest wrap comparator.

**byte-read** `0x6accf0..0x6acd98`: scan N*B record positions in that sorted buffer, compact every record whose sequence halfword is not `0xffff` to the buffer front, copying S bytes. Check callbacks are not called. If count-output is nonnull, write compacted byte length and return `-255` for zero bytes, otherwise zero. With null count-output return zero even for an empty collection. Tail bytes after the compacted prefix remain whatever the reads, markers, swaps and copies produced. This entry changes wrapper.slot/cursors/cache and caller memory but has no persistent effect: its callees do not erase or program.

### `0x6acda0`: read at byte position

**exact-C**, `record_read_position.c:7..45`; **byte-read** `0x6acda0..0x6acef4`: ABI `(kind, unsigned_byte_position, length, caller_output)`. Query B, H, L in order, then bank=`position/(H*L)`, page=`(position%(H*L))/L`, offset=`position%L`. Query failure zero-fills length bytes and returns its status (`-14` or `-30`). If bank>=B, write only caller_output[0]=0 and return `-30`, leaving the rest unchanged. Call prepare-read-buffer(kind,bank,page,offset,length,&buffer); its failure zero-fills length bytes and returns that status. Success copies length bytes from staging buffer to caller_output and returns zero. It neither validates records nor calls check/prepare callbacks. The prepare helper prevents a normal request from crossing the L-byte bank region. No program/erase occurs.

### `0x6ae0fc`: constant success

**exact-C**, `tu_006ac2f4.c:819..822`; **byte-read** `0x6ae0fc..0x6ae110`: ignores argument registers, returns zero, writes no client-visible memory and makes no calls. It has no persistent effect.

## Internal function contracts and ordered effects

### Service and slot lifecycle

**exact-C**, `tu_006ac2f4.c:73..81`; **byte-read** `0x6ac2f4..0x6ac324`: outer shutdown is void; if initialized it calls inner shutdown then clears outer initialized. It does not clear wrapper entries or client words and makes no raw request.

**exact-C**, `record_service.c:26..33`; **byte-read** `0x6ad054..0x6ad0b0`: inner initialize(argument) returns `-255` if already initialized or cache initialization fails, otherwise zeroes `0x168` slot bytes, sets inner initialized=1, and returns zero. Cache initialization supplies its own zero on success. No flash operation occurs.

**byte-read** `0x6ad0b4..0x6ad234`: open-slot ABI is `(kind, bank_index, S, N, [sp] Q, [sp+4] prepare, [sp+8] prepare_context, [sp+12] check, [sp+16] check_context, [sp+20] slot_pointer_output)`. Inner-off returns `-255`. It calls B/L/H queries and ignores their statuses. Validation returns `-30` when signed bank<0, unsigned bank>B, unsigned S>L, `floor(L/S)*H*S < N*S` in 32-bit arithmetic, odd Q, or unsigned S<Q+2. Bank==B is accepted by this check; it is not normalized. S=0 is not explicitly rejected before division. On success claim first inactive slot, store the configuration fields at offsets `+4..+0x34`, set active=1, and write its pointer to output. Slot exhaustion returns `-255`. Failure does not null the output. Cursor, page, offset and buffer are not reset when reusing a slot. No raw call occurs.

**exact-C**, `record_service.c:34..39`; **byte-read** `0x6ad23c..0x6ad288`: close-slot(slot) is void; inactive or service-off is a no-op, otherwise release nonnull buffer, clear buffer pointer, clear active. No raw effect.

**exact-C**, `record_service.c:44..53`; **byte-read** `0x6ad28c..0x6ad2f4`: inner shutdown is void; closes active slots in increasing array order, disables cache, clears inner initialized. It does not flush a dirty cache because this cache has no dirty writeback state.

### Cursor and validity

**exact-C**, `record_service.c:12..25`; **byte-read** `0x6acef8..0x6acfa4`: seek(slot,position,delta) is treated as void by its callers. Position=-1 means current+delta; otherwise position+delta. Clamp negative to zero; clamp above N to N-1, but position==N is not clamped. Set page=`position/P`, offset=`(position-P*page)*S`. Release old nonnull buffer; call prepare-read-buffer(kind,index,page,offset,S,&slot.buffer). The helper's return is ignored by callers of seek; on a failing preparation it can leave the old pointer in slot.buffer. This operation reads but does not write flash.

**exact-C**, `record_cursor.c:5..20`; **byte-read** `0x6acfa8..0x6ad050`: find-last(slot) is void. Binary-search positions `[0,N-1]` at midpoint truncating signed division by two: sequence !=`0xffff` moves low to position+1, otherwise high to position-1. Stop on low>=high or null buffer. After stopping it dereferences buffer to test sequence, even if null. If the final record is unwritten, step back one when position>0; at position zero release and null buffer. It tests occupancy, not callback validity. There is no general scan for holes or torn sequences. No raw program/erase occurs.

**byte-read** `0x6ad2f8..0x6ad3f0`: find-valid(slot,optional_position,optional_skipped,optional_buffer_pointer) returns `-14` on inner-off/inactive, without output writes. Call find-last; if buffer is null return `-255` without writing outputs. With no check callback, skipped=0 and status=0. Otherwise call check(buffer,check_context); zero accepts, any nonzero rejects. On rejection at position>0, seek relative -1, increment skipped and check again. At rejected position zero release, null slot.buffer, set status=`-255`. Then write every nonnull output pointer with current position, skipped count and buffer, including that rejected-position-zero case. Thus the rejected record at position zero does not itself increment skipped. If a seek yields null, the loop exits with seek's r0 value still live rather than an explicit fixed status (`0x6ad3a0..0x6ad3c8`). The normal successful preparation returns zero. Callback failure never erases or repairs flash.

**exact-C**, `record_cursor.c:27..45`; **byte-read** `0x6ad3f4..0x6ad518`: append(slot,sequence,data,optional_position) returns `-14` on inner-off/inactive; finds last occupied record. If nonempty and position>=N-1, return `-255` without output write. Else recompute sequence `(stored_u16+1)&0x7fff`, seek one forward; empty bank seeks zero and retains the input sequence, which is stored as a halfword without an additional mask. Copy data prefix `[0,Q)`, store sequence at Q, copy suffix `[Q+2,S)`, invoke optional prepare(buffer,context), call program-step(buffer), ignore its result, write optional position, return zero. The source record's original sequence is not copied. The prepare callback can modify staging bytes before the fixed-size program request.

### Erasure, slot query and sequential read

**byte-read** `0x6ad51c..0x6ad5c8`: erase-slot(slot) returns `-14` on inner-off/inactive. Release and null buffer. Loop page index 0 through H-1, calling erase-block(kind,bank,page); after any nonzero return stop issuing further erase calls while completing the loop, and return `-14`. H<=0 succeeds without an erase. Success returns zero. With stock H=1 it issues one bank erase. It does not reset cursor fields.

**exact-C**, `record_cursor.c:49..65`; **byte-read** `0x6ad5cc..0x6ad694`: query-slot(slot, optional kind,index,S,N,Q,prepare,prepare_context,check,check_context output pointers) returns `-14` if inner-off/inactive, otherwise writes each requested field and returns zero. Register arguments supply the first three output pointers, later pointers are stack arguments. It makes no raw calls.

**byte-read** `0x6ad698..0x6ad7ac`: read-all(slot,caller_buffer,byte_count_output) returns `-14` on inner-off/inactive without output writes. Release existing buffer and null it; seek zero; then while buffer nonnull, position<N and sequence!=`0xffff`, copy S bytes into successive caller records and seek one forward. Stop at first unwritten record or boundary; no check callback runs. Write copied byte count and return zero, including empty input. Seeking after the last copied record can cause an additional read preparation at position N because seek's clamp is strict `>`; its result is not propagated. No persistent effect.

### Cache initialization and region reads

**exact-C**, `tu_006ac2f4.c:704..721`; **byte-read** `0x6ad98c..0x6ada58`: initialize-cache ignores its incoming argument. If already enabled return `-14`. Fill kind staging buffers with `0xff`: kind0 `0x8e16a0` length96, kind1 `0x8e00a0` length4096, kind2 `0x8e1700` length512, kind3 `0x89fa80` length1536, kind4 `0x8e10a0` length1536. Invalidate both cache tags at `0x8e0080/84` to `0xffffffff`, clear ages at `0x89fa60/64`, clear timestamp at `0x8e0088`, set enabled=1, return zero. It neither fills nor reads the two full-region cache buffers at initialization.

**exact-C**, `record_cache_status.c:4`; **byte-read** `0x6ada5c..0x6ada7c`: disable-cache is void and only writes enabled=0. It does not flush.

**byte-read** `0x6ad7b0..0x6ad988`: read-cached-region(address,destination,length) takes semaphore `0x4b` with timeout `-1` through `0x724b68`, ignoring that return, and releases through `0x724c00`. If cache disabled, reset tags, ages and timestamp before processing the request. Address=`0xffffffff` is invalidate-only: set both tags invalid, return `-255`, and do not copy or call raw read. This is cache invalidation, not a writeback flush.

**byte-read** `0x6ad83c..0x6ad930`: two cache slots each hold a `0x20000`-byte aligned region. Find a tag interval containing address; slot1 wins if both match. On miss choose invalid slot0, else invalid slot1, else the slot with smaller unsigned age; an age tie selects slot0. Install tag=`address&~0x1ffff`, call `0x6aad94(tag,0x8a0080+(slot<<17))`, ignore raw return, then mark slot accessed. The tag is installed before the raw call, so failed reads can be cached as hits.

**byte-read** `0x6ad934..0x6ad988`: store selected age=current timestamp, increment timestamp modulo 32 bits, copy length bytes from `0x8a0080+(slot<<17)+(address&0x1ffff)` to destination, release semaphore, return zero. There is no explicit cross-region check here. Normal preparation bounds keep stock record requests inside a region; a direct oversized cached-region call would copy across its selected cache buffer. The method has no flash program/erase effect.

### Read preparation and program step

**byte-read** `0x6adbe4..0x6adc9c`: prepare-read-buffer(kind,index,page,offset,[sp] length,[sp+4] buffer_pointer_output) returns `-14` when disabled. Queries B,H,L, ignoring their returns. Return `-30` for signed index<0, unsigned index>B, signed page<0, unsigned page>H, or unsigned `(offset+length)>L`, using 32-bit addition. Index==B and page==H pass these checks. Errors here do not write output. The page value is validated and saved but is not added to the read address in the switch.

**byte-read** `0x6adca0..0x6adde4`: save `{index,page,offset}` at `0x917fa8+kind*12`. For kind0..4 call its per-kind bank region query(index,&address,&capacity), ignore return, read-cached-region(address+offset,kind_staging_buffer,length), ignore return, write staging pointer to output and return zero. Kind-specific staging pointers are those listed above. A default kind writes null and returns `-255`, after the descriptor-state store. Thus this function is not an allocator and does not return a direct flash-cache pointer. It returns one shared staging buffer per kind. Any subsequent same-kind read can overwrite a pointer returned earlier.

**byte-read** `0x6addf0..0x6adfa0`: program-step(buffer) returns `-14` when disabled; null buffer returns `-30`. For each recognized staging pointer first invalidate both cached regions with read-cached-region(-1,NULL,0), ignoring its `-255` result; then call its per-kind write(buffer,saved_index,saved_offset,fixed_length). Saved page is not used. Calls are kind0 `0x6aab1c` length96, kind1 `0x6aa94c` length4096, kind2 `0x6ab07c` length512, kind3 `0x6ab2f4` length1536, kind4 `0x6aacb8` length1536. Ignore every write result and return zero. An unrecognized nonnull buffer only invalidates the cache and returns zero. There is no implicit erase. The cache invalidation precedes the raw write even if that write fails.

**exact-C**, `record_cache_status.c:17..24`; **byte-read** `0x6adfa4..0x6adfd4`: release-buffer(buffer) returns `-14` when cache disabled, else `-30` for null or zero for nonnull. It modifies no buffer, tag, or flash byte. Its callers ignore the result.

**exact-C**, `record_block_commands.c:5..45`; **byte-read** `0x6adfd8..0x6ae0e0`: erase-block(kind,index,page) returns `-14` when cache disabled; queries B/H ignoring statuses; returns `-30` for index<0, index>B, page<0, or page>H. Accepted values first invalidate both cached regions. Dispatch kind0..4 to erase(bank_index): `0x6aabd8`, `0x6aaa08`, `0x6ab1e0`, `0x6ab458`, `0x6aad40` respectively. Page is not passed onward. Ignore the per-kind/raw erase result and return zero. Unknown kind returns `-255` after invalidation if it reaches the switch. The invalid-kind query failures leave locals unset, so no deterministic validation result is established for those malformed calls.

**byte-read** `0x6ae0e4..0x6ae0f8`: additional in-span constant-zero function, with no client-memory writes or calls. It is not reached by the in-span direct-call graph recovered above; included to account for the remaining function body in the replacement span.

## Cache lifetime, reboot and fault interpretation

**byte-read**: Flash occupancy and sequences drive searches anew; there is no persistent active-bank selector or sequence counter in this span. RAM contains allocated wrapper/slot identities, callback/context pointers, current cursor and selected bank, per-kind staging contents and saved program destinations, and two cache tags/ages/timestamp (`0x6ad1dc..0x6ad22c`, `0x6ac5b0..0x6ac690`, `0x6adca0..0x6adcbc`, `0x6ad934..0x6ad950`). Those identities and callbacks are established by registration, not rebuilt from flash alone.

**byte-read**: A reboot cannot reconstruct staging bytes outside the most recent read length, cached data retained after a failed raw read, allocation history, cursor history, callback contexts, or the raw-call history from flash. Initialization sets staging bytes to `0xff` and discards cache tags (`0x6ad9a8..0x6ada4c`). Kind3's larger fixed program exposes staging-tail history as a persistent write payload. A replacement seeking stock-observable raw traces must specify its initialization/handover state, not assume flash uniquely determines all subsequent bytes.

**byte-read**: Check rejection only affects find-valid/read-latest; occupancy search, append and read-all use the sequence marker alone (`0x6ad368..0x6ad3c4`, `0x6acff0..0x6ad020`, `0x6ad460..0x6ad470`, `0x6ad71c..0x6ad768`). Raw read/program/erase errors are discarded at the cache/preparation/program/erase dispatch boundaries. Zero return from append or erase-all does not establish durable success. Erase-slot's error aggregation normally sees zero because erase-block overwrites backend status (`0x6ad598..0x6ad5bc`, `0x6ae088..0x6ae0d4`).

## Explicit open questions and domain limits

- **open**: Physical NAND partial-program, ECC/spare update, power-cut and bad-block behavior is below this span. Static status swallowing cannot establish what bytes survive a failing backend operation.
- **open**: The raw read failure's destination-buffer contents depend on the lower service. This contract establishes that tags are retained and success is returned, not a fabricated fallback byte pattern (`0x6ad920..0x6ad978`).
- **open**: Callback-specific allowed mutations and failure behavior belong to the five client validators/sealers. This document establishes callback order and argument ABI, not all client format constraints.
- **open**: Zero S, invalid kind entering helpers that ignore query failure, integer-overflow arguments, null output pointers, and null buffer reached inside find-last do not have a safe total contract. The byte branches and uninitialized/null hazards above are preserved as observations; processor/runtime behavior would be needed to define their terminal result.
- **open**: Concurrency outside semaphore `0x4b` is not established. The staging buffers and saved per-kind destinations are shared, while slot search/append as a whole is not locked by this span.
- **open**: A post-boot takeover must choose whether to preserve live staging/cache/registration state or adopt a fresh-open state. Those starts can produce different raw read traces and, for kind3, different fixed-length program tails.

## Oracle comparison boundary

**byte-read-derived oracle requirements**, from the client entries and raw boundaries cited above: compare signed return values of status-returning entries; compare the client word after registration, including nulling only on the outer-unopened branch and unchanged words on other registration failures; compare handle identities through a consistent abstract handle map; compare caller byte buffers over every location actually written, including sequence markers and collection tail effects; compare output pointer nullness and the pointed-to record bytes; compare recovery-count halfword writes and byte-count words, including untouched outputs on failure; compare callback invocation order, arguments, result handling and mutations; compare ordered raw read `(pool-relative block,count,destination)`, write `(block,offset,length,payload)`, and erase `(block,count)` calls. Normalize private buffer addresses by their role while retaining alias relationships. Reads can differ between cold and warm cache starts, so initialize the two runs to equivalent observable cache histories when raw traces are the oracle.

**byte-read-derived exclusions**, from void lifecycle signatures and RAM implementation above: do not compare incidental r0 after void close/shutdown/seek/find-last calls; do not require equal physical wrapper/slot/staging addresses, register allocation, stack frames, semaphore implementation, cache replacement metadata, internal cursor representation, or dead scratch bytes. Private RAM becomes observable when used as a later returned record, callback input, or fixed-size raw write payload; at that point compare those bytes at the boundary. Do not promote stock ignored raw failures into expected client errors. Flash byte effects and ordered requests are separate oracle observations, especially where a failed request can have partial physical effects.
