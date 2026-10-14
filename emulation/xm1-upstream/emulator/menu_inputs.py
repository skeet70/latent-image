"""External X-M1 1.50 inputs and read-only firmware menu diagnostics.

ADC samples and GPIO pins are host inputs. Firmware owns scanning, debounce,
queues, UI dispatch, row visibility and setting commits.
"""
from __future__ import annotations

from dataclasses import dataclass
from enum import Enum, IntEnum
import struct

RELEASED = 1023
KEY_TABLE = 0x828120
CALIBRATION_POINTER = 0x96ca50
SCANNER_REGISTERED = 0x89f7c8
NODE_REGISTRY = 0x81babc
CURRENT_NODE = 0x9571d0
ROW_CAP = 0x4f
ALIASES = {'menu': 'ok', 'disp': 'back'}


class MenuKey(str, Enum):
    UP = 'up'
    DOWN = 'down'
    LEFT = 'left'
    RIGHT = 'right'
    OK = 'ok'
    BACK = 'back'
    Q = 'q'
    PLAY = 'play'
    FN = 'fn'
    DIAL_PUSH = 'dial-push'

    @classmethod
    def parse(cls, value):
        if isinstance(value, cls):
            return value
        if type(value) is not str:
            raise ValueError('Menu key name must be text')
        try:
            return cls(ALIASES.get(value.lower(), value.lower()))
        except ValueError:
            raise ValueError('Unknown menu key: ' + value) from None


class KeyCode(IntEnum):
    OK = 0x01
    UP = 0x02
    DOWN = 0x03
    LEFT = 0x04
    RIGHT = 0x05
    BACK = 0x09
    PLAY = 0x0b
    FN = 0x28
    DIAL_PUSH = 0x2b
    Q = 0x32
    MOVIE = 0x34


class AdcChannel(IntEnum):
    ARROWS = 8
    BUTTONS = 9


class CalibrationTag(IntEnum):
    OK_LOW = 0x3796
    OK_HIGH = 0x3794
    Q_LOW = 0x379a
    Q_HIGH = 0x3798
    BACK_LOW = 0x379e
    BACK_HIGH = 0x379c
    MOVIE_LOW = 0x37a2
    MOVIE_HIGH = 0x37a0
    UP_LOW = 0x37aa
    UP_HIGH = 0x37a8
    DOWN_LOW = 0x37ae
    DOWN_HIGH = 0x37ac
    LEFT_LOW = 0x37b2
    LEFT_HIGH = 0x37b0
    RIGHT_LOW = 0x37b6
    RIGHT_HIGH = 0x37b4


@dataclass(frozen=True)
class AdcBinding:
    code: KeyCode
    row: int
    channel: AdcChannel
    low_tag: CalibrationTag
    high_tag: CalibrationTag

    @property
    def address(self):
        return KEY_TABLE + self.row * 28


@dataclass(frozen=True)
class GpioBinding:
    code: KeyCode
    row: int
    port: int
    pin: int
    pressed_level: int

    @property
    def address(self):
        return KEY_TABLE + self.row * 28


@dataclass(frozen=True)
class CalibratedRange:
    binding: AdcBinding
    low: int
    high: int

    def contains(self, sample):
        return self.low <= sample <= self.high

    @property
    def midpoint(self):
        return (self.low + self.high) // 2


