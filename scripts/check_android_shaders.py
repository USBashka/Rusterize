"""Check the WGSL demo on an explicitly selected Android emulator, with no Python dependencies."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--adb', default='adb')
parser.add_argument('--serial', required=True)
parser.add_argument('--apk', type=Path)
parser.add_argument('--output', type=Path, default=Path('target/shader-work/android'))
args = parser.parse_args()
if not args.serial.startswith('emulator-'):
    raise SystemExit('This check runs only on an explicitly selected emulator')
args.output.mkdir(parents=True, exist_ok=True)

def adb(*command):
    return subprocess.check_output([args.adb, '-s', args.serial, *map(str, command)])

def frame():
    raw = adb('exec-out', 'screencap')
    width, height, pixel_format = struct.unpack_from('<III', raw)
    assert pixel_format in (1, 2), f'Unsupported screenshot format: {pixel_format}'
    start = len(raw) - width * height * 4
    assert start in (12, 16), 'Unexpected screenshot header'
    # Compare only the centre of the effect, excluding clock, navigation and labels.
    pixels = b''.join(raw[start+(y*width+width//4)*4:start+(y*width+width*3//4)*4]
                      for y in range(height//3, height*2//3))
    assert len(set(pixels)) > 20, 'No visible shader gradient'
    return width, height, hashlib.sha256(pixels).hexdigest()

package = 'dev.rusterize.rusterizeshaders'
activity = package + '/dev.rusterize.MainActivity'
if args.apk:
    adb('install', '-r', args.apk)
adb('shell', 'am', 'force-stop', package)
adb('shell', 'am', 'start', '-W', '-n', activity)
time.sleep(0.4)
a = frame()
time.sleep(0.4)
b = frame()
assert a[2] != b[2], 'Animation did not change pixels'
adb('shell', 'input', 'tap', a[0]//2, a[1]//2)
time.sleep(0.4)
a = frame()
time.sleep(0.4)
b = frame()
assert a == b, 'Paused shader continued changing'
args.output.joinpath('paused.png').write_bytes(adb('exec-out', 'screencap', '-p'))
settings = {}
for setting in ('size', 'density'):
    text = adb('shell', 'wm', setting).decode()
    match = re.search(r'Override \w+: (\S+)', text)
    settings[setting] = match[1] if match else 'reset'
try:
    adb('shell', 'wm', 'size', '800x1200')
    adb('shell', 'wm', 'density', '240')
    time.sleep(0.6)
    resized = frame()
    assert 'Override size: 800x1200' in adb('shell', 'wm', 'size').decode(), 'Emulator did not resize'
    assert resized[2] != b[2], 'The application did not redraw after size/DPI changed'
    # Android may recreate Activity on a screen-layout change. Re-establish pause
    # before testing the distinct suspend/resume contract of the live Activity.
    time.sleep(0.4)
    if resized != frame():
        adb('shell', 'input', 'tap', 400, 600)
        time.sleep(0.4)
    resized = frame()
    time.sleep(0.4)
    assert resized == frame(), 'Resized shader cannot be paused'
    args.output.joinpath('resized.png').write_bytes(adb('exec-out', 'screencap', '-p'))
    adb('shell', 'input', 'keyevent', 'KEYCODE_HOME')
    time.sleep(0.3)
    adb('shell', 'am', 'start', '-W', '-f', '0x20000000', '-n', activity)
    time.sleep(0.5)
    resumed = frame()
    assert resized == resumed, 'Paused state did not survive suspend/resume'
    adb('shell', 'input', 'tap', 400, 600)
    time.sleep(0.3)
    a = frame()
    time.sleep(0.3)
    assert a[2] != frame()[2], 'Animation did not resume'
finally:
    for setting, value in settings.items():
        adb('shell', 'wm', setting, value)
report = dict(animation=True, pause=True, resize=True, dpi=True, suspend_resume=True)
args.output.joinpath('checks.json').write_text(json.dumps(report, indent=2))
print(json.dumps(report))
