#!/usr/bin/env python3
"""Registry of supplied firmware images: identity, layout and paths.

Every image has `firmware/<model>/<version>/firmware.json` (format in
firmware/README.md). Tools resolve a firmware by id (`xt1-552`) or source hash
instead of hard-coding paths. This module never touches a camera and never
executes firmware.

    firmware_registry.py list
    firmware_registry.py show ID
    firmware_registry.py scan DAT            # candidate LZ streams for a new entry
    firmware_registry.py unpack ID [--force] # regions, manifest, strings, ELF
    firmware_registry.py verify ID           # hashes of the DAT and unpacked regions

Unpacked output for registry-scanned images goes to ignored
`.tools/firmware/<id>/` and is regenerable. X70 1.10 is special: its
committed `firmware/mapped/` comes from tools/unpack_x70.py and
tools/make_elf.py (research/multi-firmware.md lists the X70 special cases).
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import re
import os
import struct
import sys
import subprocess
from uuid import uuid4

ROOT = Path(__file__).resolve().parents[1]
FIRMWARE = ROOT / "firmware"
SCHEMA = 1
# 20-byte LZ stream header: unpacked, packed, pages, page_size, method.
STREAM_MARK = b"\x00\x40\x00\x00\x04\x00\x00\x00"
INNER_MARK = 0xaaaaffff


class RegistryError(Exception):
    pass


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def file_sha256(path):
    with Path(path).open("rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()


def number(value):
    return value if value is None or isinstance(value, int) else int(value, 0)


# --- registry -------------------------------------------------------------------

def entries():
    """{id: entry} for every firmware.json, with `_dir` set to its directory."""
    out = {}
    for path in sorted(FIRMWARE.glob("*/*/firmware.json")):
        entry = json.loads(path.read_text())
        if entry.get("schema") != SCHEMA:
            raise RegistryError(f"{path}: expected schema {SCHEMA}")
        if entry["id"] in out:
            raise RegistryError(f"duplicate firmware id {entry['id']}")
        entry["_dir"] = path.parent
        out[entry["id"]] = entry
    return out


def load(firmware_id):
    registry = entries()
    if firmware_id not in registry:
        raise RegistryError(f"unknown firmware {firmware_id!r}; known: {', '.join(registry)}")
    return registry[firmware_id]


def by_sha256(digest):
    for entry in entries().values():
        if entry["source"]["sha256"] == digest:
            return entry
    raise RegistryError(f"no registered firmware has DAT SHA-256 {digest}")


def path(entry, key):
    """Repository path for one of the entry's path fields (`mapped`, `corpus`, ...)."""
    value = entry["paths"].get(key)
    if value is None:
        raise RegistryError(f"{entry['id']} has no {key} path")
    return ROOT / value


def source_path(entry):
    return ROOT / entry["source"]["path"]


def emulator_profile(entry, *, profile_file=None):
    """Resolve verified emulator data without constructing a CPU or device.

    Null entries fail; the legacy object adapts through its registry directory.
    Keep this lazy so ordinary registry commands do not import runtime models.
    """
    if str(ROOT) not in sys.path:
        sys.path.insert(0, str(ROOT))
    from emulator.boards.profile import resolve
    return resolve(entry=entry, profile_file=profile_file, root=ROOT)



