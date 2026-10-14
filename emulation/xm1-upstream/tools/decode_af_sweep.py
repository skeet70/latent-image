#!/usr/bin/env python3
"""Decode the stock PDAF results saved by a focus sweep (tools/x70_focus_sweep.py).

Read-only on the evidence directory.  Writes ``research/af-sweeps/<sweep>.json``.

Inputs per sample (``pNNN-rR-*``):

* ``paf-record.bin``  0x200 bytes: the patch's 0x20-byte header (see
  ``tools/x70_focus_sweep.decode_record``) then the stock result: an 8-byte
  aggregate header (``PafAggregateResult``) and up to four 0x74-byte
  ``PafAreaResult`` records (the patch copies at most four of the six).
* ``pd-buffer.bin``  0x37510 bytes: the 4th argument of FUN_01350444 (S1 PD
  buffer).  It is *not* raw PD rows.  Layout (static: ``0x13480a0`` paf task,
  ``0x1347ddc`` initialiser, ``0x134a20c`` correlation, owner decomp
  ``decomp/app/src/s5-4-hi-u/paf_algorithm.h``; checked against the data here):

    +0x00  u32 state, processed_area_count, selected_area_index, group_mode
    +0x10 + n*0x3b00   area slot n (15 slots; slots 0..count-1 used)
        +0x0000..0x2280  engine scratch for the raw rows (0x450 bytes x 8).  The
                         rows are only filled when a debug bit (cfg word 0xa8 & 0x80) is set;
                         in normal operation it holds uninitialised memory.
        +0x2280          (abs 0x2290) the 0x74-byte area result, copied again to the
                         result array by 0x134ac34
        +0x2300 + g*0xc00  group g in {0 (A), 1 (B)}: 0x24-byte header then
                         the work area ``PafCorrelationWork`` at +0x24
    Work area (offsets from work): 4 line cost curves A at +0, average +0x320, 4 line
    curves B at +0x3e8, average +0x708 (50 u32 slots each, ``curve_samples`` valid, then the
    0xfdfdfdfd sentinel); inputs 0x7d0..0x838; four ``PafLineQuality`` (0x94 bytes) at +0x83c;
    valid line list +0xa8c, count +0xa94; group quality at +0xa9c; result state +0xb30,
    step +0xb34, target +0xb38, shift +0xb3c; conversion table copy +0xb40.

Everything downstream (``--verify``) recomputes minimum, interpolation, combination and
shift to defocus from the saved curves with Python ports of 0x13492ec, 0x13494d8,
0x1348bcc and 0x1348d18 and compares with the stored values.
"""
import argparse
import json
import math
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HARDWARE = ROOT / 'research/hardware'
OUT = ROOT / 'research/af-sweeps'
SWEEPS = {
    'dark1': 'ff80-focus-sweep-20261006T031301Z-270cd7',
    'light': 'ff80-focus-sweep-20261006T033723Z-f28780',
    'dark2': 'ff80-focus-sweep-20261006T035248Z-61b04c',
    'day': 'ff80-focus-sweep-20261006T185728Z-befbe7',      # daylight, matched to the stock trials ff80-focus-trials-20261006T185535Z-687e4d
}

PD_BYTES = 0x37510
SLOT_BYTES, SLOT_BASE, MAX_SLOTS = 0x3b00, 0x10, 15
RESULT_IN_SLOT = 0x2290 - 0x10          # relative to SLOT_BASE; abs = slot*0x3b00 + 0x2290
GROUP_BASE, GROUP_STRIDE, WORK_HEADER = 0x2310, 0xc00, 0x24
LINE_BASE, LINE_STRIDE = 0x83c, 0x94
REC = 0x74
STATE_NAMES = {0: 'valid', 1: 'marginal', 2: 'failed'}
FLAGS = {1: 'response-imbalance', 2: 'metric-limit', 4: 'low-contrast', 8: 'minimum-ceiling',
         0x10: 'slope-ratio', 0x20: 'response-ratio', 0x40: 'asymmetry', 0x80: 'group-shift',
         0x100: 'too-few-lines', 0x200: 'motion'}
PAF_POSITION_OFFSET = -21   # work.lens_position_pulses - patch position (0xccde64), observed on all samples


def flag_names(value):
    return [name for bit, name in FLAGS.items() if value & bit]


# ----------------------------------------------------------------- stock result (0x1249a3c argument)

def decode_aggregate(raw):
    state, value, count, selected = struct.unpack_from('<IhBB', raw, 0)
    return dict(state=state, lens_value=value, area_count=count, selected_area=selected)


