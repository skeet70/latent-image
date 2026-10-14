"""Execute X70 ARM display firmware with a bounded peripheral model.

The routine generates every pattern pixel and commits its own MMIO registers.
This is a diagnostic execution environment, not a complete camera boot model.
"""
from __future__ import annotations

import ctypes
import hashlib
import json
import struct
import time
from pathlib import Path

from unicorn import (Uc, UcError, UC_ARCH_ARM, UC_MODE_ARM, UC_PROT_ALL,
    UC_HOOK_CODE, UC_HOOK_BLOCK, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE,
    UC_HOOK_MEM_INVALID, UC_MEM_WRITE)
from unicorn.arm_const import (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2,
    UC_ARM_REG_R3, UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC)

from .display import render_rgb, encode_png

ROOT = Path(__file__).resolve().parents[1]
RAM_SIZE = 0x08000000
RETURN = 0x0fff0000
PATTERN = 0x010e45d4
EXPECTED_FIRMWARE = '7dd5b358a66f38d1fc7dad899a77d35fe4e6ed0edd0ed1d04fbca512d5dc8fd6'


class UnsupportedHardware(RuntimeError):
    """Execution reached a boundary for which no behavior has been defined."""


class DisplayEmulator:
    """One fresh, deterministic ARM machine per diagnostic run."""

    def __init__(self, *, instruction_limit=30_000_000):
        self.limit = instruction_limit
        self.cpu = Uc(UC_ARCH_ARM, UC_MODE_ARM)
        # Both views share actual host memory; source-register addresses work
        # through either alias. Cache-coherence/timing are not simulated.
        self.ram = (ctypes.c_ubyte * RAM_SIZE)()
        pointer = ctypes.addressof(self.ram)
        self.cpu.mem_map_ptr(0, RAM_SIZE, UC_PROT_ALL, pointer)
        self.cpu.mem_map_ptr(0x40000000, RAM_SIZE, UC_PROT_ALL, pointer)
        self.cpu.mem_map(0xfff60000, 0x20000)
        self.manifest = json.loads((ROOT/'firmware/mapped/manifest.json').read_text())
        if self.manifest['source_sha256'] != EXPECTED_FIRMWARE:
            raise ValueError('Unsupported firmware manifest')
        self.section_hashes = {}
        for section in self.manifest['sections']:
            data = (ROOT/section['file']).read_bytes()
            digest = hashlib.sha256(data).hexdigest()
            if len(data) != section['size'] or digest != section['sha256']:
                raise ValueError(f"Mapped firmware validation failed: {section['file']}")
            self.cpu.mem_write(section['address'], data)
            self.section_hashes[section['file']] = digest
        self.instructions = 0
        self.modeled_calls = []
        self.mmio_accesses = []
        self.events = 0
        self.lcd_callbacks = []
        self.pending_wait = None
        self.failure = None
        self.instance = 1
        self.used = False
        self.cpu.hook_add(UC_HOOK_BLOCK, self._block)
        self.cpu.hook_add(UC_HOOK_MEM_INVALID, self._invalid)
        self.cpu.hook_add(UC_HOOK_MEM_READ | UC_HOOK_MEM_WRITE,
                          self._mmio, begin=0xfff60000, end=0xfff7ffff)
        for address in (0x009c1300, 0x009c13d8, 0x009c19e0, 0x009c20d0,
                        0x009c2280, 0x009c2d64, 0x009174bc, 0x010b0394):
            self.cpu.hook_add(UC_HOOK_CODE, self._boundary,
                              begin=address, end=address)

    def put(self, address, value):
        self.cpu.mem_write(address, struct.pack('<I', value))

    def get(self, address):
        return struct.unpack('<I', self.cpu.mem_read(address, 4))[0]

    def _return(self, value=0):
        self.cpu.reg_write(UC_ARM_REG_R0, value)
        self.cpu.reg_write(UC_ARM_REG_PC, self.cpu.reg_read(UC_ARM_REG_LR))

    def _block(self, cpu, address, size, user):
        self.instructions += size // 4
        if self.instructions > self.limit:
            self.failure = UnsupportedHardware(f'Instruction budget exceeded at {address:#x}')
            cpu.emu_stop()

    def _invalid(self, cpu, access, address, size, value, user):
        self.failure = UnsupportedHardware(
            f'Unmapped access type {access} at {address:#x}, PC {cpu.reg_read(UC_ARM_REG_PC):#x}')
        return False

    def _mmio(self, cpu, access, address, size, value, user):
        banks = ((0xfff6b000, 'image0'), (0xfff6e000, 'image1'))
        match = next(((base, name) for base, name in banks
                      if base <= address and address + size <= base + 0x1000), None)
        if match is None:
            self.failure = UnsupportedHardware(f'Unmodeled MMIO access at {address:#x}')
            cpu.emu_stop()
            return
        if access != UC_MEM_WRITE:
            self.failure = UnsupportedHardware(f'Image hardware read is not modeled at {address:#x}')
            cpu.emu_stop()
            return
        base, name = match
        self.mmio_accesses.append({'operation': 'write' if access == UC_MEM_WRITE else 'read',
            'address': hex(address), 'bank': name, 'offset': hex(address-base),
            'size': size, 'value': hex(value if access == UC_MEM_WRITE else
                int.from_bytes(cpu.mem_read(address, size), 'little')),
            'pc': hex(cpu.reg_read(UC_ARM_REG_PC))})

    def _boundary(self, cpu, address, size, user):
        args = [cpu.reg_read(r) for r in
                (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3)]
        names = {0x9c1300: 'RTOS critical enter', 0x9c13d8: 'RTOS critical leave',
            0x9c19e0: 'RTOS current task', 0x9c20d0: 'RTOS event wait',
            0x9c2280: 'RTOS event set', 0x9c2d64: 'RTOS task send',
            0x9174bc: 'diagnostic clock request (not modeled)',
            0x10b0394: 'diagnostic completion response'}
        record = {'address': hex(address), 'name': names[address], 'args': args}
        self.modeled_calls.append(record)
        if address == 0x9c20d0:
            if args[0] not in (1, 0x200000):
                self.failure = UnsupportedHardware(f'Unmodeled event wait {args[0]:#x}')
            else:
                self.pending_wait = (args[0], args[1])
            cpu.emu_stop()
            return
        if address == 0x9c2280:
            if args[0] != 7:
                self.failure = UnsupportedHardware(f'Unmodeled event group {args[0]}')
                cpu.emu_stop()
                return
            self.events |= args[1]
        if address == 0x9c2d64:
            msg, selector, target, length, pointer = struct.unpack('<HBBII', cpu.mem_read(args[1], 12))
            record['message'] = hex(msg)
            record['queue'] = args[0]
            if args[0] != 0x22 or msg != 0x3200 or length != 8:
                self.failure = UnsupportedHardware(f'Unmodeled task message {msg:#x} queue {args[0]}')
                cpu.emu_stop()
                return
            command, callback = struct.unpack('<II', cpu.mem_read(pointer, 8))
            if command != 2 or callback != 0x10e425c:
                self.failure = UnsupportedHardware(f'Unmodeled LCD command {command}')
                cpu.emu_stop()
                return
            record['assumption'] = 'LCD power command completes; actual task and panel timing absent'
            self.lcd_callbacks.append(callback)
        self._return()

    def _execute(self, address, *args, stack=0x01ff0000):
        self.cpu.reg_write(UC_ARM_REG_SP, stack)
        self.cpu.reg_write(UC_ARM_REG_LR, RETURN)
        for register, value in zip((UC_ARM_REG_R0, UC_ARM_REG_R1,
                                    UC_ARM_REG_R2, UC_ARM_REG_R3), args):
            self.cpu.reg_write(register, value)
        pc = address
        while pc != RETURN:
            try:
                self.cpu.emu_start(pc, RETURN, count=self.limit)
            except UcError as exc:
                raise self.failure or UnsupportedHardware(
                    f'ARM exception at {self.cpu.reg_read(UC_ARM_REG_PC):#x}: {exc}') from exc
            if self.failure:
                raise self.failure
            if self.pending_wait:
                mask, result_pointer = self.pending_wait
                self.pending_wait = None
                saved = self.cpu.context_save()
                if mask == 1 and self.get(0xccbaac + self.instance*0x58) == 1:
                    self.modeled_calls.append({'name': 'modeled frame boundary',
                        'address': '0x962d54', 'args': [self.instance],
                        'assumption': 'scheduler invokes real commit at a simulated frame boundary'})
                    self._execute(0x962d54, self.instance, stack=0x01fd0000)
                elif mask == 0x200000 and self.lcd_callbacks:
                    self._execute(self.lcd_callbacks.pop(0), stack=0x01fc0000)
                if self.events & mask != mask:
                    raise UnsupportedHardware(f'Wait {mask:#x} has no modeled producer')
                self.cpu.context_restore(saved)
                self.put(result_pointer, self.events & mask)
                self.events &= ~mask
                self._return()
            pc = self.cpu.reg_read(UC_ARM_REG_PC)
            if pc != RETURN and not self.pending_wait and self.instructions >= self.limit:
                raise UnsupportedHardware('Execution did not complete within the instruction budget')
        return self.cpu.reg_read(UC_ARM_REG_R0)

    def run_pattern(self, scenario='sd', *, route='lcd', lcd_selector=0,
                    packing='yycbcr', matrix=None):
        if self.used:
            raise ValueError('Create a fresh DisplayEmulator for each run')
        if scenario not in ('sd', 'hd') or route not in ('lcd', 'hdmi') or lcd_selector not in (0, 1):
            raise ValueError('Invalid scenario, route or LCD selector')
        self.used = True
        start = time.monotonic()
        self.instance = 0 if route == 'hdmi' else 1-lcd_selector
        self.put(0xbe39fc, 0x42068000)
        self.cpu.mem_write(0x42068500, b'\x01')
        self.cpu.mem_write(0x01fe0000, struct.pack('<5H', 0x500, 0, 0x501, 0x502, 0x503))
        self.cpu.mem_write(0x017d0233, bytes((0xd8 if scenario == 'hd' else 0, 0xd2)))
        self.put(0xc64338, int(route == 'hdmi'))
        self.put(0xc6431c, lcd_selector)
        self.cpu.mem_write(0xccbaf4 + self.instance*0x58, b'\x01')
        status = self._execute(PATTERN, 0x01fe0000)
        if status:
            raise UnsupportedHardware(f'Diagnostic returned failure {status:#x}')
        base = 0xfff6b000 if self.instance == 0 else 0xfff6e000
        source = self.get(base+0x2c) & 0x3fffffff
        dimensions = self.get(base+0x28)
        width, height = ((dimensions >> 16) & 0x1fff)+1, (dimensions & 0x1fff)+1
        expected = (720, 480) if scenario == 'sd' else (1920, 1080)
        if (width, height) != expected or source != 0x04318b00:
            raise UnsupportedHardware('Committed display registers do not match pattern source')
        padding = self.get(base+0x30) & 0xfff
        stride = (width+padding)*2
        raw = bytes(self.cpu.mem_read(source, stride*height))
        matrix = matrix or ('bt601-full' if scenario == 'sd' else 'bt709-full')
        rgb = render_rgb(raw, width, height, stride=stride, packing=packing, matrix=matrix)
        png = encode_png(rgb)
        registers = {hex(offset): hex(self.get(base+offset))
                     for offset in (0, 0x24, 0x28, 0x2c, 0x30, 0xd4, 0xd8, 0xdc)}
        state = {'firmware_sha256': self.manifest['source_sha256'],
            'section_sha256': self.section_hashes, 'scenario': scenario,
            'width': width, 'height': height, 'route': route, 'lcd_selector': lcd_selector,
            'packing': packing, 'matrix': matrix,
            'frame_sha256': hashlib.sha256(raw).hexdigest(),
            'png_sha256': hashlib.sha256(png).hexdigest(), 'frame_png_url': '/frame.png',
            'instructions': self.instructions,
            'instruction_count_kind': 'basic-block estimate; includes intercepted block sizes',
            'return_code': status,
            'elapsed_seconds': round(time.monotonic()-start, 3),
            'display': {'instance': self.instance, 'image_base': hex(base),
                'source_pointer': hex(source), 'source_cpu_alias': hex(source|0x40000000),
                'source_size': [width, height], 'stride': stride, 'padding_pixels': padding,
                'video_enabled': bool(self.get(base) & 1),
                'q12_scaling': [self.get(base+0xd4)>>16, self.get(base+0xd4)&0xffff],
                'registers': registers},
            'modeled_calls': self.modeled_calls, 'mmio_accesses': self.mmio_accesses,
            'limitations': ['Starts at diagnostic pattern routine, not reset or normal shooting boot.',
                'RTOS synchronization, frame boundary and LCD power completion are explicit models.',
                'Pixel packing/matrix inferred from pattern colors; panel scanout not hardware-validated.',
                'No sensor, HDMI PHY/EDID, USB controller, touch, persistent storage or recovery model.',
                'HDMI route selects the shared image engine; it does not establish a working HDMI signal.']}
        return state, raw, png
