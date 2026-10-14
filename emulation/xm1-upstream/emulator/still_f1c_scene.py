"""Pure declared F1C residual/unity statistics and scoped identity passes.

The register widths, bank layout and 6x6 transposition are firmware ABI.
This profile assumes four ROI-relative sampling phases and horizontal/vertical
second differences after a clamped black subtraction. The other four signed
planes are explicitly unqualified zeros. Opaque filter/map blocks are sealed,
not interpreted. This module has no board, DMA, IRQ or guest-state effects.
"""
import hashlib
import struct
from . import still_f1c_gain as gain

try:
    import numpy as _numpy
except ImportError:
    _numpy = None

PROFILE = 'declared-f1c-four-phase-local-residual-statistics-unmeasured'
IDENTITY_PROFILE = 'declared-f1c-mode1-zero-correction-identity-unmeasured'
RATIO_PROFILE = 'declared-f1c-monochrome-positive-unity-pairs-unmeasured'
GAIN_IDENTITY_PROFILE = 'declared-f1c-mode3-uniform-q8-unity-identity-unmeasured'
GAIN_PROFILE = gain.PROFILE
SUM_BANKS = (0xfff1c600, 0xfff1c690, 0xfff1c7b0, 0xfff1c840,
             0xfff1c960, 0xfff1c9f0, 0xfff1cb10, 0xfff1cba0,
             0xfff1ccc0, 0xfff1cd50, 0xfff1ce60, 0xfff1cef0)
COUNT_BANKS = (0xfff1c720, 0xfff1c8d0, 0xfff1ca80, 0xfff1cc30)
PHASES = ((0, 0), (1, 0), (0, 1), (1, 1))
BLOCK_SIZES = dict(config=64, source=20, black=16, sampling=24,
                   coordinates=4, grid=20, coefficients=56,
                   gain_map=512, correction_map=768)
METADATA_FIELDS = {'address', 'capacity', 'stride_samples', 'x0', 'y0', 'x1',
                   'y1', 'center_x', 'center_y', 'packing'}
MAX_SOURCE_BYTES = 64 * 1024 * 1024


def _integer(value, low, high, name):
    if type(value) is not int or not low <= value <= high:
        raise ValueError('F1C scene: invalid ' + name)
    return value


def _validate(source, metadata, parameters, mode, profile, backend):
    if profile != PROFILE or mode != 0 or type(mode) is not int:
        raise ValueError('F1C scene requires explicit declared mode0; mode2 math is unqualified')
    if backend not in ('auto', 'scalar', 'numpy') or backend == 'numpy' and _numpy is None:
        raise ValueError('F1C scene: unavailable arithmetic backend')
    if (type(source) is not bytes or type(metadata) is not dict
            or set(metadata) != METADATA_FIELDS or metadata['packing'] != 'u16le-mono12'
            or type(parameters) is not dict or set(parameters) != set(BLOCK_SIZES)
            or any(type(parameters[name]) is not bytes or len(parameters[name]) != size
                   for name, size in BLOCK_SIZES.items())):
        raise ValueError('F1C scene requires exact owned source and parameter byte blocks')
    address = _integer(metadata['address'], 0, 0x5fffffff, 'source address')
    physical = address - 0x40000000 if address >= 0x40000000 else address
    capacity = _integer(metadata['capacity'], 2, MAX_SOURCE_BYTES, 'source capacity')
    stride = _integer(metadata['stride_samples'], 12, 8192, 'row stride')
    if len(source) != capacity or capacity % (2 * stride) or physical + capacity > 0x20000000 or address & 1:
        raise ValueError('F1C scene: source span exceeds the declared pitched allocation')
    height = capacity // (2 * stride)
    x0 = _integer(metadata['x0'], 0, stride - 1, 'ROI left')
    x1 = _integer(metadata['x1'], x0 + 8, stride - 1, 'ROI right')
    y0 = _integer(metadata['y0'], 0, height - 1, 'ROI top')
    y1 = _integer(metadata['y1'], y0 + 8, height - 1, 'ROI bottom')
    _integer(metadata['center_x'], x0, x1, 'ROI center X')
    _integer(metadata['center_y'], y0, y1, 'ROI center Y')
    expected_source = struct.pack('<II6H', address, stride, x0, y0, x1, y1,
                                  metadata['center_x'], metadata['center_y'])
    if parameters['source'] != expected_source:
        raise ValueError('F1C scene: source descriptor and ownership metadata differ')
    sampling = struct.unpack('<6I', parameters['sampling'])
    if sampling != (0, 0, 4, 0, 0, 4):
        raise ValueError('F1C scene: only the actual stride4 sampling descriptor is qualified')
    grid_width, grid_height = struct.unpack_from('<2H', parameters['grid'])
    if (not grid_width or not grid_height
            or not 5 * grid_width < x1 - x0 + 1 <= 6 * grid_width
            or not 5 * grid_height < y1 - y0 + 1 <= 6 * grid_height):
        raise ValueError('F1C scene: configured cell dimensions do not bound a 6x6 ROI grid')
    blacks = struct.unpack('<8H', parameters['black'])
    if any(black > 4095 for black in blacks):
        raise ValueError('F1C scene: black levels exceed the declared mono12 domain')
    return stride, height, grid_width, grid_height, blacks[:4]