def decode_area_record(raw):
    """``PafAreaResult`` (0x74 bytes; owner decomp paf_algorithm.h, written by 0x13489b8 / 0x134ac34)."""
    u = lambda o: struct.unpack_from('<I', raw, o)[0]
    s = lambda o: struct.unpack_from('<i', raw, o)[0]
    h = lambda o: struct.unpack_from('<H', raw, o)[0]
    hs = lambda o: struct.unpack_from('<h', raw, o)[0]
    kind = u(0)
    return dict(
        group=kind,                       # 0 = group A only, 1 = group B only, 3 = both merged (0x134ac34)
        state=u(4),
        metadata=u(8),
        lens_value=hs(0xc),               # single group: target = defocus + paf lens position; merged: defocus
        shift_8_8=hs(0xe),
        group_contrast=u(0x10),
        line_contrast=[u(0x14 + 4 * i) for i in range(4)],
        group_adjusted_minimum=s(0x24),
        line_adjusted_minimum=[s(0x28 + 4 * i) for i in range(4)],
        group_flags=u(0x38),
        line_flags_or=u(0x3c),
        slope_ratios_8_8=[[h(0x40 + 8 * i + 2 * j) for j in range(4)] for i in range(4)],
        response_ratio_difference_8_8=[h(0x60 + 2 * i) for i in range(4)],
        curve_difference_ratio_8_8=[h(0x68 + 2 * i) for i in range(4)],
        contrast_asymmetry_1_7=h(0x70),
    )


def decode_paf_record_file(raw):
    """0x200-byte ring record written by research/focus-sweep-patch/patch.S."""
    seq, position, motion, s1_count, buffer, areas, copied, request_position = struct.unpack_from('<iiIIIIIi', raw)
    header = decode_aggregate(raw[0x20:0x28])
    records = [decode_area_record(raw[0x28 + i * REC:0x28 + (i + 1) * REC]) for i in range(min(copied, 4))]
    return dict(seq=seq, position=position, motion=motion, s1_result_count=s1_count, pd_buffer=hex(buffer),
                reported_areas=areas, copied_areas=copied, request_position=request_position,
                header=header, records=records)


# ----------------------------------------------------------------- PD buffer (work areas)

class Work:
    """One ``PafCorrelationWork`` inside a PD buffer."""

    def __init__(self, buf, slot, group):
        self.slot, self.group = slot, group
        self.offset = slot * SLOT_BYTES + GROUP_BASE + group * GROUP_STRIDE + WORK_HEADER
        self.w = memoryview(buf)[self.offset:self.offset + 0xb6c]

    def i32(self, o):
        return struct.unpack_from('<i', self.w, o)[0]

    def u32(self, o):
        return struct.unpack_from('<I', self.w, o)[0]

    @property
    def samples(self):
        return self.i32(0x820)

    def curves(self, which):
        """which: 0 first (pass 0) lines 0..3, 1 second (pass 1) lines 0..3; returns 4 lists."""
        base = 0 if which == 0 else 0x3e8
        n = self.samples
        return [[self.u32(base + 200 * line + 4 * k) for k in range(n)] for line in range(4)]

    def average(self, which):
        base = 0x320 if which == 0 else 0x708
        return [self.u32(base + 4 * k) for k in range(self.samples)]

    def line(self, i):
        o = LINE_BASE + i * LINE_STRIDE
        u = lambda k: self.u32(o + k)
        s = lambda k: self.i32(o + k)
        return dict(
            metrics=[u(4 * k) for k in range(8)],
            first=dict(fraction=s(0x20), shift=s(0x24), contrast=u(0x28), adjusted_minimum=s(0x2c), edge=u(0x30)),
            second=dict(fraction=s(0x34), shift=s(0x38), contrast=u(0x3c), adjusted_minimum=s(0x40), edge=u(0x44)),
            slope_ratios=[u(0x64 + 4 * k) for k in range(4)],
            scaled_contrast=u(0x80), flags=u(0x84), shift_8_8=s(0x8c), defocus_16_16=s(0x90))

    def conversion(self):
        """Bytes shaped like cfg+0x110 for the shift to defocus port (scale at +8, table at +0x20)."""
        data = bytearray(0x4c)
        struct.pack_into('<i', data, 8, self.i32(0x824))
        data[0x20:0x4c] = bytes(self.w[0xb40:0xb40 + 0x2c])
        return bytes(data)

    def summary(self):
        lines = [self.line(i) for i in range(4)]
        valid = [i for i, line in enumerate(lines) if line['flags'] == 0]
        return dict(
            state=self.u32(0xb30), valid_lines=self.u32(0xa94), valid_index=valid,
            paf_position=self.i32(0x814), step=self.i32(0xb34), target=self.i32(0xb38), shift_8_8=self.i32(0xb3c),
            group_flags=self.u32(0xb20), group_flag_names=flag_names(self.u32(0xb20)),
            group_contrast=self.u32(0xa9c + 0x28), curve_samples=self.samples, large_move=self.u32(0xa98),
            lines=[dict(flags=l['flags'], shift_8_8=l['shift_8_8'], defocus=l['defocus_16_16'] / 65536,
                        shifts_8_8=[l['first']['shift'], l['second']['shift']],
                        contrast=[l['first']['contrast'], l['second']['contrast']],
                        scaled_saturation=l['scaled_contrast'], totals=[l['metrics'][0], l['metrics'][1],
                                                                       l['metrics'][4], l['metrics'][5]])
                   for l in lines])


