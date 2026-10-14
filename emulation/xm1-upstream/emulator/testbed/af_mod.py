"""Verified runtime-manifest adapter for the single AF C mod; no SD I/O."""
from dataclasses import asdict
import hashlib
import json
from pathlib import Path
import struct


def number(value):
    return int(value, 0) if isinstance(value, str) else int(value)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def provenance(bench):
    """Effective configuration and explicit virtual-model assumptions per result."""
    from .inputs import build_stub
    scanner = bench.tb.stub.handle
    code = build_stub() if scanner is not None else b''
    support = dict(lens_preparation=getattr(bench, '_helper_receipt', None),
                   scanner=None if scanner is None else dict(base=scanner.base,
                       reserve=scanner.reserve, code_bytes=len(code), code_sha256=digest(code),
                       word_swaps=scanner.swaps, active=scanner.active,
                       purpose='existing key-scanner queue: debug key and stock host calls'))
    return dict(firmware_sha256=bench.board.manifest['source_sha256'],
                testbed_support=support,
                mods=getattr(bench, 'mods', {}),
                attempted_mod=getattr(bench, 'mod_attempt', None),
                timing_profile=asdict(bench.timing) if bench.timing else None,
                assumptions=dict(cpu='Cortex-R5F proxy for camera Cortex-R4',
                                 instructions_per_tick=bench.instructions_per_tick,
                                 virtual_ticks_per_second=12_000_000,
                                 changed_snapshot_models=getattr(bench.board, 'stream_changed_models', []),
                                 sensor=bench.stream.summary()['input'],
                                 stream_options=dict(fps=bench.stream.fps, time_scale=bench.stream.time_scale,
                                                     exposure_ticks=bench.stream.exposure_ticks,
                                                     exposure_samples=bench.stream.exposure_samples),
                                 pdaf=bench.pdaf.summary(),
                                 pdaf_timing=dict(dma_ticks=bench.pdaf.raw.dma_ticks,
                                                  mode_change_frames=bench.pdaf.raw.mode_change_frames,
                                                  extract_ticks=bench.pdaf.extractor.data_ticks,
                                                  prefilter_ticks=bench.pdaf.prefilter.data_ticks,
                                                  prefilter_stop_ticks=bench.pdaf.prefilter.stop_ticks),
                                 lens=bench.lens.requests['focus'].get('params'),
                                 scope='Declared sensor/PDAF/lens models; not physical camera equivalence'))


