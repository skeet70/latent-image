#!/usr/bin/env python3
"""Navigate X-M1 1.50 menus using external ADC and GPIO inputs.

Every pulse advances the target scanner, debounce, queues and UI. Diagnostics
observe firmware RAM and failed CPU0 state, not rendering or commit success.
"""
from __future__ import annotations

import argparse
import copy
from dataclasses import dataclass
from enum import Enum
import json
import os
from pathlib import Path
import sys
import struct
from uuid import uuid4

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from emulator.checkpoint import capture_blocker
from emulator.menu_inputs import ALIASES, KEY_NAMES, MenuKey, MenuInputs, setup_inventory
from tools.continue_rec_scanout import MAX_CONTINUATION, WarmScanoutSession

MAX_PHASE = MAX_CONTINUATION
DEFAULT_PULSE = 8_000_000


class _ObservationWindow(Enum):
    STACK = struct.Struct('<128I')
    FRAME = struct.Struct('<64I')


class _ObservationUnavailable(Enum):
    NOT_FAILED = 'not_failed'
    GUEST_EXECUTING = 'guest_executing'
    REGISTERS_UNAVAILABLE = 'registers_unavailable'
    OUTSIDE_DRAM = 'outside_dram'
    UNALIGNED_POINTER = 'unaligned_pointer'
    LOW_ROM_ALIAS = 'low_rom_alias'


@dataclass(frozen=True)
class _StoppedCPU0Registers:
    general: tuple[int, ...]
    sp: int
    lr: int
    pc: int
    cpsr: int

    @classmethod
    def observe(cls, cpu):
        from x70_native import registers
        return cls(tuple(cpu.reg_read(register) for register in registers.GENERAL),
                   cpu.reg_read(registers.SP), cpu.reg_read(registers.LR),
                   cpu.reg_read(registers.PC), cpu.reg_read(registers.CPSR))

    def as_json(self):
        return {**{f'r{index}': value for index, value in enumerate(self.general)},
                'sp': self.sp, 'lr': self.lr, 'pc': self.pc, 'cpsr': self.cpsr}


@dataclass(frozen=True)
class _StoppedDRAMWindow:
    address: int
    physical_address: int
    words: tuple[int, ...]
    kind: _ObservationWindow

    @classmethod
    def observe(cls, board, address, kind: _ObservationWindow):
        if address & 3:
            return _ObservationUnavailable.UNALIGNED_POINTER
        offset = board.memory_bus.ram_offset(address, kind.value.size)
        if offset is None:
            return _ObservationUnavailable.OUTSIDE_DRAM
        if board.low_rom_alias and address == offset and offset < len(board.rom):
            return _ObservationUnavailable.LOW_ROM_ALIAS
        # Allocation reads do not invoke guest bus or access-probe callbacks.
        data = memoryview(board.ram).cast('B')[offset:offset + kind.value.size]
        return cls(address, offset, kind.value.unpack(data), kind)

    def as_json(self):
        return {'available': True, 'address': self.address,
                'physical_address': self.physical_address, 'bytes': self.kind.value.size,
                'little_endian_words': list(self.words)}


