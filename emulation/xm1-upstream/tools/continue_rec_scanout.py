#!/usr/bin/env python3
"""Keep a native X-M1 1.50 board warm and sample firmware-selected display RAM.

Example (the profile explicitly selects unmeasured peripheral/render laws):
  PYTHONPATH=native/python:. python tools/continue_rec_scanout.py \
      --profile firmware/x-m1/1.50/emulator-rec.json --serve

After saving a drained snapshot, start a fresh process with:
  PYTHONPATH=native/python:. .venv/bin/python tools/continue_rec_scanout.py \
      --snapshot .tools/paused-rec --instructions 5000000 --serve

Only explicit sample/run operations capture frames. GET requests serve the last
sample; frame counters count host samples, not panel refreshes. PNG and raw plane
concatenations overwrite bounded latest artifacts in a fresh local .tools folder.
Snapshots are local machine state; unsupported CPU/model state refuses save.
No guest memory edits, camera I/O or automatic input are used.
With --menus, key presses change the declared ADC input and execute the stock
scanner. The viewer exposes OK, BACK, Q and the four selector buttons.
"""
import argparse
import copy
from dataclasses import asdict
from datetime import datetime, timezone
import hashlib
import inspect
import json
from pathlib import Path
import struct
import sys
from time import perf_counter
from uuid import uuid4

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

MAX_CONTINUATION = 15_000_000
MODE_DIAL_SETTLE = 40_000_000


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('Duplicate JSON key: ' + key)
        result[key] = value
    return result


def load_profile(path, factory, *, firmware_profile=None):
    """Read options JSON or resolve the canonical X-M1 firmware profile."""
    document = json.loads(Path(path).read_text(), object_pairs_hook=_unique_object)
    if firmware_profile is not None or isinstance(document, dict) and document.get('firmware_id') == 'xm1-150':
        from emulator.boards.profile import resolve
        return {
            'firmware_profile': (firmware_profile if firmware_profile is not None else
                resolve('xm1-150', profile_file=str(Path(path).resolve().relative_to(ROOT)))),
            'instructions_per_tick': 24, 'storage_profile': 'erased',
            'irq_tie_profile': 'kernel-active-scan-order',
            'graphics_profile': 'mode1-bgra8888-straight',
            'backdrop_profile': 'declared-black-canvas-unmeasured',
            'rectangle_profile': 'xm1-150-declared-luma-rectangle-overlay-unmeasured',
            'copy_profile': 'stock-vg-32bpp-fallback',
            'clear_profile': 'declared-vg-ui-primitives-unmeasured',
            'input_processing_profile': 'xm1-150-packed-command-writes',
            'input_phase_profile': 'xm1-150-phase2-ack-unmeasured',
            'input_event_profile': 'xm1-150-configured-phase-clock-unmeasured',
            'input_event_ticks': 200000,
            'pulse_timer_profile': 'xm1-150-configured-timer-clocks-unmeasured',
            'auxiliary_control_profile': 'stock-control3-bit0-clear-ack-unmeasured',
            'producer_stop_profile': 'stock-idle-stop-handshakes-unmeasured',
            'image_work_stop_profile': 'stock-initial-image-banks-bit0-stop-ack-unmeasured',
            'image_work_data_profile': 'xm1-150-declared-continuous-postisp-scene-unmeasured',
            'image_work_data_ticks': 50000,
            'movrsz_profile': 'declared-packed-yycbcr-pyramid-unmeasured',
            'movrsz_ticks': 50000,
            'producer_data_profile': 'xm1-150-declared-continuous-statistics-scene-unmeasured',
        }
    if not isinstance(document, dict) or set(document) != {'options'}:
        raise ValueError('Profile must be X-M1 1.50 or contain only an options object')
    options = document['options']
    if not isinstance(options, dict):
        raise ValueError('Profile options must be an object')
    unknown = set(options) - set(inspect.signature(factory).parameters)
    if unknown:
        raise ValueError('Unknown board options: ' + ', '.join(sorted(unknown)))
    if 'firmware_profile' not in options:
        from emulator.boards.profile import resolve
        options['firmware_profile'] = resolve(
            'xm1-150', profile_file='firmware/x-m1/1.50/emulator-rec.json')
    return options


def attach_installed_media(board, *, media=None, nand_image=None):
    import hashlib
    if media is not None:
        payload = Path(media).read_bytes()
        if len(payload) not in (len(board.nand.flash), board.nand.FLASH_SIZE):
            raise ValueError('Installed media must cover the complete DAT-backed extent or all 128MiB of NAND')
        board.nand.add_data_overlay(0, payload, {
            'kind': 'independent-installed-logical-media', 'path': str(Path(media).resolve()),
            'sha256': hashlib.sha256(payload).hexdigest(),
            'limits': 'Data only; OOB, bad blocks and erased padding remain explicit model fixtures'})
    else:
        board.nand.load_initial_persistent_image(Path(nand_image).read_bytes())

def attach_installed_dat(board, dat):
    """Supply an X-M1 updater container as the installed inverted NAND extent."""
    payload = Path(dat).read_bytes()
    bias = 0x25c
    if len(payload) - bias != len(board.nand.flash):
        raise ValueError('Installed DAT does not match the X-M1 1.50 NAND extent')
    logical = payload[bias:].translate(bytes(255 - value for value in range(256)))
    board.nand.add_data_overlay(0, logical, {
        'kind': 'x-m1-1.50-installed-dat', 'path': str(Path(dat).resolve()),
        'sha256': hashlib.sha256(payload).hexdigest(),
        'dat_bias': bias,
        'limits': 'Container bytes after the X-M1 DAT bias are inverted into logical NAND data'})

def attach_adjustment_nand(board, nand_image):
    """Overlay the complete modeled adjustment block set on installed logical media."""
    from emulator.nand_image import restore_image
    image = Path(nand_image).read_bytes()
    _, _, data, spare = restore_image(
        image, flash_sha256=board.nand.flash_sha256, flash_size=len(board.nand.flash))
    first, end = 0x381 * 64, 0x3a2 * 64
    adjustment_data = {page: value for page, value in data.items() if first <= page < end}
    adjustment_spare = {page: value for page, value in spare.items() if first <= page < end}
    expected = set(range(first, end))
    if set(adjustment_data) != expected or set(adjustment_spare) != expected:
        raise ValueError('Adjustment NAND must commit every page in physical blocks 0x381..0x3a1')
    board.nand.volatile_pages.update(adjustment_data)
    board.nand.volatile_spare_pages.update(adjustment_spare)


def load_recovery_cpu0(board, ram_path):
    import hashlib
    import zlib
    from unicorn import arm_const as arm
    ram_path = Path(ram_path)
    document = json.loads(Path(str(ram_path) + '.state.json').read_text(), object_pairs_hook=_unique_object)
    expected = {'schema', 'kind', 'status', 'route', 'ram_bytes', 'ram_crc32',
                'registers', 'banked', 'vfp', 'cp15', 'mpu', 'mpu_selector',
                'sram_hex', 'device_writes', 'provenance'}
    if (set(document) != expected or document['schema'] != 1
            or document['kind'] != 'ram-rescue-terminal-cpu0' or document['status'] != 'handoff'
            or document['route'] not in ('fat', 'raw') or document['ram_bytes'] != 0x2000000):
        raise ValueError('Recovery requires the observed ram-rescue engine handoff companion')
    payload = ram_path.read_bytes()
    if len(payload) != document['ram_bytes'] or zlib.crc32(payload) != document['ram_crc32']:
        raise ValueError('Recovery RAM extent/checksum differs from the exported CPU0 state')
    values = document['registers']
    if (type(values) is not list or len(values) != 17
            or any(type(value) is not int or not 0 <= value <= 0xffffffff for value in values)
            or values[15] != 0x1800 or values[13] != 0xfff00f00
            or values[16] & 0x3f != 0x13):
        raise ValueError('Recovery CPU0 must be the A32 supervisor native-entry frame')
    sram = bytes.fromhex(document['sram_hex'])
    witness = int.from_bytes(sram[0xefc:0xf00], 'little')
    if (len(sram) != 4096 or witness != (0x8000015c if document['route'] == 'fat' else 0x80000174)
            or values[14] != witness):
        raise ValueError('Recovery SRAM and saved ROM return witness disagree')
    modes = (0x10, 0x11, 0x12, 0x13, 0x17, 0x1b)
    if (type(document['banked']) is not list or len(document['banked']) != len(modes)
            or any(set(row) != {'mode', 'sp', 'lr', 'spsr', 'r8_r12'}
                   or row['mode'] != mode or len(row['r8_r12']) != 5
                   or any(type(value) is not int or not 0 <= value <= 0xffffffff
                          for value in (row['sp'], row['lr'], row['spsr'], *row['r8_r12']))
                   for row, mode in zip(document['banked'], modes))):
        raise ValueError('Recovery banked register frame is incomplete')
    cp_keys = ((1,0,0), (1,0,1), (1,0,2), (3,0,0), (9,1,0), (9,1,1))
    if (type(document['cp15']) is not list or len(document['cp15']) != len(cp_keys)
            or any(type(row) is not list or len(row) != 4 or tuple(row[:3]) != key
                   or type(row[3]) is not int or not 0 <= row[3] <= 0xffffffff
                   for row, key in zip(document['cp15'], cp_keys))
            or document['cp15'][0][3] & 4
            or type(document['mpu_selector']) is not int or not 0 <= document['mpu_selector'] < 8
            or type(document['mpu']) is not list or len(document['mpu']) != 8
            or any(type(row) is not list or len(row) != 3
                   or any(type(value) is not int or not 0 <= value <= 0xffffffff for value in row)
                   for row in document['mpu'])):
        raise ValueError('Recovery requires complete captured R4F-compatible MPU/control state with cache disabled')
    vfp = document['vfp']
    if (set(vfp) != {'fpexc', 'fpscr', 'd0_d15'} or len(vfp['d0_d15']) != 16
            or any(type(value) is not int or not 0 <= value <= 0xffffffff
                   for value in (vfp['fpexc'], vfp['fpscr']))
            or any(type(value) is not int or not 0 <= value < 1 << 64 for value in vfp['d0_d15'])):
        raise ValueError('Recovery VFP register frame is incomplete')
    writes = document['device_writes']
    if (type(writes) is not list
            or any(type(row) is not list or len(row) != 2
                   or row[0] not in (0xfff88084, 0xfff893c4, 0xfff893c0)
                   or type(row[1]) is not int or not 0 <= row[1] <= 0xffffffff for row in writes)):
        raise ValueError('Recovery contains an unsupported bootstrap device transaction')
    reset_only_cp15 = ((1, 0, 1),)
    for crn, crm, opc2, value in document['cp15']:
        if ((crn, crm, opc2) in reset_only_cp15
                and board.cpus[0].cpr_read(15, 0, crn, crm, opc2, 0, False) != value):
            raise ValueError('Recovery exported a reset-only CP15 field differing from the cold engine')
    if (board.instructions or not board.low_rom_alias or board.core_state[1]['runnable']
            or board.core_state[1]['started']):
        raise ValueError('Recovery import requires an independent cold ROM board and parked reset CPU1')
    board.remap_low_ram()
    board.write_memory(0, payload)
    board.write_memory(0xfff00000, sram)
    cpu = board.cpus[0]
    for row in document['banked']:
        cpu.reg_write(arm.UC_ARM_REG_CPSR, (values[16] & ~0x1f) | row['mode'])
        cpu.reg_write(arm.UC_ARM_REG_SP, row['sp'])
        cpu.reg_write(arm.UC_ARM_REG_LR, row['lr'])
        if row['mode'] != 0x10:
            cpu.reg_write(arm.UC_ARM_REG_SPSR, row['spsr'])
        for number, value in enumerate(row['r8_r12'], 8):
            cpu.reg_write(getattr(arm, f'UC_ARM_REG_R{number}'), value)
    cpu.reg_write(arm.UC_ARM_REG_CPSR, values[16])
    for number, value in enumerate(values[:13]):
        cpu.reg_write(getattr(arm, f'UC_ARM_REG_R{number}'), value)
    for register, value in zip((arm.UC_ARM_REG_SP, arm.UC_ARM_REG_LR, arm.UC_ARM_REG_PC), values[13:16]):
        cpu.reg_write(register, value)
    for index, row in enumerate(document['mpu']):
        cpu.cpr_write(15, 0, 6, 2, 0, 0, False, index)
        for opc2, value in zip((0, 2, 4), row):
            cpu.cpr_write(15, 0, 6, 1, opc2, 0, False, value)
    cpu.cpr_write(15, 0, 6, 2, 0, 0, False, document['mpu_selector'])
    for crn, crm, opc2, value in document['cp15']:
        if (crn, crm, opc2) not in reset_only_cp15:
            cpu.cpr_write(15, 0, crn, crm, opc2, 0, False, value)
    cpu.reg_write(arm.UC_ARM_REG_FPEXC, vfp['fpexc'])
    cpu.reg_write(arm.UC_ARM_REG_FPSCR, vfp['fpscr'])
    for number, value in enumerate(vfp['d0_d15']):
        cpu.reg_write(getattr(arm, f'UC_ARM_REG_D{number}'), value)
    from emulator.boot_devices import BootControls
    controls = next(device for device in board.devices if isinstance(device, BootControls))
    for address, value in writes:
        controls.write(0, address, 4, value)
    secondary = board.cpus[1]
    cpu1_reset = {
        'scheduler': copy.deepcopy(board.core_state[1]),
        'registers': {name: secondary.reg_read(getattr(arm, 'UC_ARM_REG_' + name.upper()))
                      for name in (*[f'r{number}' for number in range(13)], 'sp', 'lr', 'pc', 'cpsr')},
        'sctlr': secondary.cpr_read(15, 0, 1, 0, 0, 0, False),
        'source': 'Independent native engine reset plus selected SoC TCM configuration; parked until the actual guest reset-release transaction'}
    board.record('Recovery CPU0 imported from bootstrap execution',
                 ram_sha256=hashlib.sha256(payload).hexdigest(), state=document,
                 cpu1=cpu1_reset,
                 device_state='Independent cold factory defaults; ROM SD/FFSYS device transitions are not captured by the bootstrap engine')
    return {'ram': str(ram_path.resolve()), 'ram_sha256': hashlib.sha256(payload).hexdigest(),
            'route': document['route'], 'cpu0_state': document, 'cpu1_reset': cpu1_reset,
            'limits': 'CPU0 and SRAM are observed bootstrap outputs. Peripheral state is an independent cold fixture, not exported ROM SD state. This continuation does not establish physical recovery.'}