def decode_pd_buffer(buf):
    if len(buf) != PD_BYTES:
        raise ValueError('PD buffer must be 0x37510 bytes')
    header = struct.unpack_from('<4I', buf, 0)
    head = dict(state=header[0], processed_area_count=header[1], selected_area_index=header[2], group_mode=header[3])
    areas = []
    for n in range(min(header[1], MAX_SLOTS)):
        record = decode_area_record(bytes(buf[n * SLOT_BYTES + 0x2290:n * SLOT_BYTES + 0x2290 + REC]))
        areas.append(dict(record=record, works=[Work(buf, n, g) for g in (0, 1)]))
    return head, areas


# ----------------------------------------------------------------- Python ports (verification)

def find_minimum(curve, prefer_central, e0ad=30, e0ae=20):
    """0x13492ec (owner C ``paf_find_curve_minimum``)."""
    n = len(curve)
    best = None
    if prefer_central == 1:
        maximum, minimum = max(curve), min(curve)
        if e0ad * 100 <= maximum and n - 2 > 2:
            candidates = []
            for i in range(2, n - 2):
                c = curve
                if (c[i - 2] - c[i - 1] > 0 and c[i - 1] - c[i] > 0 and c[i + 1] - c[i] > 0 and c[i + 2] - c[i + 1] > 0
                        and c[i] * 10 < minimum * e0ae):
                    candidates.append(i)
            if candidates:
                closest = abs((n >> 1) - candidates[0])
                best = candidates[0]
                for i in candidates[1:]:
                    d = abs((n >> 1) - i)
                    if d >= closest:
                        break
                    best, closest = i, d
                return best
    value, index = 0xffffffff, 0
    for i, x in enumerate(curve):
        if x < value:
            value, index = x, i
    return index


def interpolate(curve, prefer_central, **kw):
    """0x13494d8: returns dict(fraction, shift, contrast, adjusted_minimum, edge) (8.8 units)."""
    n = len(curve)
    m = find_minimum(curve, prefer_central, **kw)
    v = curve[m]
    edge, frac, asym = 0, 0, 0
    if m == 0 or m == n - 1:
        shoulder = curve[m + 1] if m == 0 else curve[m - 1]
        edge = 1
    else:
        left, right = curve[m - 1], curve[m + 1]
        den = 0
        if left < right:
            shoulder = right
            if m < 2:
                edge = 1
            else:
                den = curve[m - 2] - left - v + right
        else:
            shoulder = left
            if m + 2 > n - 1:
                edge = 1
            else:
                den = left - v - right + curve[m + 2]
        if not edge and den:
            frac = int((left - right) * 256 / den)       # C division truncates toward zero
            frac = (frac + 0x8000) % 0x10000 - 0x8000
        asym = abs(right - left)
    shift = frac + (m - (n >> 1)) * 256
    shift = (shift + 0x8000) % 0x10000 - 0x8000
    return dict(fraction=frac, shift=shift, contrast=shoulder - v, adjusted_minimum=v - int(asym / 2), edge=edge, index=m)


def combine(mode, first, second):
    """0x1348bcc."""
    if mode == 1:
        return first + second
    if mode == 2:
        return (first - second) * 2
    return first


def _div(a, b):
    q = abs(a) // abs(b)
    return q if (a >= 0) == (b >= 0) else -q


