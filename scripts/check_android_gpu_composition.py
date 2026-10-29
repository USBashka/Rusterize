"""Build/run device instrumentation on an explicitly selected emulator (API 29+)."""
import argparse
import json
from pathlib import Path
import subprocess
import re
import time
import zipfile
import build as builder
from check_gpu_snapshot import load_png, verify

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--adb', default='adb')
parser.add_argument('--serial', required=True)
parser.add_argument('--package', default='dev.rusterize.gpucomposition')
parser.add_argument('--output', type=Path, default=Path('target/android-composition'))
parser.add_argument('--target-dir', type=Path, help='Cargo target directory; defaults to cargo metadata')
args = parser.parse_args()
if not args.serial.startswith('emulator-'):
    raise SystemExit('This check only installs onto an explicitly selected emulator')
root = Path(__file__).resolve().parents[1]
out = args.output.resolve(); out.mkdir(parents=True, exist_ok=True)
sdk, java = builder.sdk_home(), builder.java_home()
tools = builder.latest(sdk/'build-tools', '*')
android = builder.latest(sdk/'platforms', 'android-*')/'android.jar'
suffix = '.exe' if builder.os.name == 'nt' else ''
classes, dex = out/'instrumentation-classes', out/'instrumentation-dex'
classes.mkdir(exist_ok=True); dex.mkdir(exist_ok=True)
if args.target_dir:
    target_dir = args.target_dir.resolve()
else:
    metadata = json.loads(builder.run(['cargo', 'metadata', '--no-deps', '--format-version', '1'], capture=True))
    target_dir = Path(metadata['target_directory'])
production = target_dir/'rusterize/android/canvas/classes'
manifest = out/'InstrumentationManifest.xml'
manifest.write_text(f'''<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="{args.package}.tests">
<uses-sdk android:minSdkVersion="29" android:targetSdkVersion="35"/>
<instrumentation android:name="dev.rusterize.GpuCompositionInstrumentation" android:targetPackage="{args.package}"/>
<application android:hasCode="true"/></manifest>''')
unsigned, aligned, apk = out/'tests-unsigned.apk', out/'tests-aligned.apk', out/'gpu-composition-tests.apk'
builder.run([tools/('aapt2'+suffix),'link','-I',android,'--manifest',manifest,'-o',unsigned])
builder.run([java/'bin'/('javac'+suffix),'-encoding','UTF-8','-source','8','-target','8','-Xlint:-options','-bootclasspath',android,'-classpath',production,'-d',classes,root/'scripts/android/GpuCompositionInstrumentation.java'])
builder.run([java/'bin'/('java'+suffix),'-cp',tools/'lib/d8.jar','com.android.tools.r8.D8','--min-api','29','--lib',android,'--classpath',production,'--output',dex,*classes.rglob('*.class')])
with zipfile.ZipFile(unsigned,'a',compression=zipfile.ZIP_DEFLATED) as archive:
    for file in dex.glob('*.dex'): archive.write(file,file.name)
builder.run([tools/('zipalign'+suffix),'-P','16','-f','4',unsigned,aligned])
builder.run([java/'bin'/('java'+suffix),'-jar',tools/'lib/apksigner.jar','sign','--ks',target_dir/'rusterize/android/canvas/debug.keystore','--ks-pass','pass:android','--key-pass','pass:android','--out',apk,aligned])
def adb(*command):
    return subprocess.check_output([args.adb,'-s',args.serial,*map(str,command)],text=True)
print(adb('install','-r',apk))
report=adb('shell','am','instrument','-w',args.package+'.tests/dev.rusterize.GpuCompositionInstrumentation')
(out/'instrumentation.txt').write_text(report)
print(report)
if 'ANDROID_GPU_OK ' not in report or 'host_readback=0' not in report:
    raise SystemExit('Android GPU instrumentation failed')
(out/'instrumentation.json').write_text(json.dumps(dict(success=True,report=report),indent=2))

activity=args.package+'/dev.rusterize.MainActivity'
def snapshot(name):
    path=out/name
    path.write_bytes(subprocess.check_output([args.adb,'-s',args.serial,'exec-out','screencap','-p']))
    width,height,channels,pixels=load_png(path)
    density_text=adb('shell','wm','density')
    density=int(re.findall(r'density: (\d+)',density_text)[-1])
    # The deterministic Rust scene begins with a white 128x96 logical canvas.
    origin=next(y for y in range(height) if pixels[(y*width+2)*channels:(y*width+2)*channels+3]==b'\xff\xff\xff')
    verify(path,density/160,(0,origin))
    return dict(file=str(path),density=density,origin_y=origin,width=width,height=height)
settings={}
for setting in ('size','density'):
    text=adb('shell','wm',setting)
    match=re.search(r'Override \w+: (\S+)',text)
    settings[setting]=match[1] if match else 'reset'
screens=[]
try:
    adb('shell','am','start','-W','-n',activity);time.sleep(0.5)
    screens.append(snapshot('scene.png'))
    adb('shell','wm','size','800x1200');adb('shell','wm','density','240');time.sleep(0.7)
    screens.append(snapshot('resized.png'))
    adb('shell','input','keyevent','KEYCODE_HOME');time.sleep(0.2)
    adb('shell','am','start','-W','-f','0x20000000','-n',activity);time.sleep(0.5)
    screens.append(snapshot('resumed.png'))
finally:
    for setting,value in settings.items():adb('shell','wm',setting,value)
(out/'screenshots.json').write_text(json.dumps(dict(resize=True,dpi=True,suspend_resume=True,screenshots=screens),indent=2))