def attach_rom_card(board, card_path, route):
    from emulator.boot_devices import BootControls
    from emulator.sd_card import SDCardController, load_raw_image
    controls = next(device for device in board.devices if isinstance(device, BootControls))
    controller = next(device for device in board.devices if isinstance(device, SDCardController))
    media = load_raw_image(controller.card, card_path, immutable_backing=True)
    gpio = controls.gpio_inputs[0]
    gpio = gpio & ~0xc00 if route == 'fat' else (gpio & ~0xc00) | 0x400
    controls.gpio_inputs[0] = gpio
    controls.registers[controls.GPIO + 0x14] = gpio
    controls.set_gpio_input_pin(12, 7, 0, source='explicit-complete-recovery-card')
    board.record('Independent complete ROM recovery card', route=route, media=media,
                 limits='ROM CMD17/polled completion and host1 are not admitted by the app SD0 model; unsupported traffic stops execution')
    return {'route': route, 'media': media}


def _pick(obj, keys):
    return {key: copy.deepcopy(getattr(obj, key)) for key in keys if hasattr(obj, key)}


def console_summary(state):
    """Keep ordinary execution output small; detailed state stays in the API."""
    display = state.get('display') or {}
    frame = display.get('last_frame') or {}
    ui = state.get('ui') or {}
    return {key: value for key, value in {
        'instructions': state.get('instructions'),
        'virtual_ticks': state.get('virtual_ticks'),
        'failure': state.get('failure'),
        'execution_error': state.get('execution_error'),
        'ui': {key: ui[key] for key in ('mode', 'pending', 'busy') if key in ui},
        'rec_initialization_idle': state.get('rec_initialization_idle'),
        'frame': {key: frame[key] for key in ('width', 'height', 'png_sha256') if key in frame},
        'artifact_directory': state.get('artifact_directory'),
        'performance': state.get('performance'),
    }.items() if value is not None}


def compact_board_state(board):
    """Read modeled registers/retained transactions, not stale MMIO RAM backing."""
    from emulator.rec_state import Reader, ui_state
    from emulator.producer_stream import StatisticsFrame
    ui = ui_state(Reader(board))
    devices = []
    fields = {
        'GraphicsAccelerator': ('clear_profile', 'copy_profile', 'max_command_capture_bytes',
                                'busy', 'jobs_started', 'jobs_completed', 'irqs_raised', 'acknowledgements'),
        'InputProcessingControl': ('event_profile', 'event_enable_generation', 'event_due',
                                  'event_source', 'event_irq', 'event_pending', 'event_arms',
                                  'event_assertions', 'event_acknowledgments', 'event_cancellations',
                                  'event_extra_attempts', 'event_stream_blocked', 'last_event',
                                  'registers', 'command_profile', 'command_armed',
                                  'pending_command', 'last_command', 'last_rejected',
                                  'phase_clock_due'),
        'ProducerControl': ('data_profile', 'busy', 'dma_busy', 'data_jobs', 'data_transfers',
                            'data_irqs', 'pending_data', 'pending_ack', 'last_data',
                            'ack_requests', 'ack_completions', 'rearm', 'stream'),
        'ImageWorkControl': ('data_profile', 'busy', 'dma_busy', 'pending_data', 'data_history', 'stream'),
    }
    for device in board.devices:
        name = type(device).__name__
        if name in ('DisplayScanout', 'DisplayTiming', 'DisplayFormatter'):
            item = copy.deepcopy(device.snapshot())
            item.setdefault('device', name)
        elif name in fields:
            item = {'device': name, **_pick(device, fields[name])}
            if name == 'GraphicsAccelerator':
                job = getattr(device, 'last_job', None) or {}
                item['last_job'] = {key: copy.deepcopy(job[key]) for key in (
                    'operation', 'draw_count', 'filled_paths', 'stroked_paths', 'sampled_images',
                    'destinations', 'completed', 'covered_bytes_in_draw_order', 'rejection') if key in job}
            if name == 'InputProcessingControl':
                item['phase_clock_pending'] = {int(source): asdict(edge)
                                               for source, edge in device.phase_clock_pending.items()}
                item['recent_event_history'] = copy.deepcopy(device.event_history[-8:])
            if name == 'ProducerControl' and item.get('stream') is not None:
                frame = item['stream'].get('frame')
                if type(frame) is StatisticsFrame:
                    item['stream']['frame'] = asdict(frame)
        elif name in ('BootControls', 'PulseOutputs', 'AudioSerialControl', 'FujiDecompressor', 'LoaderMemoryDMA', 'InputAuxiliaryControl', 'SDCardController'):
            item = {'device': name, 'state': device.snapshot()}
        else:
            continue
        devices.append(item)
    deadlines = []
    for device in board.devices:
        if hasattr(device, 'next_event_tick'):
            due = device.next_event_tick(board.virtual_ticks)
            if due is not None:
                deadlines.append({'device': type(device).__name__, 'tick': due})
    return {'instructions': board.instructions, 'virtual_ticks': board.virtual_ticks,
            'entry': board.entry, 'failure': copy.deepcopy(board.failure), 'quiescent': board.quiescent,
            'ui': ui, 'rec_initialization_idle': None, 'devices': devices,
            'milestones': copy.deepcopy(board.milestones),
            'events': copy.deepcopy(board.event_log[-12:]), 'pending_deadlines': deadlines,
            'queued_callbacks': len(board.scheduled),
            'readiness_limits': 'Current idle flags do not establish handshake history or responsive shooting.'}


def _guest_boundary(board):
    """Cheap execution/IRQ boundary check; this is not a whole-DRAM fingerprint."""
    from unicorn.arm_const import UC_ARM_REG_PC, UC_ARM_REG_SP, UC_ARM_REG_CPSR
    return (board.instructions, board.virtual_ticks, copy.deepcopy(board.failure),
            tuple(tuple(cpu.reg_read(reg) for reg in (UC_ARM_REG_PC, UC_ARM_REG_SP, UC_ARM_REG_CPSR))
                  for cpu in board.cpus), copy.deepcopy(board.interrupts.snapshot()),
            tuple((item[0], item[1], id(item[2])) for item in board.scheduled))


class WarmScanoutSession:
    """Small make_server-compatible adapter; also usable on an existing board."""
    def __init__(self, board, output, *, options=None):
        self.board, self.output = board, Path(output)
        self.output.mkdir(parents=True, exist_ok=False)
        self.options = dict(options or {})
        profile = self.options.get('firmware_profile')
        if profile is not None and not isinstance(profile, dict):
            self.options['firmware_profile'] = {
                'firmware_id': profile.firmware_id, 'profile_file': str(profile.profile_file),
                'profile_sha256': profile.profile_sha256}
        self.state = None
        self.raw = self.png = b''
        self.checkpoint = None
        self.execution_error = None

    def sample(self, pixels=True):
        """Sample both display planes; pixels=False keeps only plane metadata.

        A metadata sample (used between settle continuations) skips
        composition, PNG encoding and frame files; session.png is then empty
        until compose_frames() renders the same boundary.
        """
        from emulator.scanout import UnsupportedScanout
        before = _guest_boundary(self.board)
        frames, samples = {}, []
        for instance in (0, 1):
            try:
                frame = self.board.display.frame_boundary(instance, cpu_id=0, pixels=pixels)
            except UnsupportedScanout as error:
                samples.append({'instance': instance, 'render_error': str(error), 'last_frame': None,
                                'frame_count': self.board.display.frames[instance]})
                continue
            samples.append({'instance': instance, 'last_frame': None if frame is None else frame.state,
                            'frame_count': self.board.display.frames[instance], 'render_error': None})
            if frame is not None:
                frames[instance] = frame
        if before != _guest_boundary(self.board):
            raise RuntimeError('Host scanout sampling changed the CPU/tick/IRQ boundary')
        # Clear old pixels even if all planes are now disabled/unsupported.
        # This prevents the viewer from presenting an earlier frame as current.
        self.raw = self.png = b''
        selected = min(frames) if frames else None
        self._pixels_pending = not pixels
        if pixels:
            self._write_frames(frames)
        elif selected is not None:
            self.raw = frames[selected].raw
        self.state = compact_board_state(self.board)
        self.state.update(
            display={'selected_instance': selected,
                     'frame_count': 0 if selected is None else self.board.display.frames[selected],
                     'last_frame': None if selected is None else frames[selected].state,
                     'instances': samples},
            profiles=self.options, storage_profile=copy.deepcopy(getattr(self.board, 'storage_profile', None)),
            execution_backend={'name': 'rust-unicorn'}, artifact_directory=str(self.output.resolve()),
            resume_checkpoint={'present': self.checkpoint is not None},
            execution_error=copy.deepcopy(self.execution_error),
            recovery=copy.deepcopy(self.options.get('recovery')),
            sampling={'cpu_tick_irq_boundary_unchanged': True,
                      'limits': 'Host sampling changes only scanout counters/metadata and event log; no full-memory fingerprint is taken.'})
        if self.execution_error is not None and self.state['failure'] is None:
            self.state['failure'] = {'message': 'Strict continuation stopped outside CPU dispatch',
                                     **self.execution_error}
        if self.checkpoint is not None:
            self.state['resume_checkpoint']['metadata'] = self.checkpoint.metadata
        if 'recovery' in self.options:
            self.state['machine'] = self.board.snapshot()
            self.state['installed_nand'] = self.board.nand.snapshot()
            (self.output / 'state.json').write_text(json.dumps(self.state, indent=2) + '\n')
        return self.state

    def _write_frames(self, frames):
        selected = min(frames) if frames else None
        for instance in (0, 1):
            for suffix in ('bin', 'png'):
                target = self.output / f'frame-{instance}.{suffix}'
                if instance not in frames:
                    target.unlink(missing_ok=True)
                    continue
                data = frames[instance].raw if suffix == 'bin' else frames[instance].png
                temporary = target.with_suffix(target.suffix + '.tmp')
                temporary.write_bytes(data)
                temporary.replace(target)
        if selected is not None:
            self.raw, self.png = frames[selected].raw, frames[selected].png

    def compose_frames(self):
        """Render pixels for the boundary of the last metadata-only sample.

        Leaves frame counters and the event log as a pixel sample would have.
        """
        from emulator.scanout import UnsupportedScanout
        if not getattr(self, '_pixels_pending', False) or self.state is None:
            return self.state
        before = _guest_boundary(self.board)
        frames = {}
        for item in self.state['display']['instances']:
            instance = item['instance']
            if item['render_error'] is not None or item['last_frame'] is None:
                continue
            try:
                frame = self.board.display.compose(instance, cpu_id=0)
            except UnsupportedScanout as error:
                self.board.display.render_errors[instance] = str(error)
                item['render_error'] = str(error)
                continue
            if frame is not None:
                frames[instance] = frame
                item['last_frame'] = frame.state
        if before != _guest_boundary(self.board):
            raise RuntimeError('Host scanout composition changed the CPU/tick/IRQ boundary')
        self._pixels_pending = False
        self.raw = self.png = b''
        self._write_frames(frames)
        selected = min(frames) if frames else None
        self.state['display']['last_frame'] = None if selected is None else frames[selected].state
        return self.state

    def run(self, options):
        if not isinstance(options, dict) or set(options) - {'limit', 'restore', 'reset', 'pixels'}:
            raise ValueError('Only limit, restore, reset=false and pixels are supported')
        if type(options.get('pixels', True)) is not bool:
            raise ValueError('Pixels must be a boolean')
        limit = options.get('limit', 0)
        if type(limit) is not int or not 0 <= limit <= MAX_CONTINUATION:
            raise ValueError('Continuation limit must be 0..15000000; zero samples without execution')
        if type(options.get('reset', False)) is not bool or options.get('reset', False):
            raise ValueError('Start a new process to reset and cold boot')
        if type(options.get('restore', False)) is not bool:
            raise ValueError('Restore must be a boolean')
        if options.get('restore', False):
            if self.checkpoint is None:
                raise ValueError('No process-local checkpoint saved')
            self.checkpoint.restore(self.board)
            self.execution_error = None
        if limit and self.execution_error is not None:
            raise ValueError('Restore the saved checkpoint or start a new process after the strict stop')
        started, previous = perf_counter(), self.board.instructions
        if limit:
            try:
                self.board.run(limit + 2 * self.board.quantum,
                               pause_after=self.board.instructions + limit)
            except Exception as error:
                # Some strict tick errors escape Board.run before failure is
                # assigned. Preserve the paused frame/status, without altering
                # guest RAM or clearing the failed model transaction.
                self.execution_error = {'type': type(error).__name__, 'error': str(error)}
        self.sample(pixels=options.get('pixels', True))
        seconds = perf_counter() - started
        executed = self.board.instructions - previous
        self.state['performance'] = {'execution_and_frame_sampling_seconds': seconds,
                                     'instructions_this_run': executed,
                                     'instructions_per_second': executed / seconds if seconds else None}
        return self.state

    def save_checkpoint(self):
        from x70_native.checkpoint import capture
        self.checkpoint = capture(self.board)
        return self.checkpoint.metadata

    def save_snapshot(self, directory):
        from x70_native.portable_checkpoint import save
        if self.execution_error is not None:
            raise ValueError('Restore the saved checkpoint before saving a stopped machine')
        manifest = save(self.board, directory)
        return {'snapshot': str(Path(directory).resolve()),
                'instructions': self.board.instructions, 'virtual_ticks': self.board.virtual_ticks,
                'schema': manifest['schema']}

    def save_nand(self, path):
        import hashlib
        payload = self.board.nand.export_persistent_image()
        with Path(path).open('xb') as target:
            target.write(payload)
        return {'installed_nand': str(Path(path).resolve()),
                'sha256': hashlib.sha256(payload).hexdigest(),
                'limits': 'Committed NAND backing only; no CPU, RAM, IRQ or controller state survives cold reload'}

    def save_sd(self, path):
        from emulator.sd_card import SDCardController, save_raw_image
        controller = next(device for device in self.board.devices if isinstance(device, SDCardController))
        return save_raw_image(controller.card, path)