def install_mod(bench, build, config=None):
    """Install runtime manifest/regions after preflight, call af_setup, then hook.

    Config is a partial dict; defaults/schema are read from the built C image.
    Neither the runtime INIT nor camera settings registration runs on AFBench.
    """
    from .afbench import BenchError
    path = Path(build)
    path = path / 'manifest.json' if path.is_dir() else path
    raw = path.read_bytes()
    manifest = json.loads(raw)
    if manifest.get('mode') != 'runtime' or not manifest.get('live_safe'):
        raise BenchError('AFBench requires a live_safe --mode runtime manifest')
    if [m['name'] for m in manifest['mods']] != ['af']:
        raise BenchError('AFBench configuration ABI requires the single af mod')
    if manifest['firmware_sha256'] != bench.board.manifest['source_sha256']:
        raise BenchError('Mod firmware hash differs from bench firmware')
    if getattr(bench, 'mods', {}):
        raise BenchError('An AF mod is already installed')
    regions = []
    for region in manifest['regions']:
        data = (path.parent / region['file']).read_bytes()
        base, size = number(region['address']), region['size']
        if base & 3 or base < 0 or base + size > len(bench.board.ram) or not data:
            raise BenchError('Invalid mod region range')
        if len(data) != size or digest(data) != region['sha256']:
            raise BenchError('Mod region size/hash differs from manifest')
        if any(base < other + len(blob) and other < base + size for other, blob in regions):
            raise BenchError('Overlapping mod regions')
        if bytes(bench.board.read_memory(base, size)) != bytes(size):
            raise BenchError(f'Mod region {base:#x} is not unused (nonzero)')
        regions.append((base, data))
    words = []
    for write in manifest['words']:
        address, old, new = (number(write[k]) for k in ('address', 'old', 'new'))
        if address & 3 or any(address == a for a, _, _ in words):
            raise BenchError('Unaligned or duplicate mod word')
        if any(base <= address < base + len(data) for base, data in regions):
            raise BenchError('Mod word overlaps region')
        if bench._word(address) != old:
            raise BenchError(f'Mod word {address:#x} differs from expected stock {old:#x}')
        words.append((address, old, new))
    symbols = {s['name']: s for s in manifest['symbols']}

    def address(name):
        return number(symbols[name]['address'])

    def image_bytes(guest, size):
        guest &= ~0x40000000
        for base, data in regions:
            if base <= guest and guest + size <= base + len(data):
                return data[guest - base:guest - base + size]
        raise BenchError('AF ABI symbol lies outside mod regions')

    # The manifest's ABI addresses must refer to the verified payload, too:
    # candidate/status writes and the setup call cannot target arbitrary RAM.
    for name in ('af_schema', 'af_candidate', 'af_effective', 'af_status', 'af_error_key', 'af_setup'):
        size = symbols[name]['size']
        if size <= 0 or address(name) & 3:
            raise BenchError('Invalid AF ABI symbol range')
        image_bytes(address(name), size)
    candidate_size = symbols['af_candidate']['size']
    if candidate_size != symbols['af_effective']['size'] or candidate_size % 4:
        raise BenchError('Unsupported AF configuration layout')
    if any(symbols[name]['size'] != 4 for name in ('af_status', 'af_error_key')):
        raise BenchError('Unsupported AF status layout')
    schema = []
    schema_size = symbols['af_schema']['size']
    if schema_size % 24:
        raise BenchError('Unsupported AF schema layout')
    for offset in range(0, schema_size, 24):
        name, low, high, default, field, rule = struct.unpack('<IiiiIi', image_bytes(address('af_schema') + offset, 24))
        text = bytearray()
        while (char := image_bytes(name + len(text), 1)) != b'\0':
            text.extend(char)
            if len(text) > 128:
                raise BenchError('Invalid AF schema key')
        schema.append(dict(name=text.decode('ascii'), default=default, offset=field))
    if (len({k['name'] for k in schema}) != len(schema) or
            sorted(k['offset'] for k in schema) != list(range(0, candidate_size, 4))):
        raise BenchError('Unsupported AF schema fields')
    requested = dict(config or {})
    bench.mod_attempt = dict(manifest_sha256=digest(raw), firmware_sha256=manifest['firmware_sha256'],
                             requested_config=requested, defaults={k['name']: k['default'] for k in schema})
    unknown = requested.keys() - {k['name'] for k in schema}
    if unknown:
        raise BenchError(f'Unknown AF config keys: {sorted(unknown)}')
    candidate = bytearray(candidate_size)
    for key in schema:
        value = requested.get(key['name'], key['default'])
        if not isinstance(value, int) or not -(1 << 31) <= value < 1 << 31:
            raise BenchError(f"AF parameter {key['name']} must be a signed 32-bit integer")
        struct.pack_into('<i', candidate, key['offset'], value)
    # Validate C before installing the hooks. All stock words and all regions
    # were checked before the first write. Failed configuration clears payload.
    for base, data in regions:
        bench.board.write_memory(base, data)
    applied = []
    setup_returned = False
    try:
        bench.board.write_memory(address('af_candidate'), bytes(candidate))
        returned = bench.tb.host_call(address('af_setup'), address('af_candidate'),
                                      timeout_instructions=bench._instructions(12_000_000))
        setup_returned = True
        bench._check()
        status = struct.unpack('<i', bench.board.read_memory(address('af_status'), 4))[0]
        if returned != 0 or status != 0:
            index = bench._word(address('af_error_key'))
            key = schema[index - 1]['name'] if 0 < index <= len(schema) else '<configuration>'
            reason = 'feature is unimplemented' if status == -95 else 'invalid parameter'
            raise BenchError(f'AF config {key}: {reason} (C setup status {status})')
        effective = {k['name']: struct.unpack('<i', bench.board.read_memory(address('af_effective') + k['offset'], 4))[0]
                     for k in schema}
        # host_call advances the guest; recheck stock words at the boundary.
        if any(bench._word(a) != old for a, old, _ in words):
            raise BenchError('Stock word changed during AF setup')
        for a, old, new in words:
            bench.board.write_memory(a, struct.pack('<I', new))
            applied.append((a, old))
    except Exception:
        for a, old in reversed(applied):
            bench.board.write_memory(a, struct.pack('<I', old))
        if setup_returned:
            for base, data in regions:
                bench.board.write_memory(base, bytes(len(data)))
        else:
            # A scanner ticket may still be queued or executing. Keep its
            # code alive rather than erasing code the guest may still call.
            bench.mod_attempt['failure'] = 'setup did not return; payload retained, discard this bench'
        raise
    record = dict(manifest_sha256=digest(raw), firmware_sha256=manifest['firmware_sha256'],
                  effective_config=effective, settings_io='bypassed; direct C af_setup',
                  regions=[dict(address=hex(base), size=len(data), sha256=digest(data)) for base, data in regions],
                  words=manifest['words'])
    bench.mods = {'af': record}
    return record