class MenuSession(WarmScanoutSession):
    """Warm viewer adapter; owner thread must serialize key/run/restore calls."""
    def __init__(self, board, output=None, *, options=None, observe=False, firmware=None):
        if observe:
            raise ValueError('Instruction observers are not supported for X-M1 1.50')
        output = output or ROOT / '.tools/menu-probes' / uuid4().hex[:12]
        super().__init__(board, output, options=options)
        self.inputs = MenuInputs(board)
        if firmware is None:
            from tools import firmware_registry as registry
            selected = self.options.get('firmware_profile') or {}
            entry = registry.load(selected['firmware_id']) if selected.get('firmware_id') else registry.by_sha256(
                board.manifest['source_sha256'])
            firmware = dict(filename=entry['source']['filename'], sha256=entry['source']['sha256'],
                            model=entry['model'], version=entry['version'],
                            modified=entry['source']['sha256'] != registry.load('xm1-150')['source']['sha256'])
        self.firmware = dict(firmware, can_replace=bool(os.environ.get('XM1_VIEWER_VALIDATOR')))
        from x70_native.provenance import provenance
        self._execution_provenance = provenance()

    @classmethod
    def cold_boot(cls, options, output, *, firmware, retained_from=None):
        from emulator.boot import make_board, restart_with_sd_media
        board = None
        output = Path(output)
        try:
            board = make_board(**options) if retained_from is None else restart_with_sd_media(retained_from)
            session = cls(board, output, options=board._portable_factory_options, firmware=firmware)
            board.run(80_002_000, pause_after=80_000_000)
            if board.failure is not None:
                raise RuntimeError(str(board.failure))
            session.save_checkpoint()
            session.sample()
        except Exception as error:
            output.mkdir(parents=True, exist_ok=True)
            diagnostics = output / 'boot-failure.json'
            diagnostics.write_text(json.dumps({
                'firmware': firmware, 'instructions': None if board is None else board.instructions,
                'virtual_ticks': None if board is None else board.virtual_ticks,
                'failure': None if board is None else board.failure,
                'exception': {'type': type(error).__name__, 'error': str(error)}}, indent=2) + '\n')
            raise ValueError(f'Firmware boot failed: {error}; private diagnostics: {diagnostics.resolve()}') from error
        return session

    def replace_firmware(self, source, filename):
        if not self.firmware['can_replace']:
            raise ValueError('Firmware selection requires the xm1-viewer launcher')
        from tools.firmware_registry import prepare_xm1_viewer
        profile, identity = prepare_xm1_viewer(source, filename)
        options = dict(self.board._portable_factory_options)
        options['firmware_profile'] = profile
        replacement = type(self).cold_boot(
            options, self.output.parent / ('firmware-' + uuid4().hex), firmware=identity)
        self.__dict__ = replacement.__dict__
        return self.state

    def _sd_controller(self):
        from emulator.sd_card import SDCardController
        cards = [device for device in self.board.devices if isinstance(device, SDCardController)]
        if len(cards) != 1:
            raise ValueError('Media controls require exactly one supported SD controller')
        return cards[0]

    def media_control(self, options):
        from emulator.sd_card import MediaAction
        if type(options) is not dict or set(options) != {'action'} or type(options['action']) is not str:
            raise ValueError('SD media control requires exactly one named action')
        action = MediaAction(options['action'])
        self._sd_controller().media_control(action)
        return self.sample()

    def restart_media(self, options):
        if options != {}:
            raise ValueError('Media-only cold restart takes an empty options object')
        replacement = type(self).cold_boot(
            {}, self.output.parent / ('media-restart-' + uuid4().hex),
            firmware=self.firmware, retained_from=self.board)
        self.__dict__ = replacement.__dict__
        return self.state

    def stopped_architecture(self):
        """Observe failed CPU0's current register bank and two fixed DRAM windows."""
        if self.board.executing_cpu is not None:
            return {'available': False, 'cpu': 0,
                    'reason': _ObservationUnavailable.GUEST_EXECUTING.value}
        if self.board.failure is None and self.execution_error is None:
            return {'available': False, 'cpu': 0,
                    'reason': _ObservationUnavailable.NOT_FAILED.value}
        try:
            registers = _StoppedCPU0Registers.observe(self.board.cpus[0])
        except (RuntimeError, ValueError) as error:
            return {'available': False, 'cpu': 0,
                    'reason': _ObservationUnavailable.REGISTERS_UNAVAILABLE.value,
                    'detail': str(error)}
        windows = {}
        for kind, address in ((_ObservationWindow.STACK, registers.sp),
                              (_ObservationWindow.FRAME, registers.general[11] - 32)):
            window = _StoppedDRAMWindow.observe(self.board, address, kind)
            windows[kind.name.lower()] = ({'available': False, 'reason': window.value, 'address': address}
                             if isinstance(window, _ObservationUnavailable) else window.as_json())
        return {'available': True, 'cpu': 0, 'registers': registers.as_json(), 'windows': windows}

    def sample(self, pixels=True):
        state = super().sample(pixels)
        state['execution_backend'].update(
            provenance=copy.deepcopy(self._execution_provenance),
            metrics=self.board._native.metrics())
        state['firmware'] = copy.deepcopy(self.firmware)
        from emulator.sd_card import SDCardController
        cards = [device for device in self.board.devices if isinstance(device, SDCardController)]
        state['sd_media'] = None if not cards else cards[0].snapshot()
        state['stopped_architecture'] = self.stopped_architecture()
        availability = self.inputs.availability()
        ready = (availability['ready'] and self.board.failure is None
                 and self.execution_error is None)
        input_state = self.inputs.current_state(ready=availability['ready'])
        checkpoint_blocker = capture_blocker(self.board)
        state['navigation'] = {
            'supported': True,
            'keys': list(KEY_NAMES), 'aliases': dict(ALIASES),
            'default_hold_instructions': DEFAULT_PULSE,
            'default_release_instructions': DEFAULT_PULSE,
            'max_instructions_per_phase': MAX_PHASE,
            'checkpoint_present': self.checkpoint is not None,
            'can_save_checkpoint': ready and input_state['inputs_released'] and checkpoint_blocker is None,
            'checkpoint_unavailable_reason': None if checkpoint_blocker is None else str(checkpoint_blocker),
            'ready': ready, 'can_input': ready, 'input_readiness': availability,
            'render_supported': True,
            **input_state,
            'limits': 'External ADC/GPIO levels and source-bound sub-command Gray cycles; electrical timing and clockwise sense are unmeasured.',
            'unsupported_inputs': 'Main command-dial rotation is not bound to this input adapter.',
            'observer_supported': False}
        if getattr(self, '_last_render_settle', None) is not None:
            state['navigation']['render_settle'] = copy.deepcopy(self._last_render_settle)
        return state

    def run(self, options):
        # Sampling preserves the last render-settle result until continuation or restore.
        if isinstance(options, dict) and (options.get('restore') is True or
                type(options.get('limit')) is int and options['limit'] > 0):
            self._last_render_settle = None
        return super().run(options)

    def key(self, options):
        if not isinstance(options, dict) or set(options) - {
                'key', 'hold_instructions', 'release_instructions', 'settle_instructions', 'render'}:
            raise ValueError('Unknown menu key options')
        render = options.get('render', False)
        if type(render) is not bool:
            raise ValueError('Render must be a boolean')
        if render and 'settle_instructions' in options:
            raise ValueError('Render settling cannot be combined with settle_instructions')
        key = MenuKey.parse(options.get('key'))
        budgets = [options.get('hold_instructions', DEFAULT_PULSE),
                   options.get('release_instructions', DEFAULT_PULSE),
                   options.get('settle_instructions', 0)]
        if any(type(value) is not int or not minimum <= value <= MAX_PHASE
               for value, minimum in zip(budgets, (1, 1, 0))):
            raise ValueError(f'Hold/release must be 1..{MAX_PHASE}; settle 0..{MAX_PHASE}')
        if key in (MenuKey.DIAL_LEFT, MenuKey.DIAL_RIGHT) and budgets[0] < 4:
            raise ValueError('Sub-command rotation hold must allow all four GPIO phases')
        if self.board.failure is not None or self.execution_error is not None:
            raise ValueError('Restore the checkpoint before another input after a strict stop')
        availability = self.inputs.availability()
        if not availability['ready']:
            raise ValueError(availability['reason'])
        if render:
            from tools.menu_render_settle import MenuRenderSettle
            return MenuRenderSettle(self, key_action=self.key).key(
                {name: value for name, value in options.items() if name != 'render'})
        try:
            self.inputs.press(key)
            phase_budget, remainder = divmod(budgets[0], self.inputs.phase_count)
            for phase in range(self.inputs.phase_count):
                if phase:
                    self.inputs.phase(phase)
                phase_limit = phase_budget + (phase < remainder)
                if self.inputs.phase_count == 1:
                    self.run({'limit': phase_limit, 'pixels': False})
                else:
                    deadline = self.board.instructions + phase_limit
                    while not self.inputs.phase_acknowledged():
                        remaining = deadline - self.board.instructions
                        if remaining <= 0:
                            raise RuntimeError('X-M1 sub-command scanner did not acknowledge the external GPIO phase')
                        self.run({'limit': min(remaining, 2 * self.board.quantum), 'pixels': False})
                        if self.board.failure is not None or self.execution_error is not None:
                            break
                if self.board.failure is not None or self.execution_error is not None:
                    break
        finally:
            self.inputs.release()
        if self.board.failure is not None or self.execution_error is not None:
            return self.sample()
        state = self.run({'limit': budgets[1]})
        if budgets[2] and self.board.failure is None and self.execution_error is None:
            state = self.run({'limit': budgets[2]})
        return state

    def checkpoint_current(self, options):
        """Replace the host restore point without advancing or changing inputs."""
        if options != {}:
            raise ValueError('Set checkpoint takes an empty options object')
        if self.board.failure is not None or self.execution_error is not None:
            raise ValueError('Restore before replacing the checkpoint after a strict stop')
        availability = self.inputs.availability()
        if not availability['ready']:
            raise ValueError(availability['reason'])
        if not self.inputs.current_state()['inputs_released']:
            raise ValueError('Release the key before replacing the checkpoint')
        self.save_checkpoint()
        return self.sample()


    def print_state(self, state=None):
        state = state or self.sample()
        navigation = state['navigation']
        menu = navigation['current_menu']
        print(f'instructions={self.board.instructions} ticks={self.board.virtual_ticks}'
              f' failure={state["failure"]} error={self.execution_error}', flush=True)
        print(f'  ADC={navigation["adc_samples"]} pressed={navigation["pressed"]}'
              f' UI={state["ui"]["mode"]}/{state["ui"]["pending"]}/{state["ui"]["busy"]}'
              f' menu={menu["pointer"]:#x} id={menu["menu_id"]} type={menu["type_code"]}'
              f' setting={menu["setting_id"]} cursor={menu["cursor"]}'
              f' subcursor={menu["subcursor"]} visible={menu["visible_count"]}'
              f' selected={menu.get("selected_item")}')
        frame = state['display']['last_frame'] or {}
        print(f'  frame={frame.get("frame_sha256")} output={self.output}')
        print('  Frame identity and retained menu pointer alone do not establish visible menu coverage.', flush=True)

    def print_inventory(self):
        inventory = setup_inventory(self.board)
        roots = set(item['child_pointer'] for item in inventory['tabs'])
        print(f'SETUP static candidates: tabs={len(inventory["tabs"])} nodes={len(inventory["nodes"])}')
        for node in inventory['nodes']:
            if node['pointer'] in roots:
                print(f'  root={node["pointer"]:#x} id={node["menu_id"]:#x}'
                      f' kind={node["kind"]} raw_items={len(node["raw_items"])}')
                for item in node['raw_items']:
                    print(f'    index={item["index"]} value={item["value"]:#x} child={item["child_pointer"]:#x}')
        print(inventory['limits'], flush=True)

    def interactive(self):
        self.print_state()
        print('Commands: key NAME, run COUNT, state, inventory, checkpoint, restore, ram HEX COUNT, quit.', flush=True)
        for line in sys.stdin:
            fields = line.split()
            try:
                if fields == ['quit']:
                    break
                if len(fields) == 2 and fields[0] == 'key':
                    self.print_state(self.key({'key': fields[1]}))
                elif len(fields) == 2 and fields[0] == 'run':
                    limit = int(fields[1], 0)
                    if not 0 <= limit <= MAX_PHASE:
                        raise ValueError(f'Interactive continuation must be 0..{MAX_PHASE}')
                    self.print_state(self.run({'limit': limit}))
                elif fields == ['state']:
                    self.print_state()
                elif fields == ['inventory']:
                    self.print_inventory()
                elif fields == ['checkpoint']:
                    self.checkpoint_current({})
                    self.print_state()
                elif fields == ['restore']:
                    self.print_state(self.run({'limit': 0, 'restore': True}))
                elif len(fields) == 3 and fields[0] == 'ram':
                    from emulator.menu_inputs import _raw
                    address, count = int(fields[1], 16), int(fields[2], 0)
                    if not 0 < count <= 256:
                        raise ValueError('RAM read must be 1..256 bytes')
                    print(f'{address:#x}: {_raw(self.board, address, count).hex()}', flush=True)
                else:
                    raise ValueError('Unknown command')
            except Exception as error:
                print(f'{type(error).__name__}: {error}', flush=True)