KEY_RECORDS = {
    MenuKey.UP: AdcBinding(KeyCode.UP, 14, AdcChannel.ARROWS, CalibrationTag.UP_LOW, CalibrationTag.UP_HIGH),
    MenuKey.DOWN: AdcBinding(KeyCode.DOWN, 15, AdcChannel.ARROWS, CalibrationTag.DOWN_LOW, CalibrationTag.DOWN_HIGH),
    MenuKey.LEFT: AdcBinding(KeyCode.LEFT, 16, AdcChannel.ARROWS, CalibrationTag.LEFT_LOW, CalibrationTag.LEFT_HIGH),
    MenuKey.RIGHT: AdcBinding(KeyCode.RIGHT, 17, AdcChannel.ARROWS, CalibrationTag.RIGHT_LOW, CalibrationTag.RIGHT_HIGH),
    MenuKey.OK: AdcBinding(KeyCode.OK, 18, AdcChannel.BUTTONS, CalibrationTag.OK_LOW, CalibrationTag.OK_HIGH),
    MenuKey.BACK: AdcBinding(KeyCode.BACK, 20, AdcChannel.BUTTONS, CalibrationTag.BACK_LOW, CalibrationTag.BACK_HIGH),
    MenuKey.Q: AdcBinding(KeyCode.Q, 19, AdcChannel.BUTTONS, CalibrationTag.Q_LOW, CalibrationTag.Q_HIGH),
}
GPIO_KEY_RECORDS = {
    MenuKey.PLAY: GpioBinding(KeyCode.PLAY, 12, 2, 9, 0),
    MenuKey.FN: GpioBinding(KeyCode.FN, 13, 0, 11, 0),
    MenuKey.DIAL_PUSH: GpioBinding(KeyCode.DIAL_PUSH, 22, 0, 10, 0),
}
ADC_RECORDS = tuple(KEY_RECORDS.values()) + (
    AdcBinding(KeyCode.MOVIE, 21, AdcChannel.BUTTONS, CalibrationTag.MOVIE_LOW, CalibrationTag.MOVIE_HIGH),
)
KEY_NAMES = tuple(key.value for key in MenuKey)


class RowState(IntEnum):
    ACTIVE = 0
    DISABLED = 2
    HIDDEN = 3


def _raw(board, address, count):
    physical = address - 0x40000000 if 0x40000000 <= address < 0x60000000 else address
    if not (count > 0 and 0 < physical and physical + count <= 0x20000000):
        raise ValueError(f'Menu diagnostic requires a bounded RAM range: {address:#x}/{count}')
    data = bytes(board.read_memory(address, count, cpu_id=0))
    if len(data) != count:
        raise ValueError('Truncated menu RAM read')
    return data


def _word(board, address):
    return int.from_bytes(_raw(board, address, 4), 'little')


def descriptor(board, pointer, *, max_items=ROW_CAP):
    """Decode the 0x2c-byte node and FUN_00bf9794's terminated values array."""
    if type(max_items) is not int or not 1 <= max_items <= ROW_CAP:
        raise ValueError('Menu list limit must be 1..79')
    words = struct.unpack('<11I', _raw(board, pointer, 0x2c))
    menu_id, type_code, setting_id, kind, wrap, unfiltered, flag, parent, items, labels, selector = words
    values = []
    if items:
        for index in range(max_items + 1):
            value, child = struct.unpack('<2I', _raw(board, items + index * 8, 8))
            if value == 0x7ffffffe and child == 0:
                break
            if index == max_items:
                raise ValueError('Menu list lacks its bounded terminator')
            values.append({'index': index, 'value': value, 'child_pointer': child})
    return {'pointer': pointer, 'menu_id': menu_id, 'type_code': type_code,
            'setting_id': setting_id, 'kind': kind, 'flags_word': flag,
            'wraps_navigation': wrap != 1, 'wrap_word': wrap,
            'unfiltered_word': unfiltered, 'parent_pointer': parent,
            'item_array': items, 'labels_pointer': labels, 'selector': selector,
            'raw_items': values,
            'limits': 'Static candidate rows; visibility is reported only for the retained current node.'}


def current_menu(board):
    pointer = _word(board, CURRENT_NODE)
    if not pointer:
        return {'ready': False, 'reason': 'Firmware has not selected a menu node',
                'pointer': 0, 'menu_id': None, 'type_code': None, 'setting_id': None,
                'cursor': None, 'subcursor': None, 'visible_count': None,
                'filtered_count': None, 'raw_count': None, 'selected_item': None,
                'wraps_navigation': False, 'item_statuses': [], 'raw_items': []}
    result = descriptor(board, pointer)
    result.update(ready=True, reason=None, cursor=_word(board, CURRENT_NODE + 8),
                  subcursor=_word(board, CURRENT_NODE + 0xc),
                  visible_count=_word(board, CURRENT_NODE + 0x10),
                  page_cursor=_word(board, CURRENT_NODE + 0x14),
                  raw_count=_word(board, CURRENT_NODE + 0x18),
                  filtered_count=_word(board, CURRENT_NODE + 0x1c))
    statuses = [_word(board, CURRENT_NODE + 0x20 + index * 4)
                for index in range(len(result['raw_items']))]
    result['item_statuses'] = statuses
    for item, status in zip(result['raw_items'], statuses):
        item.update(status=status, visible=result['unfiltered_word'] != 0 or status != RowState.HIDDEN,
                    selectable=result['unfiltered_word'] == 1 or status < RowState.DISABLED)
    cursor = result['cursor']
    result['selected_item'] = (dict(result['raw_items'][cursor])
                               if cursor < len(result['raw_items']) else None)
    result['limits'] = 'Retained firmware node and row states; not proof that a menu is displayed or a setting committed.'
    return result