def _scalar(source, metadata, stride, grid_width, grid_height, blacks):
    if any(value[0] > 4095 for value in struct.iter_unpack('<H', source)):
        raise ValueError('F1C scene: source is not U16 little-endian mono12')
    sums = [[0] * 36 for _ in range(8)]
    counts = [[0] * 36 for _ in PHASES]
    x0, y0, x1, y1 = (metadata[key] for key in ('x0', 'y0', 'x1', 'y1'))
    for phase, (dx, dy) in enumerate(PHASES):
        black = blacks[phase]
        for y in range(y0 + dy + 4, y1 - 3, 4):
            cy = (y - y0) // grid_height
            for x in range(x0 + dx + 4, x1 - 3, 4):
                cell = 6 * cy + (x - x0) // grid_width
                at = 2 * (y * stride + x)
                values = [max(0, int.from_bytes(source[p:p + 2], 'little') - black)
                          for p in (at, at - 8, at + 8, at - 8 * stride, at + 8 * stride)]
                sums[2 * phase][cell] += values[0] - (values[1] + values[2]) // 2
                sums[2 * phase + 1][cell] += values[0] - (values[3] + values[4]) // 2
                counts[phase][cell] += 1
    return sums, counts


def _vector(source, metadata, stride, height, grid_width, grid_height, blacks):
    # frombuffer is a read-only view; all subtraction operates on small cell copies.
    image = _numpy.frombuffer(source, dtype='<u2').reshape(height, stride)
    if int(image.max()) > 4095:
        raise ValueError('F1C scene: source is not U16 little-endian mono12')
    sums = [[0] * 36 for _ in range(8)]
    counts = [[0] * 36 for _ in PHASES]
    x0, y0, x1, y1 = (metadata[key] for key in ('x0', 'y0', 'x1', 'y1'))
    for phase, (dx, dy) in enumerate(PHASES):
        xs = _numpy.arange(x0 + dx + 4, x1 - 3, 4)
        ys = _numpy.arange(y0 + dy + 4, y1 - 3, 4)
        for cy in range(6):
            y = ys[(ys - y0) // grid_height == cy]
            for cx in range(6):
                x = xs[(xs - x0) // grid_width == cx]
                cell = cy * 6 + cx
                counts[phase][cell] = int(x.size * y.size)
                if not counts[phase][cell]:
                    continue
                values = [_numpy.maximum(0, image[_numpy.ix_(y + yy, x + xx)].astype(_numpy.int32) - blacks[phase])
                          for xx, yy in ((0, 0), (-4, 0), (4, 0), (0, -4), (0, 4))]
                sums[2 * phase][cell] = int((values[0] - (values[1] + values[2]) // 2).sum(dtype=_numpy.int64))
                sums[2 * phase + 1][cell] = int((values[0] - (values[3] + values[4]) // 2).sum(dtype=_numpy.int64))
    return sums, counts


def _mono12(source, backend):
    malformed = (int(_numpy.frombuffer(source, dtype='<u2').max()) > 4095
                 if backend != 'scalar' and _numpy is not None
                 else any(value[0] > 4095 for value in struct.iter_unpack('<H', source)))
    if malformed:
        raise ValueError('F1C scene requires its owned U16 little-endian mono12 input')


def _positive_pairs(source, metadata, stride, height, grid_width, grid_height, blacks, backend):
    """Declared achromatic reference: two identical positive intensity sums.

    Coverage counts only centers with positive post-black intensity. This
    shares only the explicit phase/cell geometry with the residual law and
    never reinterprets a signed residual as an unsigned accumulator.
    """
    _mono12(source, backend)
    vector = backend == 'numpy' or backend == 'auto' and _numpy is not None
    image = _numpy.frombuffer(source, dtype='<u2').reshape(height, stride) if vector else None
    sums = [[0] * 36 for _ in range(8)]
    counts = [[0] * 36 for _ in PHASES]
    x0, y0, x1, y1 = (metadata[key] for key in ('x0', 'y0', 'x1', 'y1'))
    for phase, (dx, dy) in enumerate(PHASES):
        xs = range(x0 + dx + 4, x1 - 3, 4)
        ys = range(y0 + dy + 4, y1 - 3, 4)
        if vector:
            xs, ys = _numpy.array(xs), _numpy.array(ys)
            for cy in range(6):
                y = ys[(ys - y0) // grid_height == cy]
                for cx in range(6):
                    x = xs[(xs - x0) // grid_width == cx]
                    cell = 6 * cy + cx
                    values = _numpy.maximum(0, image[_numpy.ix_(y, x)].astype(_numpy.int32) - blacks[phase])
                    counts[phase][cell] = int(_numpy.count_nonzero(values))
                    total = int(values.sum(dtype=_numpy.int64))
                    sums[2 * phase][cell] = sums[2 * phase + 1][cell] = total
        else:
            for y in ys:
                for x in xs:
                    cell = 6 * ((y - y0) // grid_height) + (x - x0) // grid_width
                    at = 2 * (y * stride + x)
                    value = max(0, int.from_bytes(source[at:at + 2], 'little') - blacks[phase])
                    sums[2 * phase][cell] += value
                    sums[2 * phase + 1][cell] += value
                    counts[phase][cell] += int(value > 0)
        if any(count and not sums[2 * phase][i] for i, count in enumerate(counts[phase])):
            raise ValueError('F1C unity-pair law refuses covered cells with a zero denominator')
    return sums, counts


def _words(sums, counts):
    words = {}
    for bank, plane in zip(SUM_BANKS, sums):
        for y in range(6):
            for x in range(6):
                words[bank + 4 * (6 * x + y)] = plane[6 * y + x] & 0x3ffffff
    for bank, plane in zip(COUNT_BANKS, counts):
        for y in range(6):
            for x in range(6):
                words[bank + 4 * (6 * x + y)] = plane[6 * y + x]
    return words


def prepare_gain(source, *, source_metadata, parameters, profile, backend='auto'):
    _validate(source, source_metadata, parameters, 0, PROFILE, backend)
    result = gain.prepare(source, source_metadata=source_metadata, gain_map=parameters['gain_map'],
                          black_levels=struct.unpack('<8H', parameters['black'])[:4],
                          profile=profile, backend=backend)
    result['identity'].update(mode=3, source_sha256=result['identity']['input_sha256'],
        parameters_sha256={name: hashlib.sha256(data).hexdigest() for name, data in parameters.items()})
    return result


def prepare(source, *, source_metadata, parameters, mode=0, profile, backend='auto'):
    """Return hardware result words and immutable identities; perform no I/O.

    Phase offsets, black clipping and second-difference filters are declared
    simulation choices. Neighbor samples stay inside the ROI and owned span.
    Hardware filter/map meanings and the final four signed planes are unknown.
    """
    if type(mode) is int and mode == 3 and profile == GAIN_PROFILE:
        result = prepare_gain(source, source_metadata=source_metadata, parameters=parameters,
                              profile=profile, backend=backend)
        return dict(words={}, identity=result['identity'])
    if type(mode) is int and ((mode == 1 and profile == IDENTITY_PROFILE)
                             or (mode == 3 and profile == GAIN_IDENTITY_PROFILE)):
        _validate(source, source_metadata, parameters, 0, PROFILE, backend)
        if mode == 1 and parameters['correction_map'] != bytes(768):
            raise ValueError('F1C mode1 zero-correction identity refuses a nonzero correction map')
        if mode == 3 and parameters['gain_map'] != struct.pack('<256H', *([256] * 256)):
            raise ValueError('F1C mode3 identity refuses a gain map other than uniform Q8 unity')
        _mono12(source, backend)
        return dict(words={}, identity=dict(profile=profile, mode=mode, source_metadata=dict(source_metadata),
            source_sha256=hashlib.sha256(source).hexdigest(),
            parameters_sha256={name: hashlib.sha256(data).hexdigest() for name, data in parameters.items()},
            effect=('Declared all-zero correction identity; no RAW byte is changed' if mode == 1
                    else 'Declared uniform Q8 unity identity; no RAW byte is changed'),
            physical_filter_math='Unqualified'))
    if type(mode) is int and mode == 2 and profile == RATIO_PROFILE:
        stride, height, grid_width, grid_height, blacks = _validate(
            source, source_metadata, parameters, 0, PROFILE, backend)
        sums, counts = _positive_pairs(source, source_metadata, stride, height,
                                       grid_width, grid_height, blacks, backend)
        if (any(not 0 <= value < 1 << 26 for plane in sums for value in plane)
                or any(not 0 <= value <= 0x3fff for plane in counts for value in plane)):
            raise ValueError('F1C unity-pair accumulation exceeds the recovered unsigned result widths')
        words = _words(sums, counts)
        packed = b''.join(struct.pack('<II', address, value) for address, value in sorted(words.items()))
        return dict(words=words, identity=dict(profile=profile, mode=mode,
            source_metadata=dict(source_metadata), source_sha256=hashlib.sha256(source).hexdigest(),
            parameters_sha256={name: hashlib.sha256(data).hexdigest() for name, data in parameters.items()},
            words_sha256=hashlib.sha256(packed).hexdigest(), phase_offsets=PHASES,
            sampling_step=4, grid=(grid_width, grid_height), black_levels=blacks,
            eligible_samples=tuple(sum(plane) for plane in counts),
            count_range=(min(min(plane) for plane in counts), max(max(plane) for plane in counts)),
            unsigned_sum_range=(min(min(plane) for plane in sums), max(max(plane) for plane in sums)),
            coverage='Declared positive-only post-black samples; zero samples omitted from both sums and counts',
            physical_filter_math='Unqualified; declared identical positive monochrome reference pairs'))
    stride, height, grid_width, grid_height, blacks = _validate(
        source, source_metadata, parameters, mode, profile, backend)
    vector = backend == 'numpy' or backend == 'auto' and _numpy is not None
    sums, counts = (_vector(source, source_metadata, stride, height, grid_width, grid_height, blacks)
                    if vector else _scalar(source, source_metadata, stride, grid_width, grid_height, blacks))
    if (any(not -(1 << 25) <= value < 1 << 25 for plane in sums for value in plane)
            or any(not 0 <= value <= 0x3fff for plane in counts for value in plane)):
        raise ValueError('F1C scene: declared accumulation exceeds the recovered result widths')
    words = _words(sums + [[0] * 36 for _ in range(4)], counts)
    packed = b''.join(struct.pack('<II', address, value) for address, value in sorted(words.items()))
    identity = dict(profile=PROFILE, mode=mode, source_metadata=dict(source_metadata),
                    source_sha256=hashlib.sha256(source).hexdigest(),
                    parameters_sha256={name: hashlib.sha256(data).hexdigest() for name, data in parameters.items()},
                    words_sha256=hashlib.sha256(packed).hexdigest(),
                    phase_offsets=PHASES, sampling_step=4, grid=(grid_width, grid_height),
                    black_levels=blacks, eligible_samples=tuple(sum(plane) for plane in counts),
                    count_range=(min(min(plane) for plane in counts), max(max(plane) for plane in counts)),
                    signed_sum_range=(min(min(plane) for plane in sums), max(max(plane) for plane in sums)),
                    unqualified_zero_planes=SUM_BANKS[8:])
    return dict(words=words, identity=identity)