def main():
    from emulator.image_parameter_devices import UIConversionProfile
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--snapshot', type=Path, required=True)
    parser.add_argument('--keys', default='', help='Comma-separated stock key pulses')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--inventory', action='store_true')
    parser.add_argument('--observe', action='store_true')
    parser.add_argument('--interactive', action='store_true')
    parser.add_argument('--ui-conversion-profile', type=UIConversionProfile.parse,
                        choices=tuple(UIConversionProfile),
                        help='Require this profile in the incoming snapshot; never attach it on restore')
    args = parser.parse_args()
    if args.output and (ROOT / '.tools' not in args.output.resolve().parents or args.output.exists()):
        parser.error('--output must be a fresh ignored .tools directory')
    if args.ui_conversion_profile is not None:
        manifest = json.loads((args.snapshot / 'manifest.json').read_text())
        if manifest.get('factory_options', {}).get('ui_conversion_profile') != args.ui_conversion_profile.value:
            parser.error('A snapshot owns its UI conversion profile; selection cannot be overridden')
    from x70_native.portable_checkpoint import load
    board = load(args.snapshot)
    session = MenuSession(board, args.output, options=board._portable_factory_options, observe=args.observe)
    session.save_checkpoint()
    if args.inventory:
        session.print_inventory()
    session.print_state()
    for name in filter(None, args.keys.split(',')):
        state = session.key({'key': name})
        session.print_state(state)
        if state['failure'] is not None or session.execution_error is not None:
            break
    if args.interactive:
        session.interactive()


if __name__ == '__main__':
    main()
