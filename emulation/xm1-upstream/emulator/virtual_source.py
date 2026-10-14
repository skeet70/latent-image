"""Declared virtual-scene identities from peripheral input events, never RAM.

This projects already asserted external edges. It does not create an edge,
change a camera counter, invoke firmware or infer physical sensor timing.
"""
from typing import NewType

from .hardware import UnsupportedDevice


FrameIndex = NewType('FrameIndex', int)
VirtualTick = NewType('VirtualTick', int)


def xm1_source_tick(board) -> VirtualTick | None:
    from .input_processing_devices import InputProcessingControl, XM1_CLOCK_PROFILE, PhaseSource
    inputs = [device for device in board.devices if type(device) is InputProcessingControl]
    if len(inputs) != 1 or inputs[0].event_profile != XM1_CLOCK_PROFILE:
        raise UnsupportedDevice('X-M1 scene source requires its declared physical phase clock')
    edge = next((event for event in reversed(inputs[0].event_history)
                 if event['source'] == PhaseSource.PRIMARY), None)
    if edge is None:
        return None
    tick = edge['asserted_tick']
    if type(tick) is not int or not 0 <= tick <= board.virtual_ticks:
        raise UnsupportedDevice('X-M1 scene source edge has an invalid physical clock identity')
    return VirtualTick(tick)


def source_epochs(board):
    inputs = [d for d in board.devices if type(d).__name__ == 'InputProcessingControl']
    if len(inputs) != 1:
        raise UnsupportedDevice('Virtual source requires one input-processing controller')
    epochs = []
    for event in inputs[0].event_history:
        if event.get('state') not in ('asserted', 'acknowledged'):
            continue
        if event.get('generation') != 3 or event.get('source') != 0:
            continue
        ordinal = event.get('ordinal')
        if type(ordinal) is not int or not 2 <= ordinal <= 4:
            continue
        if (type(event.get('cpu')) is not int or event['cpu'] != 0
                or event.get('physical_irq') != '0x27'
                or type(event.get('attempt')) is not int or event['attempt'] < 1
                or type(event.get('asserted_tick')) is not int or event['asserted_tick'] < 0
                or not isinstance(event.get('profile'), str)):
            raise UnsupportedDevice('Virtual source edge has an invalid typed identity')
        epochs.append({name:event[name] for name in ('profile','generation','ordinal',
            'attempt','cpu','source','physical_irq','asserted_tick')} | {'frame':ordinal-1})
    return epochs


def epoch_key(epoch):
    return tuple(epoch[name] for name in ('profile','generation','ordinal','attempt','asserted_tick'))


def ram_span(address, size):
    """Validate a DRAM bus span; this does not assert software pool ownership."""
    if type(address) is not int or type(size) is not int or size < 1:
        raise UnsupportedDevice('Virtual DMA requires a positive typed DRAM span')
    physical = address & 0x3fffffff if 0x40000000 <= address < 0x60000000 else address
    if not 0x4000 <= physical < 0x20000000 or physical+size > 0x20000000:
        raise UnsupportedDevice('Virtual DMA span is outside DRAM or overlaps boot ROM')
    return physical, physical+size