def shift_to_defocus(shift, position, conversion):
    """Port of 0x1348d18 (owner C ``paf_phase_shift_to_lens_defocus``) -> signed 16.16."""
    scale = struct.unpack_from('<i', conversion, 8)[0]
    coeff = list(struct.unpack_from('<11H', conversion, 0x20))
    bps = list(struct.unpack_from('<11h', conversion, 0x36))
    signed = _div(scale * -shift * 256, 1000)
    remaining = abs(signed)
    if signed < 0:
        coeff = coeff[::-1]
        bps = [bps[10] - b for b in bps[::-1]]
        position = bps_last_orig(conversion) - position
    acc = 0

    def solve(integral, c, slope):
        if slope == 0:
            return _div(integral * 100, c)
        radicand = c * c * 65536 + slope * integral * 2
        f = struct.unpack('<f', struct.pack('<f', float(radicand)))[0]
        root = int(struct.unpack('<f', struct.pack('<f', math.sqrt(f)))[0])
        return _div(root * 0x6400 - c * 0x640000, slope)

    def finish(v):
        v = (v + 2 ** 31) % 2 ** 32 - 2 ** 31
        return -v if signed < 0 else v

    if position >= bps[10]:
        return finish(_div(remaining * 100, coeff[10]))
    if position < bps[0]:
        d = bps[0] - position
        seg = _div(d * coeff[0] * 65536, 100)
        if remaining < seg:
            return finish(_div(remaining * 100, coeff[0]))
        remaining -= seg
        acc = d
        idx = 0
    else:
        idx = 1
        while position >= bps[idx]:
            idx += 1
        d = bps[idx] - position
        cur = coeff[idx] + _div((coeff[idx - 1] - coeff[idx]) * -d, bps[idx - 1] - bps[idx])
        seg = _div(d * (coeff[idx] + cur) * 65536, 200)
        if remaining < seg:
            slope = _div((coeff[idx] - coeff[idx - 1]) * 100, bps[idx] - bps[idx - 1])
            return finish(_div(remaining * 100, coeff[idx - 1]) if slope == 0 else solve(remaining, cur, slope))
        remaining -= seg
        acc = d
    while idx < 10:
        pulses = bps[idx + 1] - bps[idx]
        seg = _div(pulses * (coeff[idx] + coeff[idx + 1]) * 65536, 200)
        if remaining < seg:
            slope = _div((coeff[idx + 1] - coeff[idx]) * 100, pulses)
            return finish(acc * 65536 + solve(remaining, coeff[idx], slope))
        remaining -= seg
        acc += pulses
        idx += 1
    return finish(acc * 65536 + _div(remaining * 100, coeff[10]))


def bps_last_orig(conversion):
    return struct.unpack_from('<11h', conversion, 0x36)[10]


PHASE_BIAS_8_8, COMBINE_MODE, PREFER_CENTRAL = 26, 1, 1   # cfg+8, cfg+0x110, cfg+4 of the S1 request: fitted 5,000+ lines


def verify_work(work, prefer_central=PREFER_CENTRAL, combine_mode=COMBINE_MODE, bias=PHASE_BIAS_8_8):
    """Recompute each line from the saved curves; returns mismatch counts against the stored values."""
    conv = work.conversion()
    first, second = work.curves(0), work.curves(1)
    result = dict(lines=0, shift_mismatch=0, interp_mismatch=0, defocus_mismatch=0)
    for i in range(4):
        stored = work.line(i)
        a = interpolate(first[i], prefer_central)
        b = interpolate(second[i], prefer_central)
        result['lines'] += 1
        if (a['shift'], b['shift'], a['contrast'], b['contrast']) != (
                stored['first']['shift'], stored['second']['shift'], stored['first']['contrast'], stored['second']['contrast']):
            result['interp_mismatch'] += 1
        shift = combine(combine_mode, a['shift'], b['shift']) + bias
        if shift != stored['shift_8_8']:
            result['shift_mismatch'] += 1
        if shift_to_defocus(stored['shift_8_8'], work.i32(0x814), conv) != stored['defocus_16_16']:
            result['defocus_mismatch'] += 1
    return result


# ----------------------------------------------------------------- sweep

def target_of(record):
    return record['lens_value']


