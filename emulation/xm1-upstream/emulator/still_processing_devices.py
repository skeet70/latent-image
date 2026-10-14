"""Explicit mode0 processing experiment; configuration alone emits nothing.

The ARM initializer owns geometry, IRQ61 registration and start. A separately
armed monochrome row law supplies pixels before a declared status and physical
IRQ. Stock10CE01C acknowledges, unregisters and invokes its saved callback.
This is not a recovered Fujifilm CFA or color-processing implementation.
"""
from __future__ import annotations

import hashlib
import struct

from .hardware import UnsupportedDevice
from . import still_processing_scene as scene
from . import still_processing_prelude as prelude


PROFILE = 'declared-mode0-still-processing-monochrome-irq61-unmeasured'


class StillProcessingControl:
    BASE = 0xfff40000
    OFFSETS = (0, 4, 8, 0xc, 0x10, 0x18, 0x24, 0x2c, 0x38, 0x44, 0x48,
               0x50, 0x58, 0x64, 0x84, 0x88, 0x8c, 0x90, 0xa0, 0xa4, 0xa8)
    AUXILIARY = (0xfff30000, 0xfff38000, 0xfff3805c)

    def __init__(self, board, *, profile=None):
        if profile not in (None, PROFILE):
            raise ValueError('Unknown still processing profile')
        self.board, self.profile = board, profile
        self.registers = dict.fromkeys((*[self.BASE + o for o in self.OFFSETS], *self.AUXILIARY), 0)
        # Declared idle handshakes; stock clears and polls each bit in teardown.
        self.registers[self.BASE] = 0x10001
        self.uploads = []
        self.request = None
        self.state = 'idle'

    def contains(self, address, size):
        if any(a <= address < a + 4 for a in self.registers):
            return True
        if hasattr(self, 'f1c_processing'):
            from . import still_f1c_processing
            if still_f1c_processing.contains(self, address, size):
                return True
        if hasattr(self, 'f1c_idle'):
            from . import still_f1c_idle
            if still_f1c_idle.contains(self, address, size):
                return True
        if hasattr(self, 'prefilter_empty'):
            from . import prefilter_empty
            if prefilter_empty.contains(self, address, size):
                return True
        if hasattr(self, 'prefilter_third'):
            from . import prefilter_third
            if prefilter_third.contains(self, address, size):
                return True
        if hasattr(self, 'prefilter_second'):
            from . import prefilter_second
            if prefilter_second.contains(self, address, size):
                return True
        if hasattr(self, 'prefilter_job'):
            from . import prefilter_active
            if prefilter_active.contains(self, address, size):
                return True
        if hasattr(self, 'prefilter_initialization'):
            from . import prefilter_initialization
            return prefilter_initialization.contains(self, address, size)
        return False

    def _typed(self, cpu, address, size):
        if self.profile != PROFILE or type(cpu) is not int or cpu != 0 or size != 4 or address not in self.registers:
            raise UnsupportedDevice('Still processing requires its explicit CPU0 WORD profile/register')

    def read(self, cpu, address, size):
        if hasattr(self, 'f1c_processing'):
            from . import still_f1c_processing
            if still_f1c_processing.contains(self, address, size):
                return still_f1c_processing.access(self, 'read', cpu, address, size)
        if hasattr(self, 'f1c_idle'):
            from . import still_f1c_idle
            if still_f1c_idle.contains(self, address, size):
                return still_f1c_idle.access(self, 'read', cpu, address, size)
        if hasattr(self, 'prefilter_empty'):
            from . import prefilter_empty
            if prefilter_empty.contains(self, address, size):
                return prefilter_empty.access(self, 'read', cpu, address, size)
        if hasattr(self, 'prefilter_third'):
            from . import prefilter_third
            if prefilter_third.contains(self, address, size):
                return prefilter_third.access(self, 'read', cpu, address, size)
        if hasattr(self, 'prefilter_second'):
            from . import prefilter_second
            if prefilter_second.contains(self, address, size):
                return prefilter_second.access(self, 'read', cpu, address, size)
        if hasattr(self, 'prefilter_job'):
            from . import prefilter_active
            if prefilter_active.contains(self, address, size):
                return prefilter_active.access(self, 'read', cpu, address, size)
        if hasattr(self, 'prefilter_initialization'):
            from . import prefilter_initialization
            if prefilter_initialization.contains(self, address, size):
                return prefilter_initialization.access(self, 'read', cpu, address, size)
        self._typed(cpu, address, size)
        handled, result = prelude.access(self, 'read', address)
        if handled:
            return result
        return self.registers[address]

    def write(self, cpu, address, size, value):
        if hasattr(self, 'f1c_processing'):
            from . import still_f1c_processing
            if still_f1c_processing.contains(self, address, size):
                return still_f1c_processing.access(self, 'write', cpu, address, size, value)
        if hasattr(self, 'f1c_idle'):
            from . import still_f1c_idle
            if still_f1c_idle.contains(self, address, size):
                return still_f1c_idle.access(self, 'write', cpu, address, size, value)
        if hasattr(self, 'prefilter_empty'):
            from . import prefilter_empty
            if prefilter_empty.contains(self, address, size):
                return prefilter_empty.access(self, 'write', cpu, address, size, value)
        if hasattr(self, 'prefilter_third'):
            from . import prefilter_third
            if prefilter_third.contains(self, address, size):
                return prefilter_third.access(self, 'write', cpu, address, size, value)
        if hasattr(self, 'prefilter_second'):
            from . import prefilter_second
            if prefilter_second.contains(self, address, size):
                return prefilter_second.access(self, 'write', cpu, address, size, value)
        if hasattr(self, 'prefilter_job'):
            from . import prefilter_active
            if prefilter_active.contains(self, address, size):
                return prefilter_active.access(self, 'write', cpu, address, size, value)
        if hasattr(self, 'prefilter_initialization'):
            from . import prefilter_initialization
            if prefilter_initialization.contains(self, address, size):
                return prefilter_initialization.access(self, 'write', cpu, address, size, value)
        self._typed(cpu, address, size)
        if type(value) is not int or not 0 <= value <= 0xffffffff:
            raise UnsupportedDevice('Still processing register is not a WORD')
        handled, _ = prelude.access(self, 'write', address, value)
        if handled:
            return
        if address == self.BASE + 4:
            if value != 1 or self.request is None or self.state != 'armed':
                raise UnsupportedDevice('Still processing start requires separately armed mode0 rows')
            prelude.require_descriptor(self)
            self._route()
            self.request['due_tick'] = self.board.virtual_ticks + self.request['completion_ticks']
            if self.request['due_tick'] >= 1 << 64:
                raise UnsupportedDevice('Still processing completion time overflows')
            self.state = 'running'
        elif address == self.BASE + 0x84:
            if self.state != 'irq-pending' or value != self.registers[address] or not value:
                raise UnsupportedDevice('Still processing status requires its actual completion W1C')
            self.registers[address] = 0
            self.state = 'acknowledged'
            return
        elif address == self.BASE:
            if self.state != 'acknowledged':
                raise UnsupportedDevice('Still processing idle teardown precedes completion ACK')
            self.registers[address] = value | 0x10001
            return
        elif self.state not in ('idle', 'armed', 'acknowledged'):
            raise UnsupportedDevice('Still processing configuration changed while running')
        self.registers[address] = value
        if len(self.uploads) >= 65536:
            raise UnsupportedDevice('Still processing configuration exceeds its bounded upload')
        self.uploads.append((address, value))

    def _route(self):
        banks = self.board.interrupts.banks
        bank = banks[0]
        if (not bank.enabled & (1 << 61) or bank.config.get(0x400 + 4 * 61) != 0x10ce01c
                or bank.config.get(0x800 + 4 * 61) != 13 or bank.source_config & (1 << 61)
                or any(b.pending & (1 << 61) or b.active == 61 or b.injected == 61 for b in banks)):
            raise UnsupportedDevice('Still processing lacks the real drained mode0 IRQ61 route')

    def arm_scene(self, *, profile, output_capacity, source_sha256, completion_ticks=1000):
        if (profile != PROFILE or self.profile != PROFILE or self.state != 'idle' or self.request is not None
                or self.board.failure is not None or type(completion_ticks) is not int
                or not 1 <= completion_ticks <= 400000):
            raise UnsupportedDevice('Still processing requires explicit one-shot scene admission')
        prelude.require_descriptor(self)
        self._route()
        descriptor = bytes(self.board.read_memory(0x17d0b68, 0xf0, cpu_id=0))
        words = struct.unpack('<60I', descriptor)
        auxiliary = (getattr(self, 'initialization', None) or {}).get('auxiliary')
        calibrated = words[38] == 1 and auxiliary is not None and auxiliary['descriptor'] == descriptor
        # Only the independently executed ordinary mode0, single source/output
        # request is covered. Additional sources, outputs and processing modes
        # require their own pixel/format contract.
        if (words[0] != 0 or words[5] not in (1, 2) or words[17] != 2
                or any(words[i] for i in (6, 18, 25, 48)) or words[38] and not calibrated):
            raise UnsupportedDevice('Still processing scene supports only the audited mode0 RAW-to-422 request')
        packing = words[5] == 1
        width = words[3] * (2 if packing else 1) // (3 if packing else 2)
        plan = scene.geometry_plan(source_address=words[1], width=width, height=words[4],
            stride_bytes=words[2], packed=packing, destination_address=words[11],
            output_width=words[13] // 2, output_height=words[14], output_stride_bytes=words[12],
            output_capacity=output_capacity, profile=scene.PROFILE)
        if words[3] != plan['source']['active_row_bytes'] or words[13] & 1:
            raise UnsupportedDevice('Still processing descriptor byte density differs')
        expected = {0x50: words[1], 0x44: words[2] * (2 if packing else 1) // (3 if packing else 2),
                    0x58: words[11], 0x48: words[12] // 2,
                    8: (0x10100 if packing else 0x10000) if not calibrated else 0x1000d}
        if calibrated and packing:
            raise UnsupportedDevice('Calibrated still scene requires its qualified U16 source packing')
        if any(self.registers[self.BASE + o] != v for o, v in expected.items()):
            raise UnsupportedDevice('Still processing retained upload disagrees with its real descriptor')
        source = bytes(self.board.read_memory(plan['source']['address'], plan['source']['span_bytes'], cpu_id=0))
        if type(source_sha256) is not str or hashlib.sha256(source).hexdigest() != source_sha256:
            raise UnsupportedDevice('Still processing requires the explicitly owned immutable source bytes')
        geometry = dict(source_address=words[1], width=width, height=words[4], stride_bytes=words[2],
            packed=packing, destination_address=words[11], output_width=words[13] // 2,
            output_height=words[14], output_stride_bytes=words[12], output_capacity=output_capacity,
            profile=scene.PROFILE)
        rendered = scene.render_rows(source, **geometry)
        self.request = dict(descriptor=descriptor, source_sha256=source_sha256, plan=plan,
            rows=rendered['rows'], completion_ticks=completion_ticks, due_tick=None, asserted_tick=None)
        self.state = 'armed'
        return self.snapshot()

    def arm_initialization(self, *, profile):
        return prelude.arm(self, profile)

    def initialization_summary(self):
        return prelude.summary(self)

    def arm_prefilter_initialization(self, *, profile, idle_ticks=16):
        from . import prefilter_initialization
        return prefilter_initialization.arm(self, profile, idle_ticks=idle_ticks)

    def prefilter_initialization_summary(self):
        from . import prefilter_initialization
        return prefilter_initialization.summary(self)

    def arm_prefilter_scene(self, *, profile, completion_ticks=1000, idle_ticks=16):
        from . import prefilter_active
        return prefilter_active.arm(self, profile, completion_ticks=completion_ticks, idle_ticks=idle_ticks)

    def prefilter_scene_summary(self):
        from . import prefilter_active
        return prefilter_active.summary(self)

    def arm_prefilter_second_initialization(self, *, profile, idle_ticks=16):
        from . import prefilter_second
        return prefilter_second.arm_initialization(self, profile, idle_ticks=idle_ticks)

    def arm_prefilter_second_scene(self, *, profile, completion_ticks=1000, idle_ticks=16):
        from . import prefilter_second
        return prefilter_second.arm_scene(self, profile, completion_ticks=completion_ticks, idle_ticks=idle_ticks)

    def prefilter_second_summary(self):
        from . import prefilter_second
        return prefilter_second.summary(self)

    def arm_prefilter_third_initialization(self, *, profile, idle_ticks=16):
        from . import prefilter_third
        return prefilter_third.arm_initialization(self, profile, idle_ticks=idle_ticks)

    def arm_prefilter_third_scene(self, *, profile, completion_ticks=1000, idle_ticks=16):
        from . import prefilter_third
        return prefilter_third.arm_scene(self, profile, completion_ticks=completion_ticks, idle_ticks=idle_ticks)

    def prefilter_third_summary(self):
        from . import prefilter_third
        return prefilter_third.summary(self)

    def arm_prefilter_empty_initialization(self, *, profile, idle_ticks=16):
        from . import prefilter_empty
        return prefilter_empty.arm_initialization(self, profile, idle_ticks=idle_ticks)

    def arm_prefilter_empty_scene(self, *, profile, idle_ticks=16):
        from . import prefilter_empty
        return prefilter_empty.arm_scene(self, profile, idle_ticks=idle_ticks)

    def prefilter_empty_summary(self):
        from . import prefilter_empty
        return prefilter_empty.summary(self)

    def arm_f1c_idle(self, *, profile, idle_ticks=16):
        from . import still_f1c_idle
        return still_f1c_idle.arm(self, profile, idle_ticks=idle_ticks)

    def f1c_idle_summary(self):
        from . import still_f1c_idle
        if hasattr(self, 'f1c_processing'):
            from . import still_f1c_processing
            return still_f1c_idle.summary(still_f1c_processing.idle_view(self))
        return still_f1c_idle.summary(self)

    def arm_f1c_processing(self, *, profile, source_capacity, source_sha256,
                           completion_ticks=1000, idle_ticks=16):
        from . import still_f1c_processing
        return still_f1c_processing.arm(self, profile, source_capacity=source_capacity,
            source_sha256=source_sha256, completion_ticks=completion_ticks, idle_ticks=idle_ticks)

    def arm_f1c_next_initialization(self, *, profile, idle_ticks=16):
        from . import still_f1c_processing
        return still_f1c_processing.arm_next_initialization(self, profile=profile, idle_ticks=idle_ticks)

    def arm_f1c_scene(self, *, profile):
        from . import still_f1c_processing
        return still_f1c_processing.arm_scene(self, profile=profile)

    def f1c_processing_summary(self):
        from . import still_f1c_processing
        return still_f1c_processing.summary(self)

    def tick(self, board):
        if hasattr(self, 'f1c_processing'):
            from . import still_f1c_processing
            still_f1c_processing.tick(self)
        if hasattr(self, 'f1c_idle'):
            from . import still_f1c_idle
            still_f1c_idle.tick(self)
        if hasattr(self, 'prefilter_empty'):
            from . import prefilter_empty
            prefilter_empty.tick(self)
        if hasattr(self, 'prefilter_third'):
            from . import prefilter_third
            prefilter_third.tick(self)
        if hasattr(self, 'prefilter_second'):
            from . import prefilter_second
            prefilter_second.tick(self)
        if hasattr(self, 'prefilter_job'):
            from . import prefilter_active
            prefilter_active.tick(self)
        if hasattr(self, 'prefilter_initialization'):
            from . import prefilter_initialization
            prefilter_initialization.tick(self)
        if self.state != 'running' or board.virtual_ticks < self.request['due_tick']:
            return
        self._route()
        if bytes(board.read_memory(0x17d0b68, 0xf0, cpu_id=0)) != self.request['descriptor']:
            raise UnsupportedDevice('Still processing request changed before its declared completion')
        src = self.request['plan']['source']
        if hashlib.sha256(board.read_memory(src['address'], src['span_bytes'], cpu_id=0)).hexdigest() != self.request['source_sha256']:
            raise UnsupportedDevice('Still processing input changed before its declared completion')
        for address, data in self.request['rows']:
            board.write_memory(address, data, cpu_id=0)
        self.registers[self.BASE + 0x84] = 1
        self.request['asserted_tick'] = board.virtual_ticks
        self.state = 'irq-pending'
        board.queue_irq(0, 61)

    def next_event_tick(self, now):
        due = self.request['due_tick'] if self.state == 'running' else None
        if hasattr(self, 'f1c_processing'):
            from . import still_f1c_processing
            other = still_f1c_processing.next_event_tick(self, now)
            if other is not None:
                due = other if due is None else min(due, other)
        if hasattr(self, 'f1c_idle'):
            from . import still_f1c_idle
            other = still_f1c_idle.next_event_tick(self, now)
            if other is not None:
                due = other if due is None else min(due, other)
        if hasattr(self, 'prefilter_empty'):
            from . import prefilter_empty
            other = prefilter_empty.next_event_tick(self, now)
            if other is not None:
                due = other if due is None else min(due, other)
        if hasattr(self, 'prefilter_third'):
            from . import prefilter_third
            other = prefilter_third.next_event_tick(self, now)
            if other is not None:
                due = other if due is None else min(due, other)
        if hasattr(self, 'prefilter_second'):
            from . import prefilter_second
            other = prefilter_second.next_event_tick(self, now)
            if other is not None:
                due = other if due is None else min(due, other)
        if hasattr(self, 'prefilter_job'):
            from . import prefilter_active
            other = prefilter_active.next_event_tick(self, now)
            if other is not None:
                due = other if due is None else min(due, other)
        if hasattr(self, 'prefilter_initialization'):
            from . import prefilter_initialization
            other = prefilter_initialization.next_event_tick(self, now)
            if other is not None:
                due = other if due is None else min(due, other)
        return due

    def snapshot(self):
        result = dict(device=type(self).__name__, profile=self.profile, state=self.state,
            registers={hex(k): v for k, v in self.registers.items()}, uploads=len(self.uploads),
            due_tick=self.next_event_tick(self.board.virtual_ticks),
            geometry=self.request['plan'] if self.request is not None else None,
            limits='Declared monochrome RAW/nearest YYCbCr rows and timing; real ARM owns ACK, unregister and callback; no CFA/color calibration or capture result')
        if hasattr(self, 'initialization'):
            result['initialization'] = self.initialization_summary()
        if hasattr(self, 'prefilter_initialization'):
            result['prefilter_initialization'] = self.prefilter_initialization_summary()
        if hasattr(self, 'prefilter_job'):
            result['prefilter_scene'] = self.prefilter_scene_summary()
        if hasattr(self, 'prefilter_second'):
            result['prefilter_second'] = self.prefilter_second_summary()
        if hasattr(self, 'prefilter_third'):
            result['prefilter_third'] = self.prefilter_third_summary()
        if hasattr(self, 'prefilter_empty'):
            result['prefilter_empty'] = self.prefilter_empty_summary()
        if hasattr(self, 'f1c_idle'):
            result['f1c_idle'] = self.f1c_idle_summary()
        if hasattr(self, 'f1c_processing'):
            result['f1c_processing'] = self.f1c_processing_summary()
        return result
