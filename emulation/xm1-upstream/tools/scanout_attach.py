"""X-M1 board assembly and the first-start route to the attach boundary.

Everything here decides the guest state a portable checkpoint saves: the board
the profile and media inputs build, the first-start key pulses, the cable
attach and the ff80 enumeration. Bridge command handling lives in
`scanout_commands.py`; the launcher in `continue_rec_scanout.py` serves it.
"""
import copy
import hashlib
import inspect
import json
import sys
from pathlib import Path
from uuid import uuid4

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

FIRST_START_PULSE = 8_000_000
ATTACH_HOLD_PULSE = 8_000_000
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
            'movrsz_profile': 'xm1-150-declared-movrsz-scene-identity-unmeasured',
            'movrsz_ticks': 50000,
            'face_stop_profile': 'declared-armed-face-dma-idle-abort-unmeasured',
            'producer_data_profile': 'xm1-150-declared-continuous-statistics-scene-unmeasured',
            'fff63_profile': 'xm1-150-declared-fff63-job4-completion-unmeasured',
            'mode_devices_profile': 'xm1-150-declared-mode-devices-unmeasured',
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


def restore_private_menu_card(board, source, output):
    """Rebind host permission without replacing checkpointed guest sectors."""
    from dataclasses import replace
    import hashlib
    from emulator.sd_card import SDCardController
    from x70_native._native import copy_menu_card
    controller = next(device for device in board.devices if isinstance(device, SDCardController))
    if controller.image is None:
        raise ValueError('A private card restore requires checkpointed card media')
    private_card = copy_menu_card(source, output.with_name(output.name + '-private-card'))
    with Path(private_card.path).open('rb') as image:
        digest = hashlib.file_digest(image, 'sha256').hexdigest()
    if digest != controller.image.sha256:
        raise ValueError('Private card input differs from checkpointed card media')
    controller.image = replace(controller.image, path=private_card.path)
    board._s2_private_card = private_card
    return controller.image


def cold_board(args, make_board, fail, output):
    """The board the selected inputs build, and the options it was built with."""
    installed_input = args.installed_media or args.installed_nand or args.installed_dat
    recovery_start = args.recovery_ram is not None or args.rom_card is not None
    options = load_profile(args.cold_profile if args.recovery_ram is not None else args.profile, make_board)
    if args.settings is not None:
        options["settings"] = args.settings
    if installed_input is not None:
        if (options.get('storage_profile', 'erased') != 'erased'
                or options.get('user_journals') is not None or options.get('settings') == 'owner'):
            fail('Independent installed NAND requires erased fixture selection and no injected owner journals')
    if recovery_start or installed_input is not None:
        options['entry'] = 'rom'
    if args.rom_card is not None or args.sd_image is not None or args.menu_sd_image is not None:
        from emulator.sd_card import BLANK_SDHC_PROFILE
        options.update(sd_card_profile=BLANK_SDHC_PROFILE, sd_card_present=True)
    if args.ui_conversion_profile is not None:
        options['ui_conversion_profile'] = args.ui_conversion_profile
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
        from emulator.sd_card import MediaImage, SDCardController, load_raw_image
        from x70_native._native import copy_menu_card
        private_card = copy_menu_card(args.menu_sd_image, output.with_name(output.name + '-private-card'))
        controller = next(device for device in board.devices if isinstance(device, SDCardController))
        options['recovery']['card'] = load_raw_image(
            controller.card, Path(private_card.path), immutable_backing=False)
        controller.image = MediaImage(**options['recovery']['card'])
        board._s2_private_card = private_card
    if args.recovery_ram is not None:
        options['recovery'] = load_recovery_cpu0(board, args.recovery_ram)
        if args.sd_image is not None:
            from emulator.sd_card import SDCardController, load_raw_image
            controller = next(device for device in board.devices if isinstance(device, SDCardController))
            options['recovery']['card'] = load_raw_image(controller.card, args.sd_image, immutable_backing=True)
    elif args.rom_card is not None:
        options['recovery'] = attach_rom_card(board, args.rom_card, args.rom_route)
    return board, options


