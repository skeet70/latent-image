"""Strict idle reset and explicitly selected channel8 JPEG transaction.

The complete stock 91F620 routine requests idle, polls status24 bit16,
acknowledges it, then requests control0 bit8 and polls its self-clear.
Active encoding remains refused until the caller explicitly arms a declared
row-major YYCbCr scene/descriptor after reset. Its completion is a plain
retained action, serviced by tick, and changes no guest result/callback.
"""
from __future__ import annotations

from .hardware import UnsupportedDevice
from . import jpeg_declared, jpeg_successor, jpeg_streamed


IDLE_RESET_PROFILE = 'declared-channel8-idle-reset-unmeasured'
DECLARED_ENCODE_PROFILE = jpeg_declared.PROFILE
STREAM_ENCODE_PROFILE = jpeg_streamed.PROFILE
MAIN_STREAM_ENCODE_PROFILE = jpeg_streamed.MAIN_PROFILE


class JPEGEncodeControl:
    BASE = 0xfff62000
    OFFSETS = (0, 0x24, 0x118)

    def __init__(self, board, *, profile=None, reset_ticks=2):
        if profile not in (None, IDLE_RESET_PROFILE):
            raise ValueError('Unknown JPEG encode idle-reset profile')
        if type(reset_ticks) is not int or not 1 <= reset_ticks < 1 << 64:
            raise ValueError('JPEG encode reset delay must be positive virtual ticks')
        self.board, self.profile, self.reset_ticks = board, profile, reset_ticks
        self.registers = dict.fromkeys(self.OFFSETS, 0)
        self.state = 'idle'
        self.pending = None
        self.resets = 0
        self.last_rejected = None
        self.declared = None

    def contains(self, address, size):
        return any(self.BASE + offset <= address < self.BASE + offset + 4
                   for offset in (jpeg_declared.OFFSETS if self.declared is not None else self.OFFSETS))

    def _reject(self, reason):
        self.last_rejected = reason
        raise UnsupportedDevice('JPEG encode: ' + reason)

    def _offset(self, cpu, address, size):
        offset = address - self.BASE
        offsets = jpeg_declared.OFFSETS if self.declared is not None else self.OFFSETS
        if (cpu not in (0, 1) or type(cpu) is not int or size != 4 or offset not in offsets
                or self.declared is not None and cpu != 0):
            self._reject('requires an aligned stock WORD register and CPU owner')
        return offset

    def read(self, cpu, address, size):
        offset = self._offset(cpu, address, size)
        if self.declared is not None and 'successor' in self.declared:
            value = self.registers[offset]
            jpeg_successor.access(self, 'read', offset, value)
            return value
        if self.declared is not None and self.declared['state'] not in ('closing', 'closed'):
            return jpeg_declared.access(self, cpu, 'read', offset)
        return self.registers[offset]

    def arm_declared_encode(self, profile=DECLARED_ENCODE_PROFILE, *, descriptor_address,
                            stride_bytes, source_sha256, completion_ticks, restart_interval=0):
        """Admit one explicit scene/descriptor at the post-reset upload boundary.

        Source hash and row pitch are caller declarations; opaque hardware
        sampling/restart fields never imply a packing or timing selection.
        Admission supplies neither RAM bytes, status, IRQ nor guest results.
        """
        return jpeg_declared.arm(self, profile, descriptor_address=descriptor_address,
            stride_bytes=stride_bytes, source_sha256=source_sha256,
            completion_ticks=completion_ticks, restart_interval=restart_interval)

    def declared_encode_summary(self):
        return jpeg_declared.summary(self)

    def arm_declared_stream_encode(self, profile=STREAM_ENCODE_PROFILE, *, descriptor_address,
                                   stride_bytes, source_sha256, completion_ticks, restart_interval=0):
        """Admit explicitly owned virtual Resize rows; never populate guest scratch RAM."""
        if profile not in jpeg_streamed.PROFILES:
            self._reject('Resize stream requires its explicit JPEG profile')
        return jpeg_declared.arm(self, profile, descriptor_address=descriptor_address,
            stride_bytes=stride_bytes, source_sha256=source_sha256,
            completion_ticks=completion_ticks, restart_interval=restart_interval)

    def arm_next_initialization(self, profile=jpeg_successor.PROFILE):
        """Admit the next genuine reset after IRQ/task cleanup, without effects."""
        return jpeg_successor.arm_initialization(self, profile)

    def arm_next_declared_encode(self, profile=DECLARED_ENCODE_PROFILE, *, descriptor_address,
                                 stride_bytes, source_sha256, completion_ticks, restart_interval=0):
        """Admit a distinct context after its explicitly owned stock reset."""
        return jpeg_declared.arm(self, profile, descriptor_address=descriptor_address,
            stride_bytes=stride_bytes, source_sha256=source_sha256,
            completion_ticks=completion_ticks, restart_interval=restart_interval, successor=True)

    def tick(self, board):
        if board is not self.board:
            self._reject('completion requires its owning board')
        return jpeg_declared.tick(self)

    def next_event_tick(self, now):
        action = self.declared['pending_action'] if self.declared is not None else None
        return max(now, action['due_tick']) if action is not None else None

    def _schedule(self, cpu, phase):
        due = self.board.virtual_ticks + self.reset_ticks
        if due >= 1 << 64:
            self._reject('reset due tick overflows')
        self.pending = {'cpu': cpu, 'phase': phase, 'due_tick': due}

        def complete():
            if phase == 'idle-requested':
                self.registers[0x24] = 0x10000
                self.state = 'idle-ready'
            else:
                jpeg_successor.before_reset(self)
                self.registers[0] = 0
                self.registers[0x118] = 0
                self.state = 'idle'
                self.resets += 1
                if self.declared is not None and self.declared['state'] == 'closing':
                    self.declared['state'] = 'closed'
                jpeg_successor.completed_reset(self)
            self.pending = None

        self.board.schedule(self.reset_ticks, complete)

    def write(self, cpu, address, size, value):
        offset = self._offset(cpu, address, size)
        if type(value) is not int or not 0 <= value <= 0xffffffff:
            self._reject('invalid register WORD')
        if self.declared is not None:
            if 'successor' in self.declared:
                # The inherited reset law supplies hardware words/delay; the
                # helper owns only this explicit fresh initialization ledger.
                jpeg_successor.access(self, 'write', offset, value)
            elif offset == 0x118 and value == 1:
                jpeg_declared.begin_close(self)
            elif self.declared['state'] not in ('closing', 'closed'):
                jpeg_declared.access(self, cpu, 'write', offset, value)
                return
        if self.profile != IDLE_RESET_PROFILE:
            self._reject('idle-reset profile is not selected')
        if self.pending is not None:
            self._reject('reset transaction is already pending')
        if offset == 0x118 and value == 1 and self.state == 'idle':
            self._schedule(cpu, 'idle-requested')
            self.registers[offset] = value
            self.state = 'idle-requested'
        elif offset == 0x24 and value == 0x10000 and self.state == 'idle-ready':
            self.registers[offset] = 0
            self.state = 'idle-acknowledged'
        elif offset == 0 and value == 0x100 and self.state == 'idle-acknowledged':
            self._schedule(cpu, 'reset-requested')
            self.registers[offset] = value
            self.state = 'reset-requested'
        else:
            self._reject('unknown active operation or idle-reset order')

    def snapshot(self):
        result = {'device': type(self).__name__, 'profile': self.profile,
                'state': self.state, 'resets': self.resets,
                'pending': dict(self.pending) if self.pending else None,
                'last_rejected': self.last_rejected,
                'registers': {hex(self.BASE + offset): value
                              for offset, value in self.registers.items()},
                'limits': ['Stock91F620 idle reset; active operation requires an explicit declared scene/descriptor.',
                           'Row-major YYCbCr/codec/delay are declared; no physical packing, hardware bit identity or capture/file completion.']}
        if self.declared is not None:
            result['declared_encode'] = self.declared_encode_summary()
        return result