def prepare_xm1_viewer(source, filename):
    """Validate a 1.50-compatible DAT and materialize a private immutable profile."""
    if not isinstance(source, bytes) or not 0 < len(source) <= 64 * 1024 * 1024:
        raise RegistryError('Firmware must be 1..64 MiB')
    validator = os.environ.get('XM1_VIEWER_VALIDATOR')
    if not validator:
        raise RegistryError('Firmware selection requires the xm1-viewer launcher validator')
    if not any(parent.name == 'private-data' for parent in ROOT.resolve().parents):
        raise RegistryError('Firmware selection requires a staged private-data runtime')
    staging = ROOT / '.tools' / 'viewer-uploads' / uuid4().hex
    staging.mkdir(parents=True, mode=0o700)
    dat = staging / 'firmware.DAT'
    dat.write_bytes(source)
    result = subprocess.run([validator, '--validate-firmware', str(dat)],
                            capture_output=True, text=True, timeout=120)
    if result.returncode:
        raise RegistryError(result.stderr.strip() or 'Firmware compatibility validation failed')
    stock = load('xm1-150')
    entry = copy.deepcopy(stock)
    inverted = invert(source)
    from tools.unpack_x70 import decompress
    for index, stream in enumerate(entry['unpack']['streams'], 4):
        row = struct.unpack_from('<9I', inverted, 0x25c + 0x140040 + index * 36)
        offset = number(stream['dat_offset'])
        window = row[6]
        declared, packed = struct.unpack_from('<2I', inverted, offset)
        shift = number(stock['unpack']['streams'][index - 4]['base']) - row[1]
        if not 20 <= packed <= window or declared != stream['decoded_size'] + shift:
            raise RegistryError(f'{stream["name"]}: unsupported compressed extent')
        decoded, _ = decompress(inverted[offset:offset + packed])
        expected = (32, number(stream['base']), number(stream['initialized_size']))
        if inner_header(decoded[shift:]) != expected:
            raise RegistryError(f'{stream["name"]}: stock stage header changed')
        words = struct.unpack_from('<8I', decoded, shift)
        expected_entry = 0x69cdf8 if index == 4 else 0xd985a4
        if words != (0, expected[1], expected[2], expected_entry, 0, 0, 0, INNER_MARK):
            raise RegistryError(f'{stream["name"]}: stage entry changed')
        stream.update(base=hex(row[1]), initialized_size=hex(row[2]), decoded_size=len(decoded),
                      packed_size=packed, header_offset=shift,
                      stage_base=stock['unpack']['streams'][index - 4]['base'],
                      stage_initialized_size=stock['unpack']['streams'][index - 4]['initialized_size'],
                      sha256=sha256(decoded))
    for spec in entry['unpack']['raw_units']:
        offset, size = number(spec['dat_offset']), number(spec['initialized_size'])
        data = inverted[offset:offset + size]
        if (inner_header(data) != (32, number(spec['base']), size) or
                struct.unpack_from('<I', data, 12)[0] != number(spec['entry_point'])):
            raise RegistryError(f'{spec["name"]}: kernel header changed')
        spec['sha256'] = sha256(data)
    overlays = entry['unpack']['overlays']
    offset, size = number(overlays['dat_offset']), number(overlays['size'])
    overlays['sha256'] = sha256(inverted[offset:offset + size])
    for spec in overlays['units']:
        offset, size = number(spec['dat_offset']), number(spec['size'])
        spec['sha256'] = sha256(inverted[offset:offset + size])
    overlay_records(entry, inverted)
    digest = sha256(source)
    candidate_id = 'xm1-150-viewer-' + digest[:16] + '-' + uuid4().hex[:12]
    directory = FIRMWARE / 'viewer-candidates' / candidate_id
    directory.mkdir(parents=True, mode=0o700)
    entry['_dir'] = directory
    entry['id'] = candidate_id
    entry['source'] = dict(path=str(dat.relative_to(ROOT)), filename=filename,
                           size=len(source), sha256=digest, outer_type=4)
    entry['paths'] = dict(mapped=f'.tools/firmware/{candidate_id}')
    profile_path = directory / 'emulator.json'
    entry['emulator'] = str(profile_path.relative_to(ROOT))
    unpack(entry)
    template = json.loads((ROOT / stock['emulator']).read_text())
    from emulator.boards.profile import checked_bytes, resolve
    firmware = json.loads(checked_bytes(ROOT, template['firmware']))
    firmware['firmware_id'] = candidate_id
    firmware['source_sha256'] = digest
    firmware['manifest'] = dict(path=str((path(entry, 'mapped') / 'manifest.json').relative_to(ROOT)),
                                sha256=file_sha256(path(entry, 'mapped') / 'manifest.json'))
    for item in firmware['boot_layout']['units']:
        manifest = unit_path(entry, 'mapped', item['unit']) / 'manifest.json'
        item['manifest'] = dict(path=str(manifest.relative_to(ROOT)), sha256=file_sha256(manifest))
    for item in firmware['boot_layout']['flash_slices']:
        offset = number(firmware['flash_dat_bias']) + number(item['offset'])
        size = number(item['size'])
        item['sha256'] = sha256(inverted[offset:offset + size])
        item['file'] = f'.tools/firmware/{candidate_id}/emulator/{item["name"]}.bin'
    views = json.loads(checked_bytes(ROOT, firmware['control_bus_views']))
    views['firmware_id'] = candidate_id
    views['source_sha256'] = digest
    views_path = directory / 'control-bus.json'
    views_path.write_text(json.dumps(views, indent=2) + '\n')
    firmware['control_bus_views'] = dict(path=str(views_path.relative_to(ROOT)),
                                        sha256=file_sha256(views_path))
    firmware_path = directory / 'boot.json'
    firmware_path.write_text(json.dumps(firmware, indent=2) + '\n')
    template['firmware_id'] = candidate_id
    template['firmware'] = dict(path=str(firmware_path.relative_to(ROOT)),
                                sha256=file_sha256(firmware_path))
    profile_path.write_text(json.dumps(template, indent=2) + '\n')
    (directory / 'firmware.json').write_text(json.dumps(
        {key: value for key, value in entry.items() if key != '_dir'}, indent=2) + '\n')
    profile = resolve(entry=entry, profile_file=entry['emulator'])
    identity = dict(filename=filename, sha256=digest, model='X-M1', version='1.50',
                    modified=digest != stock['source']['sha256'], can_replace=True)
    return profile, identity