def setup_inventory(board, *, max_nodes=512):
    """Follow X-M1 root tabs C0/C1/C2, keyed by descriptor pointer."""
    if type(max_nodes) is not int or not 1 <= max_nodes <= 512:
        raise ValueError('Inventory node bound must be 1..512')
    root_pointer = _word(board, NODE_REGISTRY)
    if not root_pointer:
        return {'ready': False, 'reason': 'Firmware node registry is not loaded',
                'root_pointer': 0, 'tabs': [], 'nodes': [],
                'limits': 'Static descriptor inventory is not ready.'}
    root = descriptor(board, root_pointer)
    tabs = [item for item in root['raw_items'] if item['value'] in (0xc0, 0xc1, 0xc2)]
    if len(tabs) != 3 or any(not item['child_pointer'] for item in tabs):
        raise ValueError('X-M1 root does not contain the three SETUP page tabs')
    pending = list(dict.fromkeys(item['child_pointer'] for item in tabs))
    nodes = {}
    while pending:
        pointer = pending.pop()
        if pointer in nodes:
            continue
        if len(nodes) == max_nodes:
            raise ValueError('SETUP graph exceeds the declared node bound')
        node = descriptor(board, pointer)
        nodes[pointer] = node
        pending.extend(item['child_pointer'] for item in node['raw_items']
                       if item['child_pointer'] and item['child_pointer'] not in nodes)
    return {'ready': True, 'reason': None, 'root_pointer': root['pointer'],
            'tabs': tabs, 'nodes': list(nodes.values()),
            'limits': 'Potential static descriptor graph; not observed navigation, enabled rows or rendered coverage.'}


