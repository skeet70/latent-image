"""PDAF hardware input path for the continuous sensor stream (declared, unmeasured).

Opt-in and separate from the strict finite models. Three cooperating pieces keep
firmware in charge of every register, ACK and callback; nothing here writes a
result, flag, callback or task state:

* ``RawChannelStream`` delivers one scene RAW frame per sensor frame into the
  destination the firmware programmed on RAW channel 2/3 (retained fields of
  ``InputProcessingControl``), only while that channel is armed and the stock
  IRQ 0x24/0x25 route is live, then waits for the stock ACK (write to 0x350/0x36C).
* ``PhasePrefilterControl`` is the FFF1A000 unit: it accepts the stock mode-3
  programming, completes it with IRQ 0x31 after a declared delay and leaves the
  source bytes unchanged (declared IDENTITY law; the real transform is unknown).
* ``sample_stream`` (called from ``AFExtractionControl`` when its stream hook is
  set) reads the extractor's programmed sources, applies the selector fields the
  firmware wrote, and computes the same cost curve/scalars/totals as the finite
  declared model.

Declared RAW layout law (not a recovered sensor layout): the RAW raster is two
column-interleaved aperture planes. Raster column ``2*a + p`` holds sample ``a``
of aperture ``p`` (p=0 left, p=1 right). Extractor ROI x coordinates are plane
samples, the left selector (reg 2C bit16) and right selector (bit20) pick ``p``
(bit21 is refused). Scene disparity moves the right texture by
``round(defocus / pulses_per_sample)`` plane samples. The RAW channel format bit
selects U16 (0) or packed12 (1, intensities >>2, restored <<2 by the reader).
"""
from __future__ import annotations

import hashlib
import struct

import numpy as np

from .afs_scene_fixture import MAX_INTENSITY, MAX_RADIUS, MAX_SAMPLES
from .hardware import UnsupportedDevice

RAW_PROFILE = 'declared-pdaf-raw-channel-interleaved-aperture-stream-unmeasured'
PREFILTER_PROFILE = 'declared-phase-prefilter-identity-stream-unmeasured'
EXTRACT_PROFILE = 'declared-pdaf-interleaved-aperture-sad-stream-unmeasured'
DESCRIPTOR = 0x15f6458          # RAW descriptor shared by still and continuous drivers (14 words)
CHANNELS = {2: dict(irq=0x24, handler=0x920c08, ack=0x350, base=0x350),
            3: dict(irq=0x25, handler=0x920c4c, ack=0x36c, base=0x36c)}
SATURATION = 0x3ff0              # declared saturation level of a U14 sample (stream-mode AC scalar)
# Site offset of the right source relative to the left one, raster columns, by pattern pair (left selector bit 0 = stock group A, 1 = B).
# Recorded pass splits (research/af-sensor/README.md section 5): half the difference of the two passes' stock shifts is -127.5 +- 1 units
# in group B in the light and the daylight sweeps, exactly -0.5 sample (the 6-column pitch): B's pairs sit 3 columns apart, both passes
# alike. Group A's is +32..36 units (+0.13 sample, 0.8 column; 15 in the dark with scatter 24), declared 0 here: with the offset the
# stock rejects A on noise-free textured frames far more often (every line fails the curve-difference test) than the camera's 17 %
# of lines in the light, so it stays a knob (``device.pair_offsets``) rather than a default.
PAIR_OFFSET_COLUMNS = (0.0, -3.0)
MAX_FRAME_BYTES = 24 << 20
RAM_BYTES = 0x20000000


def _int(value, lo, hi, name):
    if type(value) is not int or not lo <= value <= hi:
        raise UnsupportedDevice('PDAF stream: invalid ' + name)
    return value


def _physical(address):
    return address - 0x40000000 if address >= 0x40000000 else address


# ---------------------------------------------------------------- scene arithmetic

KNOT = 40      # plane samples between random texture knots (smooth texture: SAD curves stay V-shaped)


