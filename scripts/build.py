"""Build native Rusterize hosts with the platform SDK and Python's standard library."""
from __future__ import annotations
import argparse
import json
import os
from pathlib import Path
import platform
import plistlib
import re
import shlex
import shutil
import subprocess
import sys
import zipfile

ROOT = Path(__file__).resolve().parents[1]

def run(args, *, env=None, capture=False):
    args = [str(a) for a in args]
    print('+ ' + shlex.join(args), flush=True)
    return subprocess.run(args, cwd=ROOT, env=env, check=True, text=True,
                          stdout=subprocess.PIPE if capture else None).stdout

def latest(directory: Path, pattern: str) -> Path:
    candidates = list(directory.glob(pattern))
    if not candidates:
        raise RuntimeError(f'Missing {directory / pattern}')
    def version(path):
        return tuple(int(n) for n in re.findall(r'\d+', path.name))
    return max(candidates, key=version)

def java_home() -> Path:
    if os.environ.get('JAVA_HOME'):
        return Path(os.environ['JAVA_HOME'])
    bundled = Path(r'C:\Program Files\Android\Android Studio\jbr')
    if bundled.exists():
        return bundled
    java = shutil.which('javac')
    if java:
        return Path(java).resolve().parents[1]
    raise RuntimeError('Set JAVA_HOME to a JDK (17 or newer)')

def sdk_home() -> Path:
    for key in ('ANDROID_HOME', 'ANDROID_SDK_ROOT'):
        if os.environ.get(key):
            return Path(os.environ[key])
    choices = [Path.home()/'AppData/Local/Android/Sdk', Path.home()/'Android/Sdk', Path.home()/'Library/Android/sdk']
    for p in choices:
        if p.exists():
            return p
    raise RuntimeError('Set ANDROID_HOME to your Android SDK')