class _FF80EndpointBridge:
    """Host-side adapter for opaque transfers through the modeled X-M1 USB endpoint."""
    def __init__(self, board, session, *, attached=False):
        from emulator.power_devices import PowerDevices
        from emulator.usb_devices import USBControl
        self.board = board
        self.session = session
        self.power = next(device for device in board.devices if type(device) is PowerDevices)
        usb = next(device for device in board.devices if type(device) is USBControl)
        self.endpoint = usb._endpoint
        if self.endpoint is None or self.power.usb_connected != attached:
            raise ValueError('RAM-push bridge requires endpoint transport and a post-boot attach')
        self.trace_sites = {}
        self.trace_hits = []
        self.screenshots = []
        self.timing_transitions = []
        self.transfer_count = 0
        self.keepalive_inputs = []
        self.keepalive_enabled = False
        self.rendezvous_activity = []
        self.rendezvous_activity_complete = False
        self.key_log = []
        self.transfer_log = []

    def use_timing_ratio(self, ratio, phase):
        wanted = (ratio, 1)
        if self.board.tick_ratio != wanted:
            before = self.board.tick_ratio
            self.board.set_instructions_per_tick(ratio)
            self.timing_transitions.append({
                'phase': phase, 'instructions': self.board.instructions,
                'before': f'{before[0]}/{before[1]}', 'after': f'{ratio}/1'})

    # Command-dial push in stock 1.50 live view resets auto power-off and leaves the screen and the
    # MENU path unchanged (emulated).
    KEEPALIVE_KEY = 'dial-push'
    # Each keepalive runs inside a host transfer's 5 s budget.
    KEEPALIVE_PULSE = 2_000_000

    def keep_session_awake(self):
        self.use_timing_ratio(24, 'inactivity reset')
        before = self.board.instructions
        state = self.session.key({'key': self.KEEPALIVE_KEY, 'render': False,
                                  'hold_instructions': self.KEEPALIVE_PULSE,
                                  'release_instructions': self.KEEPALIVE_PULSE})
        if self.board.failure is not None or self.session.execution_error is not None:
            raise RuntimeError(f'inactivity reset failed: {state}')
        self.keepalive_inputs.append({
            'key': self.KEEPALIVE_KEY, 'before_instructions': before,
            'after_instructions': self.board.instructions})
        self.use_timing_ratio(2400, 'USB transfer')

    # A 455,645-instruction UP hold at the USB ratio did not enter stock 1.50 focus-area selection;
    # 1,778,831 did (emulated).
    RENDEZVOUS_HOLD = 4_000_000

    def hold_rendezvous_inputs(self):
        from emulator.menu_inputs import MenuKey
        from emulator.sd_card import MediaAction
        self.session.inputs.press(MenuKey.UP)
        self.session._sd_controller().media_control(MediaAction.PROTECT)
        return self.board.instructions

    def release_rendezvous_inputs(self, latched):
        from emulator.sd_card import MediaAction
        held = self.board.instructions - latched
        if held < self.RENDEZVOUS_HOLD:
            self.run_for(self.RENDEZVOUS_HOLD - held)
        self.session.inputs.release()
        self.session._sd_controller().media_control(MediaAction.UNPROTECT)

    def release_rendezvous_activity(self):
        if not self.rendezvous_activity_complete:
            return
        self.release_rendezvous_inputs(self.rendezvous_activity[-1]['latched_instructions'])
        event = self.rendezvous_activity[-1]
        event['released_instructions'] = self.board.instructions
        event['menu_after'] = self.key_state(self.session.sample(False))
        event['cores_after'] = self.board.snapshot()['cores']
        self.rendezvous_activity_complete = False

    def run_until(self, label, predicate, budget):
        end = self.board.instructions + budget
        while not predicate():
            failure = self.board.failure or self.session.execution_error
            if failure is not None:
                raise RuntimeError(f'{label}: {failure}')
            if self.board.instructions >= end:
                raise TimeoutError(f'{label}: instruction budget expired; usb={self.endpoint.snapshot()}')
            target = min(end, self.board.instructions + 1_000_000)
            try:
                self.board.run(target - self.board.instructions + 2 * self.board.quantum,
                               pause_after=target)
            except Exception as error:
                self.session.execution_error = {'type': type(error).__name__, 'error': str(error)}

    def run_for(self, count):
        target = self.board.instructions + count
        while self.board.instructions < target:
            self.run_until('bounded continuation',
                           lambda: self.board.instructions >= target,
                           target - self.board.instructions)

    def take_in(self, endpoint_address, count):
        chunks = bytearray()
        port = dict(self.endpoint.contract.in_endpoints)[endpoint_address]
        while len(chunks) < count:
            def reply_ready():
                if self.endpoint.tx[port]:
                    return True
                if not self.endpoint.regs[0x030] & 4:
                    self.endpoint.host_in_token(endpoint_address)
                return False
            self.run_until('bulk IN reply', reply_ready, 30_000_000)
            packet = self.endpoint.host_in(endpoint_address, count - len(chunks))
            chunks.extend(packet)
            self.run_for(20_000)
            if len(packet) < self.endpoint.contract.packet_bytes:
                break
        self.run_until('bulk IN DMA teardown',
                       lambda: all(not channel[0x004] & 1 for channel in self.endpoint.dma),
                       30_000_000)
        return bytes(chunks)

    def control_out(self, payload):
        setup = bytes((0x43, 0, 0, 0, 0, 0, len(payload) & 0xff, len(payload) >> 8))
        self.endpoint.host_setup(setup)
        self.run_until('control OUT setup consumption',
                       lambda: self.endpoint.setup_position == len(self.endpoint.setup),
                       30_000_000)
        self.run_for(20_000)
        self.endpoint.host_out(0, payload)
        self.run_until('control OUT status', lambda: bool(self.endpoint.tx[0]), 30_000_000)
        if self.endpoint.host_in(0x80, 64):
            raise RuntimeError('control OUT status packet was not empty')
        self.run_for(20_000)

    def control_status(self, setup):
        self.endpoint.host_setup(setup)
        self.run_until('control status IN', lambda: bool(self.endpoint.tx[0]), 30_000_000)
        if self.endpoint.host_in(0x80, 64):
            raise RuntimeError('control status packet was not empty')

    def control_in(self, setup, count):
        self.endpoint.host_setup(setup)
        self.run_until('control IN data', lambda: bool(self.endpoint.tx[0]), 30_000_000)
        payload = self.endpoint.host_in(0x80, count)
        self.endpoint.host_out(0, b'')
        return payload

    def enumerate(self):
        self.power.set_usb_connected(True)
        return self.enumerate_attached(140_000_000, bytes((0xcb, 0x04, 0x80, 0xff)))

    def enumerate_attached(self, budget, expected_ids=None):
        """Standard enumeration after the cable level is raised. EXPECTED_IDS, when given, is the
        required VID:PID field (device descriptor bytes 8..11)."""
        self.run_until('USB controller activation',
                       lambda: self.endpoint.regs[0x090] & 6 == 6, budget)
        self.endpoint.host_reset()
        self.run_for(1_000_000)
        device = self.control_in(bytes((0x80, 6, 0, 1, 0, 0, 18, 0)), 18)
        if len(device) != 18 or (expected_ids is not None and device[8:12] != expected_ids):
            raise RuntimeError(f'unexpected device descriptor {device.hex()}')
        self.control_status(bytes((0, 5, 1, 0, 0, 0, 0, 0)))
        head = self.control_in(bytes((0x80, 6, 0, 2, 0, 0, 9, 0)), 9)
        count = int.from_bytes(head[2:4], 'little')
        if len(head) != 9 or not 9 <= count <= 256:
            raise RuntimeError(f'unexpected configuration descriptor head {head.hex()}')
        configuration = self.control_in(
            bytes((0x80, 6, 0, 2, 0, 0, count & 0xff, count >> 8)), count)
        self.control_status(bytes((0, 9, configuration[5], 0, 0, 0, 0, 0)))
        self.run_for(1_000_000)
        return device, configuration

    def bulk_out(self, endpoint_address, payload):
        if endpoint_address != 2:
            raise ValueError('RAM-push bridge admits only bulk OUT endpoint 2')
        self.run_until('bulk OUT DMA readiness',
                       lambda: (self.endpoint.dma_active is None
                                or (self.endpoint.dma_active['direction'] == 'out'
                                    and self.endpoint.dma_active['exact'] == len(payload))),
                       30_000_000)
        for start in range(0, len(payload), self.endpoint.contract.packet_bytes):
            chunk = payload[start:start + self.endpoint.contract.packet_bytes]
            self.endpoint.host_out(endpoint_address, chunk)
            self.run_until('bulk OUT packet consumption',
                           lambda: self.endpoint.rx_positions[1] == len(self.endpoint.rx[1]),
                           30_000_000)
            self.run_for(20_000)
        self.run_until('bulk OUT DMA completion',
                       lambda: self.endpoint.dma_active is None,
                       30_000_000)
        self.run_until('bulk OUT DMA teardown',
                       lambda: all(not channel[0x004] & 1 for channel in self.endpoint.dma),
                       30_000_000)
        self.run_for(100_000)

    def transfer(self, request):
        started, instructions = perf_counter(), self.board.instructions
        count, payload = self._transfer(request)
        frame = bytes.fromhex(request.get('bytes', ''))
        self.transfer_log.append({
            'transfer': request.get('transfer'), 'bytes': count,
            'opcode': frame[:4][::-1].hex() if len(frame) >= 4 else None,
            'function': int.from_bytes(frame[16:20], 'little') if len(frame) >= 20 else None,
            'instructions': self.board.instructions - instructions,
            'seconds': round(perf_counter() - started, 4)})
        return count, payload

    def _transfer(self, request):
        self.release_rendezvous_activity()
        self.transfer_count += 1
        if self.keepalive_enabled and self.transfer_count % 32 == 0:
            self.keep_session_awake()
        self.use_timing_ratio(2400, 'USB transfer')
        kind = request.get('transfer')
        if kind == 'control':
            payload = bytes.fromhex(request['bytes'])
            if len(payload) != 12:
                raise ValueError('control transfer must carry the internal twelve-byte request')
            self.control_out(payload)
            return len(payload), b''
        if kind == 'bulk-out':
            payload = bytes.fromhex(request['bytes'])
            self.bulk_out(request['endpoint'], payload)
            return len(payload), b''
        if kind == 'bulk-in':
            count = request['count']
            payload = self.take_in(request['endpoint'], count)
            return len(payload), payload
        raise ValueError('unknown bridge transfer kind')

    def set_trace_sites(self, sites, enable_keepalive=True):
        from emulator.rec_state import Reader, task_state
        from unicorn.arm_const import (UC_ARM_REG_CPSR, UC_ARM_REG_PC, UC_ARM_REG_R0,
                                       UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3,
                                       UC_ARM_REG_SP)
        if not isinstance(sites, dict) or any(not isinstance(key, str)
                or not isinstance(value, str) for key, value in sites.items()):
            raise ValueError('trace sites must be a string map')
        parsed = {int(address, 0): label for address, label in sites.items()}
        _, tasks = task_state(Reader(self.board))
        stacks = []
        for task in tasks:
            if all(name in task for name in ('stack_base', 'stack_bytes', 'task_id')):
                base = int(task['stack_base'], 16)
                stacks.append((task['cpu'], base, base + task['stack_bytes'], task['task_id']))
        self.trace_sites = parsed


        def core_states():
            states = []
            for owner, owner_cpu in enumerate(self.board.cpus):
                stack = owner_cpu.reg_read(UC_ARM_REG_SP)
                task_id = next((task for cpu_id, low, high, task in stacks
                                if cpu_id == owner and low <= stack < high), None)
                states.append({
                    'core': owner, 'pc': hex(owner_cpu.reg_read(UC_ARM_REG_PC)),
                    'cpsr': hex(owner_cpu.reg_read(UC_ARM_REG_CPSR)),
                    'irq_masked': bool(owner_cpu.reg_read(UC_ARM_REG_CPSR) & 0x80),
                    'sp': hex(stack), 'task': task_id,
                    'task_hex': None if task_id is None else hex(task_id),
                    'retired_instructions': self.board.core_state[owner]['instructions'],
                    'virtual_ticks': self.board.virtual_ticks})
            return states
        def handler(cpu, core, address):
            sp = cpu.reg_read(UC_ARM_REG_SP)
            task = next((task_id for owner, low, high, task_id in stacks
                         if owner == core and low <= sp < high), None)
            identities = list(self.board._native.cp14_identity())
            request = bytes(self.board.read_memory(0x01469a9c, 0xa4))
            self.trace_hits.append({
                'site': self.trace_sites[address], 'address': hex(address), 'core': core,
                'task': task, 'task_hex': None if task is None else hex(task),
                'registers': {name: hex(cpu.reg_read(register)) for name, register in
                              (('r0', UC_ARM_REG_R0), ('r1', UC_ARM_REG_R1),
                               ('r2', UC_ARM_REG_R2), ('r3', UC_ARM_REG_R3))},
                'raw_identity': hex(identities[core]), 'all_raw_identities': list(map(hex, identities)),
                'sp': hex(sp), 'instructions': self.board.instructions,
                'request': {'opcode': hex(int.from_bytes(request[0:4], 'little')),
                            'tag_or_function': hex(int.from_bytes(request[0x10:0x14], 'little')),
                            'tail_sha256': hashlib.sha256(request[0x84:0xa4]).hexdigest()}})
            label = self.trace_sites[address]
            if label == 'rendezvous-locked' and (
                    not self.rendezvous_activity
                    or 'released_instructions' in self.rendezvous_activity[-1]):
                locked = core_states()
                if core != 0 or any(not state['irq_masked'] for state in locked):
                    raise RuntimeError(f'rendezvous trace lacks two masked cores: {locked}')
                latched = self.hold_rendezvous_inputs()
                self.rendezvous_activity.append({
                    'latched_instructions': latched,
                    'latched_virtual_ticks': self.board.virtual_ticks,
                    'inputs': ['menu-up', 'sd-write-protect'],
                    'cores_locked': locked,
                    'window_exits': []})
            elif (label in ('cpu0-window-exit', 'cpu1-window-exit')
                  and self.rendezvous_activity
                  and 'callback_instructions' not in self.rendezvous_activity[-1]):
                self.rendezvous_activity[-1]['window_exits'].append({
                    'label': label, 'global_instructions': self.board.instructions,
                    'virtual_ticks': self.board.virtual_ticks, 'cores': core_states()})
            elif label == 'query-callback-result' and self.rendezvous_activity:
                event = self.rendezvous_activity[-1]
                if 'callback_instructions' not in event:
                    labels = [item['label'] for item in event['window_exits']]
                    if labels.count('cpu0-window-exit') != 1 or labels.count('cpu1-window-exit') != 1:
                        raise RuntimeError(f'incomplete rendezvous window trace: {labels}')
                    event['callback_instructions'] = self.board.instructions
                    event['callback_virtual_ticks'] = self.board.virtual_ticks
                    event['cores_at_callback'] = core_states()
                    event['retired_instruction_deltas'] = [
                        after['retired_instructions'] - before['retired_instructions']
                        for before, after in zip(event['cores_locked'], event['cores_at_callback'])]
                    event['virtual_tick_delta'] = (
                        event['callback_virtual_ticks'] - event['latched_virtual_ticks'])
                    if any(delta <= 0 for delta in event['retired_instruction_deltas']):
                        raise RuntimeError(
                            f'rendezvous lacks two-core execution: {event["retired_instruction_deltas"]}')
                    self.rendezvous_activity_complete = True
        for core in range(2):
            self.board._native.set_breakpoints(core, sorted(parsed))
        self.board.breakpoint_handler = handler
        self._breakpoint_handler = handler
        if enable_keepalive:
            self.transfer_count = 0
            self.keepalive_enabled = True

    def drain_scheduled(self):
        """Run in 200,000-instruction steps, at most 50, until no Python closure is scheduled."""
        for _ in range(50):
            if not self.board.scheduled:
                return
            self.run_for(200_000)

    def disarm_for_checkpoint(self):
        """Release held rendezvous inputs, clear the trace breakpoints and return to the UI tick
        ratio. Portable checkpoints cannot carry the breakpoint handler."""
        self.release_rendezvous_activity()
        self.use_timing_ratio(24, 'install checkpoint')
        for core in range(2):
            self.board._native.set_breakpoints(core, [])
        self.board.breakpoint_handler = None

    def rearm(self):
        for core in range(2):
            self.board._native.set_breakpoints(core, sorted(self.trace_sites))
        self.board.breakpoint_handler = self._breakpoint_handler

    def key(self, name):
        self.use_timing_ratio(24, 'UI input')
        started, instructions = perf_counter(), self.board.instructions
        state = self.session.key({'key': name, 'render': True})
        settle = state.get('navigation', {}).get('render_settle') or {}
        self.key_log.append({'key': name, 'seconds': round(perf_counter() - started, 3),
                             'instructions': self.board.instructions - instructions,
                             'key_instructions': settle.get('key_instructions'),
                             'settle_instructions': settle.get('settle_instructions'),
                             'settle_status': settle.get('status')})
        current = state.get('navigation', {}).get('current_menu', {})
        return {key: current.get(key) for key in ('menu_id', 'setting_id', 'cursor')}

    def mode_dial(self, name):
        self.use_timing_ratio(24, 'UI input')
        window = self.session.inputs.set_mode_dial(name)
        self.run_for(MODE_DIAL_SETTLE)
        return {'position': window.binding.position.value, 'sample': window.midpoint,
                'low': window.low, 'high': window.high,
                'menu': self.key_state(self.session.sample(False))}

    def capture(self, label, output):
        if not label or any(character not in 'abcdefghijklmnopqrstuvwxyz0123456789-' for character in label):
            raise ValueError('screenshot label must be lowercase ASCII')
        state = self.session.sample()
        if not self.session.png:
            raise RuntimeError('no display plane to capture')
        target = Path(output) / 'screenshots' / f'{label}.png'
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(self.session.png)
        frame = state.get('display', {}).get('last_frame') or {}
        self.screenshots.append({'label': label, 'path': str(target),
                                 'frame_number': frame.get('frame_number'),
                                 'sha256': hashlib.sha256(self.session.png).hexdigest(),
                                 'menu': self.key_state(state)})
        return self.screenshots[-1]

    @staticmethod
    def key_state(state):
        current = state.get('navigation', {}).get('current_menu', {})
        return {key: current.get(key) for key in ('menu_id', 'setting_id', 'cursor')}