class MenuInputs:
    """One external input pulse at a time using loaded X-M1 calibration."""
    keys = KEY_NAMES
    aliases = dict(ALIASES)

    def __init__(self, board):
        self.board = board
        devices = [device for device in board.devices if type(device).__name__ == 'PowerDevices']
        if len(devices) != 1:
            raise ValueError('Menu input requires exactly one PowerDevices')
        self.power = devices[0]
        controls = [device for device in board.devices if type(device).__name__ == 'BootControls']
        if len(controls) != 1:
            raise ValueError('Menu input requires exactly one BootControls')
        self.controls = controls[0]
        self._pressed = None
        self._samples()

    def _samples(self):
        samples = self.power.adc_samples
        if (type(samples) is not tuple or len(samples) != 12 or
                any(type(value) is not int or not 0 <= value <= 0xffff for value in samples)):
            raise ValueError('ADC input requires the existing typed 12-sample tuple')
        return samples

    def intervals(self):
        base = _word(self.board, CALIBRATION_POINTER)
        if not base:
            raise ValueError('X-M1 calibration is not loaded')
        result = {}
        for binding in ADC_RECORDS:
            record = _raw(self.board, binding.address, 28)
            code = int.from_bytes(record[:4], 'little')
            channel = int.from_bytes(record[12:16], 'little')
            low_tag, high_tag = struct.unpack_from('<2H', record, 16)
            if (code != binding.code or record[9] != 1 or channel != binding.channel or
                    low_tag != binding.low_tag or high_tag != binding.high_tag):
                raise ValueError('Key record does not match X-M1 1.50: ' + binding.code.name)
            low = int.from_bytes(_raw(self.board, base + low_tag, 2), 'little')
            high = int.from_bytes(_raw(self.board, base + high_tag, 2), 'little')
            if not 0 <= low <= high <= RELEASED:
                raise ValueError('X-M1 key calibration is outside the 10-bit range')
            result[binding.code] = CalibratedRange(binding, low, high)
        return result

    def _gpio_record(self, binding):
        record = _raw(self.board, binding.address, 28)
        target = int.from_bytes(record[12:16], 'little')
        pressed = struct.unpack_from('<H', record, 0x12)[0]
        if (int.from_bytes(record[:4], 'little') != binding.code or record[9] != 2 or
                target != (binding.port << 16 | binding.pin) or pressed != binding.pressed_level):
            raise ValueError('GPIO key record does not match X-M1 1.50: ' + binding.code.name)

    def availability(self):
        if not _word(self.board, CALIBRATION_POINTER):
            return {'ready': False, 'reason': 'X-M1 calibration is not loaded'}
        if _word(self.board, SCANNER_REGISTERED) != 1:
            return {'ready': False, 'reason': 'X-M1 input scanner is not registered'}
        try:
            intervals = self.intervals()
            for interval in intervals.values():
                channel_ranges = [candidate for candidate in intervals.values()
                                  if candidate.binding.channel == interval.binding.channel]
                if any(candidate.contains(RELEASED) for candidate in channel_ranges):
                    raise ValueError('Release level asserts a calibrated key')
                if [candidate.binding.code for candidate in channel_ranges
                        if candidate.contains(interval.midpoint)] != [interval.binding.code]:
                    raise ValueError('Calibration midpoint asserts competing keys on one ADC channel')
            for binding in GPIO_KEY_RECORDS.values():
                self._gpio_record(binding)
        except ValueError as error:
            return {'ready': False, 'reason': 'X-M1 input calibration is not ready: ' + str(error)}
        return {'ready': True, 'reason': None}

    def _set(self, channel, sample, key):
        samples = list(self._samples())
        previous = samples[channel]
        samples[channel] = sample
        self.power.adc_samples = tuple(samples)
        self.board.record('Declared external menu ADC input', channel=int(channel),
                          previous=previous, sample=sample, key=None if key is None else key.value,
                          source='X-M1 1.50 loaded-calibration midpoint',
                          limits='Virtual external ADC level; physical voltage/timing unmeasured')
        return sample

    def _gpio(self, binding, pressed):
        self.controls.set_gpio_input_pin(binding.port, binding.pin,
                                         binding.pressed_level if pressed else 1 - binding.pressed_level,
                                         source='MenuInputs X-M1 1.50 ' + binding.code.name)

    def press(self, name):
        key = MenuKey.parse(name)
        if self._pressed is not None:
            raise ValueError('Release the selected input before pressing another key')
        availability = self.availability()
        if not availability['ready']:
            raise ValueError(availability['reason'])
        binding = GPIO_KEY_RECORDS.get(key) or KEY_RECORDS[key]
        self._pressed = (key, binding)
        if isinstance(binding, GpioBinding):
            self._gpio(binding, True)
            return None
        return self._set(binding.channel, self.intervals()[binding.code].midpoint, key)

    def release(self):
        if self._pressed is None:
            return None
        _, binding = self._pressed
        if isinstance(binding, GpioBinding):
            self._gpio(binding, False)
            result = None
        else:
            result = self._set(binding.channel, RELEASED, None)
        self._pressed = None
        return result

    def current_state(self, *, ready=True):
        samples = self._samples()
        active = []
        if ready:
            intervals = self.intervals()
            active.extend(key.value for key, binding in KEY_RECORDS.items()
                          if intervals[binding.code].contains(samples[binding.channel]))
            active.extend(key.value for key, binding in GPIO_KEY_RECORDS.items()
                          if self.controls.gpio_input_pin(binding.port, binding.pin) == binding.pressed_level)
        return {'adc_samples': {str(int(channel)): samples[channel] for channel in AdcChannel},
                'pressed': active[0] if len(active) == 1 else None, 'asserted_keys': active,
                'inputs_released': (self._pressed is None and ready and not active
                                    and all(samples[channel] == RELEASED for channel in AdcChannel)),
                'current_menu': current_menu(self.board)}