def _knots(k, ys, seed):
    h = (k * 0x9E3779B1 ^ (ys // 2) * 0x85EBCA6B ^ (seed * 0xC2B2AE35)) & 0xffffffff
    h ^= h >> 15
    h = (h * 0x2C1B3C6D) & 0xffffffff
    h ^= h >> 12
    h = (h * 0x297A2D39) & 0xffffffff
    h ^= h >> 15
    return 2048 + h % 12288


def _texture(xs, ys, seed, flat):
    """Deterministic piecewise-linear noise along x (knots every KNOT plane samples), U14."""
    if flat:
        return np.full((ys.shape[0], xs.shape[1]), 8192, dtype=np.int64)
    k, frac = xs // KNOT, xs % KNOT
    a, b = _knots(k, ys, seed), _knots(k + 1, ys, seed)
    return (a * (KNOT - frac) + b * frac) // KNOT


def aperture_planes(width, height, *, focus_position, target_position, pulses_per_sample=8,
                    flat=False, seed=1):
    """Left/right aperture planes (height x width, U14) of one deterministic textured target.

    A positive target-minus-focus moves the right texture right by
    ``round(defocus/pulses_per_sample)`` samples; box blur grows with |disparity|
    (``min(3, |d|//4)``), as ``afs_scene_fixture.paired_raster`` declares for
    the finite model. Defocus beyond the comparison radius is refused.
    """
    _int(width, 8, 1 << 14, 'plane width')
    _int(height, 1, 1 << 14, 'plane height')
    _int(focus_position, -32768, 32767, 'focus position')
    _int(target_position, -32768, 32767, 'target position')
    _int(pulses_per_sample, 1, 4096, 'parallax scale')
    disparity = round((target_position - focus_position) / pulses_per_sample)
    if abs(disparity) > MAX_RADIUS:
        raise UnsupportedDevice('PDAF stream: disparity exceeds the comparison radius')
    blur = min(3, abs(disparity) // 4)
    margin = MAX_RADIUS + blur
    ys = np.arange(height, dtype=np.int64)[:, None]
    xs = np.arange(-margin, width + margin, dtype=np.int64)[None, :]
    texture = _texture(xs, ys, seed, flat)
    if blur:
        padded = np.pad(texture, ((0, 0), (blur, blur)), mode='edge')
        csum = np.cumsum(padded, axis=1)
        csum = np.concatenate([np.zeros((height, 1), dtype=np.int64), csum], axis=1)
        window = 2 * blur + 1
        texture = (csum[:, window:] - csum[:, :-window]) // window
    left = texture[:, margin:margin + width]
    right = texture[:, margin - disparity:margin - disparity + width]
    return dict(left=left.astype(np.uint16), right=right.astype(np.uint16), disparity=disparity,
                blur_radius=blur, flat=bool(flat))


def aperture_planes_shifted(width, height, *, disparity, flat=False, seed=1, max_disparity=63.0):
    """Left/right aperture planes (U14) of the same deterministic texture with a *fractional* disparity.

    The right plane is the left texture moved by ``disparity`` plane samples (positive: toward larger x),
    linearly interpolated, so the stock correlation (``0x13494d8``) can recover sub-sample shifts. Box
    blur ``min(3, int(|d|) // 4)`` as in ``aperture_planes``. ``flat`` gives a featureless plane pair.
    """
    _int(width, 8, 1 << 14, 'plane width')
    _int(height, 1, 1 << 14, 'plane height')
    disparity = float(disparity)
    if not abs(disparity) <= max_disparity:
        raise UnsupportedDevice('PDAF stream: disparity exceeds the declared range')
    whole = int(np.floor(disparity))
    fraction = disparity - whole
    blur = min(3, int(abs(disparity)) // 4)
    margin = MAX_RADIUS + blur + int(abs(whole)) + 2
    ys = np.arange(height, dtype=np.int64)[:, None]
    xs = np.arange(-margin, width + margin, dtype=np.int64)[None, :]
    texture = _texture(xs, ys, seed, flat).astype(np.float64)
    if blur:
        padded = np.pad(texture, ((0, 0), (blur, blur)), mode='edge')
        csum = np.concatenate([np.zeros((height, 1)), np.cumsum(padded, axis=1)], axis=1)
        window = 2 * blur + 1
        texture = (csum[:, window:] - csum[:, :-window]) / window
    left = texture[:, margin:margin + width]
    a = texture[:, margin - whole:margin - whole + width]
    b = texture[:, margin - whole - 1:margin - whole - 1 + width]
    right = a * (1.0 - fraction) + b * fraction
    return dict(left=np.rint(left).astype(np.uint16), right=np.rint(right).astype(np.uint16),
                disparity=disparity, blur_radius=blur, flat=bool(flat))


def encode_raster(left, right, *, packed, pitch):
    """Interleave planes and encode rows (U16, or packed12 of intensity>>2), zero padded to ``pitch``."""
    height, plane = left.shape
    if right.shape != left.shape or (2 * plane) & 1:
        raise UnsupportedDevice('PDAF stream: aperture planes must have equal shape')
    raster = np.empty((height, 2 * plane), dtype=np.uint16)
    raster[:, 0::2], raster[:, 1::2] = left, right
    return encode_rows(raster, packed=packed, pitch=pitch)


def encode_rows(raster, *, packed, pitch):
    """Encode a full raster (U16, or packed12 of intensity>>2), zero padded to ``pitch`` bytes per row."""
    height, columns = raster.shape
    plane = columns // 2
    if packed:
        values = (raster >> 2).astype(np.uint32)
        a, b = values[:, 0::2], values[:, 1::2]
        out = np.empty((height, plane, 3), dtype=np.uint8)       # plane = pairs per row
        out[:, :, 0] = a & 255
        out[:, :, 1] = (a >> 8) | ((b & 15) << 4)
        out[:, :, 2] = b >> 4
        rows = out.reshape(height, plane * 3)
    else:
        rows = raster.astype('<u2').view(np.uint8).reshape(height, plane * 4)
    if pitch < rows.shape[1]:
        raise UnsupportedDevice('PDAF stream: pitch is narrower than the active row')
    result = np.zeros((height, pitch), dtype=np.uint8)
    result[:, :rows.shape[1]] = rows
    return result.tobytes()


def decode_samples(raw, *, packed, columns):
    """First ``columns`` raster samples of one row (U14; packed12 restored by <<2)."""
    if packed:
        data = np.frombuffer(raw[:columns * 3 // 2 + 3], dtype=np.uint8).astype(np.uint32)
        pairs = (columns + 1) // 2
        triples = data[:pairs * 3].reshape(pairs, 3)
        a = triples[:, 0] | (triples[:, 1] & 15) << 8
        b = triples[:, 1] >> 4 | triples[:, 2] << 4
        out = np.empty(2 * pairs, dtype=np.int64)
        out[0::2], out[1::2] = a, b
        return out[:columns] << 2
    return np.frombuffer(raw[:columns * 2], dtype='<u2').astype(np.int64)


def sad_curve(left, right, radius):
    """Shared declared law: cost[s] = sum |L[x] - R[x+s]| over the guarded columns (all rows)."""
    rows, width = left.shape
    if width <= 2 * radius:
        raise UnsupportedDevice('PDAF stream: insufficient guard for the comparison radius')
    base = left[:, radius:width - radius]
    return [int(np.abs(base - right[:, radius + s:width - radius + s]).sum()) for s in range(-radius, radius + 1)]


# ---------------------------------------------------------------- source registry

class RawSources:
    """Layouts of delivered (or host-declared) RAW frames, keyed by physical base address."""

    def __init__(self):
        self.frames = {}

    def declare(self, address, *, packed, pitch, width, height, tick=0, frame=0, layout='interleave'):
        base = _physical(address) & 0x3ffffff0
        self.frames[base] = dict(address=base, packed=bool(packed), pitch=pitch, width=width,
                                 height=height, tick=tick, frame=frame, layout=layout)
        for stale in sorted(self.frames)[:-8]:
            del self.frames[stale]
        return self.frames[base]

    def packed_format(self):
        """Packing of the delivered frames (one RAW channel format per stream)."""
        return next(iter(self.frames.values()))['packed'] if self.frames else False

    def lookup(self, address):
        """The delivered/declared frame containing an address (crop offsets are inside the frame)."""
        physical = _physical(address)
        for base, frame in self.frames.items():
            if base <= physical < base + frame['pitch'] * frame['height']:
                return frame
        raise UnsupportedDevice('PDAF stream: extractor source is not a delivered RAW frame')

    def write(self, board, address, left, right, *, packed, pitch, tick=0, frame=0):
        """Host helper for tests: write one encoded frame and declare its layout."""
        data = encode_raster(left, right, packed=packed, pitch=pitch)
        board.write_memory(_physical(address), data)
        return self.declare(address, packed=packed, pitch=pitch, width=2 * left.shape[1],
                            height=left.shape[0], tick=tick, frame=frame)


def sample_stream(device, regs):
    """Extractor sampling for the stream profile; returns ``dict(configuration, sources, result)``.

    The caller (``AFExtractionControl``) has already checked the format1 flag; this reads the delivered
    frames at the programmed source bases and applies the selector fields. ROI x is in raster columns
    relative to the source pointer (as the firmware programs it: stride 5120, ROI 2230..2704, step 6) and
    row addresses are `pointer + y * pitch`, each looked up in whichever delivered frame contains them (the
    stock RAW ring puts the S1 request's rows inside the second slot). Sample k reads raster column
    `((x0 + k*pitch) & ~1) | parity` with `pitch = xstep << left selector bit` (12129DC halves the per-row sample
    count for the odd patterns, so both stock groups have 80 samples over the same 480 columns and the same 6-column
    in-row pitch; the row pitch of the odd pattern is halved likewise, 80 x 10 samples for both) and `phase = ((x0 // 3) ^ y0) & 1`, the declared lattice phase of the ROI origin. A selector pair
    naming both apertures (stock group A: 0/1, group B: 1/0) reads aperture `phase` on the left and the other on the
    right in either encoding (the camera's two groups agree in sign and size); the shift unit of the cost curve is
    one sample step (`pitch` columns).
    """
    scene = device.board.pdaf_stream.scene
    measurement = getattr(scene, 'pd_measurement', None)
    if measurement is not None:
        recorded = measurement(device, regs)
        if recorded is not None:
            return recorded
    raw = device.stream_raw
    word = regs[0x2c]
    if word & 0x00200000:
        raise UnsupportedDevice('AF extraction: right selector bit21 is outside the declared aperture layout')
    left_p, right_p = (word >> 16) & 1, (word >> 20) & 1
    stride = regs[0x14]
    x0, y0 = regs[0x18] & 0x1fff, regs[0x18] >> 16
    x1, y1 = regs[0x1c] & 0x1fff, regs[0x1c] >> 16
    # Sample pitch: 12129DC halves the per-row sample count (reg 0x28: 80 for both stock groups) when the left pattern is
    # not 0/4; in the S1 requests that is exactly the odd patterns 1/5 = left selector bit 1 (group B: cell 3 columns, 80
    # samples over the same 480 columns, so its in-row pitch is 6 columns like group A's).
    xstep = ((word & 15) + 1) << left_p
    # Rows: the nominal cell is n1 + n2 + 2 rows (A 3, B 6). The recorded groups have equal sample totals and cost levels on the same
    # frame (B's four totals are A's, see research/af-sensor/README.md section 5), i.e. the same sample count, so group B's 6-row cell
    # is sampled at half its nominal pitch (3 rows): both groups read 80 columns x 10 rows.
    ystep = ((word >> 4 & 15) + (word >> 8 & 15) + 2) >> left_p
    radius_word = word >> 24 & 31
    radius = radius_word   # stock 12129DC stores req[0x18] here and 1212874 copies 2*req[0x18]+1 costs
    if (not stride or stride & 15 or x1 < x0 or y1 < y0 or x1 + 1 >= stride
            or radius_word & 1 or radius > MAX_RADIUS):
        raise UnsupportedDevice('AF extraction: unknown stride/ROI/radius geometry')
    xs = np.arange(x0, x1 + 1, xstep)
    ys = list(range(y0, y1 + 1, ystep))
    if len(xs) <= 2 * radius or len(xs) * len(ys) > MAX_SAMPLES:
        raise UnsupportedDevice('AF extraction: ROI exceeds bounded raw statistics or comparison guard')
    # Lattice phase of the ROI origin: the stock second pass moves the ROI by +3 columns (group 0) or +1 row
    # (group 1) and sums the two passes' shifts, so each such step must exchange the apertures.
    phase = ((x0 // 3) ^ y0) & 1
    # Aperture parity of each source. A selector pair that names both apertures (0/1 for group A, 1/0 for group B)
    # selects the same two pupil halves in either encoding: the recorded cost curves of both groups, taken from the same
    # frame, give the same shift sign and size (research/af-sensor/README.md section 5), so the left source reads
    # aperture `phase` and the right one the other. Equal selectors compare one aperture with itself.
    if left_p != right_p:
        parities = (phase, 1 - phase)
    else:
        parities = (left_p ^ phase, right_p ^ phase)
    planes, spans = [], []
    offsets = getattr(device, 'pair_offsets', PAIR_OFFSET_COLUMNS)
    for source, (pointer, parity) in enumerate(((regs[8], parities[0]), (regs[0xc], parities[1]))):
        if pointer & 7:
            raise UnsupportedDevice('AF extraction: unaligned paired source pointer')
        rows = []
        for y in ys:
            frame = raw.lookup(_physical(pointer) + y * (stride * 3 // 2 if raw.packed_format() else stride * 2))
            pitch = stride * 3 // 2 if frame['packed'] else stride * 2
            if pitch != frame['pitch'] or stride < frame['width']:
                raise UnsupportedDevice('AF extraction: stride differs from the delivered RAW pitch')
            offset = _physical(pointer) + y * pitch - frame['address']
            row, remainder = divmod(offset, pitch)
            if frame['packed']:
                if remainder % 3:
                    raise UnsupportedDevice('AF extraction: crop offset splits a packed12 sample pair')
                column0 = remainder * 2 // 3
            else:
                if remainder & 1:
                    raise UnsupportedDevice('AF extraction: crop offset splits a U16 sample')
                column0 = remainder // 2
            columns = (column0 + xs & ~1) | parity   # the aperture parity belongs to the raster
            shift = offsets[left_p] / 2.0 if source == 1 else 0.0    # plane samples (two raster columns each)
            if shift:
                whole = int(np.floor(shift))
                fraction = shift - whole
                last = frame['width'] - 2 + parity
                upper = np.clip(columns + 2 * (whole + 1), parity, last)        # plane edges repeat
                columns = np.clip(columns + 2 * whole, parity, last)
            if row >= frame['height'] or columns.max() >= frame['width']:
                raise UnsupportedDevice('AF extraction: ROI lies outside the delivered RAW frame')
            address = frame['address'] + row * pitch
            data = bytes(device.board.read_memory(address, pitch))
            if len(data) != pitch:
                raise UnsupportedDevice('AF extraction: source reader returned an incomplete row')
            spans.append(dict(address=address, bytes=pitch, sha256=hashlib.sha256(data).hexdigest()))
            decoded = decode_samples(data, packed=frame['packed'], columns=frame['width'])
            if shift:
                rows.append(np.rint(decoded[columns] * (1.0 - fraction) + decoded[upper] * fraction).astype(np.int64))
            else:
                rows.append(decoded[columns])
        planes.append(np.array(rows, dtype=np.int64))
    left, right = planes
    if left.max() > MAX_INTENSITY or right.max() > MAX_INTENSITY:
        raise UnsupportedDevice('AF extraction: sample exceeds U14')
    all_left, all_right = left.ravel(), right.ravel()
    curve = sad_curve(left, right, radius)
    if max(curve) > 0xffffffff:
        raise UnsupportedDevice('AF extraction: comparison sum exceeds U32')
    result = dict(radius=radius, curve=curve,
                  # AC: stock 0134A20C flags a line when (AC0+AC1)*100/(2*ROI samples) >= 30, so AC is declared as the
                  # per-source count of saturated samples (>= SATURATION), not the intensity range of the finite model.
                  scalars=[int((all_left >= SATURATION).sum()), int((all_right >= SATURATION).sum())],
                  totals=[int(all_left.sum()), int(all_right.sum())],
                  samples=int(left.size), comparison_samples=int((left.shape[1] - 2 * radius) * left.shape[0]))
    return dict(configuration=regs, sources=spans, result=result,
                selectors=dict(left=left_p, right=right_p), profile=EXTRACT_PROFILE)


# ---------------------------------------------------------------- RAW channel stream

class RawChannelStream:
    """One declared RAW frame per sensor frame into the firmware-programmed channel destination."""

    def __init__(self, board, scene, sources, *, channels=(2,), dma_ticks=20000, max_bytes=MAX_FRAME_BYTES,
                 mode_change_frames=0):
        if any(c not in CHANNELS for c in channels) or type(dma_ticks) is not int or dma_ticks < 1:
            raise ValueError('RAW stream needs channels 2/3 and a positive DMA delay')
        if type(mode_change_frames) is not int or not 0 <= mode_change_frames <= 16:
            raise ValueError('RAW mode-change frames must be an integer 0..16')
        self.board, self.scene, self.sources = board, scene, sources
        self.channels, self.dma_ticks, self.max_bytes = tuple(channels), dma_ticks, max_bytes
        # Declared (default 0 = the original behavior): after the sensor mode word
        # changes, source0 edges are withheld until readout is valid. This delay
        # precedes the stock RAW wait completion, not the subsequent DMA wait.
        # A standalone stream without a sensor clock gates armed RAW frames.
        self.mode_change_frames = mode_change_frames
        self.delivered_mode, self.armed_mode, self.skip_left = None, None, 0
        self.sensor_clock = False
        self.pending = dict.fromkeys(self.channels)       # delivered frame awaiting the stock ACK
        self.counts = dict(frames=0, delivered=0, acknowledged=0, skipped_unarmed=0, skipped_pending=0,
                           skipped_busy=0, skipped_no_scene=0, skipped_mode_change=0)
        self.last = None
        self._cache = {}

    def _input(self):
        found = [d for d in self.board.devices if type(d).__name__ == 'InputProcessingControl']
        if len(found) != 1:
            raise UnsupportedDevice('RAW stream requires exactly one input processing engine')
        return found[0]

    def _word(self, address, size=4):
        return int.from_bytes(self.board.read_memory(address, size), 'little')

    def geometry(self, channel):
        """Armed-channel geometry from retained registers and the shared RAW descriptor, or None when unarmed."""
        regs = self._input().registers
        base = CHANNELS[channel]['base']
        mode, destination = regs.get(base + 4, 0), regs.get(base + 8, 0) & 0x3ffffff0
        if not (mode >> 28) & 7 or not destination:
            return None
        d = struct.unpack('<14I', self.board.read_memory(DESCRIPTOR, 56))
        _, _, x, y, height, width, pitch = d[:7]
        packed = d[10]
        if not (8 <= width <= 1 << 14 and 1 <= height <= 1 << 14 and width & 1 == 0 and packed in (0, 1)):
            raise UnsupportedDevice('RAW stream: the shared RAW descriptor has no valid raster geometry')
        row_bytes = width * (3 if packed else 4) // 2
        if pitch < row_bytes or (pitch - row_bytes) & 15 or destination & 15:
            raise UnsupportedDevice('RAW stream: unaligned RAW pitch/destination')
        if pitch * height > self.max_bytes or _physical(destination) + pitch * height > RAM_BYTES:
            raise UnsupportedDevice('RAW stream: frame exceeds the bounded DRAM span')
        return dict(width=width, height=height, pitch=pitch, packed=bool(packed), destination=destination,
                    origin=(x, y), mode=mode)

    def _route(self, channel):
        info, line = CHANNELS[channel], CHANNELS[channel]['irq']
        banks = self.board.interrupts.banks
        owners = [i for i, b in enumerate(banks) if b.enabled & (1 << line)]
        if len(owners) != 1:
            return None
        bank = banks[owners[0]]
        if (bank.config.get(0x400 + 4 * line) != info['handler'] or bank.config.get(0x800 + 4 * line) != 8
                or bank.pending & (1 << line) or bank.active == line or bank.injected == line):
            return None
        return owners[0]

    def sensor_frame_ready(self, mode):
        """One readiness decision per external frame, shared by all RAW channels."""
        if not self.mode_change_frames:
            return True
        if mode != self.armed_mode:
            self.armed_mode, self.skip_left = mode, self.mode_change_frames
        if self.skip_left:
            self.skip_left -= 1
            self.counts['skipped_mode_change'] += 1
            return False
        return True

    def on_frame(self, ctx, tick):
        self.counts['frames'] += 1
        ready = None
        for channel in self.channels:
            try:
                geometry = self.geometry(channel)
            except UnsupportedDevice:
                self._fail_unarmed(channel)
                raise
            if geometry is None:
                self.counts['skipped_unarmed'] += 1
                continue
            if self.pending[channel] is not None:
                self.counts['skipped_pending'] += 1
                continue
            if self._route(channel) is None:
                self.counts['skipped_unarmed'] += 1
                continue
            if self._busy():
                self.counts['skipped_busy'] += 1
                continue
            if not self.sensor_clock:
                if ready is None:
                    ready = self.sensor_frame_ready(ctx.sensor_mode)
                if not ready:
                    continue
            spec = dict(geometry, plane_width=geometry['width'] // 2)
            planes = self.scene.raw_pd(ctx, spec)
            if planes is None:
                self.counts['skipped_no_scene'] += 1
                continue
            if 'raster' in planes:
                raster = planes['raster']
                if raster.shape != (geometry['height'], geometry['width']):
                    raise UnsupportedDevice('RAW stream: scene raster differs from the programmed geometry')
                data = encode_rows(raster, packed=geometry['packed'], pitch=geometry['pitch'])
            else:
                left, right = planes['left'], planes['right']
                if left.shape != (geometry['height'], spec['plane_width']) or right.shape != left.shape:
                    raise UnsupportedDevice('RAW stream: scene planes differ from the programmed geometry')
                data = encode_raster(left, right, packed=geometry['packed'], pitch=geometry['pitch'])
            geometry = dict(geometry, layout='interleave')
            self.delivered_mode = ctx.sensor_mode
            self.pending[channel] = dict(state='dma', frame=ctx.index, geometry=geometry)
            self.board.schedule(self.dma_ticks, lambda c=channel, g=geometry, d=data, i=ctx.index, t=tick:
                                self._complete(c, g, d, i, t))

    def _fail_unarmed(self, channel):
        self.pending[channel] = None

    def _busy(self):
        return any(getattr(d, 'pending', None) is not None or getattr(d, 'state', None) in ('armed', 'completion-pending')
                   for d in self.board.devices if type(d).__name__ in ('AFExtractionControl', 'PhasePrefilterControl'))

    def _complete(self, channel, geometry, data, index, tick):
        pending = self.pending[channel]
        if pending is None or pending['frame'] != index:
            return
        try:
            now = self.geometry(channel)
            route = self._route(channel)
            if {k: v for k, v in geometry.items() if k != 'layout'} != now or route is None:
                raise UnsupportedDevice('RAW stream: channel programming or IRQ route changed during the DMA')
            self.board.write_memory(_physical(geometry['destination']), data)
            self.sources.declare(geometry['destination'], packed=geometry['packed'], pitch=geometry['pitch'],
                                 width=geometry['width'], height=geometry['height'],
                                 tick=self.board.virtual_ticks, frame=index, layout=geometry['layout'])
            pending.update(state='irq', asserted_tick=self.board.virtual_ticks)
            self.counts['delivered'] += 1
            self.last = dict(channel=channel, frame=index, destination=hex(geometry['destination']),
                             bytes=len(data), sha256=hashlib.sha256(data).hexdigest(), source_tick=tick,
                             packed=geometry['packed'], width=geometry['width'], height=geometry['height'])
            self.board.queue_irq(route, CHANNELS[channel]['irq'])
        except UnsupportedDevice as error:
            self.pending[channel] = None
            if getattr(self.board, 'failure', None) is None:
                self.board.failure = dict(message=str(error), peripheral='PDAF RAW channel stream')

    def on_ack(self, channel, value):
        """Called by InputProcessingControl for ACK writes to 0x350/0x36C."""
        if channel in self.pending and self.pending[channel] is not None and self.pending[channel]['state'] == 'irq' \
                and value & 1:
            self.pending[channel] = None
            self.counts['acknowledged'] += 1

    def summary(self):
        return dict(profile=RAW_PROFILE, counts=dict(self.counts), last=self.last,
                    pending={c: (p and p['state']) for c, p in self.pending.items()})


# ---------------------------------------------------------------- prefilter unit

class PhasePrefilterControl:
    """FFF1A000 phase prefilter: stock mode programming, declared identity transform, IRQ 0x31."""
    BASE = 0xfff1a000
    IRQ, ISR, PRIORITY = 0x31, 0x1212848, 13
    MASKS = {0: 0x10107, 4: 0x3fffffff, 8: 0x1fff, 0xc: 0x3fffffff, 0x10: 0x3ffff, 0x14: 0x13ff13ff, 0x20: 0x1f}

    def __init__(self, board, sources, *, data_ticks=20000, stop_ticks=16):
        if type(data_ticks) is not int or type(stop_ticks) is not int or data_ticks < 1 or stop_ticks < 1:
            raise ValueError('Prefilter delays must be positive virtual ticks')
        self.board, self.sources = board, sources
        self.data_ticks, self.stop_ticks = data_ticks, stop_ticks
        self.registers = {offset: 0 for offset in self.MASKS}
        self.state, self.completion_due, self.stop_due = 'idle', None, None
        self.jobs, self.acknowledgements = [], []
        self.pending, self.route = None, None

    def contains(self, address, size):
        return self.BASE <= address and address + size <= self.BASE + 0x230

    def _offset(self, address, size):
        if size != 4 or address & 3:
            raise UnsupportedDevice('Prefilter: aligned words only')
        return address - self.BASE

    def read(self, cpu, address, size):
        offset = self._offset(address, size)
        self.tick(self.board)
        if offset != 0:
            raise UnsupportedDevice('Prefilter: read is not a recovered status access')
        return self.registers[0]

    def _route(self):
        banks = self.board.interrupts.banks
        owners = [i for i, b in enumerate(banks) if b.enabled & (1 << self.IRQ)]
        if len(owners) != 1:
            raise UnsupportedDevice('Prefilter: requires one enabled IRQ0x31 owner')
        bank = banks[owners[0]]
        if (bank.config.get(0x400 + 4 * self.IRQ) != self.ISR or bank.config.get(0x800 + 4 * self.IRQ) != self.PRIORITY
                or bank.pending & (1 << self.IRQ) or bank.active == self.IRQ or bank.injected == self.IRQ):
            raise UnsupportedDevice('Prefilter: IRQ0x31 differs from the stock handler/priority route')
        return owners[0]

    def write(self, cpu, address, size, value):
        offset = self._offset(address, size)
        if offset == 0x1c:
            if value != 0x1f:
                raise UnsupportedDevice('Prefilter: ACK mask is not 0x1F')
            self.acknowledgements.append(self.board.virtual_ticks)
            if self.state == 'completion-pending':
                self.state, self.pending = 'acknowledged', None
            return
        if offset not in self.MASKS or value & ~self.MASKS[offset]:
            raise UnsupportedDevice('Prefilter: write is outside the recovered fields')
        self.registers[offset] = value
        if offset == 0:
            if value == 0:
                self.stop_due = self.board.virtual_ticks + self.stop_ticks
                self.state, self.completion_due, self.pending = 'stopping', None, None
            elif value & 7:
                if not 1 <= value & 7 <= 4:
                    raise UnsupportedDevice('Prefilter: unknown mode')
                self._arm(cpu)

    def _arm(self, cpu):
        regs = self.registers
        source, stride = regs[4], regs[8]
        mode, packing = regs[0] & 7, regs[0] >> 8 & 1
        if self.state not in ('idle', 'acknowledged', 'stopping'):
            raise UnsupportedDevice('Prefilter: armed while a job is outstanding')
        if not source or not stride:
            raise UnsupportedDevice('Prefilter: armed without a programmed source')
        frame = self.sources.lookup(source)           # identity needs a real delivered RAW source
        self.route = self._route()
        self.pending = dict(mode=mode, source=source, stride=stride, packing_flag=packing,
                            source_sha256=hashlib.sha256(bytes(self.board.read_memory(
                                frame['address'], frame['pitch'] * min(frame['height'], 4)))).hexdigest(),
                            frame=frame['frame'])
        self.state, self.stop_due = 'armed', None
        self.completion_due = self.board.virtual_ticks + self.data_ticks

    def tick(self, board):
        now = board.virtual_ticks
        if self.stop_due is not None and now >= self.stop_due:
            self.registers[0] |= 0x10000
            self.stop_due, self.state = None, 'idle'
        if self.completion_due is not None and now >= self.completion_due and self.state == 'armed':
            self.completion_due = None
            self._route()
            self.state = 'completion-pending'
            self.jobs.append(dict(self.pending, completed_tick=now, transform='identity'))
            board.queue_irq(self.route, self.IRQ)

    def next_event_tick(self, now):
        due = [t for t in (self.stop_due, self.completion_due) if t is not None]
        return min(due) if due else None

    def snapshot(self):
        return dict(profile=PREFILTER_PROFILE, state=self.state, jobs=self.jobs[-8:],
                    limits='Declared identity transform; real prefilter pixel law unrecovered')


# ---------------------------------------------------------------- composition

class PdafStream:
    def __init__(self, board, scene, *, sensor_stream=None, channels=(2,), dma_ticks=20000, prefilter_ticks=20000,
                 extract_ticks=None, mode_change_frames=0):
        self.board, self.scene = board, scene
        self.sources = RawSources()
        self.raw = RawChannelStream(board, scene, self.sources, channels=channels, dma_ticks=dma_ticks,
                                    mode_change_frames=mode_change_frames)
        inputs = [d for d in board.devices if type(d).__name__ == 'InputProcessingControl']
        extractors = [d for d in board.devices if type(d).__name__ == 'AFExtractionControl']
        if len(inputs) != 1 or len(extractors) != 1:
            raise ValueError('PDAF stream requires one input processing engine and one AF extractor')
        self.input, self.extractor = inputs[0], extractors[0]
        if getattr(self.input, 'raw_stream', None) is not None or getattr(self.extractor, 'stream_raw', None) is not None:
            raise ValueError('PDAF stream is already enabled')
        from .afs_scene_fixture import PROFILE
        if self.extractor.profile is None:
            self.extractor.profile = PROFILE
        self.input.raw_stream, self.extractor.stream_raw = self.raw, self.sources
        if extract_ticks is not None:
            self.extractor.data_ticks = extract_ticks
        self.prefilter = PhasePrefilterControl(board, self.sources, data_ticks=prefilter_ticks)
        board.attach(self.prefilter)
        board.pdaf_stream = self
        self.sensor_stream = sensor_stream
        if sensor_stream is not None:
            self._hook(sensor_stream)

    def _hook(self, stream):
        self.raw.sensor_clock = True
        self.raw.armed_mode = stream._word(0x173D308)   # the existing mode is already running
        original = stream.on_edge

        def on_edge(pending, tick):
            original(pending, tick)
            if pending['source'] != 0:
                return
            ctx = stream.contexts.get(pending['frame'])
            if ctx is None:
                from emulator.testbed.scene_sources import FrameContext
                committed, physical, window = stream.pose(tick)
                ctx = FrameContext(index=pending['frame'], tick=tick, sensor_mode=stream._word(0x173D308),
                                   lens_position=committed, physical_position=physical, exposure_window=window)
            self.raw.on_frame(ctx, tick)

        stream.on_edge = on_edge

    def summary(self):
        return dict(raw=self.raw.summary(), prefilter=self.prefilter.snapshot(),
                    extractor=dict(state=self.extractor.state, jobs=len(self.extractor.jobs)))


def enable(board, scene, *, sensor_stream=None, **options):
    """Attach the PDAF hardware stream (opt-in); ``sensor_stream`` defaults to ``board.sensor_stream``."""
    return PdafStream(board, scene, sensor_stream=sensor_stream or getattr(board, 'sensor_stream', None), **options)


def scene_pd(spec, lens, target, *, pulses_per_sample, flat, seed, cache):
    """Declared PD aperture planes for one RAW geometry (cached by their disparity)."""
    key = (spec['plane_width'], spec['height'], round((target - lens) / pulses_per_sample), flat, seed)
    planes = cache.get(key)
    if planes is None:
        # Sign convention: the stock PDAF adds its defocus result to the position, so the roles of lens and target
        # are exchanged here to make a positive target-minus-lens drive the lens toward the target.
        planes = aperture_planes(spec['plane_width'], spec['height'], focus_position=target, target_position=lens,
                                 pulses_per_sample=pulses_per_sample, flat=flat, seed=seed)
        if len(cache) >= 8:
            cache.pop(next(iter(cache)))
        cache[key] = planes
    return planes


class PdScene:
    """Any scene source plus the declared PD law (statistics etc. come from the wrapped scene).

    ``pulses_per_sample`` is pulses per aperture-plane sample of disparity (default 48: the S1 group-0
    request steps 6 columns = 3 plane samples and sums two passes, so 48 pulses/sample makes the stock defocus
    result about equal to target minus lens).
    """

    def __init__(self, scene, *, target_position=0, pulses_per_sample=48, flat=False, seed=1):
        self._scene = scene
        self.target_position, self.flat, self.seed, self.pulses_per_sample = target_position, flat, seed, pulses_per_sample
        self._cache = {}

    def __getattr__(self, name):
        return getattr(self._scene, name)

    def raw_pd(self, ctx, spec):
        lens = 0 if ctx.position is None else round(ctx.position)
        return scene_pd(spec, lens, self.target_position, pulses_per_sample=self.pulses_per_sample, flat=self.flat,
                        seed=self.seed, cache=self._cache)

    def describe(self):
        return dict(self._scene.describe(), pd=dict(target_position=self.target_position,
                                                    pulses_per_sample=self.pulses_per_sample, flat=self.flat))
