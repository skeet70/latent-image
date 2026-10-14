"""Rear LCD configuration-byte sink, not a panel pixel source.

The default profile uses command/data bit15 and byte bits14..7. X-M1 1.50
uses whole sixteen-bit words. No panel serial capture or controller datasheet
establishes register effects or receive data. PowerDevices owns bus
latency/FIFO/status; this helper records completed configuration words.
"""
from .hardware import UnsupportedDevice


RAW16 = (16, None, None)  # Whole sixteen-bit words, no command/data or byte fields.


class PanelSerial:
    ENDPOINT = 1
    HISTORY_LIMIT = 512
    _transport = (0xe0, 6)
    _selection = (11, 3, 0, 12, 3, 1)
    _framing = (16, 15, 7)

    def __init__(self, board, *, transport=None, selection=None, framing=None, endpoint_id=None):
        transport, selection, framing = self.parameters(transport, selection, framing)
        endpoint_id = self.ENDPOINT if endpoint_id is None else endpoint_id
        if type(endpoint_id) is not int or not 0 <= endpoint_id <= 255:
            raise ValueError('Invalid panel endpoint identifier')
        self._transport, self._selection, self._framing = transport, selection, framing
        self.ENDPOINT = endpoint_id
        self.board = board
        self.words_received = 0
        self.words = []

    @staticmethod
    def parameters(transport=None, selection=None, framing=None):
        transport = PanelSerial._transport if transport is None else transport
        selection = PanelSerial._selection if selection is None else selection
        framing = PanelSerial._framing if framing is None else framing
        if (type(transport) is not tuple or len(transport) != 2
                or any(type(v) is not int or not 0 <= v <= 0xffffffff for v in transport)
                or transport[0] & 1):
            raise ValueError('Panel transport requires control/configuration words without a busy bit')
        if (type(selection) is not tuple or len(selection) != 6
                or any(type(v) is not int for v in selection)
                or any(not (0 <= selection[n] < 18 and 0 <= selection[n+1] < 16
                            and selection[n+2] in (0, 1)) for n in (0, 3))
                or selection[:2] == selection[3:5]):
            raise ValueError('Panel selection requires two distinct GPIO pins and explicit levels')
        if framing == RAW16:
            return transport, selection, framing
        if (type(framing) is not tuple or len(framing) != 3
                or any(type(v) is not int for v in framing)
                or framing[0] != 16 or not 0 <= framing[2] <= 7
                or framing[1] != framing[2] + 8):
            raise ValueError('Panel framing requires a sixteen-bit word with adjacent command/data and byte fields')
        return transport, selection, framing

    @staticmethod
    def validate(word, *, control, configuration, selection,
                 transport=None, selection_contract=None, framing=None):
        transport, required_selection, framing = PanelSerial.parameters(transport, selection_contract, framing)
        # Current stock open/select contract: SPI1 channel1, endpoint1. Bit0
        # is the shifter busy flag, excluded from retained configuration.
        if (control & ~1, configuration) != transport:
            raise UnsupportedDevice('Unsupported rear-panel SPI1 transport configuration')
        if (selection.get('select_port'), selection.get('select_pin'), selection.get('select_level'),
                selection.get('other_select_port'), selection.get('other_select_pin'),
                selection.get('other_select_level')) != required_selection:
            raise UnsupportedDevice('Rear-panel SPI1 requires its declared GPIO chip-select levels')
        if framing == RAW16:
            # X-M1 1.50 sends whole sixteen-bit words (first: 0x401); no field split is known.
            if not isinstance(word, int) or not 0 <= word < 1 << 16:
                raise UnsupportedDevice('Rear-panel SPI1 requires one sixteen-bit word')
            return {'word16': word}
        word_bits, command_shift, byte_shift = framing
        mask = 0x1ff << byte_shift
        if not isinstance(word, int) or not 0 <= word < 1 << word_bits or word & ~mask:
            raise UnsupportedDevice('Rear-panel SPI1 requires one left-justified 9-bit command/data word')
        return {'command_data': (word >> command_shift) & 1, 'byte': (word >> byte_shift) & 0xff}

    def receive(self, core, word, *, control, configuration, selection):
        """Record a completed write-only shift; return explicit unused RX0.

        Adapters should call validate before scheduling and receive only on
        completed transfer. Command/data is retained as a raw bit: no panel
        command semantics, register readback or pixel update is inferred.
        """
        decoded = self.validate(word, control=control, configuration=configuration,
                                selection=selection, transport=self._transport,
                                selection_contract=self._selection, framing=self._framing)
        item = {'cpu': core, 'word': hex(word), **decoded}
        self.words_received += 1
        self.words.append(item)
        if len(self.words) > self.HISTORY_LIMIT:
            del self.words[:-self.HISTORY_LIMIT]
        self.board.record('Rear LCD serial word received', endpoint=self.ENDPOINT,
                          **item, receive_fixture='unused virtual idle RX0',
                          provenance='declared panel framing; no panel effects or pixel source modeled')
        return 0

    def snapshot(self):
        state = {'endpoint': self.ENDPOINT, 'words_received': self.words_received,
                'retained_words': list(self.words), 'history_limit': self.HISTORY_LIMIT,
                'receive_data': 'unused virtual idle RX0',
                'contract': {'transport': list(self._transport), 'selection': list(self._selection),
                             'framing': list(self._framing)},
                'limits': ['Only completed transmitted configuration words are recorded',
                           'RX0 is an explicit unused controller input',
                           'Panel register effects, electrical latency, display power and pixels are not modeled']}
        return state
