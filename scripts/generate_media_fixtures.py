"""Generate original, redistributable demo audio/art (no third-party assets)."""
import math
from pathlib import Path
import struct
import subprocess
import tempfile
import wave
import zlib

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'examples/media/assets'
OUT.mkdir(parents=True, exist_ok=True)
rate, seconds = 22050, 12
notes = [261.63, 329.63, 392.0, 493.88, 440.0, 392.0, 329.63, 293.66]
with tempfile.TemporaryDirectory(dir=ROOT / 'target') as temp:
    wav = Path(temp) / 'demo.wav'
    with wave.open(str(wav), 'wb') as audio:
        audio.setparams((1, 2, rate, 0, 'NONE', 'not compressed'))
        samples = bytearray()
        for i in range(rate * seconds):
            t = i / rate
            pulse = (t % 0.375) / 0.375
            freq = notes[int(t / 0.375) % len(notes)]
            value = math.sin(math.tau * freq * t) * math.exp(-pulse * 4) * 0.24
            value += math.sin(math.tau * 130.815 * t) * 0.06
            value *= min(t * 8, 1, (seconds - t) * 4)
            samples += struct.pack('<h', int(value * 32767))
        audio.writeframes(samples)
    subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', '-i', str(wav), '-codec:a', 'libmp3lame', '-b:a', '48k', '-map_metadata', '-1', str(OUT / 'demo.mp3')], check=True)

def chunk(name, data):
    return struct.pack('>I', len(data)) + name + data + struct.pack('>I', zlib.crc32(name + data))

size = 256
rows = bytearray()
for y in range(size):
    rows.append(0)
    for x in range(size):
        dx, dy = (x - 128) / 128, (y - 128) / 128
        radius = math.hypot(dx, dy)
        rings = (math.sin(radius * 38 - math.atan2(dy, dx) * 2) + 1) / 2
        glow = max(0, 1 - radius / 1.35)
        rows += bytes((int(25 + rings * 45 + glow * 40), int(40 + rings * 70 + glow * 95), int(55 + rings * 60 + glow * 75), 255))
png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(rows, 9)) + chunk(b'IEND', b'')
(OUT / 'cover.png').write_bytes(png)
for path in OUT.iterdir():
    print(path.name, path.stat().st_size)