def _ram_push_attach(board, session, hold_key=None):
    """Boot past the first-start language screen into live view and enumerate ff80. HOLD_KEY is
    pressed one default key pulse before the cable level rises and released after enumeration."""
    from tools.probe_setup_menus import DEFAULT_PULSE
    bridge = _FF80EndpointBridge(board, session)
    bridge.run_until('first-start screen', lambda: board.instructions >= 100_000_000, 100_000_000)
    for key in ['up'] * 12 + ['ok', 'ok']:
        session.key({'key': key})
    bridge.run_for(40_000_000)
    pre_attach = {'instructions': board.instructions,
                  'raw_identities': list(map(hex, board._native.cp14_identity()))}
    if hold_key is not None:
        session.inputs.press(hold_key)
        bridge.run_for(DEFAULT_PULSE)
        hold = {'key': hold_key, 'pressed_instructions': pre_attach['instructions'],
                'attach_instructions': board.instructions}
    device, configuration = bridge.enumerate()
    if hold_key is not None:
        session.inputs.release()
        hold['released_instructions'] = board.instructions
        bridge.run_for(DEFAULT_PULSE)
        hold['menu_after_release'] = bridge.key_state(session.sample(False))
        pre_attach['attach_hold'] = hold
    return bridge, {'pre_attach': pre_attach, 'device': device.hex(),
                    'configuration': configuration.hex()}


# Staged runtime trees whose bytes decide the machine state at the attach boundary. Portable
# checkpoint identity also carries absolute code paths under the runtime root.
RAM_PUSH_SOURCE_TREES = ('emulator', 'firmware', 'inputs/x-m1-1.50', 'native/python', 'tools')
RAM_PUSH_CHECKPOINT_SCHEMA = 1


