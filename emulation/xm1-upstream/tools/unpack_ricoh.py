#!/usr/bin/env python3
"""Strict, offline decoder for the registered Ricoh GR III framed LZ image.

Format reference: yeahnope/gr_unpack at 25f0cd513876655f355e7587302a5dd7b9ee6620
(https://github.com/yeahnope/gr_unpack). This is an independent implementation
with bounded reads/backreferences and verified footer lengths and word sums.
No firmware execution, camera access, recovery guesses or update-image writer.
"""
import hashlib
import json
import re
import struct


class FormatError(ValueError):
    pass


def word_sum(data):
    if len(data) % 4:
        raise FormatError('word-sum input is not aligned')
    return sum(x[0] for x in struct.iter_unpack('<I', data)) & 0xffffffff


def decode_frames(data, *, start=0x80, max_output=256 * 1024 * 1024):
    """Return (decoded bytes, position after zero frame, frame count)."""
    position, frames, output = start, 0, bytearray()

    def take(count, limit):
        nonlocal position
        if position + count > limit:
            raise FormatError(f'truncated token at {position:#x}')
        result = data[position:position + count]
        position += count
        return result

    while True:
        prefix = int.from_bytes(take(2, len(data)), 'big')
        if prefix == 0:
            return bytes(output), position, frames
        size = prefix & 0x7fff
        end = position + size
        if size == 0 or end > len(data):
            raise FormatError('invalid frame extent')
        frames += 1
        if prefix & 0x8000:
            if len(output) + size > max_output:
                raise FormatError('decoded size exceeds limit')
            output.extend(take(size, end))
            continue
        while position < end:
            flags = int.from_bytes(take(2, end), 'big')
            for bit in range(15, -1, -1):
                if position == end:
                    break
                if not (flags >> bit) & 1:
                    if len(output) == max_output:
                        raise FormatError('decoded size exceeds limit')
                    output.extend(take(1, end))
                    continue
                first, second = take(2, end)
                distance = ((first & 0xf8) << 5) | second
                length = first & 7
                if length == 7:
                    extra = take(1, end)[0]
                    length += extra
                    if extra == 255:
                        while extra == 255:
                            extra = take(1, end)[0]
                            length += extra
                if distance == 0:
                    if position != end:
                        raise FormatError('frame terminator before declared end')
                    break
                length += 3
                if distance > len(output):
                    raise FormatError('backreference precedes decoded image')
                if len(output) + length > max_output:
                    raise FormatError('decoded size exceeds limit')
                # Preserve overlapping-copy semantics without copying byte by byte.
                while length:
                    count = min(length, distance)
                    index = len(output) - distance
                    output.extend(output[index:index + count])
                    length -= count


def decode(data):
    if len(data) < 0x98 or data[:8] != b'RICOH\0\0\0':
        raise FormatError('not a Ricoh GR framed update')
    if struct.unpack_from('<I', data, 0x7c)[0] != 0xa55a5aa5:
        raise FormatError('header marker mismatch')
    if word_sum(data):
        raise FormatError('packed word sum is not zero')
    output, position, frames = decode_frames(data)
    footer_offset = (position + 3) & ~3
    if any(data[position:footer_offset]) or len(data) != footer_offset + 24:
        raise FormatError('unexpected trailing data')
    model, marker, kind, packed_size, decoded_size, checksum = struct.unpack_from('<6I', data, footer_offset)
    model_id = struct.unpack_from('<I', data, 0x30)[0]
    if model != model_id | (model_id << 16) or marker != 0xa55a5aa5 or kind != 2:
        raise FormatError('footer marker/model mismatch')
    if packed_size != position - 0x80 or decoded_size != len(output):
        raise FormatError('footer size mismatch')
    if word_sum(output):
        raise FormatError('decoded word sum is not zero')
    return output, dict(frames=frames, footer_offset=hex(footer_offset),
                       packed_size=packed_size, decoded_size=decoded_size,
                       packed_word_sum='0x00000000', decoded_word_sum='0x00000000',
                       decoded_sha256=hashlib.sha256(output).hexdigest())


def unpack(entry, force=False):
    import firmware_registry as reg
    out = reg.path(entry, 'mapped')
    if not out.resolve().is_relative_to((reg.ROOT / '.tools').resolve()):
        raise reg.RegistryError('Ricoh output must be under ignored .tools/')
    if (out / 'manifest.json').exists() and not force:
        raise reg.RegistryError(f'{out} exists; pass --force to rewrite it')
    source = reg.source_path(entry).read_bytes()
    if len(source) != entry['source']['size'] or reg.sha256(source) != entry['source']['sha256']:
        raise reg.RegistryError('source size/hash mismatch')
    decoded, report = decode(source)
    if report['decoded_sha256'] != entry['unpack']['decoded_sha256']:
        raise reg.RegistryError('decoded image hash mismatch')
    sections, segments, strings = [], [], []
    # Validate all regions before creating output.
    for spec in entry['unpack']['regions']:
        offset, size, base = (reg.number(spec[k]) for k in ('decoded_offset', 'size', 'base'))
        data = decoded[offset:offset + size]
        if offset < 0 or size <= 0 or len(data) != size or reg.sha256(data) != spec['sha256']:
            raise reg.RegistryError('region extent/hash mismatch')
        filename = out / (spec['name'] + '.bin')
        sections.append(dict(name=spec['name'], file=str(filename.relative_to(reg.ROOT)),
                             address=base, size=size, decoded_offset=offset,
                             sha256=spec['sha256'], executable=spec.get('executable', True)))
        segments.append((base, data, len(data), 7 if spec.get('executable', True) else 4))
        strings.extend(f"{spec['name']}\t0x{base + m.start():08x}\t{m.group().decode('ascii')}\n"
                       for m in re.finditer(rb'[\x20-\x7e]{4,}', data))
    out.mkdir(parents=True, exist_ok=True)
    (out / 'decoded.bin').write_bytes(decoded)
    for section, (_, data, _, _) in zip(sections, segments):
        (reg.ROOT / section['file']).write_bytes(data)
    image = out / (entry['id'] + '.elf')
    image.write_bytes(reg.elf(entry['cpu']['arch'], reg.number(entry['unpack']['analysis_entry']), segments))
    manifest = dict(firmware_id=entry['id'], source_sha256=entry['source']['sha256'],
                    arch=entry['cpu']['arch'], aliases=reg.aliases(entry),
                    elf=str(image.relative_to(reg.ROOT)), sections=sections, unmapped_BSS=[],
                    decoder=report, notes=entry['unpack']['limits'])
    (out / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    (out / 'strings.tsv').write_text(''.join(strings))
    return manifest
