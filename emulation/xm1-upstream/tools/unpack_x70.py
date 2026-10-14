#!/usr/bin/env python3
"""Decode the two X70 v1.10 LZ streams and export mapped ARM sections.

Recovered from FPUPDATE.DAT: a 20-byte stream header then literal/match
tokens. A token below 128 copies that many literal bytes. A token >=128
and the following byte encode an 11-bit backward distance and 4-bit length:
distance = ((token & 127) << 4) | (second >> 4), length = second & 15.
Overlapping copies are intentional. Reject invalid/truncated streams.
"""
import hashlib
import json
from pathlib import Path
import re
import struct

ROOT = Path(__file__).resolve().parents[1]


def decompress(source):
    if len(source) < 20:
        raise ValueError("Truncated stream header")
    unpacked, packed, pages, page_size, method = struct.unpack_from("<5I", source)
    if method != 4 or page_size != 0x4000 or not 20 <= packed <= len(source):
        raise ValueError("Unexpected stream header")
    if pages != (packed + page_size - 1) // page_size:
        raise ValueError("Unexpected compressed page count")
    output = bytearray()
    position = 20
    counts = {"literals": 0, "matches": 0}
    while position < packed:
        command = source[position]
        position += 1
        if command < 0x80:
            if command == 0 or position + command > packed:
                raise ValueError("Invalid literal run")
            output.extend(source[position:position + command])
            position += command
            counts["literals"] += 1
        else:
            if position >= packed:
                raise ValueError("Truncated match")
            second = source[position]
            position += 1
            distance = ((command & 0x7f) << 4) | (second >> 4)
            length = second & 15
            if not 0 < distance <= len(output) or length == 0:
                raise ValueError("Invalid back reference")
            for _ in range(length):
                output.append(output[-distance])
            counts["matches"] += 1
        if len(output) > unpacked:
            raise ValueError("Output exceeds declared size")
    if len(output) != unpacked:
        raise ValueError("Output does not match declared size")
    return bytes(output), {"packed_size": packed, "unpacked_size": unpacked,
                           "page_size": page_size, "page_count": pages,
                           "method": method, "token_counts": counts}


def main():
    original = (ROOT / "firmware/original/FPUPDATE.DAT").read_bytes()
    if hashlib.sha256(original).hexdigest() != "7dd5b358a66f38d1fc7dad899a77d35fe4e6ed0edd0ed1d04fbca512d5dc8fd6":
        raise ValueError("This map is specific to the supplied X70 v1.10 image")
    payload = original[532:].translate(bytes(255 - n for n in range(256)))
    # DAT payload+0x48 is the native flash image, as confirmed by the boot
    # loader's section table at native flash 0x140040.
    flash = payload[0x48:0x32a0048]
    out = ROOT / "firmware/mapped"
    out.mkdir(parents=True, exist_ok=True)
    sections = []

    def save(name, data, address, offset, extra=None):
        (out / f"{name}.bin").write_bytes(data)
        meta = {"name": name, "file": f"firmware/mapped/{name}.bin",
                "address": address, "size": len(data),
                "native_flash_offset": offset, "DAT_offset": offset + 0x25c,
                "sha256": hashlib.sha256(data).hexdigest()}
        meta.update(extra or {})
        sections.append(meta)

    save("boot", flash[0x40000:0x60000], 0, 0x40000)
    save("kernel0", flash[0x80000:0x8dba0], 0x40000, 0x80000)
    save("kernel1", flash[0xa0000:0xadb80], 0xd0000, 0xa0000)
    save("resources2", flash[0x10a0000:0x12e23e4], 0x130000, 0x10a0000,
         {"executable": False})
    save("resources3", flash[0x1660000:0x16e4294], 0x640000, 0x1660000,
         {"executable": False})
    save("section_table", flash[0x140000:0x140400], 0x8a0000, 0x140000,
         {"executable": False})
    for number, offset, base in [(4, 0x160000, 0x8a0400), (5, 0x320000, 0xf586a0)]:
        data, header = decompress(flash[offset:])
        if struct.unpack_from("<I", data, 4)[0] != base or data[28:32] != b"\xff\xff\xaa\xaa":
            raise ValueError("Unexpected decoded section header")
        save(f"section{number}", data, base, offset, {"compression": header})
    # Section6 is loaded as one contiguous flash range 0x660000..0xc00000
    # at RAM 0x19a0000. load_overlay_sections() (0x94aca8) gets its flash
    # base/length from descriptor6 and its RAM destination from the overlay
    # group descriptor. overlay_load_progress() (0x9d9800) notifies users
    # as their subranges arrive. Preserve the three named subranges here.
    save("overlay48", flash[0x660000:0x961500], 0x19a0000, 0x660000)
    save("overlay4a", flash[0x980000:0x99d340], 0x1cc0000, 0x980000)
    save("overlay4b", flash[0x9a0000:0xbffc40], 0x1ce0000, 0x9a0000)
    (out / "manifest.json").write_text(json.dumps({
        "source_sha256": hashlib.sha256(original).hexdigest(),
        "native_flash_DAT_offset": 0x25c,
        "sections": sections,
        "unmapped_BSS": [
            {"address": 0xbe0960, "size": 0x377d40},
            {"address": 0x1514160, "size": 0x473000},
        ],
        "notes": "Static map, not validated by a camera RAM dump. The decompressed section4 is 0x340560 bytes, while its inner header and section descriptor declare 0x340588; BSS begins at the decoded end 0xbe0960. Firmware checksum is not validated.",
    }, indent=2) + "\n")
    lines = []
    for section in sections:
        data = (ROOT / section["file"]).read_bytes()
        for match in re.finditer(rb"[\x20-\x7e]{4,}", data):
            lines.append(f'{section["name"]}\t0x{section["address"] + match.start():08x}\t{match.group().decode("ascii")}\n')
    (out / "strings.tsv").write_text("".join(lines))
    for section in sections:
        print(f'{section["name"]}: {section["address"]:#010x} + {section["size"]:#x}')


if __name__ == "__main__":
    main()