class _FF80EndpointBridge:
    """The guest-side endpoint primitives the first-start route and ff80 attach use."""
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

    READY_STEP = 1_000

    def run_until(self, label, predicate, budget, step=1_000_000):
        end = self.board.instructions + budget
        while not predicate():
            failure = self.board.failure or self.session.execution_error
            if failure is not None:
                raise RuntimeError(f'{label}: {failure}')
            if self.board.instructions >= end:
                raise TimeoutError(f'{label}: instruction budget expired; usb={self.endpoint.snapshot()}')
            target = min(end, self.board.instructions + step)
            try:
                self.board.run(target - self.board.instructions + 2 * self.board.quantum,
                               pause_after=target)
            except Exception as error:
                self.session.execution_error = {'type': type(error).__name__, 'error': str(error)}
            step = min(2 * step, 1_000_000)

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
            self.run_until('bulk IN reply', reply_ready, 30_000_000, self.READY_STEP)
            packet = self.endpoint.host_in(endpoint_address, count - len(chunks))
            chunks.extend(packet)
            self.run_for(20_000)
            if len(packet) < self.endpoint.contract.packet_bytes:
                break
        self.run_until('bulk IN DMA teardown',
                       lambda: all(not channel[0x004] & 1 for channel in self.endpoint.dma),
                       30_000_000, self.READY_STEP)
        return bytes(chunks)

    def control_out(self, payload):
        setup = bytes((0x43, 0, 0, 0, 0, 0, len(payload) & 0xff, len(payload) >> 8))
        self.endpoint.host_setup(setup)
        self.run_until('control OUT setup consumption',
                       lambda: self.endpoint.setup_position == len(self.endpoint.setup),
                       30_000_000, self.READY_STEP)
        self.run_for(20_000)
        self.endpoint.host_out(0, payload)
        self.run_until('control OUT status', lambda: bool(self.endpoint.tx[0]), 30_000_000, self.READY_STEP)
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

    def drain_scheduled(self):
        """Run in 200,000-instruction steps, at most 50, until no Python closure is scheduled."""
        for _ in range(50):
            if not self.board.scheduled:
                return
            self.run_for(200_000)

    @staticmethod
    def key_state(state):
        current = state.get('navigation', {}).get('current_menu', {})
        return {key: current.get(key) for key in ('menu_id', 'setting_id', 'cursor')}


def _ram_push_attach(board, session, hold_key=None, bridge=None):
    """Boot past the first-start language screen into live view and enumerate ff80. HOLD_KEY is
    pressed ATTACH_HOLD_PULSE instructions before the cable level rises and released after enumeration."""
    if bridge is None:
        from tools.scanout_commands import _ram_push_bridge
        bridge = _ram_push_bridge(board, session)
    bridge.run_until('first-start screen', lambda: board.instructions >= 100_000_000, 100_000_000)
    for key in ['up'] * 12 + ['ok', 'ok']:
        session.key({'key': key, 'hold_instructions': FIRST_START_PULSE,
                     'release_instructions': FIRST_START_PULSE})
    bridge.run_for(40_000_000)
    pre_attach = {'instructions': board.instructions,
                  'raw_identities': list(map(hex, board._native.cp14_identity()))}
    if hold_key is not None:
        session.inputs.press(hold_key)
        bridge.run_for(ATTACH_HOLD_PULSE)
        hold = {'key': hold_key, 'pressed_instructions': pre_attach['instructions'],
                'attach_instructions': board.instructions}
    device, configuration = bridge.enumerate()
    if hold_key is not None:
        session.inputs.release()
        hold['released_instructions'] = board.instructions
        bridge.run_for(ATTACH_HOLD_PULSE)
        hold['menu_after_release'] = bridge.key_state(session.sample(False))
        pre_attach['attach_hold'] = hold
    return bridge, {'pre_attach': pre_attach, 'device': device.hex(),
                    'configuration': configuration.hex()}


RAM_PUSH_CHECKPOINT_SCHEMA = 1

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