def decode_sample(directory, sample, with_curves=False):
    prefix = f"p{sample['index']:03d}-r{sample['repeat']}"
    out = dict(index=sample['index'], repeat=sample['repeat'], target_requested=sample['target'],
               position=sample['position'], position_after_af=sample.get('position_after_af'),
               result_ms=sample.get('result_ms'))
    rec = decode_paf_record_file((directory / f'{prefix}-paf-record.bin').read_bytes())
    out['patch'] = dict(seq=rec['seq'], motion=rec['motion'], s1_result_count=rec['s1_result_count'],
                        request_position=rec['request_position'], reported_areas=rec['reported_areas'])
    out['header'] = rec['header']
    buf = (directory / f'{prefix}-pd-buffer.bin').read_bytes()
    head, areas = decode_pd_buffer(buf)
    out['buffer_header'] = head
    out_areas = []
    for n, area in enumerate(areas):
        record = area['record']
        item = dict(area=n, state=record['state'], state_name=STATE_NAMES.get(record['state']),
                    group=record['group'], lens_value=record['lens_value'], shift_8_8=record['shift_8_8'],
                    group_contrast=record['group_contrast'], line_contrast=record['line_contrast'],
                    group_flags=record['group_flags'], group_flag_names=flag_names(record['group_flags']),
                    line_flags_or=record['line_flags_or'], line_flag_names=flag_names(record['line_flags_or']))
        if n < len(rec['records']) and rec['records'][n] != record:
            item['mismatch_with_paf_record'] = True
        used = {0: [0], 1: [1], 3: [0, 1]}.get(record['group'], [])
        groups = {}
        for g in (0, 1):
            work = area['works'][g]
            if work.i32(0x814) == 0 and work.samples == 0:
                continue
            s = work.summary()
            if with_curves:
                s['curves'] = dict(average=[work.average(0), work.average(1)])
            groups['AB'[g]] = s
        item['groups'] = groups
        item['used_groups'] = ['AB'[g] for g in used if 'AB'[g] in groups]
        if item['used_groups']:
            g0 = groups[item['used_groups'][0]]
            item['paf_position'] = g0['paf_position']
            item['defocus'] = g0['step']
            item['target'] = g0['target']
            item['valid_lines'] = g0['valid_lines']
            valid = [l for l in g0['lines'] if l['flags'] == 0]
            item['line_shift_spread_8_8'] = (max(l['shift_8_8'] for l in valid) - min(l['shift_8_8'] for l in valid)) if valid else None
        out_areas.append(item)
    out['areas'] = out_areas
    return out


def decode_sweep(directory, with_curves=False):
    directory = Path(directory)
    session = json.loads((directory / 'session.json').read_text())
    samples = session['lease']['samples']
    decoded = []
    for s in samples:
        try:
            decoded.append(decode_sample(directory, s, with_curves))
        except FileNotFoundError:       # the daylight sweep has a few samples without a saved PAF record (167 of 170 usable)
            pass
    return dict(sweep=directory.name, source_session_sha256=session.get('source_sha256'), firmware_sha256=session.get('firmware_sha256'),
                samples=decoded)


def verify(directory):
    """Recompute from curves for every used group of every sample; returns totals."""
    directory = Path(directory)
    session = json.loads((directory / 'session.json').read_text())
    totals = dict(groups=0, lines=0, interp_mismatch=0, shift_mismatch=0, defocus_mismatch=0)
    for s in session['lease']['samples']:
        prefix = f"p{s['index']:03d}-r{s['repeat']}"
        head, areas = decode_pd_buffer((directory / f'{prefix}-pd-buffer.bin').read_bytes())
        for area in areas:
            for g in (0, 1):
                w = area['works'][g]
                if w.samples <= 0 or w.i32(0x814) == 0:
                    continue
                r = verify_work(w)
                totals['groups'] += 1
                for key in ('lines', 'interp_mismatch', 'shift_mismatch', 'defocus_mismatch'):
                    totals[key] += r[key]
    return totals


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    ap.add_argument('sweeps', nargs='*', help='sweep directories (default: the three recorded sweeps)')
    ap.add_argument('--out', type=Path, default=OUT)
    ap.add_argument('--curves', action='store_true', help='include the averaged cost curves')
    ap.add_argument('--verify', action='store_true', help='recompute minima from the saved curves and report')
    args = ap.parse_args(argv)
    dirs = [Path(p) for p in args.sweeps] or [HARDWARE / name for name in SWEEPS.values()]
    args.out.mkdir(parents=True, exist_ok=True)
    for d in dirs:
        if args.verify:
            print(d.name, json.dumps(verify(d)))
            continue
        data = decode_sweep(d, args.curves)
        path = args.out / f'{d.name}.json'
        path.write_text(json.dumps(data, separators=(',', ':')) + '\n')
        print(path, len(data['samples']), 'samples')
    return 0


if __name__ == '__main__':
    sys.exit(main())