def _file_sha256(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def _emulator_source_identity():
    import os
    digest = hashlib.sha256()
    for tree in RAM_PUSH_SOURCE_TREES:
        for path in sorted((ROOT / tree).rglob('*')):
            if path.is_file() and '__pycache__' not in path.parts:
                digest.update(path.relative_to(ROOT).as_posix().encode() + b'\0')
                digest.update(bytes.fromhex(_file_sha256(path)))
    unicorn = os.environ.get('X70_UNICORN_LIBRARY')
    digest.update(json.dumps([str(ROOT), os.path.realpath(sys.executable), list(sys.version_info),
                              None if unicorn is None else os.path.realpath(unicorn)]).encode())
    return digest.hexdigest()


def _ram_push_checkpoint_key(installed_dat, adjustment_nand, menu_sd_image, attach_hold_key=None):
    inputs = {'installed_dat': _file_sha256(installed_dat),
              'adjustment_nand': _file_sha256(adjustment_nand),
              'menu_sd_image': _file_sha256(menu_sd_image),
              'emulator_source': _emulator_source_identity()}
    if attach_hold_key is not None:
        inputs['attach_hold_key'] = attach_hold_key
    document = json.dumps({'schema': RAM_PUSH_CHECKPOINT_SCHEMA, **inputs},
                          sort_keys=True, separators=(',', ':'))
    return hashlib.sha256(document.encode()).hexdigest(), inputs


# A USB-toggle boot checkpoint: saved at the shooting screen with the cable level as the boot left it.
TOGGLE_RECORD = 'usb-toggle-boot.json'


def _toggle_checkpoint_key(installed_dat, adjustment_nand, boot):
    inputs = {'installed_dat': _file_sha256(installed_dat),
              'adjustment_nand': _file_sha256(adjustment_nand),
              'emulator_source': _emulator_source_identity(), 'boot': boot}
    document = json.dumps({'schema': RAM_PUSH_CHECKPOINT_SCHEMA, **inputs},
                          sort_keys=True, separators=(',', ':'))
    return hashlib.sha256(document.encode()).hexdigest(), inputs


ATTACH_RECORD = 'ram-push-attach.json'
INSTALL_RECORD = 'ram-push-install.json'


def _save_ram_push_checkpoint(session, root, name, record_file, record, *, replace):
    """Save the machine as ROOT/NAME with RECORD; replace swaps out an existing one."""
    import shutil
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    partial = root / f'.partial-{uuid4().hex}'
    session.save_snapshot(partial)
    (partial / record_file).write_text(json.dumps(
        {'schema': RAM_PUSH_CHECKPOINT_SCHEMA, 'recovery': session.options['recovery'], **record},
        indent=2) + '\n')
    target = root / name
    if target.exists() and replace:
        stale = root / f'.stale-{uuid4().hex}'
        target.rename(stale)
        partial.rename(target)
        shutil.rmtree(stale)
    elif target.exists():
        shutil.rmtree(partial)
    else:
        partial.rename(target)
    return target


def _read_ram_push_record(directory, record_file, key):
    record = json.loads((directory / record_file).read_text(), object_pairs_hook=_unique_object)
    if record.get('schema') != RAM_PUSH_CHECKPOINT_SCHEMA or record.get('key') != key:
        raise ValueError(f'RAM-push checkpoint record {directory / record_file} differs from key {key}')
    return record


def _restore_ram_push_checkpoint(directory, record, output):
    from x70_native.portable_checkpoint import load
    from tools.probe_setup_menus import MenuSession
    board = load(directory)
    options = dict(board._portable_factory_options, recovery=record['recovery'])
    session = MenuSession(board, output, options=options)
    bridge = _FF80EndpointBridge(board, session, attached=True)
    attachment = {name: record[name] for name in ('pre_attach', 'device', 'configuration')}
    return board, session, bridge, attachment


def _install_checkpoint_name(key, manifest_sha256, runtime_sha256):
    digest = hashlib.sha256(f'{manifest_sha256}:{runtime_sha256}'.encode()).hexdigest()
    return f'{key}-installed-{digest}'


def _install_checkpoints(root, key):
    """Install checkpoints saved over the attach checkpoint KEY, with their host records."""
    found = []
    for directory in sorted(root.glob(f'{key}-installed-*')):
        if directory.is_dir() and (directory / INSTALL_RECORD).is_file():
            record = _read_ram_push_record(directory, INSTALL_RECORD, key)
            found.append({'directory': str(directory.resolve()), 'base': record['base'],
                          'manifest_sha256': record['manifest_sha256'],
                          'runtime_sha256': record['runtime_sha256'], 'host': record['host']})
    return found


INSTALL_FIELDS = ('base', 'manifest_sha256', 'runtime_sha256', 'host')


def _save_install_checkpoint(bridge, session, root, checkpoint, attachment, request):
    from emulator.checkpoint import CheckpointError
    if set(request) != {'command', *INSTALL_FIELDS}:
        raise ValueError('save-install-checkpoint requires exactly ' + ', '.join(INSTALL_FIELDS))
    name = _install_checkpoint_name(checkpoint['key'], request['manifest_sha256'], request['runtime_sha256'])
    if (root / name).is_dir():
        return {'directory': str((root / name).resolve()), 'saved': False, 'existing': True}
    started = perf_counter()
    bridge.drain_scheduled()
    bridge.disarm_for_checkpoint()
    try:
        target = _save_ram_push_checkpoint(
            session, root, name, INSTALL_RECORD,
            {'key': checkpoint['key'], **attachment, **{field: request[field] for field in INSTALL_FIELDS}},
            replace=False)
    except CheckpointError as error:
        return {'saved': False, 'existing': False, 'save_error': str(error)}
    finally:
        bridge.rearm()
    return {'directory': str(target.resolve()), 'saved': True, 'existing': False,
            'seconds': round(perf_counter() - started, 3)}


def _run_ram_push_server(board, session, output, bridge, attachment, checkpoint, checkpoint_root):
    pre_attach = attachment['pre_attach']
    device = bytes.fromhex(attachment['device'])
    configuration = bytes.fromhex(attachment['configuration'])
    fixed_sites = {
        '0x1469b20': 'request-bootstrap',
        '0x1469b38': 'request-bootstrap-return',
        '0xd806e8': 'query-callback-result',
        '0xd8073c': 'query-response-ready',
        '0x6b0aa8': 'cpu1-sync-wrapper',
        '0x6b0c3c': 'cpu1-range-wrapper',
        '0x61c490': 'cpu1-cache-control',
        '0x6c6b5c': 'cpu1-worker',
        '0x6b0704': 'cpu1-worker-completion',
    }
    bridge.set_trace_sites(fixed_sites, enable_keepalive=False)
    ready = {'ok': True, 'event': 'ready', 'device': device.hex(),
             'configuration': configuration.hex(), 'pre_attach': pre_attach,
             'checkpoint': checkpoint}
    print(json.dumps(ready, separators=(',', ':')), flush=True)
    recovery = session.options['recovery']
    inputs = {}
    for name, path in (('installed_dat', recovery['media']),
                       ('adjustment_nand', recovery['adjustment_nand'])):
        source = Path(path)
        with source.open('rb') as stream:
            digest = hashlib.file_digest(stream, 'sha256').hexdigest()
        inputs[name] = {'path': str(source), 'bytes': source.stat().st_size,
                        'sha256': digest}
    inputs['menu_sd_image'] = recovery['card']
    receipt = {'schema': 1, 'scope': 'emulated X-M1 ordinary-boot RAM push',
               'inputs': inputs, 'pre_attach': pre_attach,
               'device_descriptor': device.hex(),
               'configuration_descriptor': configuration.hex(),
               'checkpoint': checkpoint}
    try:
        for line in sys.stdin:
            request = json.loads(line, object_pairs_hook=_unique_object)
            command = request.get('command')
            if command == 'list':
                result = {'candidate': {'path': 'emulator:xm1', 'vid': 0x04cb, 'pid': 0xff80,
                    'configuration': configuration[5], 'interface': 0, 'alternate': 0,
                    'class': configuration[14], 'subclass': configuration[15],
                    'protocol': configuration[16], 'input': 0x81, 'output': 2,
                    'input_packet': 64, 'output_packet': 64}}
            elif command == 'transfer':
                count, payload = bridge.transfer(request)
                result = {'count': count, 'bytes': payload.hex()}
            elif command == 'trace-sites':
                bridge.set_trace_sites({**fixed_sites, **request['sites']})
                result = {'sites': len(bridge.trace_sites)}
            elif command == 'save-install-checkpoint':
                result = _save_install_checkpoint(bridge, session, checkpoint_root, checkpoint,
                                                  attachment, request)
            elif command == 'trace-hits':
                start = request.get('from')
                if type(start) is not int or not 0 <= start <= len(bridge.trace_hits):
                    raise ValueError('trace-hits requires a hit index within the recorded hits')
                result = {'count': len(bridge.trace_hits), 'hits': bridge.trace_hits[start:]}
            elif command == 'key':
                result = {'menu': bridge.key(request['key'])}
            elif command == 'mode-dial':
                result = bridge.mode_dial(request['position'])
            elif command == 'read-memory':
                address, count = request.get('address'), request.get('count')
                if (type(address) is not int or type(count) is not int or not 0 < count <= 4096
                        or not 0 <= address <= 0x100000000 - count):
                    raise ValueError('read-memory requires an address and 1..4096 bytes')
                result = {'bytes': bytes(board.read_memory(address, count)).hex()}
            elif command == 'capture':
                result = bridge.capture(request['label'], output)
            elif command == 'finish':
                result = {'finished': True}
                print(json.dumps({'ok': True, **result}, separators=(',', ':')), flush=True)
                break
            else:
                raise ValueError('unknown RAM-push bridge command')
            print(json.dumps({'ok': True, **result}, separators=(',', ':')), flush=True)
    except Exception as error:
        receipt['bridge_error'] = {'type': type(error).__name__, 'message': str(error)}
        print(json.dumps({'ok': False, 'error': type(error).__name__,
                          'message': str(error)}, separators=(',', ':')), flush=True)
    from emulator.rec_state import Reader, task_state
    task_tables, tasks = task_state(Reader(board))
    receipt['task_tables'] = task_tables
    receipt['worker_tasks'] = [
        task for task in tasks if task.get('task_id') in (0x24, 0x58, 0x59, 0x5a)]
    receipt['diagnostic_memory'] = {
        'query_output': bytes(board.read_memory(0x01469878, 8)).hex(),
        'callback_slot': bytes(board.read_memory(0x015c22ac, 4)).hex(),
        'response_descriptor': bytes(board.read_memory(0x0146989c, 32)).hex(),
        'response_state': bytes(board.read_memory(0x01469864, 32)).hex(),
        'worker_rows': bytes(board.read_memory(0x0089e608, 0x330)).hex(),
        'worker_payloads': bytes(board.read_memory(0x009c2f5c, 0x324)).hex(),
    }
    receipt['usb_endpoint'] = bridge.endpoint.snapshot()
    receipt['timing_transitions'] = bridge.timing_transitions
    receipt['keepalive_inputs'] = bridge.keepalive_inputs
    receipt['key_log'] = bridge.key_log
    receipt['transfer_log'] = bridge.transfer_log
    receipt['rendezvous_activity'] = bridge.rendezvous_activity
    receipt['cores'] = board.snapshot()['cores']
    receipt.update(contexts=bridge.trace_hits, screenshots=bridge.screenshots,
                   boundary={'instructions': board.instructions, 'failure': board.failure,
                             'execution_error': session.execution_error,
                             'service_mode': int.from_bytes(
                                 board.read_memory(0x0146a0c0, 4), 'little')})
    target = Path(output) / 'emulator-session.json'
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(receipt, indent=2) + '\n')