def build_android(args, package, library, target_dir, output, env):
    sdk = sdk_home()
    ndk_setting = args.ndk or os.environ.get('ANDROID_NDK_HOME')
    if ndk_setting:
        ndk = Path(ndk_setting)
    elif (ROOT/'.tools/android-ndk-r30').exists():
        ndk = ROOT/'.tools/android-ndk-r30'
    else:
        ndk = latest(sdk/'ndk', '*')
    host = 'windows-x86_64' if os.name == 'nt' else 'darwin-x86_64' if sys.platform == 'darwin' else 'linux-x86_64'
    toolchain = ndk/'toolchains/llvm/prebuilt'/host/'bin'
    tool_suffix = '.exe' if os.name == 'nt' else ''
    # Call clang.exe directly: Rust cannot reliably execute the NDK's Windows .cmd wrappers.
    clang = toolchain/('clang'+tool_suffix)
    if not clang.exists():
        raise RuntimeError(f'NDK compiler not found: {clang}')
    abis = args.abi or ['arm64-v8a']
    targets = {'arm64-v8a':('aarch64-linux-android','aarch64-linux-android'), 'x86_64':('x86_64-linux-android','x86_64-linux-android')}
    native = []
    for abi in abis:
        target, triple = targets[abi]
        build_env = env.copy()
        build_env['CARGO_TARGET_'+target.upper().replace('-','_')+'_LINKER'] = str(clang)
        build_env['RUSTFLAGS'] = f'-C link-arg=--target={triple}26 -C link-arg=-Wl,-z,max-page-size=16384'
        run(['cargo','build','--release','--lib','-p',package['name'],'--target',target],env=build_env)
        native.append((abi,target_dir/target/'release'/('lib'+library+'.so')))
    java = java_home()
    tools = latest(sdk/'build-tools','*')
    android_jar = latest(sdk/'platforms','android-*')/'android.jar'
    build = target_dir/'rusterize/android'
    classes, dex = build/'classes', build/'dex'
    classes.mkdir(parents=True,exist_ok=True)
    dex.mkdir(parents=True,exist_ok=True)
    manifest = build/'AndroidManifest.xml'
    app_id = args.app_id or ('dev.rusterize.'+package['name'].replace('-','').replace('_',''))
    if not re.fullmatch(r'[a-zA-Z]\w*(\.[a-zA-Z]\w*)+',app_id):
        raise RuntimeError('Invalid --app-id')
    manifest.write_text((ROOT/'hosts/android/AndroidManifest.xml').read_text(encoding='utf-8').replace('dev.rusterize.gallery',app_id),encoding='utf-8')
    unsigned, aligned = build/'unsigned.apk',build/'aligned.apk'
    run([tools/('aapt2'+tool_suffix),'link','-I',android_jar,'--manifest',manifest,'-o',unsigned],env=env)
    sources = sorted((ROOT/'hosts/android/java').rglob('*.java'))
    run([java/'bin'/('javac'+tool_suffix),'-encoding','UTF-8','-source','8','-target','8','-Xlint:-options','-bootclasspath',android_jar,'-d',classes,*sources],env=env)
    run([java/'bin'/('java'+tool_suffix),'-cp',tools/'lib/d8.jar','com.android.tools.r8.D8','--min-api','26','--lib',android_jar,'--output',dex,*sorted(classes.rglob('*.class'))],env=env)
    with zipfile.ZipFile(unsigned,'a',compression=zipfile.ZIP_DEFLATED) as apk:
        for file in dex.glob('*.dex'):
            apk.write(file,file.name)
        for abi, library_path in native:
            apk.write(library_path,f'lib/{abi}/librusterize_app.so')
    run([tools/('zipalign'+tool_suffix),'-P','16','-f','4',unsigned,aligned],env=env)
    keystore = build/'debug.keystore'
    if not keystore.exists():
        run([java/'bin'/('keytool'+tool_suffix),'-genkeypair','-noprompt','-keystore',keystore,'-storepass','android','-keypass','android','-alias','androiddebugkey','-keyalg','RSA','-keysize','2048','-validity','10000','-dname','CN=Rusterize Development'],env=env)
    artifact = output/(package['name']+'.apk')
    run([java/'bin'/('java'+tool_suffix),'-jar',tools/'lib/apksigner.jar','sign','--ks',keystore,'--ks-pass','pass:android','--key-pass','pass:android','--out',artifact,aligned],env=env)
    run([java/'bin'/('java'+tool_suffix),'-jar',tools/'lib/apksigner.jar','verify','--verbose',artifact],env=env)
    return artifact

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--platform',choices=['windows','android','linux','macos'],default={'Windows':'windows','Darwin':'macos'}.get(platform.system(),'linux'))
    parser.add_argument('--package')
    parser.add_argument('--out',type=Path,default=ROOT/'dist')
    parser.add_argument('--ndk')
    parser.add_argument('--abi',action='append',choices=['arm64-v8a','x86_64'])
    parser.add_argument('--app-id')
    args = parser.parse_args()
    env = os.environ.copy()
    # Keep Rust/LTO temporary archives on the same writable filesystem.
    temporary = ROOT/'target/rusterize-tmp'
    temporary.mkdir(parents=True,exist_ok=True)
    env['TMP'] = env['TEMP'] = env['TMPDIR'] = str(temporary)
    metadata = json.loads(run(['cargo','metadata','--no-deps','--format-version','1'],env=env,capture=True))
    candidates = [p for p in metadata['packages'] if p['id'] in metadata['workspace_members']]
    name = args.package or ('rusterize-gallery' if any(p['name']=='rusterize-gallery' for p in candidates) else candidates[0]['name'])
    package = next((p for p in candidates if p['name']==name),None)
    if package is None:
        raise RuntimeError(f'Package {name} is not a workspace member')
    library = next((t['name'] for t in package['targets'] if 'staticlib' in t['kind']),None)
    binary = next((t['name'] for t in package['targets'] if t['kind']==['bin']),None)
    target_dir = Path(metadata['target_directory'])
    output = args.out.resolve()
    output.mkdir(parents=True,exist_ok=True)
    if args.platform=='android':
        if not library:
            raise RuntimeError('Android app needs a cdylib target and export_app!(App)')
        artifact = build_android(args,package,library,target_dir,output,env)
    elif args.platform=='windows':
        if os.name!='nt' or not binary:
            raise RuntimeError('Windows builds need Windows/MSVC and an application binary target')
        env['RUSTFLAGS'] = (env.get('RUSTFLAGS','')+' -C target-feature=+crt-static').strip()
        run(['cargo','build','--release','-p',name,'--bin',binary],env=env)
        artifact = output/(binary+'.exe')
        shutil.copy2(target_dir/'release'/(binary+'.exe'),artifact)
    elif args.platform=='linux':
        if sys.platform!='linux' or not library:
            raise RuntimeError('Linux builds need Linux, a staticlib target and libgtk-4-dev')
        run(['cargo','build','--release','--lib','-p',name],env=env)
        flags = shlex.split(run(['pkg-config','--cflags','--libs','gtk4','pangocairo'],capture=True))
        artifact = output/name
        run(['cc','-std=c11','-O2','-Wall','-Wextra','-Werror',ROOT/'hosts/linux/main.c',target_dir/'release'/('lib'+library+'.a'),*flags,'-ldl','-lpthread','-lm','-lrt','-lutil','-o',artifact],env=env)
        run(['strip',artifact],env=env)
    else:
        if sys.platform!='darwin' or not library:
            raise RuntimeError('macOS builds need macOS/Xcode and a staticlib target')
        env['MACOSX_DEPLOYMENT_TARGET'] = '11.0'
        run(['cargo','build','--release','--lib','-p',name],env=env)
        artifact = output/(name+'.app')
        executable = artifact/'Contents/MacOS'/name
        executable.parent.mkdir(parents=True,exist_ok=True)
        run(['swiftc','-Osize','-target',platform.machine()+'-apple-macosx11.0','-import-objc-header',ROOT/'hosts/rusterize.h',ROOT/'hosts/macos/main.swift',target_dir/'release'/('lib'+library+'.a'),'-framework','AppKit','-framework','CoreGraphics','-framework','CoreText','-o',executable],env=env)
        with (artifact/'Contents/Info.plist').open('wb') as file:
            plistlib.dump({'CFBundleExecutable':name,'CFBundleName':name,'CFBundleIdentifier':args.app_id or 'dev.rusterize.app','CFBundlePackageType':'APPL','NSHighResolutionCapable':True,'LSMinimumSystemVersion':'11.0'},file)
        run(['codesign','--force','--sign','-',artifact],env=env)
    size = artifact.stat().st_size if artifact.is_file() else sum(p.stat().st_size for p in artifact.rglob('*') if p.is_file())
    print(f'Built {artifact} ({size:,} bytes)',flush=True)

if __name__=='__main__':
    try:
        main()
    except (RuntimeError,FileNotFoundError,subprocess.CalledProcessError) as error:
        raise SystemExit(str(error)) from error