def unit_names(entry):
    """Independent mapped units. Overlays may reuse addresses across units."""
    names = ["main"] + [s["name"] for s in entry["unpack"].get("overlays", {}).get("units", [])]
    names += [s['name'] for s in entry['unpack'].get('raw_units', [])]
    if (any(not isinstance(name, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", name)
            for name in names) or len(names) != len(set(names))):
        raise RegistryError(f"{entry['id']}: mapped units require unique safe names")
    return names


def unit_path(entry, key, unit="main"):
    if unit not in unit_names(entry):
        raise RegistryError(f"{entry['id']}: unknown unit {unit!r}")
    root = path(entry, key)
    if unit in {s['name'] for s in entry['unpack'].get('raw_units', [])}:
        return root / 'raw' / unit
    return root if unit == "main" else root / "overlays" / unit


def mapped_manifest(entry, verify=True, unit="main"):
    """The section manifest (firmware/mapped/manifest.json schema) for an entry.
    Sections' `file` fields are repository-relative."""
    manifest_path = unit_path(entry, "mapped", unit) / "manifest.json"
    if not manifest_path.is_file():
        hint = ("run tools/unpack_x70.py" if entry["unpack"]["method"] == "x70-flash-map"
                else f"run tools/firmware_registry.py unpack {entry['id']}")
        raise RegistryError(f"{manifest_path} is missing; {hint}")
    manifest = json.loads(manifest_path.read_text())
    if manifest.get("unit", "main") != unit:
        raise RegistryError(f"{manifest_path}: wrong mapped unit")
    if manifest["source_sha256"] != entry["source"]["sha256"]:
        raise RegistryError(f"{manifest_path} belongs to {manifest['source_sha256']}, "
                            f"not {entry['id']}")
    if verify:
        for section in manifest["sections"]:
            if file_sha256(ROOT / section["file"]) != section["sha256"]:
                raise RegistryError(f"{section['file']} does not match its manifest hash")
    return manifest


def aliases(entry):
    """Address offsets at which the mapped sections are also visible (0 = itself)."""
    alias = number(entry["cpu"].get("uncached_alias"))
    return [0] + ([alias] if alias else [])


def regions(entry, unit="main"):
    """[(base, bytes)] for every executable mapped section, sorted by base."""
    manifest = mapped_manifest(entry, unit=unit)
    return sorted((s["address"], (ROOT / s["file"]).read_bytes())
                  for s in manifest["sections"] if s.get("executable", True))


def unique_regions(entry):
    """Explicit compatibility view; refuse overlapping banks instead of picking one."""
    result = sorted(r for unit in unit_names(entry) for r in regions(entry, unit))
    for (base, data), (following, _) in zip(result, result[1:]):
        if base + len(data) > following:
            raise RegistryError(f"{entry['id']}: overlapping units require a unit selector")
    return result


# --- unpacking --------------------------------------------------------------------

def invert(data):
    return data.translate(bytes(255 - n for n in range(256)))


def inner_header(decoded):
    """(word bits, base, initialized size) of a mapped stream, or None."""
    if len(decoded) >= 32:
        words = struct.unpack_from("<8I", decoded)
        if words[0] == 0 and words[7] == INNER_MARK:
            return 32, words[1], words[2]
    if len(decoded) >= 64:
        words = struct.unpack_from("<8Q", decoded)
        if words[0] == 0 and words[7] == INNER_MARK:
            return 64, words[1], words[2]
    return None


def raw_records(entry, inverted):
    """Verify complete raw initialized images against their own inner headers.

    Header identity, DAT extent, initialized bytes and entry all have to agree;
    a matching function byte-search hit cannot supply a run address.
    """
    unit_names(entry)
    records = []
    for spec in entry['unpack'].get('raw_units', []):
        if spec.get('method') != 'inner-header':
            raise RegistryError('unsupported raw unit method')
        offset, size = number(spec['dat_offset']), number(spec['initialized_size'])
        if offset < 0 or size < 32 or offset+size > len(inverted):
            raise RegistryError(f"{spec['name']}: raw initialized extent outside DAT")
        data = inverted[offset:offset+size]
        expected = (number(spec['header_word_bits']), number(spec['base']), size)
        if inner_header(data) != expected:
            raise RegistryError(f"{spec['name']}: raw inner header disagrees with registry")
        bits, base, _ = expected
        words = struct.unpack_from('<8I' if bits == 32 else '<8Q', data)
        if (bits == 32) != (entry['cpu']['arch'] == 'arm32'):
            raise RegistryError('raw header architecture disagrees with firmware')
        if words[3] != number(spec['entry_point']) or not base <= words[3] < base+size:
            raise RegistryError(f"{spec['name']}: raw entry disagrees with initialized mapping")
        if sha256(data) != spec['sha256']:
            raise RegistryError(f"{spec['name']}: raw initialized bytes hash mismatch")
        records.append(dict(spec, address=base, size=size, dat_offset=offset,
                            entry_point=words[3], data=data))
    return records


def resident_raw_records(entry, inverted):
    """Hash-pinned raw ranges included alongside resident compressed streams.

    An inner-header range additionally verifies base, initialized size and entry.
    Extent-only ranges require explicit mappings: no byte search infers addresses.
    """
    records = []
    names = {s['name'] for s in entry['unpack'].get('streams', [])}
    for spec in entry['unpack'].get('raw_regions', []):
        name = spec['name']
        if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]*', name):
            raise RegistryError('raw region requires a safe name')
        if name in names:
            raise RegistryError(f'{name}: duplicate resident region name')
        names.add(name)
        offset, size, base = (number(spec[k]) for k in ('dat_offset', 'size', 'base'))
        if base < 0 or base + size > 1 << 64:
            raise RegistryError(f'{name}: raw mapping outside 64-bit address space')
        if offset < 0 or size <= 0 or offset + size > len(inverted):
            raise RegistryError(f'{name}: raw extent outside DAT')
        data = inverted[offset:offset + size]
        if sha256(data) != spec['sha256']:
            raise RegistryError(f'{name}: raw bytes hash mismatch')
        method = spec.get('method')
        if method == 'inner-header':
            bits = number(spec['header_word_bits'])
            if inner_header(data) != (bits, base, size):
                raise RegistryError(f'{name}: raw inner header disagrees with registry')
            if (bits == 32) != (entry['cpu']['arch'] == 'arm32'):
                raise RegistryError(f'{name}: raw header architecture disagrees with firmware')
            words = struct.unpack_from('<8I' if bits == 32 else '<8Q', data)
            if words[3] != number(spec['entry_point']) or not base <= words[3] < base + size:
                raise RegistryError(f'{name}: raw entry outside initialized mapping')
        elif method != 'extent':
            raise RegistryError(f'{name}: unsupported raw region method')
        records.append(dict(spec, address=base, size=size, dat_offset=offset, data=data))
    return records


