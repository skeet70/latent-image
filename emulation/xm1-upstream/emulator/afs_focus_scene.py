"""Declared scene inputs for the stock contrast AF record consumer.

The supplied image is normalized onto the programmed AF ROI.  The two
gradient grids and one band depend on its pixels and the actual sampled lens
coordinate.  Physical ISP filters, optical scale and ROI-to-sensor mapping
remain uncalibrated; opaque auxiliary/packed windows explicitly contain zero
experiment inputs.  No camera state, pose, callback or AF outcome is written.
"""
import hashlib
import struct

from .hardware import UnsupportedDevice
from . import measurement_scene


PROFILE = 'declared-afs-normalized-programmed-roi-contrast-statistics-unmeasured'
CONTRAST_PROFILE = 'declared-afs-normalized-squared-horizontal-gradient-grid-unmeasured'
MAX_GRID = 8
ROW_BYTES = 64
OPAQUE_OFFSETS = (0x850, 0xc50, 0x1050, 0x1990, 0x1d90)


def _squared_grid(image, width, height, stride_bytes, columns, rows):
    """Declared energy, not recovered ISP arithmetic or a focus decision.

    Squared gradients decrease when an isolated edge spreads under blur.
    Plane0 uses three-sample box-gradient energy; plane1 uses adjacent
    samples. Each row is divided by256 (and9 for the box gradient), keeping
    worst-case qualified one-megapixel U8 input within a U32 plane.
    """
    size = (rows - 1) * ROW_BYTES + columns * 4
    planes = [bytearray(size), bytearray(size)]
    energies = [[[] for _ in range(rows)] for _ in range(2)]
    np = measurement_scene._numpy
    luma = None
    if np is not None:
        packed = np.frombuffer(image, dtype=np.uint8).reshape(height, stride_bytes)
        luma = packed[:, :width * 2].reshape(height, width // 2, 4)[:, :, :2].reshape(height, width).astype(np.int64)
    for cy in range(rows):
        for cx in range(columns):
            x0, x1 = cx * width // columns, (cx + 1) * width // columns
            y0, y1 = cy * height // rows, (cy + 1) * height // rows
            coarse = fine = 0
            if luma is not None:
                cell = luma[y0:y1, x0:x1]
                adjacent = cell[:, 1:] - cell[:, :-1]
                box = cell[:, 3:] - cell[:, :-3]
                fine = int(((adjacent * adjacent).sum(axis=1, dtype=np.uint64) // 256).sum(dtype=np.uint64))
                coarse = int(((box * box).sum(axis=1, dtype=np.uint64) // 2304).sum(dtype=np.uint64))
            else:
                for y in range(y0, y1):
                    start = y * stride_bytes
                    values = [image[start + (x // 2) * 4 + (x & 1)] for x in range(x0, x1)]
                    fine += sum((b - a) ** 2 for a, b in zip(values, values[1:])) // 256
                    coarse += sum((values[x + 3] - values[x]) ** 2 for x in range(len(values) - 3)) // 2304
            for index, value in enumerate((coarse, fine)):
                _integer(value, 0, 0xffffffff)
                struct.pack_into('<I', planes[index], cy * ROW_BYTES + cx * 4, value)
                energies[index][cy].append(value)
    return dict(profile=CONTRAST_PROFILE, coarse=energies[0], fine=energies[1],
                bank_spans=[(offset, bytes(raw)) for offset, raw in zip((0, 0x400), planes)])


def _integer(value, low, high):
    if type(value) is not int or not low <= value <= high:
        raise UnsupportedDevice('AF contrast scene requires bounded programmed integers')
    return value


def prepare(image, *, width, height, stride_bytes, focus_position, target_position,
            dimensions, grid, windows, band_count, band_scale, band_orientation):
    """Pure consumer-byte plan for a single measured frame opportunity.

    ``dimensions`` is the six bytes read by12FCF74 at16227A4..A9. ``grid``
    binds the six programmed packet words16224D8..EC; ``windows`` binds the
    ten region rectangles passed to11F88F0.  These are input metadata, never
    rewritten.  Only the currently observed one valid AF band is qualified.
    """
    if (type(dimensions) is not bytes or len(dimensions) != 6
            or any(not 1 <= value <= MAX_GRID for value in dimensions)
            or len(set(tuple(dimensions[index:index + 2]) for index in (0, 2, 4))) != 1
            or type(grid) is not dict
            or set(grid) != {'cell_width', 'cell_height', 'x', 'y', 'gap_x', 'gap_y', 'columns', 'rows'}
            or type(windows) is not list or len(windows) != 10
            or type(band_count) is not int or band_count != 1
            or type(band_scale) is not int or band_scale != 1000
            or type(band_orientation) is not int or band_orientation not in (0, 1)):
        raise UnsupportedDevice('AF contrast scene requires the observed grids and one qualified band')
    columns, rows = dimensions[:2]
    for key in ('columns', 'rows'): _integer(grid[key], 1, MAX_GRID)
    for key in ('cell_width', 'cell_height'): _integer(grid[key], 1, 8192)
    for key in ('x', 'y', 'gap_x', 'gap_y'): _integer(grid[key], 0, 8191)
    if (grid['columns'], grid['rows']) != (columns, rows):
        raise UnsupportedDevice('AF contrast scene dimensions differ from the actual programmed cells')
    for rectangle in windows:
        if type(rectangle) not in (tuple, list) or len(rectangle) != 4:
            raise UnsupportedDevice('AF contrast scene requires four-word window rectangles')
        for value in rectangle[:2]: _integer(value, 0, 8191)
        for value in rectangle[2:]: _integer(value, 1, 8192)
        if rectangle[0] + rectangle[2] > 8192 or rectangle[1] + rectangle[3] > 8192:
            raise UnsupportedDevice('AF contrast scene window exceeds the programmed coordinate bound')
    if ([tuple(rectangle) for rectangle in windows[1:]] != [(0, 0, 1, 1)] * 9
            or grid['x'] + columns * grid['cell_width'] + (columns - 1) * grid['gap_x'] > 8192
            or grid['y'] + rows * grid['cell_height'] + (rows - 1) * grid['gap_y'] > 8192):
        raise UnsupportedDevice('Additional AF region bands require their own declared scene projection')
    optical = measurement_scene.lens_scene_grid(image, width=width, height=height,
        stride_bytes=stride_bytes, focus_position=focus_position, target_position=target_position,
        columns=columns, rows=rows)
    blurred = optical['image']
    # A contiguous band includes genuine gradients across cell boundaries;
    # the grid law correctly excludes those boundaries from individual cells.
    contrast = _squared_grid(blurred, width, height, stride_bytes, columns, rows)
    band = _squared_grid(blurred, width, height, stride_bytes, 1, 1)['fine'][0][0]
    _integer(band, 0, (1 << 64) - 1)
    window_size = (rows - 1) * ROW_BYTES + columns * 4
    spans = list(contrast['bank_spans'])
    spans.append((0x800, struct.pack('<Q', band)))
    spans.extend((offset, bytes(window_size)) for offset in OPAQUE_OFFSETS)
    return dict(profile=PROFILE, spans=spans, dimensions=dimensions,
        programmed_grid=dict(grid), programmed_windows=[list(rectangle) for rectangle in windows],
        band_count=band_count, band_scale=band_scale, band_orientation=band_orientation,
        band_raw=band, contrast=contrast,
        optical={key:optical[key] for key in ('profile', 'focus_position', 'target_position',
                    'pulses_per_blur_pass', 'blur_passes', 'source_sha256')},
        source_sha256=hashlib.sha256(image).hexdigest(),
        assumptions='Supplied YYCbCr scene normalized to the programmed AF ROI; equal fractional cells, declared binomial defocus filtering, and squared horizontal gradients with per-row division256 (and9 for the box-gradient plane). Source1 sampled lens pose and fixed scene target are external inputs. Auxiliary850/C50/1050, packed1990 and extra1D90 windows are explicit zero experiment inputs because their physical ISP meaning is unknown; programmed geometry, crop correspondence, gain and DMA byte counts are not calibrated. No AF decision is supplied.')
