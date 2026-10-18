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
import inspect
import json
from pathlib import Path
import sys
from time import perf_counter
from uuid import uuid4

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

MAX_CONTINUATION = 15_000_000


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
    parser.add_argument('--export-nand', type=Path, help='Save committed installed NAND to a fresh file after this run')
    parser.add_argument('--export-sd', type=Path, help='Save logical SD bytes and guest write overlays to a fresh image')
    budget = parser.add_mutually_exclusive_group()
    budget.add_argument('--pause-after', type=int, help='Absolute instruction threshold; default60M for cold boot, zero for restore')
    budget.add_argument('--instructions', type=int, help='Instructions to run from the selected starting state')
    parser.add_argument('--settings', choices=('default', 'owner'), help='Persistent settings selection; verify restored selection')
    parser.add_argument('--save-snapshot', type=Path, help='Save drained machine state to a fresh local directory')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--serve', action='store_true')
    parser.add_argument('--menus', action='store_true',
                        help='Enable declared ADC button input and stock menu navigation')
    parser.add_argument('--port', type=int, default=0)
    parser.add_argument('--bind', default='127.0.0.1')
    parser.add_argument('--hostname')
    args = parser.parse_args()
    recovery_start = args.recovery_ram is not None or args.rom_card is not None
    if recovery_start and (args.snapshot is not None or not (args.installed_media or args.installed_nand)):
        parser.error('Recovery requires a cold board and independent --installed-media or --installed-nand')
    if args.recovery_ram is not None and args.rom_card is not None:
        parser.error('Select direct ROM card execution or exported RAM continuation, not both')
    if args.snapshot is not None and (args.installed_media or args.installed_nand):
        parser.error('Installed-media inputs belong to independent cold boards, not machine checkpoints')
    if args.sd_image is not None and args.recovery_ram is None:
        parser.error('--sd-image belongs to --recovery-ram; use --rom-card for direct ROM execution')
    for value in (args.pause_after, args.instructions):
        if value is not None and not 0 <= value <= 100_000_000:
            parser.error('Instruction threshold/budget must be 0..100000000')
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
        if args.installed_media is not None or args.installed_nand is not None:
            if (options.get('storage_profile', 'erased') != 'erased'
                    or options.get('user_journals') is not None or options.get('settings') == 'owner'):
                parser.error('Independent installed NAND requires erased fixture selection and no injected owner journals')
        if recovery_start or args.installed_media is not None or args.installed_nand is not None:
            options['entry'] = 'rom'
        if args.rom_card is not None or args.sd_image is not None:
            from emulator.sd_card import BLANK_SDHC_PROFILE
            options.update(sd_card_profile=BLANK_SDHC_PROFILE, sd_card_present=True)
        board = make_board(**options)
        if args.installed_media is not None or args.installed_nand is not None:
            attach_installed_media(board, media=args.installed_media, nand_image=args.installed_nand)
            options['recovery'] = {'phase': 'independent-installed-cold-boot',
                                   'media': str(args.installed_media or args.installed_nand),
                                   'card_present': args.rom_card is not None or args.sd_image is not None}
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
    output = args.output or ROOT / '.tools/rec-scanout' / (
        datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ') + '-' + uuid4().hex[:8])
    if args.menus:
        from tools.probe_setup_menus import MenuSession
        session = MenuSession(board, output, options=options)
    else:
        session = WarmScanoutSession(board, output, options=options)
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
    print('Commands: sample, status, run COUNT, checkpoint, snapshot DIRECTORY, nand FILE, sd FILE, restore, quit. No camera I/O.', flush=True)
    if args.menus:
        print('Menu input: key ok|back|q|up|down|left|right. Stock ADC scanning/debounce.', flush=True)
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
            else:
                raise ValueError('Unknown command')
            if fields != ['status'] and isinstance(value, dict) and 'instructions' in value and 'display' in value:
                value = console_summary(value)
            print(json.dumps(value, separators=(',', ':')), flush=True)
        except Exception as error:
            print(json.dumps({'error': type(error).__name__, 'message': str(error)}), flush=True)


if __name__ == '__main__':
    main()
