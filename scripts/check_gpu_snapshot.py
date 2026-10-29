"""Check the composition-check Rust scene in a native host PNG, using only stdlib."""
import argparse
from pathlib import Path
import struct
import zlib


def load_png(path):
    data = path.read_bytes()
    if data[:8] != b'\x89PNG\r\n\x1a\n':
        raise ValueError('Not a PNG')
    offset, compressed = 8, bytearray()
    width = height = channels = 0
    while offset < len(data):
        length = struct.unpack_from('>I', data, offset)[0]
        tag, payload = data[offset + 4:offset + 8], data[offset + 8:offset + 8 + length]
        offset += length + 12
        if tag == b'IHDR':
            width, height, bits, kind, compression, filtering, interlace = struct.unpack('>IIBBBBB', payload)
            if bits != 8 or kind not in (2, 6) or compression or filtering or interlace:
                raise ValueError('Expected an 8-bit, non-interlaced RGB/RGBA PNG')
            channels = 4 if kind == 6 else 3
        elif tag == b'IDAT':
            compressed.extend(payload)
        elif tag == b'IEND':
            break
    raw, stride = zlib.decompress(compressed), width * channels
    if not channels or len(raw) != (stride + 1) * height:
        raise ValueError('Invalid PNG scanline data')
    pixels, previous = bytearray(), bytearray(stride)
    for y in range(height):
        start = y * (stride + 1)
        kind, row = raw[start], bytearray(raw[start + 1:start + 1 + stride])
        for x in range(stride):
            left, above = row[x - channels] if x >= channels else 0, previous[x]
            corner = previous[x - channels] if x >= channels else 0
            if kind == 1:
                row[x] = (row[x] + left) & 255
            elif kind == 2:
                row[x] = (row[x] + above) & 255
            elif kind == 3:
                row[x] = (row[x] + ((left + above) // 2)) & 255
            elif kind == 4:
                p = left + above - corner
                distances = (abs(p - left), abs(p - above), abs(p - corner))
                predictor = (left, above, corner)[distances.index(min(distances))]
                row[x] = (row[x] + predictor) & 255
            elif kind != 0:
                raise ValueError('Unknown PNG filter')
        pixels.extend(row)
        previous = row
    return width, height, channels, pixels


def verify(path, scale=1.0, origin=(0, 0)):
    width, height, channels, pixels = load_png(path)
    checks = [
        (1, 1, (255, 255, 255), 'canvas background'),
        (10, 10, (147, 147, 159), 'shader UV/alpha'),
        (12, 12, (0, 0, 0), 'canvas above shader'),
        (42, 10, (147, 147, 159), 'translated shader inside path clip'),
        (54, 22, (255, 255, 255), 'outside arbitrary path clip'),
        (8, 40, (255, 0, 0), 'input top-left'),
        (23, 40, (0, 255, 0), 'input top-right'),
        (8, 55, (0, 0, 255), 'input bottom-left'),
        (23, 55, (255, 255, 127), 'input transparency'),
        (63, 40, (255, 0, 0), 'rotated top-left'),
        (63, 55, (0, 255, 0), 'rotated top-right'),
        (48, 40, (0, 0, 255), 'rotated bottom-left'),
        (48, 55, (255, 255, 127), 'rotated input transparency'),
    ]
    for x, y, expected, label in checks:
        px, py = int((x + 0.5) * scale) + origin[0], int((y + 0.5) * scale) + origin[1]
        if not (0 <= px < width and 0 <= py < height):
            raise AssertionError(f'{label}: pixel outside {width}x{height}')
        at = (py * width + px) * channels
        actual = pixels[at:at + 3]
        if 'UV/alpha' in label or 'inside path' in label:
            # The sampled physical pixel centre can differ slightly at fractional DPI.
            u = (((px - origin[0]) + 0.5) / scale - (8 if x < 40 else 40)) / 16
            v = (((py - origin[1]) + 0.5) / scale - 8) / 16
            expected = (round(127.5 + 127.5 * u), round(127.5 + 127.5 * v), 159)
        if any(abs(a - b) > 3 for a, b in zip(actual, expected)):
            raise AssertionError(f'{label} at {px},{py}: {list(actual)} != {expected}')
        if channels == 4 and pixels[at + 3] != 255:
            raise AssertionError(f'{label}: unexpected non-opaque output')
    print(f'GPU_SNAPSHOT_OK: {path}; {len(checks)} pixels, scale={scale}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('png', type=Path)
    parser.add_argument('--scale', type=float, default=1)
    parser.add_argument('--origin-x', type=int, default=0)
    parser.add_argument('--origin-y', type=int, default=0)
    args = parser.parse_args()
    verify(args.png, args.scale, (args.origin_x, args.origin_y))
