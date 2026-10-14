"""Bounded FFF24000 PDAF extractor with an explicitly declared U16 SAD law.

Programming and raw-result register addresses come from whole stock drivers
92FABC/92FA0C/92FD8C/92FDB8/92FDD8/92FDF8. Pixel selection, arithmetic,
latency are declared experiment conventions.
Data are read from the actual latched RAM sources. Only physical IRQ33 is
asserted; no AF result, guest callback, task state or RTOS event is supplied.
"""
from __future__ import annotations

import hashlib
import struct

from .afs_scene_fixture import PROFILE, PROFILES, MAX_SAMPLES, raw_statistics
from .hardware import UnsupportedDevice
from .virtual_source import ram_span


class AFExtractionControl:
    BASE = 0xfff24000
    RESULT_BASE = 0xfff25004
    IRQ = 0x33
    CALLBACK = 0x1212874
    CONFIG_MASKS = {0x04:0x30000080, 0x08:0x3fffffff, 0x0c:0x3fffffff,
        0x10:0x3fffffff, 0x14:0x1fff, 0x18:0x1fff1fff, 0x1c:0x1fff1fff,
        0x28:0x3ff, 0x2c:0x1f310fff, 0x40:0x3fff, 0xa0:0xffffffff,
        0xa4:0xffffffff, 0xa8:0x3fff, 0xb0:0xf, 0xc0:0x71ff71ff}
    CONFIG_ORDER = tuple(CONFIG_MASKS)
    MAX_JOBS = 256
    stream_raw = None  # Opt-in pdaf_stream.RawSources; class default keeps older checkpoints loadable.

    def __init__(self, board, *, profile=None, data_ticks=400000, stop_ticks=2):
        if profile not in (None, *PROFILES):
            raise ValueError('Unknown AF extraction profile')
        if any(type(v) is not int or not 1 <= v < 1 << 64 for v in (data_ticks, stop_ticks)):
            raise ValueError('AF extraction delays must be positive virtual ticks')
        self.board, self.profile = board, profile
        self.data_ticks, self.stop_ticks = data_ticks, stop_ticks
        self.registers = {self.BASE + o: 0 for o in (*self.CONFIG_MASKS, 0x00, 0xac, 0xb4, 0xb8, 0xc8, 0xcc)}
        self.configuration_step = 0
        self.state = 'idle'
        self.pending = None
        self.result = None
        self.jobs = []
        self.stop = None
        self.busy = False
        self.last_rejected = None

    def contains(self, address, size):
        return (self.BASE <= address < self.BASE + 0xd0
                or self.RESULT_BASE <= address < self.RESULT_BASE + 41 * 4)

    def _reject(self, message):
        self.last_rejected = message
        raise UnsupportedDevice('AF extraction: ' + message)

    def _offset(self, core, address, size):
        if type(core) is not int or core not in (0, 1) or size != 4 or address & 3 or not self.contains(address, size):
            self._reject('requires an aligned WORD and CPU owner')
        return address - self.BASE

    def _route(self, core, *, pending=False):
        banks = self.board.interrupts.banks
        if len(banks) != 2 or [i for i, b in enumerate(banks) if b.enabled & (1 << self.IRQ)] != [core]:
            self._reject('requires one enabled IRQ33 owner')
        bank = banks[core]
        if (bank.config.get(0x400 + 4 * self.IRQ) != self.CALLBACK
                or bank.config.get(0x800 + 4 * self.IRQ) != 13
                or bank.source_config >> self.IRQ & 1 != 0
                or bank.config.get(0x180 + 4 * (self.IRQ // 16), 0) >> (2 * (self.IRQ % 16)) & 3 != 1
                or bank.config.get(0x140 + 4 * (self.IRQ // 16), 0) >> (2 * (self.IRQ % 16)) & 3 != 0
                or not pending and (bank.pending & (1 << self.IRQ) or bank.active == self.IRQ or bank.injected == self.IRQ)):
            self._reject('IRQ33 differs from the stock callback/priority/source route')

    def _sample(self):
        r = {o:self.registers[self.BASE + o] for o in self.CONFIG_MASKS}
        # Stream mode also accepts bits 28/29 of 0xFFF24004: stock 92FABC stores req[9]!=0 at bit28 and the
        # cfg-E003 flag at bit29; their silicon effect is unrecovered and is declared to leave the arithmetic unchanged.
        if r[4] & (0x80 if self.stream_raw is not None else 0x30000080) != 0x80:
            self._reject('only the declared format1 U16 paired-input subset is supported')
        if self.stream_raw is not None:
            from .pdaf_stream import sample_stream
            return sample_stream(self, r)
        if r[8] == r[0xc]:
            self._reject('same-source phase-pixel selection is unresolved; equal RAW bases are not identical apertures')
        if r[0x2c] & 0x00310000:
            self._reject('phase-pixel selector is outside the declared separate-aperture regular-row subset')
        stride = r[0x14]
        x0, y0 = r[0x18] & 0x1fff, r[0x18] >> 16
        x1, y1 = r[0x1c] & 0x1fff, r[0x1c] >> 16
        xstep = (r[0x2c] & 15) + 1
        ystep = (r[0x2c] >> 4 & 15) + (r[0x2c] >> 8 & 15) + 2
        radius_word = r[0x2c] >> 24 & 31
        radius = radius_word // 2
        if (not stride or stride & 15 or x1 < x0 or y1 < y0 or x1 >= stride
                or radius_word & 1 or radius > 20):
            self._reject('unknown U16 stride/ROI/radius geometry')
        xs, ys = tuple(range(x0, x1 + 1, xstep)), tuple(range(y0, y1 + 1, ystep))
        if len(xs) <= 2 * radius or len(xs) * len(ys) > MAX_SAMPLES:
            self._reject('ROI exceeds bounded raw statistics or comparison guard')
        # These ROI sampling meanings are explicit. The silicon phase-pixel
        # pattern/LUT/preprocessing contract is still unresolved.
        rows, spans = [], []
        for pointer in (r[8], r[0xc]):
            if pointer & 7:
                self._reject('unaligned paired source pointer')
            source_rows = []
            for y in ys:
                address, length = pointer + (y * stride + x0) * 2, (x1 - x0 + 1) * 2
                ram_span(address, length)
                raw = bytes(self.board.read_memory(address, length))
                if len(raw) != length:
                    self._reject('source reader returned an incomplete row')
                spans.append(dict(address=address, bytes=length, sha256=hashlib.sha256(raw).hexdigest()))
                samples = struct.unpack('<' + 'H' * (length // 2), raw)
                source_rows.append(tuple(samples[x - x0] for x in xs))
            rows.append(source_rows)
        result = raw_statistics(*rows, radius=radius)
        return dict(configuration=r, sources=spans, result=result)

    def write(self, core, address, size, value):
        offset = self._offset(core, address, size)
        if type(value) is not int or not 0 <= value <= 0xffffffff:
            self._reject('invalid register WORD')
        if offset == 0 and value == 0:
            if self.profile != PROFILE or self.state not in ('delivered', 'stop-observed') or self.stop is not None:
                self._reject('status clear requires a completed declared job or first observed cleanup poll')
            self.stop = dict(cpu=core, requested_tick=self.board.virtual_ticks,
                             due_tick=self.board.virtual_ticks + self.stop_ticks)
            if self.stop['due_tick'] >= 1 << 64:
                self.stop = None
                self._reject('cleanup due tick overflows')
            self.registers[address] = 0
            self.state = 'stopping'
            return
        if offset == 4 and value & 7 == 1:
            if self.state in ('stopping', 'stop-ready') and self.stop is not None:
                if core != self.stop['cpu']:
                    self._reject('cleanup owner changed')
                # Stock92FA38 starts its first clear/poll cycle. This is a
                # cleanup acknowledgement, never another AF measurement.
                return
            if self.profile != PROFILE or self.configuration_step != len(self.CONFIG_ORDER) or self.pending is not None or len(self.jobs) >= self.MAX_JOBS:
                self._reject('start requires a complete declared configuration and remaining job budget')
            if self.state not in ('idle', 'stop-observed'):
                self._reject('previous raw result has not drained')
            self._route(core)
            sample = self._sample()
            now = self.board.virtual_ticks
            if now + self.data_ticks >= 1 << 64:
                self._reject('data due tick overflows')
            self.pending = dict(cpu=core, requested_tick=now, due_tick=now + self.data_ticks, **sample)
            self.state, self.busy = 'pending', True
            return
        if offset in (0xc8, 0xcc):
            if value != 1 or self.state == 'pending':
                self._reject('unknown raw-result acknowledge or busy reset')
            self.registers[address] = value
            return
        if offset in (0xac, 0xb4, 0xb8):
            if value != 0 or self.state == 'pending':
                self._reject('raw scalars only support an idle/completed clear')
            self.registers[address] = 0
            return
        if offset not in self.CONFIG_MASKS or value & ~self.CONFIG_MASKS[offset]:
            self._reject('unknown configuration register or unsupported mask')
        if self.state in ('pending', 'delivered', 'stopping'):
            self._reject('configuration changed before result/cleanup drained')
        if offset == 4 and value & 7 == 0:
            self.configuration_step = 0
            self.result = None
            self.state = 'idle'
        if self.configuration_step >= len(self.CONFIG_ORDER) or offset != self.CONFIG_ORDER[self.configuration_step]:
            self._reject('configuration requires the whole92FABC store order')
        self.registers[address] = value
        self.configuration_step += 1

    def read(self, core, address, size):
        offset = self._offset(core, address, size)
        if self.RESULT_BASE <= address < self.RESULT_BASE + 41 * 4:
            if self.state not in ('delivered', 'stopping', 'stop-ready', 'stop-observed') or self.result is None:
                self._reject('comparison curve read precedes data completion')
            index = (address - self.RESULT_BASE) // 4 - 20
            if not -self.result['radius'] <= index <= self.result['radius']:
                self._reject('comparison read exceeds the configured radius')
            return self.result['curve'][index + self.result['radius']]
        if offset == 0:
            value = self.registers[address]
            if self.state == 'stop-ready' and value == 1:
                self.state, self.stop = 'stop-observed', None
            return value
        if address in self.registers:
            return self.registers[address]
        self._reject('unknown raw-result/configuration read')

    def tick(self, board):
        if board is not self.board:
            self._reject('board owner changed')
        now = board.virtual_ticks
        if self.pending is not None and now >= self.pending['due_tick']:
            job = self.pending
            self._route(job['cpu'])
            if {o:self.registers[self.BASE + o] for o in self.CONFIG_MASKS} != job['configuration']:
                self._reject('latched configuration changed during measurement')
            for span in job['sources']:
                raw = bytes(board.read_memory(span['address'], span['bytes']))
                if hashlib.sha256(raw).hexdigest() != span['sha256']:
                    self._reject('latched source rows changed during measurement')
            result = job['result']
            self.registers[self.BASE + 0xac] = result['scalars'][0] | result['scalars'][1] << 16
            self.registers[self.BASE + 0xb4], self.registers[self.BASE + 0xb8] = result['totals']
            self.registers[self.BASE] = 1
            self.result = result
            self.jobs.append(dict(**job, completed_tick=now))
            self.pending = None
            self.state, self.busy = 'delivered', False
            board.queue_irq(job['cpu'], self.IRQ)
        if self.stop is not None and self.state == 'stopping' and now >= self.stop['due_tick']:
            self.registers[self.BASE] = 1
            self.state = 'stop-ready'

    def next_event_tick(self, now):
        if self.pending is not None:
            return self.pending['due_tick']
        if self.stop is not None and self.state == 'stopping':
            return self.stop['due_tick']
        return None

    def snapshot(self):
        return dict(profile=self.profile, state=self.state, pending=self.pending,
                    jobs=self.jobs, configuration_step=self.configuration_step,
                    last_rejected=self.last_rejected,
                    limits='Declared U16 ROI sampling/SAD/scalars/timing; no recovered PDAF sensor selection or optical calibration')