def _run_ff80_rehearsal(board, session, output):
    """Drive the stock ordinary-boot service session through the guest USB stack."""
    from emulator.power_devices import PowerDevices
    from emulator.rec_state import Reader, task_state
    from emulator.usb_devices import USBControl
    from tools.probe_setup_menus import MenuSession
    from unicorn.arm_const import UC_ARM_REG_SP

    if not isinstance(session, MenuSession):
        raise ValueError('ff80 rehearsal requires --menus')
    power = next(device for device in board.devices if type(device) is PowerDevices)
    usb = next(device for device in board.devices if type(device) is USBControl)
    endpoint = usb._endpoint
    if endpoint is None:
        raise ValueError('ff80 rehearsal requires the selected endpoint transport')
    if power.usb_connected:
        raise ValueError('ff80 rehearsal requires a post-boot cable attach')

    def run_until(label, predicate, budget):
        end = board.instructions + budget
        while not predicate():
            if board.failure is not None or session.execution_error is not None:
                machine = board.snapshot()
                cores = [{'pc': core.get('pc'), 'lr': core.get('lr'), 'sp': core.get('sp'),
                          'recent': core.get('recent_instructions', [])[-12:]}
                         for core in machine.get('cores', [])]
                raise RuntimeError(f'{label}: {board.failure or session.execution_error}; cores={cores}')
            if board.instructions >= end:
                raise TimeoutError(f'{label}: instruction budget expired; usb={endpoint.snapshot()}')
            target = min(end, board.instructions + 1_000_000)
            try:
                board.run(target - board.instructions + 2 * board.quantum, pause_after=target)
            except Exception as error:
                session.execution_error = {'type': type(error).__name__, 'error': str(error)}

    def run_for(count):
        target = board.instructions + count
        while board.instructions < target:
            run_until('bounded continuation', lambda: board.instructions >= target,
                      target - board.instructions)

    def take_in(endpoint_address, count):
        chunks = bytearray()
        while len(chunks) < count:
            if not endpoint.tx[dict(endpoint.contract.in_endpoints)[endpoint_address]]:
                endpoint.host_in_token(endpoint_address)
            port = dict(endpoint.contract.in_endpoints)[endpoint_address]
            run_until('bulk IN reply', lambda: bool(endpoint.tx[port]), 30_000_000)
            chunks.extend(endpoint.host_in(endpoint_address, count - len(chunks)))
            run_for(200_000)
        return bytes(chunks)

    def control_out(payload):
        setup = bytes((0x43, 0, 0, 0, 0, 0, len(payload) & 0xff, len(payload) >> 8))
        endpoint.host_setup(setup)
        run_for(200_000)
        endpoint.host_out(0, payload)
        run_until('control OUT status', lambda: bool(endpoint.tx[0]), 30_000_000)
        status = endpoint.host_in(0x80, 64)
        if status:
            raise RuntimeError('control OUT status packet was not empty')
        run_for(200_000)

    def control_status(setup):
        endpoint.host_setup(setup)
        run_until('control status IN', lambda: bool(endpoint.tx[0]), 30_000_000)
        status = endpoint.host_in(0x80, 64)
        if status:
            raise RuntimeError('control status packet was not empty')
        run_for(200_000)

    def control_in(setup, count):
        endpoint.host_setup(setup)
        run_until('control IN data', lambda: bool(endpoint.tx[0]), 30_000_000)
        payload = endpoint.host_in(0x80, count)
        endpoint.host_out(0, b'')
        run_for(200_000)
        return payload

    def command(op, frame, reply_bytes, sid):
        control_out(bytes((op, 0, 0, 0, 0, 0, 0, 0,
                           len(frame) >> 8, len(frame) & 0xff, sid >> 8, sid & 0xff)))
        if frame:
            endpoint.host_out(2, frame)
            run_until('bulk OUT completion',
                      lambda: (endpoint.rx_positions[1] == len(endpoint.rx[1])
                               and endpoint.dma_active is None),
                      30_000_000)
            run_for(1_000_000)
        if reply_bytes is None:
            return b''
        opcode = int.from_bytes(frame[:4], 'little') if len(frame) >= 4 else 0
        if opcode in (0x1a0002, 0x200001):
            published = 0x124 if opcode == 0x1a0002 else 0x11c
            run_until('service reply publication',
                      lambda: int.from_bytes(board.read_memory(0x0146986c, 4), 'little') == published,
                      30_000_000)
        else:
            run_for(1_000_000)
        control_out(bytes((0xd1, 0, 0, 0, 0, 0, 0, 0,
                           reply_bytes >> 8, reply_bytes & 0xff, sid >> 8, sid & 0xff)))
        return take_in(0x81, reply_bytes)

    def words(*values):
        return b''.join(struct.pack('<I', value) for value in values)

    def debug_frame(function, address, count, payload=b''):
        return words(0x200001, 0, 0, 0, function, 0, 0, address, count) + payload

    def trace_contexts():
        _, tasks = task_state(Reader(board))
        stacks = []
        for task in tasks:
            if all(name in task for name in ('stack_base', 'stack_bytes', 'task_id')):
                base = int(task['stack_base'], 16)
                stacks.append((task['cpu'], base, base + task['stack_bytes'], task['task_id']))
        hits = []
        sites = {0x00d803cc: 'debug-dispatch', 0x00647b78: 'function-3',
                 0x00dc7874: 'stock-query-callback'}
        def handler(cpu, core, address):
            sp = cpu.reg_read(UC_ARM_REG_SP)
            task = next((task_id for owner, low, high, task_id in stacks
                         if owner == core and low <= sp < high), None)
            request = bytes(board.read_memory(0x01469a9c, 0x24))
            hits.append({'site': sites[address], 'address': hex(address), 'core': core,
                         'task': task, 'task_hex': None if task is None else hex(task),
                         'sp': hex(sp), 'instructions': board.instructions,
                         'request': {'opcode': hex(int.from_bytes(request[0:4], 'little')),
                                     'function_or_tag': hex(int.from_bytes(request[0x10:0x14], 'little')),
                                     'address': hex(int.from_bytes(request[0x1c:0x20], 'little')),
                                     'length': int.from_bytes(request[0x20:0x24], 'little')}})
        for core in range(2):
            board._native.set_breakpoints(core, sorted(sites))
        board.breakpoint_handler = handler
        return hits

    recovery = session.options.get('recovery', {})
    nand_path = Path(recovery['media'])
    with nand_path.open('rb') as source:
        nand_sha256 = hashlib.file_digest(source, 'sha256').hexdigest()
    receipt = {'schema': 1, 'scope': 'emulated X-M1 1.50 ordinary-boot ff80 service',
               'firmware_sha256': board.snapshot().get('firmware_sha256'),
               'installed_nand': {'sha256': nand_sha256, 'stored_f7': 1},
               'source': 'stock guest controller, IRQ, USB task and service handlers',
               'limits': ['Synthetic USB host timing and endpoint register reset values.',
                          'Shared emulator RAM is not a hardware cache-coherence model.']}
    run_until('first-start screen', lambda: board.instructions >= 100_000_000, 100_000_000)
    for key in ['up'] * 12 + ['ok', 'ok']:
        session.key({'key': key})
    run_for(40_000_000)
    receipt['pre_attach'] = {'instructions': board.instructions,
                             'cp14': list(board._native.cp14_identity())}
    power.set_usb_connected(True)
    run_until('USB controller activation', lambda: endpoint.regs[0x090] & 6 == 6,
              140_000_000)
    endpoint.host_reset()
    run_for(1_000_000)

    device = control_in(bytes((0x80, 6, 0, 1, 0, 0, 18, 0)), 18)
    if len(device) != 18 or device[8:12] != bytes((0xcb, 0x04, 0x80, 0xff)):
        raise RuntimeError(f'unexpected device descriptor {device.hex()}')
    receipt['device_descriptor'] = {'vid': '0x04cb', 'pid': '0xff80',
                                    'reply': device.hex()}
    control_status(bytes((0, 5, 1, 0, 0, 0, 0, 0)))
    config_head = control_in(bytes((0x80, 6, 0, 2, 0, 0, 9, 0)), 9)
    config_bytes = int.from_bytes(config_head[2:4], 'little')
    if len(config_head) != 9 or not 9 <= config_bytes <= 256:
        raise RuntimeError(f'unexpected configuration descriptor head {config_head.hex()}')
    configuration = control_in(
        bytes((0x80, 6, 0, 2, 0, 0, config_bytes & 0xff, config_bytes >> 8)),
        config_bytes)
    control_status(bytes((0, 9, configuration[5], 0, 0, 0, 0, 0)))
    run_for(1_000_000)
    receipt['configuration_descriptor'] = configuration.hex()

    open_request = bytes((0xc0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0, 0))
    control_out(open_request)
    opened = take_in(0x81, 8)
    sid = int.from_bytes(opened[:2], 'little')
    if opened[2:] != bytes((0, 0, 0, 0, 1, 0)):
        raise RuntimeError(f'unexpected C0 reply {opened.hex()}')
    receipt['c0'] = {'opcode': '0xc0', 'sid': sid, 'reply': opened.hex()}
    contexts = trace_contexts()

    identity_request = words(0x150000, 0, 0, 0, 4, 0, 0, 0)
    identity = command(0xd0, identity_request, 128, sid)
    if identity[:16] != words(0x150000, 0, 0, 0) or identity[16:26] != b'X\x00-\x00M\x001\x00\x00\x00':
        raise RuntimeError(f'unexpected identity reply {identity.hex()}')
    receipt['identity'] = {'tag': 4, 'reply': identity.hex()}

    config_request = words(0x1a0002, 0, 0, 0, 2, 1, 0, 0, 256)
    config = command(0xd0, config_request, 0x124, sid)
    if config[0x24 + 0xf7] != 1:
        raise RuntimeError(f'config f7 was {config[0x24 + 0xf7]:#x}')
    receipt['config'] = {'opcode': '0x1a0002', 'function': 2, 'window': 0, 'bytes': 256,
                         'reply_sha256': hashlib.sha256(config).hexdigest(),
                         'f7': config[0x24 + 0xf7]}

    ram_address, ram_count = 0x015c22a0, 32
    expected = bytes(board.read_memory(ram_address, ram_count))
    ram = command(0xd0, debug_frame(2, ram_address, ram_count), 0x24 + ram_count, sid)
    if ram[0x24:] != expected:
        raise RuntimeError('function-2 reply differs from guest memory')
    receipt['ram_read'] = {'address': hex(ram_address), 'bytes': ram_count,
                           'guest_memory_hex': expected.hex(), 'reply_hex': ram[0x24:].hex(),
                           'sha256': hashlib.sha256(expected).hexdigest()}

    scratch, payload = 0x015c22d0, b'XM1!'
    before = bytes(board.read_memory(scratch, len(payload)))
    write_reply = command(0xd0, debug_frame(3, scratch, len(payload), payload), 0x24, sid)
    readback = command(0xd0, debug_frame(2, scratch, len(payload)), 0x24 + len(payload), sid)
    if readback[0x24:] != payload or bytes(board.read_memory(scratch, len(payload))) != payload:
        raise RuntimeError('function-3 scratch write did not read back')
    receipt['ram_write'] = {'address': hex(scratch), 'before': before.hex(),
                            'written': payload.hex(), 'reply': write_reply.hex(),
                            'readback': readback[0x24:].hex()}

    query = command(0xd0, words(0x450006, 0, 0, 0, 0, 0, 0, 0), 32, sid)
    receipt['query'] = {'tag': 0, 'reply': query.hex()}
    receipt['contexts'] = contexts
    receipt['boundary'] = {'instructions': board.instructions,
                           'failure': board.failure,
                           'execution_error': session.execution_error,
                           'service_mode': int.from_bytes(board.read_memory(0x0146a0c0, 4), 'little')}
    output.mkdir(parents=True, exist_ok=True)
    target = output / 'ff80-session.json'
    target.write_text(json.dumps(receipt, indent=2) + '\n')
    return receipt


class TraceSites:
    """Pre-instruction breakpoints on both cores recording r0-r3, sp, lr and the site's memory
    windows; observation never writes. A site given an r0 records only entries with that r0.
    `hits` is append-only; `counts` counts every matching entry, including those past
    `recorded_per_site`."""
    REGISTERS = ('r0', 'r1', 'r2', 'r3', 'sp', 'lr')

    def __init__(self, board, recorded_per_site=None):
        self.board, self.recorded_per_site = board, recorded_per_site
        self.sites, self.hits, self.counts = {}, [], {}

    def arm(self, sites):
        """Replace the armed sites with `sites`: dicts of address, label and optional r0 and windows."""
        parsed = {}
        for site in sites:
            if (type(site) is not dict or not {'address', 'label'} <= set(site) <= {'address', 'label', 'r0', 'windows'}
                    or type(site['address']) is not int or type(site['label']) is not str
                    or type(site.get('r0', 0)) is not int):
                raise ValueError('a trace site takes an integer address, a label and an optional integer r0')
            windows = tuple(tuple(window) for window in site.get('windows', ()))
            if any(len(window) != 2 or any(type(value) is not int for value in window) for window in windows):
                raise ValueError('a trace window is an integer start and length')
            parsed[site['address']] = (site['label'], site.get('r0'), windows)
        self.sites = parsed
        for address in parsed:
            self.counts.setdefault(address, 0)
        for core in range(len(self.board.cpus)):
            self.board._native.set_breakpoints(core, sorted(parsed))
        self.board.breakpoint_handler = self._hit if parsed else None

    def specs(self):
        """The armed sites in `arm`'s input form."""
        return [{'address': address, 'label': label, **({} if r0 is None else {'r0': r0}),
                 **({'windows': [list(window) for window in windows]} if windows else {})}
                for address, (label, r0, windows) in self.sites.items()]

    def _hit(self, cpu, core, address):
        from unicorn import arm_const as arm
        label, wanted, windows = self.sites[address]
        if wanted is not None and cpu.reg_read(arm.UC_ARM_REG_R0) != wanted:
            return
        self.counts[address] += 1
        if self.recorded_per_site is not None and self.counts[address] > self.recorded_per_site:
            return
        hit = {'label': label, 'address': address, 'core': core,
               'registers': {name: cpu.reg_read(getattr(arm, 'UC_ARM_REG_' + name.upper()))
                             for name in self.REGISTERS},
               'instructions': self.board.instructions}
        if windows:
            hit['windows'] = {start: bytes(self.board.read_memory(start, length)).hex()
                              for start, length in windows}
        self.hits.append(hit)


