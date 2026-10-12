"""Run the WebView self-check APK on an explicitly selected, disposable emulator."""
import argparse
from pathlib import Path
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--adb', default='adb')
parser.add_argument('--serial', required=True)
parser.add_argument('--apk', type=Path, required=True)
args = parser.parse_args()
if not args.serial.startswith('emulator-'):
    raise SystemExit('This check only installs on an explicitly selected emulator')

def adb(*command):
    return subprocess.check_output([args.adb, '-s', args.serial, *map(str, command)],
                                   timeout=30).decode('utf-8', errors='replace')

package = 'dev.rusterize.rusterizewebview'
print(adb('install', '-r', args.apk))
adb('shell', 'am', 'force-stop', package)
stamp = adb('shell', "date '+%m-%d %H:%M:%S.000'").strip()
print(adb('shell', 'am', 'start', '-W', '-n', package+'/dev.rusterize.MainActivity'))
deadline = time.monotonic()+40
while time.monotonic() < deadline:
    log = adb('logcat', '-d', '-v', 'brief', '-T', stamp, '-s', 'RusterizeWebView:I', '*:S')
    if 'WEBVIEW_SMOKE_FAILED' in log:
        raise SystemExit(log)
    if 'WEBVIEW_SMOKE_OK' in log:
        print(log)
        break
    time.sleep(0.5)
else:
    raise SystemExit('No WebView success marker; build the APK with --features self-check')