def write_raw_units(entry, inverted, selected=None, force=False):
    records = raw_records(entry, inverted)
    if selected is not None and selected not in {r['name'] for r in records}:
        raise RegistryError(f'{selected}: --unit unpack requires a registered raw initialized unit')
    manifests = []
    for record in records:
        unit = record['name']
        if selected is not None and selected != unit:
            continue
        out = unit_path(entry, 'mapped', unit)
        if (out/'manifest.json').exists() and not force:
            raise RegistryError(f'{out} exists; pass --force to rewrite it')
        if not out.resolve().is_relative_to((ROOT/'.tools').resolve()):
            raise RegistryError('raw unit output must be under ignored .tools/')
        out.mkdir(parents=True, exist_ok=True)
        data = record['data']
        binary = out/f'{unit}.bin'; binary.write_bytes(data)
        section = dict(name=unit, address=record['address'], size=record['size'],
                       file=str(binary.relative_to(ROOT)), sha256=sha256(data),
                       DAT_offset=record['dat_offset'], executable=True, compression='raw inner-header image')
        image = out/f"{entry['id']}-{unit}.elf"
        image.write_bytes(elf(entry['cpu']['arch'], record['entry_point'], [(record['address'], data, len(data), 7)]))
        manifest = dict(firmware_id=entry['id'], unit=unit, unit_kind='raw-initialized',
                        source_sha256=entry['source']['sha256'], arch=entry['cpu']['arch'],
                        aliases=aliases(entry), elf=str(image.relative_to(ROOT)), sections=[section], unmapped_BSS=[],
                        notes='Static raw inner-header mapping; entry and complete initialized bytes verified. No camera behavior or BSS claim.')
        (out/'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n')
        (out/'strings.tsv').write_text(''.join(f"{unit}\t0x{record['address']+m.start():08x}\t{m.group().decode('ascii')}\n"
                                             for m in re.finditer(rb'[\x20-\x7e]{4,}', data)))
        manifests.append(manifest)
    return manifests


