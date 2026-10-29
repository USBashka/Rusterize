"""Exercise the media demo on an explicitly selected Android emulator (no Python packages)."""
import argparse
import hashlib
from pathlib import Path
import re
import struct
import subprocess
import time
import xml.etree.ElementTree as ET

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--adb', default='adb')
p.add_argument('--serial', required=True)
p.add_argument('--apk', type=Path)
p.add_argument('--output', type=Path, default=Path('target/media-check/android'))
a = p.parse_args()
if not a.serial.startswith('emulator-'):
    raise SystemExit('Select an emulator explicitly; physical devices are not modified')
a.output.mkdir(parents=True, exist_ok=True)
package = 'dev.rusterize.rusterizemedia'
activity = package + '/dev.rusterize.MainActivity'

def adb(*args):
    return subprocess.check_output([a.adb, '-s', a.serial, *map(str, args)])

def tree():
    adb('shell', 'uiautomator', 'dump', '/sdcard/rusterize-media-check.xml')
    return ET.fromstring(adb('exec-out', 'cat', '/sdcard/rusterize-media-check.xml'))

def bounds(node):
    return list(map(int, re.findall(r'\d+', node.attrib['bounds'])))

def click_node(node):
    x, y, r, b = bounds(node)
    adb('shell', 'input', 'tap', (x+r)//2, (y+b)//2)

def canvas():
    nodes = list(tree().iter('node'))
    hint = next((n for n in nodes if n.get('resource-id') == 'com.android.systemui:id/ok'), None)
    if hint is not None:
        click_node(hint)
        nodes = list(tree().iter('node'))
    return bounds(next(n for n in nodes
                       if n.get('package') == package and n.get('class') == 'android.view.View'))

def tap(x, y):
    adb('shell', 'input', 'tap', round(origin[0]+x*density), round(origin[1]+y*density))

def region(x, y, right, bottom):
    raw = adb('exec-out', 'screencap')
    width, height, fmt = struct.unpack_from('<III', raw)
    assert fmt in (1, 2)
    start = len(raw)-width*height*4
    x, right = round(origin[0]+x*density), round(origin[0]+right*density)
    y, bottom = round(origin[1]+y*density), round(origin[1]+bottom*density)
    assert 0 <= x < right <= width and 0 <= y < bottom <= height
    return hashlib.sha256(b''.join(raw[start+(row*width+x)*4:start+(row*width+right)*4]
                                   for row in range(y, bottom))).hexdigest()

def screenshot(name):
    (a.output/name).write_bytes(adb('exec-out', 'screencap', '-p'))

if a.apk:
    adb('install', '-r', a.apk)
adb('shell', 'am', 'force-stop', package)
adb('shell', 'am', 'start', '-W', '-n', activity)
time.sleep(1)
if any('documentsui' in n.get('package', '') for n in tree().iter('node')):
    adb('shell', 'input', 'keyevent', 'KEYCODE_BACK')
    adb('shell', 'am', 'start', '-W', '-n', activity)
    time.sleep(0.3)
origin = canvas()
density = int(re.findall(r'density: (\d+)', adb('shell', 'wm', 'density').decode())[-1])/160
width = (origin[2]-origin[0])/density
tap(130+(width-154)*0.37, 474)
time.sleep(0.3)
saved = region(125, 455, width-20, 495)
adb('shell', 'am', 'force-stop', package)
adb('shell', 'am', 'start', '-W', '-n', activity)
time.sleep(1)
origin = canvas()
assert region(125, 455, width-20, 495) == saved, 'Volume did not survive process restart'
screenshot('restored.png')
tap(80, 416)
time.sleep(0.35)
first = region(204, 180, width-24, 304)
time.sleep(0.3)
assert region(204, 180, width-24, 304) != first, 'Audio clock/spectrum did not advance'
screenshot('playing.png')
tap(width-65, 416)  # Simultaneous effect.
tap(80, 416)
time.sleep(0.3)
paused = region(204, 180, width-24, 304)
time.sleep(0.25)
assert region(204, 180, width-24, 304) == paused, 'Paused spectrum kept advancing'
tap(24+(width-48)*0.5, 350)  # Seek while paused.
tap(80, 416)
time.sleep(0.3)
adb('shell', 'input', 'keyevent', 'KEYCODE_HOME')
time.sleep(0.3)
adb('shell', 'am', 'start', '-W', '-f', '0x20000000', '-n', activity)
time.sleep(0.4)
paused = region(204, 180, width-24, 304)
time.sleep(0.3)
assert region(204, 180, width-24, 304) == paused, 'Audio automatically resumed after background'
screenshot('interrupted.png')
tap(80, 100)
assert any('documentsui' in n.get('package', '') for n in tree().iter('node'))
adb('shell', 'input', 'keyevent', 'KEYCODE_BACK')
time.sleep(0.3)
assert canvas() == origin, 'Document cancellation did not return to the app'
# CREATE_DOCUMENT defaults to the most recently used directory. Select Downloads
# manually once if the emulator is fresh; the import walkthrough uses this directory.
before = set(adb('shell', 'ls', '/sdcard/Download').decode().splitlines())
tap(80, 536)
save = next(n for n in tree().iter('node') if n.get('resource-id') == 'android:id/button1')
click_node(save)
time.sleep(0.5)
after = set(adb('shell', 'ls', '/sdcard/Download').decode().splitlines())
created = [name for name in after-before if name.startswith('volume.txt')]
assert len(created) == 1, 'Expected a new volume.txt export in Downloads'
value = float(adb('exec-out', 'cat', '/sdcard/Download/'+created[0]))
assert abs(value-0.37) < 0.01, 'Export did not contain restored settings'
origin = canvas()
tap(width-85, 536)
time.sleep(0.4)
full = canvas()
assert full[3]-full[1] > origin[3]-origin[1], 'Fullscreen did not hide system panels'
origin = full
tap(width-85, 536)
time.sleep(0.4)
origin = canvas()
screenshot('finished.png')
print('ANDROID_MEDIA_OK: playback, pause, seek, effect, background, restart, picker cancellation, export, fullscreen')
