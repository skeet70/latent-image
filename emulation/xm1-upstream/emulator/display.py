"""Decode modeled X70 display memory and encode a portable PNG.

The diagnostic routine at 0x010e45d4 writes two luma samples in the low
halfword and two chroma bytes in the high halfword of each little-endian
word. ``yycbcr`` is the current interpretation of those words, not a
hardware-validated format for all X70 framebuffers. Alternative packings
and color matrices are explicit so captures can test the interpretation.
"""

from __future__ import annotations

import operator
import struct
import zlib

import numpy as np


PACKINGS = ("yycbcr", "yycrcb", "yuyv", "uyvy", "rgb565")
MATRICES = ("bt601-limited", "bt601-full", "bt709-limited", "bt709-full")


def _positive_integer(value: object, name: str) -> int:
    try:
        result = operator.index(value)
    except TypeError as exc:
        raise ValueError(f"{name} must be a positive integer") from exc
    if isinstance(value, bool) or result <= 0:
        raise ValueError(f"{name} must be a positive integer")
    return result


def render_rgb(
    data: bytes | bytearray | memoryview,
    width: int,
    height: int,
    stride: int | None = None,
    packing: str = "yycbcr",
    matrix: str = "bt601-limited",
) -> np.ndarray:
    """Convert a packed two-byte-per-pixel buffer into H×W×3 uint8 RGB.

    ``stride`` is the distance between row starts in bytes. Padding at the
    end of the final row is optional, and trailing bytes are ignored.
    All four YUV formats are 4:2:2 and require an even width. RGB565 words
    are little-endian; the color matrix is unused for this packing.

    Unknown formats, matrices, dimensions, and truncated buffers raise
    ValueError instead of supplying plausible but invented pixels.
    """
    width = _positive_integer(width, "width")
    height = _positive_integer(height, "height")
    if packing not in PACKINGS:
        raise ValueError(f"unsupported packing {packing!r}; choose from {PACKINGS}")
    if matrix not in MATRICES:
        raise ValueError(f"unsupported matrix {matrix!r}; choose from {MATRICES}")
    if packing != "rgb565" and width % 2:
        raise ValueError("YUV 4:2:2 requires an even width")
    row_bytes = width * 2
    stride = row_bytes if stride is None else _positive_integer(stride, "stride")
    if stride < row_bytes:
        raise ValueError(f"stride {stride} is shorter than the {row_bytes}-byte row")
    try:
        raw = memoryview(data).cast("B")
    except (TypeError, ValueError) as exc:
        raise ValueError("data must be a contiguous bytes-like buffer") from exc
    required = (height - 1) * stride + row_bytes
    if raw.nbytes < required:
        raise ValueError(f"truncated buffer: need {required} bytes, have {raw.nbytes}")
    rows = np.ndarray((height, row_bytes), dtype=np.uint8, buffer=raw,
                      strides=(stride, 1))

    if packing == "rgb565":
        pairs = rows.reshape(height, width, 2)
        words = pairs[..., 0].astype(np.uint16) | (pairs[..., 1].astype(np.uint16) << 8)
        red = (words >> 11) & 31
        green = (words >> 5) & 63
        blue = words & 31
        # Bit replication preserves the endpoints of the original fields.
        return np.stack(((red << 3) | (red >> 2),
                         (green << 2) | (green >> 4),
                         (blue << 3) | (blue >> 2)), axis=-1).astype(np.uint8)

    groups = rows.reshape(height, width // 2, 4)
    channels = {
        "yycbcr": (0, 1, 2, 3),
        "yycrcb": (0, 1, 3, 2),
        "yuyv": (0, 2, 1, 3),
        "uyvy": (1, 3, 0, 2),
    }
    y0, y1, cb_index, cr_index = channels[packing]
    y = groups[..., [y0, y1]].reshape(height, width).astype(np.float64)
    cb = np.repeat(groups[..., cb_index], 2, axis=1).astype(np.float64) - 128.0
    cr = np.repeat(groups[..., cr_index], 2, axis=1).astype(np.float64) - 128.0

    standard, value_range = matrix.split("-")
    kr, kb = (0.299, 0.114) if standard == "bt601" else (0.2126, 0.0722)
    kg = 1.0 - kr - kb
    if value_range == "limited":
        y = (y - 16.0) * (255.0 / 219.0)
        cb *= 255.0 / 224.0
        cr *= 255.0 / 224.0
    red = y + 2.0 * (1.0 - kr) * cr
    blue = y + 2.0 * (1.0 - kb) * cb
    green = y - 2.0 * kb * (1.0 - kb) / kg * cb - 2.0 * kr * (1.0 - kr) / kg * cr
    return np.clip(np.rint(np.stack((red, green, blue), axis=-1)), 0, 255).astype(np.uint8)


def _png_chunk(kind: bytes, payload: bytes) -> bytes:
    return (struct.pack(">I", len(payload)) + kind + payload
            + struct.pack(">I", zlib.crc32(kind + payload) & 0xFFFFFFFF))


def encode_png(rgb: np.ndarray) -> bytes:
    """Encode uint8 H×W×3 RGB as PNG using only the Python standard library.

    Each scanline uses PNG filter 0. Noncontiguous numpy views are accepted;
    invalid shapes and dtypes are rejected rather than silently converted.
    """
    array = np.asarray(rgb)
    if array.ndim != 3 or array.shape[2] != 3 or array.dtype != np.uint8:
        raise ValueError("RGB image must have shape (height, width, 3) and dtype uint8")
    height, width, _ = array.shape
    if width == 0 or height == 0 or width > 0x7FFFFFFF or height > 0x7FFFFFFF:
        raise ValueError("PNG dimensions must be between 1 and 2^31-1")
    filtered_rows = b"".join(b"\0" + row.tobytes() for row in array)
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + _png_chunk(b"IHDR", ihdr)
            + _png_chunk(b"IDAT", zlib.compress(filtered_rows))
            + _png_chunk(b"IEND", b""))
