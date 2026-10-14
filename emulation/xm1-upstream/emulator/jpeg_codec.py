"""Explicit row-major YYCbCr baseline JPEG surrogate, with no device effects.

The software thumbnail path writes two luminance samples and shared Cb/Cr
per four-byte group. This profile declares 4:2:2 sampling and standard Huffman
coding; it does not identify the full-resolution engine's source layout or
translate unknown sampling/restart MMIO fields. Actual uploaded quantization
bytes are retained exactly. Nothing here writes RAM or asserts an interrupt.
"""
from __future__ import annotations

import io

from .hardware import UnsupportedDevice


PACKED_422_PROFILE = 'declared-row-major-yycbcr-baseline-jpeg-unmeasured'
MAX_PIXELS = 32_000_000
MAX_BYTES = 128 * 1024 * 1024
ZIGZAG = (
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5,
    12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28,
    35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51,
    58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
)


class JPEGCapacityExceeded(UnsupportedDevice):
    def __init__(self, encoded_bytes, capacity):
        self.encoded_bytes, self.capacity = encoded_bytes, capacity
        super().__init__(f'Declared JPEG needs {encoded_bytes} bytes; capacity is {capacity}')


def natural_tables(qtable_fifo: bytes):
    """Convert the actual two zigzag-byte FIFO tables to coefficient order."""
    if type(qtable_fifo) is not bytes or len(qtable_fifo) != 128 or 0 in qtable_fifo:
        raise UnsupportedDevice('Declared JPEG requires two complete nonzero 8-bit quantization tables')
    result = []
    for table in (qtable_fifo[:64], qtable_fifo[64:]):
        values = [0] * 64
        for index, coordinate in enumerate(ZIGZAG): values[coordinate] = table[index]
        result.append(values)
    return result


def encode_packed_yycbcr(source: bytes, width: int, height: int, *,
                       stride_bytes: int, qtable_fifo: bytes,
                       output_capacity: int, profile: str | None = None,
                       restart_interval: int = 0) -> bytes:
    """Return a complete SOI..EOI JPEG for one explicitly declared source.

    The caller owns source/destination addresses and future hardware timing.
    Capacity failure returns no partial bytes. The restart interval is a
    declared MCU count, not an inferred interpretation of register62080.
    """
    if profile != PACKED_422_PROFILE or type(profile) is not str:
        raise UnsupportedDevice('Row-major JPEG codec profile is not selected')
    if (type(width) is not int or type(height) is not int
            or not 2 <= width <= 65535 or width % 2 or not 1 <= height <= 65535
            or width * height > MAX_PIXELS or type(stride_bytes) is not int
            or stride_bytes < width * 2 or stride_bytes * height > MAX_BYTES
            or type(source) is not bytes or len(source) != stride_bytes * height
            or type(output_capacity) is not int or not 1 <= output_capacity <= MAX_BYTES
            or type(restart_interval) is not int or not 0 <= restart_interval <= 65535):
        raise UnsupportedDevice('Declared JPEG source geometry/capacity/restart escapes its bounded row-major class')
    tables = natural_tables(qtable_fifo)
    try:
        import numpy as np
        from PIL import Image, features
    except ImportError as error:
        raise UnsupportedDevice('Declared JPEG requires NumPy and Pillow with libjpeg') from error
    if not features.check('jpg'):
        raise UnsupportedDevice('Declared JPEG requires a libjpeg-enabled Pillow build')
    pairs = np.frombuffer(source, dtype=np.uint8).reshape(height, stride_bytes)
    pairs = pairs[:, :width * 2].reshape(height, width // 2, 4)
    pixels = np.empty((height, width, 3), dtype=np.uint8)
    pixels[:, :, 0] = pairs[:, :, :2].reshape(height, width)
    pixels[:, 0::2, 1:] = pairs[:, :, 2:]
    pixels[:, 1::2, 1:] = pairs[:, :, 2:]
    output = io.BytesIO()
    Image.frombytes('YCbCr', (width, height), pixels.tobytes()).save(output,
        format='JPEG', qtables=tables, subsampling=1, optimize=False,
        progressive=False, restart_marker_blocks=restart_interval)
    payload = output.getvalue()
    if not payload.startswith(b'\xff\xd8') or not payload.endswith(b'\xff\xd9'):
        raise UnsupportedDevice('Declared JPEG backend did not return a complete baseline stream')
    if len(payload) > output_capacity:
        raise JPEGCapacityExceeded(len(payload), output_capacity)
    return payload