def overlay_records(entry, inverted):
    """Decode the loader's bounded 9-word section / 6-word overlay tables.

    DAT bias is checked independently against every declared compressed stream.
    All pointers must stay inside the pinned descriptor block; payloads must
    stay inside raw section 6. No X70 address or byte pattern is assumed.
    """
    spec = entry["unpack"].get("overlays")
    if not spec:
        return []
    if spec["method"] != "section-table-32":
        raise RegistryError("unknown overlay table format")
    offset, base, size = (number(spec[k]) for k in ("dat_offset", "base", "size"))
    table = inverted[offset:offset + size]
    if len(table) != size or sha256(table) != spec["sha256"]:
        raise RegistryError("overlay descriptor hash/size mismatch")
    alias = number(entry["cpu"].get("uncached_alias")) or 0

    def words(address, count):
        if alias and address >= alias:
            address -= alias
        index = address - base
        if index < 0 or index % 4 or index + count * 4 > size:
            raise RegistryError("overlay descriptor pointer outside pinned table")
        return struct.unpack_from(f"<{count}I", table, index)

    section_pointer, group_pointer = words(base, 2)
    sections = {}
    for i in range(size // 36 + 1):
        address = section_pointer + i * 36
        if words(address, 1)[0] == 0xffffffff:
            break
        row = words(address, 9)
        if row[0] in sections:
            raise RegistryError("duplicate section id")
        sections[row[0]] = row
    bias = number(spec["flash_dat_bias"])
    for stream in entry["unpack"]["streams"]:
        if not stream.get("executable"):
            continue
        matches = [r for r in sections.values() if r[1] == number(stream["base"]) and r[8] == 1]
        if len(matches) != 1 or number(stream["dat_offset"]) - matches[0][5] != bias:
            raise RegistryError("overlay flash/DAT bias disagrees with compressed stream")
    section = sections.get(number(spec["section_id"]))
    if section is None or section[8] != 0:
        raise RegistryError("overlay section is absent or compressed")
    entries_pointer, group_base, group_size = words(group_pointer, 3)
    if group_size != section[6] or group_base % 0x20000 or section[5] % 0x20000:
        raise RegistryError("overlay group disagrees with raw flash section")
    records = []
    for i in range(size // 24 + 1):
        address = entries_pointer + i * 24
        if words(address, 1)[0] == 0xffffffff:
            break
        logical_id, run, length, reserved, flags, semaphore = words(address, 6)
        dat = bias + section[5] + run - group_base
        if (not length or reserved or flags or run < group_base or
                run + length > group_base + group_size or dat < 0 or dat + length > len(inverted)):
            raise RegistryError("overlay entry outside raw flash group or unsupported flags")
        data = inverted[dat:dat + length]
        records.append(dict(name=f"overlay{semaphore:02x}", logical_id=logical_id,
                            semaphore=semaphore, address=run, size=length,
                            dat_offset=dat, sha256=sha256(data)))
    expected = [{k: number(v) if k in ("logical_id", "semaphore", "address", "size", "dat_offset") else v
                 for k, v in row.items()} for row in spec["units"]]
    if records != expected:
        raise RegistryError("overlay table/payload disagrees with registered units")
    return records


def scan(source):
    """Candidate LZ streams in an inverted DAT, each decoded and described."""
    from unpack_x70 import decompress
    inverted = invert(source)
    found, position = [], 0
    while (match := inverted.find(STREAM_MARK, position)) >= 0:
        position = match + 1
        offset = match - 12
        if offset < 0:
            continue
        _, packed, pages, page_size, _ = struct.unpack_from("<5I", inverted, offset)
        if not (20 <= packed <= len(inverted) - offset and
                pages == (packed + page_size - 1) // page_size):
            continue
        try:
            decoded, header = decompress(inverted[offset:])
        except ValueError:
            continue
        inner = inner_header(decoded)
        stream = dict(name=f"stream-{offset:08x}", dat_offset=f"{offset:#x}",
                      packed_size=packed, decoded_size=len(decoded), sha256=sha256(decoded))
        if inner:
            bits, base, initialized = inner
            stream.update(header_word_bits=bits, base=f"{base:#x}",
                          initialized_size=f"{initialized:#x}", executable=True)
        found.append(stream)
        position = offset + packed  # compressed bytes cannot hold another header
    return found


def elf(arch, entry_point, segments):
    """Minimal ELF for static Ghidra import; not a bootable camera image.
    segments: [(base, data, memory size, flags)]."""
    wide = arch == "aarch64"
    if wide:
        header_size, ph_size = 64, 56
    elif arch == "arm32":
        header_size, ph_size = 52, 32
    else:
        raise RegistryError(f"no ELF writer for {arch}")
    position = (header_size + ph_size * len(segments) + 0xfff) & ~0xfff
    if wide:
        header = struct.pack("<16sHHIQQQIHHHHHH", b"\x7fELF\x02\x01\x01" + bytes(9),
                             2, 183, 1, entry_point, header_size, 0, 0,
                             header_size, ph_size, len(segments), 0, 0, 0)
    else:
        header = struct.pack("<16sHHIIIIIHHHHHH", b"\x7fELF\x01\x01\x01" + bytes(9),
                             2, 40, 1, entry_point, header_size, 0, 0x05000000,
                             header_size, ph_size, len(segments), 0, 0, 0)
    body = bytearray(position - header_size - ph_size * len(segments))
    headers = bytearray()
    for base, data, size, flags in segments:
        if wide:
            headers += struct.pack("<IIQQQQQQ", 1, flags, position, base, base,
                                   len(data), size, 0x1000)
        else:
            headers += struct.pack("<8I", 1, position, base, base, len(data), size, flags, 0x1000)
        body += data
        position += len(data)
        padding = -position & 0xfff
        body += bytes(padding)
        position += padding
    return header + headers + body


def unpack(entry, force=False, unit=None):
    """Decode every declared stream, verify it, and write regions, manifest,
    strings.tsv and an analysis ELF to the entry's mapped directory."""
    if entry['unpack']['method'] == 'ricoh-gr-lz':
        if unit is not None:
            raise RegistryError('Ricoh decoder currently exposes only the resident analysis unit')
        from unpack_ricoh import unpack as unpack_ricoh, FormatError
        try:
            return unpack_ricoh(entry, force=force)
        except FormatError as error:
            raise RegistryError(str(error)) from error
    from unpack_x70 import decompress
    if entry["unpack"]["method"] == "x70-flash-map":
        # X70 special case: firmware/mapped is committed and has its own tools.
        raise RegistryError("X70 1.10 is unpacked by tools/unpack_x70.py and tools/make_elf.py")
    if entry["unpack"]["method"] != "inner-header-scan":
        raise RegistryError(f"unknown unpack method {entry['unpack']['method']}")
    out = path(entry, "mapped")
    if not out.resolve().is_relative_to((ROOT / ".tools").resolve()):
        raise RegistryError("unpacked output must be under ignored .tools/")
    if unit is None and (out / "manifest.json").exists() and not force:
        raise RegistryError(f"{out} exists; pass --force to rewrite it")
    source = source_path(entry).read_bytes()
    if sha256(source) != entry["source"]["sha256"] or len(source) != entry["source"]["size"]:
        raise RegistryError(f"{entry['source']['path']} does not match the registry")
    out.mkdir(parents=True, exist_ok=True)
    inverted = invert(source)
    if unit is not None:
        return write_raw_units(entry, inverted, selected=unit, force=force)[0]
    sections, strings, segments = [], [], []
    for stream in entry["unpack"]["streams"]:
        decoded, header = decompress(inverted[number(stream["dat_offset"]):])
        if sha256(decoded) != stream["sha256"]:
            raise RegistryError(f"{stream['name']}: decoded hash mismatch")
        relative = (out / f"{stream['name']}.bin").relative_to(ROOT)
        if not stream.get("executable"):
            (ROOT / relative).write_bytes(decoded)
            continue
        header_offset = number(stream.get('header_offset', 0))
        bits, base, initialized = inner_header(decoded[header_offset:])
        expected = (number(stream.get('stage_base', stream['base'])),
                    number(stream.get('stage_initialized_size', stream['initialized_size'])))
        if (base, initialized) != expected:
            raise RegistryError(f"{stream['name']}: inner header disagrees with the registry")
        base, initialized = number(stream['base']), number(stream['initialized_size'])
        # Only bytes present inside the declared initialized extent are mapped;
        # several streams decode shorter or longer than that extent.
        data = decoded[:initialized]
        (ROOT / relative).write_bytes(data)
        sections.append(dict(name=stream["name"], file=str(relative), address=base,
                             size=len(data), DAT_offset=number(stream["dat_offset"]),
                             sha256=sha256(data), decoded_size=len(decoded),
                             initialized_size=initialized, compression=header))
        segments.append((base, data, len(data), 7))
        for m in re.finditer(rb"[\x20-\x7e]{4,}", data):
            strings.append(f"{stream['name']}\t0x{base + m.start():08x}\t{m.group().decode('ascii')}\n")
    for record in resident_raw_records(entry, inverted):
        name, base, data = record['name'], record['address'], record['data']
        relative = (out / f'{name}.bin').relative_to(ROOT)
        (ROOT / relative).write_bytes(data)
        executable = record.get('executable', True)
        sections.append(dict(name=name, file=str(relative), address=base, size=len(data),
                             DAT_offset=record['dat_offset'], sha256=sha256(data),
                             executable=executable, compression='raw XOR-DAT bytes'))
        segments.append((base, data, len(data), 7 if executable else 6))
        strings.extend(f"{name}\t0x{base + m.start():08x}\t{m.group().decode('ascii')}\n"
                       for m in re.finditer(rb'[\x20-\x7e]{4,}', data))
    ordered = sorted(segments, key=lambda s: s[0])
    for current, following in zip(ordered, ordered[1:]):
        if current[0] + current[2] > following[0]:
            raise RegistryError('resident raw/stream mappings overlap')
    segments.sort(key=lambda s: s[0])
    arch = entry["cpu"]["arch"]
    entry_point = number(entry["unpack"].get("entry_point")) or segments[0][0]
    elf_name = f"{entry['id']}.elf"
    (out / elf_name).write_bytes(elf(arch, entry_point, segments))
    (out / "strings.tsv").write_text("".join(strings))
    manifest = dict(firmware_id=entry["id"], source_sha256=entry["source"]["sha256"],
                    arch=arch, aliases=aliases(entry), elf=str((out / elf_name).relative_to(ROOT)),
                    sections=sections, unmapped_BSS=[],
                    notes="Static inner-header map from firmware_registry.py; not validated "
                          "by a camera RAM capture. BSS and update checksums are not modelled.")
    (out / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    records = overlay_records(entry, inverted)
    if records:
        spec = entry["unpack"]["overlays"]
        table = inverted[number(spec["dat_offset"]):number(spec["dat_offset"]) + number(spec["size"])]
        table_file = out / "overlay-descriptors.bin"
        table_file.write_bytes(table)
        descriptor = dict(name="overlay-descriptors", file=str(table_file.relative_to(ROOT)),
                          address=number(spec["base"]), size=len(table),
                          DAT_offset=number(spec["dat_offset"]), sha256=sha256(table), executable=False)
        for record in records:
            unit = record["name"]
            unit_dir = unit_path(entry, "mapped", unit)
            unit_dir.mkdir(parents=True, exist_ok=True)
            data = inverted[record["dat_offset"]:record["dat_offset"] + record["size"]]
            binary = unit_dir / f"{unit}.bin"
            binary.write_bytes(data)
            section = dict(record, file=str(binary.relative_to(ROOT)), executable=True,
                           DAT_offset=record["dat_offset"], compression="raw section bytes")
            unit_elf = unit_dir / f"{entry['id']}-{unit}.elf"
            unit_elf.write_bytes(elf(arch, record["address"], [
                (descriptor["address"], table, len(table), 6),
                (record["address"], data, len(data), 7)]))
            unit_manifest = dict(firmware_id=entry["id"], unit=unit,
                                 source_sha256=entry["source"]["sha256"], arch=arch,
                                 aliases=aliases(entry), elf=str(unit_elf.relative_to(ROOT)),
                                 sections=[descriptor, section], unmapped_BSS=[],
                                 notes="Independent raw overlay unit from verified loader descriptors; "
                                       "not a camera RAM observation. Other overlay banks are not mapped.")
            (unit_dir / "manifest.json").write_text(json.dumps(unit_manifest, indent=2) + "\n")
            (unit_dir / "strings.tsv").write_text("".join(
                f"{unit}\t0x{record['address'] + m.start():08x}\t{m.group().decode('ascii')}\n"
                for m in re.finditer(rb"[\x20-\x7e]{4,}", data)))
    write_raw_units(entry, inverted, force=force)
    return manifest


def verify(entry):
    report = dict(id=entry["id"], source=entry["source"]["path"])
    source = source_path(entry)
    report["source_ok"] = source.is_file() and file_sha256(source) == entry["source"]["sha256"]
    try:
        manifest = mapped_manifest(entry)
        report["sections"] = len(manifest["sections"])
        report["mapped_ok"] = True
        report["units"] = {}
        for unit in unit_names(entry)[1:]:
            report["units"][unit] = len(mapped_manifest(entry, unit=unit)["sections"])
    except RegistryError as error:
        report["mapped_ok"] = False
        report["mapped_error"] = str(error)
    for key in ("corpus", "ledger"):
        if entry["paths"].get(key):
            report[f"{key}_present"] = path(entry, key).exists()
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("list")
    for name in ("show", "verify"):
        commands.add_parser(name).add_argument("id")
    scan_parser = commands.add_parser("scan")
    scan_parser.add_argument("dat", type=Path)
    unpack_parser = commands.add_parser("unpack")
    unpack_parser.add_argument("id")
    unpack_parser.add_argument("--force", action="store_true")
    unpack_parser.add_argument('--unit', help='unpack one registered raw initialized unit without rewriting resident/overlay outputs')
    args = parser.parse_args()
    try:
        if args.command == "list":
            for entry in entries().values():
                print(f"{entry['id']:12} {entry['model']:8} {entry['version']:6} "
                      f"{entry['cpu']['arch']:8} {entry['generation']['processor']}")
        elif args.command == "show":
            entry = load(args.id)
            print(json.dumps({k: v for k, v in entry.items() if not k.startswith("_")}, indent=2))
        elif args.command == "scan":
            print(json.dumps(scan(args.dat.read_bytes()), indent=2))
        elif args.command == "unpack":
            manifest = unpack(load(args.id), args.force, args.unit)
            for section in manifest["sections"]:
                print(f"{section['name']}: {section['address']:#010x} + {section['size']:#x}")
            print(manifest["elf"])
        elif args.command == "verify":
            report = verify(load(args.id))
            print(json.dumps(report, indent=2))
            if not report["source_ok"]:
                return 1
    except RegistryError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
