"""Secondary-loader serial/ADC board boundaries with explicit reply fixtures.

The transaction framing is recovered from unchanged X70 loader instructions.
No early MCU replies or ADC samples have been captured, so defaults are
synthetic protocol fixtures. They do not establish real button/lens states.
Unknown requests, DMA jobs, and register accesses stop instead of succeeding.
"""
import hashlib

from .hardware import UnsupportedDevice
from .panel_serial import PanelSerial

ABSENT_EXTERNAL_FLASH_PROFILE = 'absent-external-flash-fixed-low-miso'


def absent_external_flash_profile():
    """Opt-in synthetic SPI0 electrical input; no flash MCU is emulated.

    Paired with the existing released KEY_FLASH_EXT GPIO input, each
    endpoint10 byte shift samples a fixed-low receive line. No camera SPI0
    wire capture establishes this level or the four-tick shift duration.
    """
    return {'spi0_input_profile': ABSENT_EXTERNAL_FLASH_PROFILE}


def nominal_power_profile():
    """Synthetic full-battery input selected from captured calibration.

    Actual 0x952d10/0x952b34 classify ADC5=640 as BATT_STS_FULL: the
    captured calibration's full range is560..695, while1016..1023 selects
    BATT_STS_HIGH. Earlier flash monitor0x9592d8 also takes its non-low
    branch at640 (both calibrated low thresholds492). The corresponding
    voltage has not been measured. Other analog/MCU inputs remain unresolved.
    """
    samples = [0]*12
    samples[5] = 640
    return {'profile_name': 'nominal-power / unresolved other inputs',
            'adc_samples': tuple(samples)}


def idle_controls_profile():
    """Named synthetic dial/key choices justified by captured calibration.

    Loader table 0x14abc and saved cfgdata thresholds identify ADC3=30..96
    as auto shutter, ADC4=508..561 as zero exposure compensation, and all
    ADC8 key ranges below 967. Predicates 0x93ec58/0x93ec84 identify ADC2
    below465/41 as remote shutter S1/S2; hold it high to release both.
    ADC7=805..845 selects AF while leaving the Function2 button released
    through 0x93efd8/0x93edf8 and captured thresholds0x37a6..0x37bc.
    ADC9=30..90 selects auto aperture (KEY_M_IRIS_A) in the ring table.
    No corresponding voltages were measured.
    """
    profile = nominal_power_profile()
    samples = list(profile['adc_samples'])
    samples[2], samples[3], samples[4], samples[7], samples[8], samples[9] = 0x3ff, 63, 534, 825, 0x3ff, 60
    return {'profile_name': 'idle controls / nominal power / unresolved other inputs',
            'adc_samples': tuple(samples)}


def normal_power_on_profile():
    """Paired with normal_power_on_gpio_profile, with a chosen MCU reset status.

    Status0 and the valid zero-body packet are retained explicit fixtures.
    Stock 0x94774c chooses mode3 with switch-on/released-playback inputs;
    0x93e724 can then proceed through its normal task-creation branch.
    """
    profile = idle_controls_profile()
    profile['boot_reply'] = 0
    profile['profile_name'] = 'virtual normal power-on / idle controls / unresolved MCU and other inputs'
    return profile