def _run_toggle_server(board, session, output, start):
    """Serve primitive commands for the USB-toggle scenarios of emulation/UsbToggleEmu.hs: run,
    key pulses, key hold and release, raw ADC levels, cable level, memory reads, trace sites, enumeration without a
    descriptor expectation, endpoint transfers, captures, the access probe and the boot checkpoint."""
    from emulator.power_devices import PowerDevices
    power = next(device for device in board.devices if type(device) is PowerDevices)
    bridge = _FF80EndpointBridge(board, session, attached=power.usb_connected)
    trace = TraceSites(board)

    def boundary():
        machine = board.snapshot()
        return {'instructions': board.instructions, 'failure': board.failure,
                'execution_error': session.execution_error,
                'cores': [{'pc': core.get('pc'), 'lr': core.get('lr'), 'sp': core.get('sp'),
                           'recent': core.get('recent_instructions', [])[-12:]}
                          for core in machine.get('cores', [])]}

    def run(count, stop_on):
        seen, stopped, end = len(trace.hits), [None], board.instructions + count

        def done():
            nonlocal seen
            for hit in trace.hits[seen:]:
                if hit['label'] in stop_on:
                    stopped[0] = hit['label']
                    return True
            seen = len(trace.hits)
            return board.instructions >= end
        bridge.run_until('toggle run', done, count + 1)
        return stopped[0]

    print(json.dumps({'ok': True, 'event': 'ready', **start, 'instructions': board.instructions,
                      'usb_connected': power.usb_connected}, separators=(',', ':')), flush=True)
    for line in sys.stdin:
        try:
            request = json.loads(line, object_pairs_hook=_unique_object)
            command = request.get('command')
            if command == 'state':
                ready = session.inputs.availability()['ready']
                inputs = session.inputs.current_state(ready=ready)
                result = {'instructions': board.instructions, 'virtual_ticks': board.virtual_ticks,
                          'usb_connected': power.usb_connected,
                          'tick_ratio': f'{board.tick_ratio[0]}/{board.tick_ratio[1]}',
                          'inputs_ready': ready, 'asserted_keys': inputs['asserted_keys'],
                          'menu': bridge.key_state(session.sample(False))}
            elif command == 'run':
                count, stop_on = request.get('instructions'), request.get('stop_on', [])
                if (type(count) is not int or not 0 < count <= 4_000_000_000 or type(stop_on) is not list
                        or any(type(label) is not str for label in stop_on)):
                    raise ValueError('run takes 1..4000000000 instructions and a list of stop labels')
                result = {'stopped_on': run(count, set(stop_on)), 'instructions': board.instructions}
            elif command == 'key':
                result = {'menu': bridge.key_state(session.key({'key': request['key']}))}
            elif command == 'press':
                result = {'sample': session.inputs.press(request['key'])}
            elif command == 'release':
                result = {'sample': session.inputs.release()}
            elif command == 'adc':
                channel, sample = request.get('channel'), request.get('sample')
                if type(channel) is not int or type(sample) is not int or not 0 <= channel < 12 or not 0 <= sample <= 0xffff:
                    raise ValueError('adc takes a channel 0..11 and a sample 0..65535')
                samples = list(power.adc_samples)
                samples[channel] = sample
                power.set_adc_samples(samples, source=f'USB-toggle ADC channel {channel} level')
                result = {'samples': list(power.adc_samples)}
            elif command == 'cable':
                if type(request.get('connected')) is not bool:
                    raise ValueError('cable takes a boolean level')
                power.set_usb_connected(request['connected'])
                result = {'usb_connected': power.usb_connected, 'instructions': board.instructions}
            elif command == 'read-memory':
                address, count = request.get('address'), request.get('count')
                if (type(address) is not int or type(count) is not int or not 0 < count <= 4096
                        or not 0 <= address <= 0x100000000 - count):
                    raise ValueError('read-memory requires an address and 1..4096 bytes')
                result = {'bytes': bytes(board.read_memory(address, count)).hex()}
            elif command == 'trace-sites':
                trace.arm(request['sites'])
                result = {'sites': len(trace.sites)}
            elif command == 'trace-hits':
                first = request.get('from')
                if type(first) is not int or not 0 <= first <= len(trace.hits):
                    raise ValueError('trace-hits requires a hit index within the recorded hits')
                result = {'count': len(trace.hits), 'hits': trace.hits[first:]}
            elif command == 'enumerate':
                budget = request.get('budget')
                if type(budget) is not int or not 0 < budget <= 1_000_000_000:
                    raise ValueError('enumerate takes an activation budget of 1..1000000000 instructions')
                device, configuration = bridge.enumerate_attached(budget)
                result = {'device': device.hex(), 'configuration': configuration.hex(),
                          'instructions': board.instructions}
            elif command == 'transfer':
                count, payload = bridge.transfer(request)
                result = {'count': count, 'bytes': payload.hex()}
            elif command == 'capture':
                result = bridge.capture(request['label'], output)
            elif command == 'access-probe':
                state = board._native.access_probe_state()
                result = {'enabled': state['enabled'], 'violation': state['violation'],
                          'ranges': state.get('ranges'), 'records': list(state['records'])}
            elif command == 'save-checkpoint':
                if start.get('checkpoint_root') is None or type(request.get('replace')) is not bool:
                    raise ValueError('save-checkpoint needs --toggle-checkpoints and a boolean replace')
                bridge.drain_scheduled()
                sites = trace.specs()
                trace.arm([])
                try:
                    target = _save_ram_push_checkpoint(
                        session, Path(start['checkpoint_root']), start['key'], TOGGLE_RECORD,
                        {'key': start['key'], 'inputs': start['inputs'], 'instructions': board.instructions},
                        replace=request['replace'])
                finally:
                    trace.arm(sites)
                result = {'directory': str(target.resolve()), 'instructions': board.instructions}
            elif command == 'finish':
                print(json.dumps({'ok': True, 'finished': True}, separators=(',', ':')), flush=True)
                break
            else:
                raise ValueError('unknown USB-toggle command')
            print(json.dumps({'ok': True, **result}, separators=(',', ':')), flush=True)
        except Exception as error:
            try:
                stop = boundary()
            except Exception as observed:
                stop = {'unavailable': f'{type(observed).__name__}: {observed}'}
            print(json.dumps({'ok': False, 'error': type(error).__name__, 'message': str(error),
                              'boundary': stop}, separators=(',', ':'), default=str), flush=True)
    target = Path(output) / 'emulator-session.json'
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps({
        'schema': 1, 'scope': 'emulated X-M1 USB-toggle primitive session', 'start': start,
        'usb_endpoint': bridge.endpoint.snapshot(), 'timing_transitions': bridge.timing_transitions,
        'transfer_log': bridge.transfer_log, 'screenshots': bridge.screenshots,
        'trace_hits': len(trace.hits), 'boundary': boundary()}, indent=2, default=str) + '\n')


def _console_window(field):
    start, _, length = field.partition(':')
    start, length = int(start, 0), int(length, 0)
    if not 0 <= start <= 0xffffffff or not 1 <= length <= 0x1000:
        raise ValueError('Window is ADDRESS:LENGTH with 1..0x1000 bytes')
    return start, length


def _console_hit(hit):
    return {'address': hex(hit['address']), 'core': hit['core'], 'instructions': hit['instructions'],
            'registers': {name: hex(value) for name, value in hit['registers'].items()},
            'windows': {hex(start): data for start, data in hit.get('windows', {}).items()}}