class PowerDevices:
    SPI0 = 0xfff84000
    SPI = 0xfff84040
    # Accesses that are fixed functions of `registers` and `spi_ready` (see
    # read/write), which the native engine may serve itself
    # (x70_native RustBoard._sync_plain_registers): SPI1 configuration words,
    # the +0xC status (ready bits 8/9, low-half write-one-to-clear) and the
    # +0x10 receive level.
    native_registers = {SPI: ((0, 0, None), 0xffffffff), SPI+4: ((0, 0, None), 0xffffffff),
                        SPI+8: ((0, 0, None), 0xffffffff), SPI+0x14: ((0, 0, None), 0xffffffff),
                        SPI+0xc: ((0x300, 0x300, 'spi_ready'), 0xffff0000),
                        SPI+0x10: ((0x1f, 1, 'spi_ready'), None)}
    ADC = 0xfff8a000
    UART = 0xfff46080
    DMA = 0xfff02080
    DMA_OFFSETS = {0, 4, 8, 0xc, 0x10, 0x14, 0x24, 0x28, 0x2c, 0x30, 0x34}
    # Both 0x13014 and 0x917034 accept channels 0..11. The secondary
    # loader samples only the first ten; application 0x93f2c4 reads all 12.
    ADC_OFFSETS = {0, 4, 8, 0xc, 0x24, 0x28} | set(range(0x30, 0x60, 4))
    # endpoint, configuration, select/other GPIO levels, read permitted.
    # None for the other level admits unknown/inactive but never asserted low.
    _spi_routes = ((0, 0x30004, (12, 3, 0, 11, 3, None), False),
                   (9, 6, (12, 3, 0, 11, 3, None), True))
    _panel_contract = ((0xe0, 6), (11, 3, 0, 12, 3, 1), (16, 15, 7), 1)
    _mcu_registers = (9, 0x60, 0x50)
    _transfer_ticks = 4
    _spi0_contract = ((0xc0, 0x30, 0), (11, 2, 0), (11, 2, 0), 10)

    def __init__(self, board, *, boot_reply=0, packet=None, adc_samples=None, usb_connected=False,
                 profile_name='zero inputs / valid protocol framing', spi0_input_profile=None,
                 firmware_profile=None, spi_endpoints=None, mcu_registers=None,
                 panel_contract=None, transfer_ticks=None, spi0_contract=None):
        if spi_endpoints is not None:
            if (type(spi_endpoints) is not tuple or not spi_endpoints
                    or any(type(row) is not tuple or len(row) != 5 for row in spi_endpoints)):
                raise ValueError('SPI endpoints require immutable route/register records')
            for endpoint, configuration, selection, readable, values in spi_endpoints:
                if (type(endpoint) is not int or not 0 <= endpoint <= 255
                        or type(configuration) is not int or not 0 <= configuration <= 0xffffffff
                        or type(selection) is not tuple or len(selection) != 6
                        or any(type(selection[n]) is not int or not 0 <= selection[n] < 18 for n in (0, 3))
                        or any(type(selection[n]) is not int or not 0 <= selection[n] < 16 for n in (1, 4))
                        or type(selection[2]) is not int or selection[2] not in (0, 1)
                        or (selection[5] is not None and (type(selection[5]) is not int or selection[5] not in (0, 1)))
                        or type(readable) is not bool
                        or selection[:2] == selection[3:5]
                        or type(values) is not tuple or not values
                        or any(type(pair) is not tuple or len(pair) != 2
                               or type(pair[0]) is not int or not 0 <= pair[0] <= 0x7f
                               or type(pair[1]) is not int or not 0 <= pair[1] <= 0xffff for pair in values)
                        or len({pair[0] for pair in values}) != len(values)):
                    raise ValueError('Invalid SPI endpoint route/register contract')
            if (len({row[0] for row in spi_endpoints}) != len(spi_endpoints)
                    or len({(row[1], row[2]) for row in spi_endpoints}) != len(spi_endpoints)):
                raise ValueError('SPI endpoint identities/routes must be unique')
            self._spi_routes = tuple(row[:4] for row in spi_endpoints)
            if mcu_registers is None:
                raise ValueError('Explicit SPI endpoints require explicit MCU packet register placement')
        if mcu_registers is not None:
            if (type(mcu_registers) is not tuple or len(mcu_registers) != 3
                    or any(type(v) is not int for v in mcu_registers)
                    or not (0 <= mcu_registers[0] <= 255 and 0 <= mcu_registers[1] <= 0x7f
                            and 0 <= mcu_registers[2] <= 0x70)
                    or mcu_registers[2] <= mcu_registers[1] < mcu_registers[2] + 16):
                raise ValueError('Invalid MCU packet register placement')
            self._mcu_registers = mcu_registers
        if isinstance(panel_contract, list):  # From a board profile's JSON power inputs.
            panel_contract = tuple(tuple(v) if isinstance(v, list) else v for v in panel_contract)
        if panel_contract is not None:
            if type(panel_contract) is not tuple or len(panel_contract) != 4:
                raise ValueError('Invalid panel contract')
            PanelSerial.parameters(*panel_contract[:3])
            if type(panel_contract[3]) is not int or not 0 <= panel_contract[3] <= 255:
                raise ValueError('Invalid panel endpoint identifier')
            self._panel_contract = panel_contract
        if transfer_ticks is not None:
            if type(transfer_ticks) is not int or not 1 <= transfer_ticks < 1 << 64:
                raise ValueError('SPI/ADC transfer delay must be positive and bounded')
            self._transfer_ticks = transfer_ticks
        if spi0_contract is not None:
            if (type(spi0_contract) is not tuple or len(spi0_contract) != 4
                    or type(spi0_contract[0]) is not tuple or len(spi0_contract[0]) != 3
                    or any(type(v) is not int or not 0 <= v <= 0xffffffff for v in spi0_contract[0])
                    or spi0_contract[0][0] & 1 or spi0_contract[0][2] != 0
                    or any(type(row) is not tuple or len(row) != 3
                           or any(type(v) is not int for v in row)
                           or not (0 <= row[0] < (18 if index == 1 else 16)
                                   and 0 <= row[1] < 16 and row[2] in (0, 1))
                           for index, row in enumerate(spi0_contract[1:3], 1))
                    or type(spi0_contract[3]) is not int or not 0 <= spi0_contract[3] <= 255):
                raise ValueError('Invalid SPI0 fixed-line transport contract')
            self._spi0_contract = spi0_contract
        self.board = board
        if spi0_input_profile not in (None, ABSENT_EXTERNAL_FLASH_PROFILE):
            raise ValueError('Unknown SPI0 electrical input profile')
        self.spi0_input_profile = spi0_input_profile
        self.spi0_busy = False
        self.spi0_rx = None
        self.spi0_generation = self.spi0_started = self.spi0_completed = self.spi0_consumed = 0
        self.panel_serial = None
        self.profile_name = str(profile_name)
        self.boot_reply = int(boot_reply)
        self.packet = bytes(packet) if packet is not None else bytes(14)+bytes((0xaa, 0x56))
        self.adc_samples = tuple(adc_samples) if adc_samples is not None else (0,)*12
        if (not 0 <= self.boot_reply <= 0xffff or len(self.packet) != 16 or
                self.packet[14] != 0xaa or sum(self.packet) % 256 or
                len(self.adc_samples) != 12 or
                any(not 0 <= int(v) <= 0xffff for v in self.adc_samples)):
            raise ValueError('Invalid secondary-loader board profile')
        if not isinstance(usb_connected, bool):
            raise ValueError('USB presence fixture must be boolean')
        self.usb_connected = usb_connected
        self.usb_control = None  # Optional explicit owner of writable PHY/core configuration.
        # Real 0x10a9688 tests bit31; 0x10aff44 normalizes/caches it for
        # USB-controller boot handling. Remaining status bits are unmeasured.
        self.registers = {0xfff55104: int(usb_connected) << 31}
        # Keep endpoints separate: the shared SPI1/GPIO12/3 path does not
        # prove a shared peripheral/reset bank. The recovered bus settings
        # distinguish endpoint0 calibrated writes from endpoint9 power/MCU.
        self.spi_endpoint_registers = {
            0: {x: 0 for x in range(0x30) if x not in (2, 0x11, 0x14, 0x15, 0x16)},
            9: {address: 0 for address in
                              (0x41, 0x42, 0x43, 0x44, 0x45, 0x46,
                               0x49, 0x4a, 0x4b, 0x4c, 0x4e,
                               *range(0x50, 0x60), 0x60, 0x68, 0x69, 0x6a)},
        }
        if spi_endpoints is not None:
            self.spi_endpoint_registers = {row[0]: dict(row[4]) for row in spi_endpoints}
        endpoint, reply_register, packet_start = self._mcu_registers
        if (endpoint not in self.spi_endpoint_registers
                or any(register not in self.spi_endpoint_registers[endpoint]
                       for register in (reply_register, *range(packet_start, packet_start + 16)))):
            raise ValueError('MCU packet registers must be admitted by the selected endpoint map')
        self.spi_endpoint_registers[endpoint][reply_register] = self.boot_reply
        for index, byte in enumerate(self.packet):
            self.spi_endpoint_registers[endpoint][packet_start+index] = byte
        self.spi_busy = self.spi_ready = self.adc_busy = self.dma_busy = False
        self.spi_reply = self.adc_done = 0
        self.spi_generation = self.adc_generation = self.dma_generation = 0
        from .boards.profile import resolve
        from .boards.firmware_blobs import device_blob
        # Legacy callers select X70 explicitly as their documented default.
        # A supplied sibling profile with no blob remains unmodeled.
        self.uart_boot_program = device_blob(resolve('x70-110') if firmware_profile is None
                                            else firmware_profile, 'uart_boot_program')
        self.transmitted = []
        self.board.record('Secondary board reply fixtures', boot_reply=hex(self.boot_reply),
                          packet_hex=self.packet.hex(), adc_samples=list(self.adc_samples),
                          usb_connected=usb_connected,
                          profile_name=self.profile_name,
                          provenance='synthetic protocol fixtures; no early hardware replies captured')
        if self.spi0_input_profile is not None:
            self.board.record('SPI0 electrical input fixture', profile=self.spi0_input_profile,
                              miso_level=0, shift_ticks=self._transfer_ticks,
                              provenance='explicit synthetic absent-flash input; no physical SPI0 capture',
                              limits='endpoint10 byte transport only; external device effects and timing unmeasured')

    def _kind(self, address, size):
        if size != 4 or address & 3:
            return None
        if address-self.SPI0 in range(0, 0x20, 4):
            return 'spi0'
        if address-self.SPI in range(0, 0x20, 4):
            return 'spi'
        if address-self.ADC in self.ADC_OFFSETS:
            return 'adc'
        if address-self.UART in (0, 4, 0xc):
            return 'uart'
        if address-self.DMA in self.DMA_OFFSETS:
            return 'dma'
        if address in (0xfff98500, 0xfff98504, 0xfff55104):
            return 'routing' if address != 0xfff55104 else 'status'
        return None

    def contains(self, address, size):
        if address==0xfff55104 and self.usb_control is not None:
            return False
        return self._kind(address, size) is not None

    def mmio_ranges(self):
        """Superset of the start addresses contains() accepts (RustBoard._mmio_device index)."""
        return ((self.SPI0, self.SPI0+0x20), (self.SPI, self.SPI+0x20),
                (self.ADC, self.ADC+max(self.ADC_OFFSETS)+1), (self.UART, self.UART+0xd),
                (self.DMA, self.DMA+max(self.DMA_OFFSETS)+1),
                (0xfff98500, 0xfff98505), (0xfff55104, 0xfff55105))

    def read(self, core, address, size):
        kind = self._kind(address, size)
        if kind is None:
            raise UnsupportedDevice(f'Unknown secondary boot read {address:#x}/{size}')
        value = self.registers.get(address, 0)
        if kind == 'spi0':
            offset = address-self.SPI0
            if offset == 0:
                value = (value & ~1) | int(self.spi0_busy)
            elif offset == 0x18:
                if self.spi0_rx is None:
                    raise UnsupportedDevice('SPI0 receive data has no completed modeled exchange')
                value = self.spi0_rx
                self.spi0_rx = None
                self.spi0_consumed += 1
                self.board.record('SPI0 receive consumed', cpu=core, value=hex(value),
                                  profile=self.spi0_input_profile)
            if offset in (0x10, 0x14):
                # 0x918a70 reports these low-five-bit fields as FIFO levels.
                # One bounded shift consumes TX immediately; completed RX
                # remains queued until 0x918a08 reads the data port.
                value = (value & ~0x1f) | int(offset == 0x10 and self.spi0_rx is not None)
        elif kind == 'spi':
            offset = address-self.SPI
            if offset == 0xc:
                # App 0x918a70 checks transmit condition bit8 as well as
                # receive condition bit9. Both follow completed exchange.
                value = (value & ~0x300) | (0x300 if self.spi_ready else 0)
            elif offset == 0x10:
                value = (value & ~0x1f) | int(self.spi_ready)
            elif offset == 0x18:
                if not self.spi_ready:
                    raise UnsupportedDevice('SPI response read before completion')
                value = self.spi_reply
                self.spi_ready = False
        elif kind == 'adc':
            offset = address-self.ADC
            if offset == 0x24:
                value = self.adc_done
            elif offset >= 0x30:
                value = self.adc_samples[(offset-0x30)//4]
        elif kind == 'uart' and address == self.UART+4:
            raise UnsupportedDevice('Boot serial receiver is not modeled')
        return int(value) & 0xffffffff

    def write(self, core, address, size, value):
        kind = self._kind(address, size)
        if kind is None:
            raise UnsupportedDevice(f'Unknown secondary boot write {address:#x}/{size}')
        value &= 0xffffffff
        if kind == 'spi0':
            offset = address-self.SPI0
            if offset == 0x18:
                raise UnsupportedDevice('SPI0 receive data is read-only')
            if offset == 0x1c:
                self._spi0_start(core, value)
                return
            if offset == 0xc:
                old = self.registers.get(address, 0)
                self.registers[address] = (value & 0xffff0000) | (old & 0xffff & ~value)
            else:
                self.registers[address] = value
            if offset == 8 and value & 0x10000000:
                self.spi0_generation += 1
                self.spi0_busy, self.spi0_rx = False, None
                self.registers[self.SPI0+0xc] = self.registers.get(self.SPI0+0xc, 0) & ~0x300
                self.board.record('SPI0 FIFO reset with empty queues', cpu=core,
                                  provenance='stock 0x917b4c configuration; cancels pending modeled shift')
            return
        if kind == 'status':
            raise UnsupportedDevice('Secondary boot input status is read-only')
        if kind == 'spi' and address == self.SPI+0x18:
            raise UnsupportedDevice('SPI receive data is read-only')
        if kind == 'spi' and address == self.SPI+0xc:
            # 0x917b4c acknowledges flags by OR-ing each low bit into this
            # word; 0x918584 writes 0x3311 before every command. Those
            # writes clear low status flags rather than setting errors.
            old = self.registers.get(address, 0)
            self.registers[address] = (value & 0xffff0000) | (old & 0xffff & ~value)
            return
        if kind == 'adc' and address >= self.ADC+0x30:
            raise UnsupportedDevice('ADC samples are read-only')
        if kind == 'adc' and address == self.ADC+0x24:
            self.adc_done &= ~value
            return
        if kind == 'uart' and address == self.UART+0xc:
            self.registers[address] = self.registers.get(address, 0) & ~value
            return
        self.registers[address] = value
        if kind == 'spi' and address == self.SPI+0x1c:
            self._spi_start(core, value)
        elif kind == 'adc' and address == self.ADC:
            if value not in (0x8000, 0x8004, 0x8005):
                raise UnsupportedDevice(f'Unsupported boot ADC control {value:#x}')
            if value == 0x8000:
                self.adc_generation += 1
                self.adc_busy, self.adc_done = False, 0
            elif value == 0x8005:
                self._adc_start(core)
        elif kind == 'dma' and address == self.DMA+0x28:
            if value == 8:
                self.dma_generation += 1
                self.dma_busy = False
                self.registers[self.DMA+0x24] = 0
            elif value == 1:
                self._dma_start(core)
            else:
                raise UnsupportedDevice(f'Unsupported boot peripheral DMA control {value:#x}')
        elif kind == 'uart' and address == self.UART+4:
            raise UnsupportedDevice('Boot serial direct writes are not modeled')

    def _spi0_start(self, core, value):
        if self.spi0_input_profile is None:
            raise UnsupportedDevice(f'Unmodeled SPI0 exchange {value:#x}; endpoint protocol unresolved')
        if self.spi0_busy or self.spi0_rx is not None:
            raise UnsupportedDevice('SPI0 shift while busy or receive FIFO unread')
        # Table AEA6E8 endpoint10 and complete stock 917B4C configuration.
        # Reject different widths, IRQ/DMA configurations and endpoint modes.
        config = tuple(self.registers.get(self.SPI0+offset, 0) for offset in (0, 4, 8))
        required_config, select, absent, endpoint_id = self._spi0_contract
        if (config != required_config or self.registers.get(self.SPI0+0xc, 0) & 0xffff0000
                or not 0 <= value <= 0xff):
            raise UnsupportedDevice(f'Unsupported SPI0 byte transport configuration {config!r}, TX={value:#x}')
        pins = next((d for d in getattr(self.board, 'devices', ())
                     if hasattr(d, 'gpio_output_pin') and hasattr(d, 'gpio_inputs')), None)
        if pins is None or pins.gpio_output_pin(*select[:2]) != select[2]:
            raise UnsupportedDevice('SPI0 endpoint requires its explicit active GPIO chip select')
        if (pins.gpio_inputs.get(absent[0], 0) >> absent[1]) & 1 != absent[2]:
            raise UnsupportedDevice('Absent-flash SPI0 input contradicts its declared presence GPIO level')
        self.registers[self.SPI0+0x1c] = value
        self.spi0_generation += 1
        generation = self.spi0_generation
        self.spi0_busy = True
        self.spi0_started += 1
        self.board.record('SPI0 byte shift start', cpu=core, endpoint=endpoint_id, transmitted=hex(value),
                          profile=self.spi0_input_profile, shift_ticks=self._transfer_ticks,
                          selection={'port': select[0], 'pin': select[1], 'level': select[2]},
                          provenance='stock transport with explicitly fixed-low synthetic MISO')

        def complete():
            if generation != self.spi0_generation:
                return
            self.spi0_rx, self.spi0_busy = 0, False
            self.spi0_completed += 1
            # 918A70 reports bits8/9; 944568 waits for bit9 and RX level.
            self.registers[self.SPI0+0xc] = self.registers.get(self.SPI0+0xc, 0) | 0x300
            self.board.record('SPI0 byte shift complete', cpu=core, endpoint=endpoint_id,
                              transmitted=hex(value), received='0x0',
                              profile=self.spi0_input_profile,
                              provenance='fixed-low synthetic input; external flash effects unmodeled')
        self.board.schedule(self._transfer_ticks, complete)

    def _spi_endpoint(self):
        # Table0xaea6e8 has 36-byte records. Stock 0x917b4c programs
        # +4=0x30004 for endpoint0 and +4=6 for endpoint9. Loader0x129f8
        # programs the latter. Endpoint1 shares value6 but selects GPIO11/3;
        # reject that path rather than interpreting its words as MCU data.
        configuration = self.registers.get(self.SPI+4, 0)
        routes = [row for row in self._spi_routes if row[1] == configuration]
        if not routes:
            raise UnsupportedDevice(f'Unknown SPI1 endpoint configuration {configuration:#x}')
        pins = next((d for d in getattr(self.board, 'devices', ())
                     if hasattr(d, 'gpio_output_pin')), None)
        if pins is None:
            raise UnsupportedDevice('SPI1 endpoint requires explicit GPIO selection state')
        matches = []
        for row in routes:
            _, _, (port, pin, level, other_port, other_pin, other_level), _ = row
            select, other = pins.gpio_output_pin(port, pin), pins.gpio_output_pin(other_port, other_pin)
            if select == level and (other != 0 if other_level is None else other == other_level):
                matches.append((row, select, other))
        if len(matches) != 1:
            raise UnsupportedDevice('SPI1 selection requires exactly one admitted GPIO/configuration route')
        (endpoint, _, (port, pin, _, other_port, other_pin, _), _), select, other = matches[0]
        return endpoint, {
            'configuration_address': hex(self.SPI+4), 'configuration': hex(configuration),
            'select_port': port, 'select_pin': pin, 'select_level': select,
            'other_select_port': other_port, 'other_select_pin': other_pin, 'other_select_level': other,
            'provenance': ('explicit unmeasured endpoint route/register contract' if '_spi_routes' in self.__dict__
                           else '0xaea6e8 endpoint table, 0x917b4c configuration and 0x9182f4 masked CS write; endpoint identity inferred from bus configuration'),
        }

    def _spi_start(self, core, command):
        if self.spi_busy or not self.registers.get(self.SPI, 0) & 0x80:
            raise UnsupportedDevice('SPI start while busy or disabled')
        if not 0 <= command <= 0xffff:
            raise UnsupportedDevice(f'Unknown auxiliary MCU SPI request {command:#x}')
        pins = next((d for d in getattr(self.board, 'devices', ())
                     if hasattr(d, 'gpio_output_pin')), None)
        panel_transport, panel_selection, panel_framing, panel_id = self._panel_contract
        port, pin, level, other_port, other_pin, other_level = panel_selection
        if (pins is not None and pins.gpio_output_pin(port, pin) == level
                and pins.gpio_output_pin(other_port, other_pin) == other_level):
            selection = {'configuration_address': hex(self.SPI+4),
                         'configuration': hex(self.registers.get(self.SPI+4, 0)),
                         'select_port': port, 'select_pin': pin, 'select_level': level,
                         'other_select_port': other_port, 'other_select_pin': other_pin, 'other_select_level': other_level,
                         'provenance': ('explicit unmeasured panel route' if '_panel_contract' in self.__dict__
                                        else '0xaea6e8 endpoint1 table and real masked CS state')}
            control, configuration = self.registers.get(self.SPI, 0), self.registers.get(self.SPI+4, 0)
            if self.registers.get(self.SPI+8, 0):
                raise UnsupportedDevice('Rear-panel SPI1 IRQ/DMA/configuration fields are unresolved')
            PanelSerial.validate(command, control=control, configuration=configuration, selection=selection,
                                 transport=panel_transport, selection_contract=panel_selection, framing=panel_framing)
            if getattr(self, 'panel_serial', None) is None:
                self.panel_serial = (PanelSerial(self.board, transport=panel_transport, selection=panel_selection,
                                    framing=panel_framing, endpoint_id=panel_id)
                                     if '_panel_contract' in self.__dict__ else PanelSerial(self.board))
            self.spi_generation += 1
            generation = self.spi_generation
            self.spi_busy, self.spi_ready = True, False
            self.board.record('Rear LCD serial shift start', cpu=core, command=hex(command),
                              endpoint=panel_id, selection=selection, shift_ticks=self._transfer_ticks,
                              limits='Byte transport only; no panel effects or physical timing modeled')

            def complete_panel():
                if generation == self.spi_generation:
                    self.spi_reply = self.panel_serial.receive(core, command, control=control,
                                                               configuration=configuration,
                                                               selection=selection)
                    self.spi_busy, self.spi_ready = False, True
            self.board.schedule(self._transfer_ticks, complete_panel)
            return
        endpoint, selection = self._spi_endpoint()
        registers = self.spi_endpoint_registers[endpoint]
        address, data, writing = command >> 8 & 0x7f, command & 0xff, bool(command & 0x8000)
        readable = next(row[3] for row in self._spi_routes if row[0] == endpoint)
        if address not in registers or (not writing and (data or not readable)):
            raise UnsupportedDevice(f'Unknown SPI endpoint{endpoint} request {command:#x}')
        # Write reply contents are unmeasured. Supply explicit zero ack,
        # retaining the byte for subsequent reads and masked updates.
        reply = 0 if writing else registers[address]
        self.spi_generation += 1
        generation = self.spi_generation
        self.spi_busy, self.spi_ready = True, False
        self.board.record('Boot SPI request', cpu=core, command=hex(command),
                          endpoint=endpoint, selection=selection,
                          register=hex(address), operation='write' if writing else 'read',
                          data=hex(data) if writing else None,
                          reply_provenance='explicit synthetic profile')
        def complete():
            if generation == self.spi_generation:
                if writing:
                    registers[address] = data
                self.spi_reply = reply
                self.spi_busy, self.spi_ready = False, True
                self.board.record('Boot SPI reply', cpu=core, endpoint=endpoint,
                                  command=hex(command), reply=hex(reply),
                                  reply_provenance='explicit synthetic profile; endpoint-specific state')
        self.board.schedule(self._transfer_ticks, complete)

    def set_adc_samples(self, samples, *, source):
        samples = tuple(samples)
        if (len(samples) != 12 or any(type(value) is not int or not 0 <= value <= 0xffff for value in samples)
                or not isinstance(source, str) or not source):
            raise ValueError('External ADC input requires12 bounded samples and its source')
        self.adc_samples = samples
        self.board.record('External ADC input', samples=list(samples), source=source,
                          basis='explicit scenario input; no firmware RAM write')

    def set_mcu_status(self, value, *, source):
        if type(value) is not int or not 0 <= value <= 255 or not isinstance(source, str) or not source:
            raise ValueError('External MCU status requires a byte and its source')
        endpoint, register, _ = self._mcu_registers
        self.spi_endpoint_registers[endpoint][register] = value
        self.board.record('External MCU status input', endpoint=endpoint, register=hex(register),
                          value=value, source=source, basis='explicit peripheral input; loader saved RAM remains untouched')

    def _adc_start(self, core):
        if self.adc_busy:
            raise UnsupportedDevice('Boot ADC start while busy')
        self.adc_generation += 1
        generation = self.adc_generation
        self.adc_busy, self.adc_done = True, 0
        channels = self.registers.get(self.ADC+8, 0) & 0xfff
        self.board.record('Boot ADC sample start', cpu=core, channels=hex(channels))
        def complete():
            if generation == self.adc_generation:
                self.adc_busy, self.adc_done = False, channels
                self.board.record('Boot ADC sample complete', cpu=core,
                                  provenance='unmeasured sample fixtures')
        self.board.schedule(self._transfer_ticks, complete)

    def _dma_start(self, core):
        if self.dma_busy:
            raise UnsupportedDevice('Boot peripheral DMA start while busy')
        config = self.registers.get(self.DMA+0x2c, 0)
        source = self.registers.get(self.DMA, 0)
        destination = self.registers.get(self.DMA+4, 0)
        count = self.registers.get(self.DMA+8, 0)
        route = (self.registers.get(0xfff98500, 0) >> 16) & 0x1f
        if (not self.uart_boot_program or config != 0x0022242a or destination != self.UART+4
                or count != len(self.uart_boot_program) or route != 0x12
                or any(self.registers.get(self.DMA+o, 0) for o in (0xc, 0x10, 0x14, 0x30, 0x34))):
            raise UnsupportedDevice(f'Unknown boot peripheral DMA job: config={config:#x}, '
                                    f'dest={destination:#x}, count={count:#x}, route={route:#x}')
        payload = self.board.read_memory(source, count, cpu_id=core)
        if payload != self.uart_boot_program:
            raise UnsupportedDevice('Unknown boot serial program payload')
        self.dma_generation += 1
        generation = self.dma_generation
        self.dma_busy = True
        self.registers[self.UART+0xc] = 0
        self.board.record('Boot serial DMA start', cpu=core, source=hex(source), bytes=count,
                          payload_sha256=hashlib.sha256(payload).hexdigest())
        def complete():
            if generation == self.dma_generation:
                self.transmitted.append(payload.hex())
                self.dma_busy = False
                self.registers[self.UART+0xc] = 1
                self.registers[self.DMA+0x24] = 0x40
                self.board.record('Boot serial DMA complete', cpu=core, bytes=count,
                                  limits='transmission accepted; external recipient effects unmodeled')
        self.board.schedule(max(4, count), complete)

    def snapshot(self):
        state = {'name': 'PowerDevices', 'profile_name': self.profile_name,
                'usb_connected': self.usb_connected,
                'boot_reply': hex(self.boot_reply),
                'packet_hex': self.packet.hex(), 'adc_samples': list(self.adc_samples),
                'spi_endpoint_registers': {endpoint: {hex(k): hex(v) for k, v in sorted(values.items())}
                                           for endpoint, values in self.spi_endpoint_registers.items()},
                'spi_busy': self.spi_busy, 'adc_busy': self.adc_busy, 'dma_busy': self.dma_busy,
                'panel_serial': (getattr(self, 'panel_serial', None).snapshot()
                                 if getattr(self, 'panel_serial', None) is not None else None),
                'spi0': {'base': hex(self.SPI0), 'input_profile': self.spi0_input_profile,
                         'busy': self.spi0_busy, 'receive_word': self.spi0_rx,
                         'fifo_levels': [int(self.spi0_rx is not None), 0],
                         'accepted_exchanges': self.spi0_started,
                         'completed_exchanges': self.spi0_completed,
                         'consumed_exchanges': self.spi0_consumed,
                         'contract': 'aligned configuration retention and low-status W1C; default unresolved; opt-in fixed-low byte input restricted to endpoint10'},
                'transmitted_programs': self.transmitted,
                'registers': {hex(k): hex(v) for k, v in sorted(self.registers.items())},
                'limits': ['SPI replies and ADC samples are synthetic, not captured hardware values.',
                           'Normal buttons, lens, power state and electrical timing remain unvalidated.',
                           'Endpoint9 MCU byte registers, endpoint0 listed calibrated writes, endpoint1 panel configuration words, and the stock loader serial program are modeled.',
                           'Endpoint1 records completed 9-bit LCD command/data framing with unused idle RX0; no panel register effects or pixels are modeled.',
                           'SPI0 transfers stop by default; the named absent-flash profile samples synthetic fixed-low MISO after four virtual ticks only for selected endpoint10 byte transport.',
                           'SPI0 fixed-low input is not a camera capture or flash MCU emulation; GPIO electrical multiplexing, physical timing and external effects are unproved.',
                           'Endpoint identity is inferred from bus configuration and firmware-written chip select; same physical chip/shared reset semantics are unproved.',
                           'MCU register initial values and write replies are synthetic; external register effects are unmodeled.',
                           'USB status FFF55104 bit31 supplies a synthetic presence input; other bits are zero fixtures and no USB protocol is emulated.',
                           'Serial transmit completion does not model its external recipient.']}
        if any(name in self.__dict__ for name in ('_spi_routes', '_mcu_registers', '_panel_contract', '_transfer_ticks', '_spi0_contract')):
            state['configuration'] = {'spi_routes': self._spi_routes, 'mcu_registers': self._mcu_registers,
                                      'panel_contract': self._panel_contract, 'transfer_ticks': self._transfer_ticks,
                                      'spi0_contract': self._spi0_contract,
                                      'uart_program_sha256': hashlib.sha256(self.uart_boot_program).hexdigest()
                                          if self.uart_boot_program else None}
            state['limits'][2:5] = ['Explicit endpoint register, GPIO, packet placement and panel framing inputs are unmeasured.',
                                    'Panel shifts record raw words only, with unused RX0 and no register or pixel effects.',
                                    'UART accepts only the selected firmware blob; an absent binding remains unmodeled.']
        return state