def main():
    from emulator.boot import make_board
    from emulator.server import make_menu_server as make_server
    parser = argparse.ArgumentParser(description=__doc__)
    starting = parser.add_mutually_exclusive_group(required=True)
    starting.add_argument('--profile', type=Path, help='Cold boot using the selected options JSON')
    starting.add_argument('--snapshot', type=Path, help='Restore a portable snapshot into fresh native engines')
    starting.add_argument('--recovery-ram', type=Path, help='Import generated bootstrap RAM and its observed .state.json CPU0 companion into cold engines')
    parser.add_argument('--cold-profile', type=Path, default=ROOT / 'firmware/x-m1/1.50/emulator-rec.json',
                        help='Independent cold-board options/profile for --recovery-ram')
    parser.add_argument('--rom-card', type=Path, help='Complete recovery SD image for a direct cold ROM attempt')
    parser.add_argument('--sd-image', type=Path, help='Complete updater/recovery card retained by the RAM continuation')
    parser.add_argument('--rom-route', choices=('fat', 'raw'), default='fat')
    installed = parser.add_mutually_exclusive_group()
    installed.add_argument('--installed-media', type=Path, help='Independent installed logical NAND data, including damaged application bytes')
    installed.add_argument('--installed-nand', type=Path, help='Committed NAND image from an earlier modeled run; not a machine checkpoint')
    installed.add_argument('--installed-dat', type=Path,
                           help='X-M1 1.50 updater container installed into the modeled NAND extent')
    parser.add_argument('--adjustment-nand', type=Path,
                        help='Complete committed adjustment blocks layered over installed logical media')
    parser.add_argument('--menu-sd-image', type=Path,
                        help='Ordinary-boot virtual SD image for menu operations')
    parser.add_argument('--export-nand', type=Path, help='Save committed installed NAND to a fresh file after this run')
    parser.add_argument('--export-sd', type=Path, help='Save logical SD bytes and guest write overlays to a fresh image')
    budget = parser.add_mutually_exclusive_group()
    budget.add_argument('--pause-after', type=int, help='Absolute instruction threshold; default60M for cold boot, zero for restore')
    budget.add_argument('--instructions', type=int, help='Instructions to run from the selected starting state')
    parser.add_argument('--settings', choices=('default', 'owner'), help='Persistent settings selection; verify restored selection')
    parser.add_argument('--save-snapshot', type=Path, help='Save drained machine state to a fresh local directory')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--serve', action='store_true')
    parser.add_argument('--ff80-rehearsal', action='store_true',
                        help='Run the closed ordinary-boot ff80 endpoint session and write its receipt')
    parser.add_argument('--menus', action='store_true',
                        help='Enable declared ADC button input and stock menu navigation')
    parser.add_argument('--ram-push-server', action='store_true',
                        help='Serve the closed emulator-backed UsbApi line protocol')
    parser.add_argument('--ram-push-checkpoints', type=Path,
                        help='Directory of RAM-push attach checkpoints keyed by inputs and emulator source')
    parser.add_argument('--fresh-boot', action='store_true',
                        help='Cold boot even when the keyed RAM-push checkpoint exists, then replace it')
    parser.add_argument('--attach-hold-key',
                        help='--ram-push-server: hold this menu key across the attach edge')
    parser.add_argument('--toggle-server', action='store_true',
                        help='Serve the primitive USB-toggle scenario commands')
    parser.add_argument('--toggle-checkpoints', type=Path,
                        help='--toggle-server: directory of boot checkpoints keyed by inputs, reset inputs and emulator source')
    parser.add_argument('--cable-at-reset', action='store_true',
                        help='--toggle-server: raise the USB cable level before the first instruction')
    parser.add_argument('--adc-at-reset', metavar='CHANNEL=SAMPLE',
                        help='--toggle-server: hold one ADC channel at SAMPLE from the first instruction')
    parser.add_argument('--access-probe-range', metavar='LOW:HIGH',
                        help='--toggle-server: record every access to [LOW, HIGH) from the first instruction')
    parser.add_argument('--port', type=int, default=0)
    parser.add_argument('--bind', default='127.0.0.1')
    parser.add_argument('--hostname')
    args = parser.parse_args()
    installed_input = args.installed_media or args.installed_nand or args.installed_dat
    recovery_start = args.recovery_ram is not None or args.rom_card is not None
    if recovery_start and (args.snapshot is not None or installed_input is None):
        parser.error('Recovery requires a cold board and independent installed storage')
    if args.recovery_ram is not None and args.rom_card is not None:
        parser.error('Select direct ROM card execution or exported RAM continuation, not both')
    if args.snapshot is not None and installed_input is not None:
        parser.error('Installed storage inputs belong to independent cold boards, not machine checkpoints')
    if args.sd_image is not None and args.recovery_ram is None:
        parser.error('--sd-image belongs to --recovery-ram; use --rom-card for direct ROM execution')
    if args.adjustment_nand is not None and not (args.installed_media or args.installed_dat):
        parser.error('--adjustment-nand requires --installed-media or --installed-dat')
    if args.menu_sd_image is not None and not args.ram_push_server:
        parser.error('--menu-sd-image belongs to --ram-push-server')
    for value in (args.pause_after, args.instructions):
        if value is not None and not 0 <= value <= 100_000_000:
            parser.error('Instruction threshold/budget must be 0..100000000')
    if args.ram_push_server and (
            args.snapshot is not None or args.installed_dat is None
            or args.adjustment_nand is None or args.menu_sd_image is None
            or not args.menus or args.ff80_rehearsal
            or args.pause_after is not None or args.instructions is not None
            or args.serve or args.save_snapshot is not None):
        parser.error('--ram-push-server requires --profile, --installed-dat, --adjustment-nand, --menu-sd-image and --menus without another run mode')
    if args.ram_push_checkpoints is not None and not args.ram_push_server:
        parser.error('--ram-push-checkpoints belongs to --ram-push-server')
    if args.fresh_boot and args.ram_push_checkpoints is None:
        parser.error('--fresh-boot requires --ram-push-checkpoints')
    if args.attach_hold_key is not None and not args.ram_push_server:
        parser.error('--attach-hold-key belongs to --ram-push-server')
    if args.toggle_server and (
            args.snapshot is not None or args.installed_dat is None or args.adjustment_nand is None
            or args.menu_sd_image is not None or not args.menus or args.ff80_rehearsal
            or args.ram_push_server or args.pause_after is not None or args.instructions is not None
            or args.serve or args.save_snapshot is not None):
        parser.error('--toggle-server requires --profile, --installed-dat, --adjustment-nand and --menus without another run mode')
    if not args.toggle_server and (args.toggle_checkpoints is not None or args.cable_at_reset
                                   or args.adc_at_reset is not None or args.access_probe_range is not None):
        parser.error('--toggle-checkpoints, --cable-at-reset, --adc-at-reset and --access-probe-range belong to --toggle-server')
    adc_at_reset = probe_range = None
    try:
        if args.adc_at_reset is not None:
            channel, sample = (int(field, 0) for field in args.adc_at_reset.split('='))
            if not 0 <= channel < 12 or not 0 <= sample <= 0xffff:
                raise ValueError
            adc_at_reset = [channel, sample]
        if args.access_probe_range is not None:
            low, high = (int(field, 0) for field in args.access_probe_range.split(':'))
            if not 0 <= low < high:
                raise ValueError
            probe_range = (low, high)
    except ValueError:
        parser.error('--adc-at-reset takes CHANNEL=SAMPLE and --access-probe-range takes LOW:HIGH')
    output = args.output or ROOT / '.tools/rec-scanout' / (
        datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ') + '-' + uuid4().hex[:8])
    checkpoint = None
    if args.ram_push_checkpoints is not None:
        started = perf_counter()
        key, key_inputs = _ram_push_checkpoint_key(args.installed_dat, args.adjustment_nand, args.menu_sd_image,
                                                   args.attach_hold_key)
        directory = args.ram_push_checkpoints / key
        checkpoint = {'key': key, 'inputs': key_inputs, 'directory': str(directory.resolve()),
                      'key_seconds': round(perf_counter() - started, 3)}
        installed = _install_checkpoints(args.ram_push_checkpoints, key)
        print(json.dumps({'ok': True, 'event': 'start-options', 'boot_key': key, 'installed': installed},
                         separators=(',', ':')), flush=True)
        choice = json.loads(sys.stdin.readline(), object_pairs_hook=_unique_object)
        if set(choice) != {'command', 'installed'} or choice['command'] != 'start':
            raise ValueError('RAM-push start requires {"command": "start", "installed": DIRECTORY or null}')
        if choice['installed'] is not None:
            if choice['installed'] not in [item['directory'] for item in installed]:
                raise ValueError('RAM-push start names no listed install checkpoint')
            started = perf_counter()
            directory = Path(choice['installed'])
            record = _read_ram_push_record(directory, INSTALL_RECORD, key)
            board, session, bridge, attachment = _restore_ram_push_checkpoint(directory, record, output)
            checkpoint.update(start='installed', installed=str(directory),
                              restore_seconds=round(perf_counter() - started, 3))
            _run_ram_push_server(board, session, output, bridge, attachment, checkpoint,
                                 args.ram_push_checkpoints)
            return
        if directory.is_dir() and not args.fresh_boot:
            started = perf_counter()
            record = _read_ram_push_record(directory, ATTACH_RECORD, key)
            board, session, bridge, attachment = _restore_ram_push_checkpoint(directory, record, output)
            checkpoint.update(start='restored', restore_seconds=round(perf_counter() - started, 3))
            _run_ram_push_server(board, session, output, bridge, attachment, checkpoint,
                                 args.ram_push_checkpoints)
            return
        checkpoint['start'] = 'fresh-boot' if args.fresh_boot else 'cold'
        boot_started = perf_counter()
    toggle_start = None
    if args.toggle_server:
        started = perf_counter()
        boot = {'cable_at_reset': args.cable_at_reset, 'adc_at_reset': adc_at_reset}
        key, key_inputs = _toggle_checkpoint_key(args.installed_dat, args.adjustment_nand, boot)
        root = None if args.toggle_checkpoints is None else args.toggle_checkpoints.resolve()
        saved = root is not None and probe_range is None and (root / key).is_dir()
        toggle_start = {'key': key, 'inputs': key_inputs, 'checkpoint_root': None if root is None else str(root),
                        'access_probe_range': None if probe_range is None else list(probe_range),
                        'key_seconds': round(perf_counter() - started, 3)}
        print(json.dumps({'ok': True, 'event': 'start-options', 'key': key, 'inputs': key_inputs,
                          'checkpoint': str(root / key) if saved else None}, separators=(',', ':')), flush=True)
        choice = json.loads(sys.stdin.readline(), object_pairs_hook=_unique_object)
        if set(choice) != {'command', 'restore'} or choice['command'] != 'start' or type(choice['restore']) is not bool:
            raise ValueError('USB-toggle start requires {"command": "start", "restore": BOOLEAN}')
        if choice['restore']:
            if not saved:
                raise ValueError('USB-toggle start names no saved checkpoint')
            from x70_native.portable_checkpoint import load
            from tools.probe_setup_menus import MenuSession
            started = perf_counter()
            record = _read_ram_push_record(root / key, TOGGLE_RECORD, key)
            board = load(root / key)
            session = MenuSession(board, output, options=dict(board._portable_factory_options,
                                                               recovery=record['recovery']))
            toggle_start.update(start='restored', restore_seconds=round(perf_counter() - started, 3))
            _run_toggle_server(board, session, output, toggle_start)
            return
        toggle_start['start'] = 'cold'
    if args.snapshot is not None:
        from x70_native.portable_checkpoint import load
        board = load(args.snapshot)
        options = board._portable_factory_options
        if args.settings is not None and options.get("settings", "default") != args.settings:
            parser.error("Snapshot settings selection differs; cold boot the requested settings")
    else:
        options = load_profile(args.cold_profile if args.recovery_ram is not None else args.profile, make_board)
        if args.settings is not None:
            options["settings"] = args.settings
        if installed_input is not None:
            if (options.get('storage_profile', 'erased') != 'erased'
                    or options.get('user_journals') is not None or options.get('settings') == 'owner'):
                parser.error('Independent installed NAND requires erased fixture selection and no injected owner journals')
        if recovery_start or installed_input is not None:
            options['entry'] = 'rom'
        if args.rom_card is not None or args.sd_image is not None or args.menu_sd_image is not None:
            from emulator.sd_card import BLANK_SDHC_PROFILE
            options.update(sd_card_profile=BLANK_SDHC_PROFILE, sd_card_present=True)
        board = make_board(**options)
        if args.installed_dat is not None:
            attach_installed_dat(board, args.installed_dat)
        elif args.installed_media is not None or args.installed_nand is not None:
            attach_installed_media(board, media=args.installed_media, nand_image=args.installed_nand)
        if installed_input is not None:
            options['recovery'] = {'phase': 'independent-installed-cold-boot',
                                   'media': str(installed_input),
                                   'card_present': any(value is not None for value in (
                                       args.rom_card, args.sd_image, args.menu_sd_image))}
        if args.adjustment_nand is not None:
            attach_adjustment_nand(board, args.adjustment_nand)
            options['recovery']['adjustment_nand'] = str(args.adjustment_nand)
        if args.menu_sd_image is not None:
            from emulator.sd_card import SDCardController, load_raw_image
            controller = next(device for device in board.devices if isinstance(device, SDCardController))
            options['recovery']['card'] = load_raw_image(
                controller.card, args.menu_sd_image, immutable_backing=False)
        if args.recovery_ram is not None:
            options['recovery'] = load_recovery_cpu0(board, args.recovery_ram)
            if args.sd_image is not None:
                from emulator.sd_card import SDCardController, load_raw_image
                controller = next(device for device in board.devices if isinstance(device, SDCardController))
                options['recovery']['card'] = load_raw_image(controller.card, args.sd_image, immutable_backing=True)
        elif args.rom_card is not None:
            options['recovery'] = attach_rom_card(board, args.rom_card, args.rom_route)
    if args.instructions is not None:
        threshold = board.instructions + args.instructions if args.instructions else 0
    else:
        threshold = args.pause_after if args.pause_after is not None else (0 if args.snapshot else 60_000_000)
    if threshold and threshold < board.instructions:
        parser.error('Absolute threshold precedes the saved machine; use --instructions for continuation')
    if args.menus:
        from tools.probe_setup_menus import MenuSession
        session = MenuSession(board, output, options=options)
    else:
        session = WarmScanoutSession(board, output, options=options)
    if args.ram_push_server:
        bridge, attachment = _ram_push_attach(board, session, args.attach_hold_key)
        if checkpoint is not None:
            from emulator.checkpoint import CheckpointError
            checkpoint['boot_seconds'] = round(perf_counter() - boot_started, 3)
            started = perf_counter()
            bridge.drain_scheduled()
            try:
                _save_ram_push_checkpoint(session, args.ram_push_checkpoints, checkpoint['key'],
                                          ATTACH_RECORD, {'key': checkpoint['key'],
                                                          'inputs': checkpoint['inputs'], **attachment},
                                          replace=args.fresh_boot)
                checkpoint['saved'] = True
            except CheckpointError as error:
                checkpoint.update(saved=False, save_error=str(error))
            checkpoint['save_seconds'] = round(perf_counter() - started, 3)
        _run_ram_push_server(board, session, output, bridge, attachment, checkpoint,
                             args.ram_push_checkpoints)
        return
    if args.toggle_server:
        if board.instructions != 0:
            raise ValueError('USB-toggle reset inputs need an unstarted board')
        power = next(device for device in board.devices if type(device).__name__ == 'PowerDevices')
        if args.cable_at_reset:
            power.set_usb_connected(True)
        if adc_at_reset is not None:
            samples = list(power.adc_samples)
            samples[adc_at_reset[0]] = adc_at_reset[1]
            power.set_adc_samples(samples, source=f'USB-toggle reset input: ADC channel {adc_at_reset[0]}')
        if probe_range is not None:
            board._native.access_probe_begin([probe_range], limit=100_000)
        _run_toggle_server(board, session, output, toggle_start)
        return
    if args.ff80_rehearsal:
        if (args.snapshot is not None or args.installed_nand is None or not args.menus
                or args.pause_after is not None or args.instructions is not None
                or args.serve or args.save_snapshot is not None):
            parser.error('--ff80-rehearsal requires --profile, --installed-nand and --menus without another run mode')
        receipt = _run_ff80_rehearsal(board, session, output)
        print(json.dumps(receipt, separators=(',', ':')), flush=True)
        return
    if args.snapshot is not None and args.serve:
        # The imported snapshot is already a validated drained boundary.
        # Retain it in this process so the viewer can undo later continuations.
        session.save_checkpoint()
    if threshold:
        print('Execute selected native profile to a natural boundary', flush=True)
        try:
            board.run(threshold - board.instructions + 2 * board.quantum, pause_after=threshold)
        except Exception as error:
            session.execution_error = {'type': type(error).__name__, 'error': str(error)}
    if args.serve and args.snapshot is None and board.failure is None and session.execution_error is None:
        session.save_checkpoint()
    if args.save_snapshot is not None:
        print(json.dumps(session.save_snapshot(args.save_snapshot)), flush=True)
    print(json.dumps(console_summary(session.sample()), separators=(',', ':')), flush=True)
    if args.export_nand is not None:
        print(json.dumps(session.save_nand(args.export_nand)), flush=True)
    if args.export_sd is not None:
        print(json.dumps(session.save_sd(args.export_sd)), flush=True)
    if args.serve:
        from emulator.server import make_menu_server
        factory = make_menu_server if args.menus else make_server
        server = factory(session, args.port, page='boot.html', bind=args.bind, hostname=args.hostname)
        print(f'Warm scanout viewer: http://{args.hostname or args.bind}:{server.server_port}/', flush=True)
        server.serve_forever()
        return
    print('Commands: sample, status, run COUNT, checkpoint, snapshot DIRECTORY, nand FILE, sd FILE, restore,'
          ' read ADDRESS LENGTH, watch ADDRESS [ADDRESS:LENGTH...], hits, nandstat, quit. No camera I/O.', flush=True)
    if args.menus:
        print('Menu input: key ok|back|q|up|down|left|right. Stock ADC scanning/debounce.', flush=True)
    trace, reported = TraceSites(board, recorded_per_site=64), 0
    for line in sys.stdin:
        fields = line.split()
        try:
            if fields == ['quit']:
                break
            if fields == ['sample']:
                value = session.sample()
            elif fields == ['status']:
                value = compact_board_state(board)
            elif fields == ['checkpoint']:
                value = session.save_checkpoint()
            elif len(fields) == 2 and fields[0] == 'snapshot':
                value = session.save_snapshot(fields[1])
            elif len(fields) == 2 and fields[0] == 'nand':
                value = session.save_nand(fields[1])
            elif len(fields) == 2 and fields[0] == 'sd':
                value = session.save_sd(fields[1])
            elif fields == ['restore']:
                value = session.run({'restore': True})
            elif len(fields) == 2 and fields[0] == 'run':
                value = session.run({'limit': int(fields[1], 0)})
            elif args.menus and len(fields) == 2 and fields[0] == 'key':
                value = session.key({'key': fields[1]})
            elif len(fields) == 3 and fields[0] == 'read':
                start, length = _console_window(fields[1] + ':' + fields[2])
                value = {'address': hex(start), 'hex': bytes(board.read_memory(start, length)).hex()}
            elif len(fields) >= 2 and fields[0] == 'watch':
                address = int(fields[1], 0)
                trace.arm([site for site in trace.specs() if site['address'] != address]
                          + [{'address': address, 'label': hex(address),
                              'windows': [_console_window(field) for field in fields[2:]]}])
                value = {'watching': [hex(site) for site in sorted(trace.sites)]}
            elif fields == ['hits']:
                value = {'hits': [_console_hit(hit) for hit in trace.hits[reported:]],
                         'counts': {hex(site): count for site, count in sorted(trace.counts.items())}}
                reported = len(trace.hits)
            elif fields == ['nandstat']:
                value = {'busy': board.nand.busy, 'transactions': dict(board.nand.transaction_counts)}
            else:
                raise ValueError('Unknown command')
            if fields != ['status'] and isinstance(value, dict) and 'instructions' in value and 'display' in value:
                value = console_summary(value)
            print(json.dumps(value, separators=(',', ':')), flush=True)
        except Exception as error:
            print(json.dumps({'error': type(error).__name__, 'message': str(error)}), flush=True)


if __name__ == '__main__':
    main()
