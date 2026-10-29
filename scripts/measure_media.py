"""Measure existing release media/minimal builds and create reproducible web archives."""
import gzip
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tarfile

root = Path(__file__).resolve().parents[1]
report = {'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
          'files': {}, 'web': {}, 'embedded_assets': {}}
for name, path in {
    'minimal_windows': root/'dist/minimal.exe',
    'media_windows': root/'dist/media.exe',
    'media_without_assets_windows': root/'target/media-size/no-assets/media.exe',
    'media_android_two_abis': root/'dist/rusterize-media.apk',
    'minimal_android_two_abis': root/'dist/rusterize-minimal.apk',
}.items():
    if path.exists():
        data = path.read_bytes()
        report['files'][name] = {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
for name, directory in {
    'minimal': root/'dist/web/rusterize-minimal',
    'media': root/'dist/web/rusterize-media',
    'media_without_assets': root/'target/media-size/no-assets/web/rusterize-media',
}.items():
    files = sorted(p for p in directory.rglob('*') if p.is_file())
    if not files or not (directory/'app.wasm').is_file():
        raise SystemExit(f'Build {directory} before measuring')
    entries = {p.relative_to(directory).as_posix(): p.read_bytes() for p in files}
    tar = io.BytesIO()
    with tarfile.open(fileobj=tar, mode='w', format=tarfile.USTAR_FORMAT) as archive:
        for path, data in entries.items():
            info = tarfile.TarInfo(path)
            info.size, info.mode, info.mtime = len(data), 0o644, 0
            archive.addfile(info, io.BytesIO(data))
    packed = gzip.compress(tar.getvalue(), compresslevel=9, mtime=0)
    destination = root/'dist'/f'rusterize-{name.replace("_", "-")}-web.tar.gz'
    destination.write_bytes(packed)
    report['web'][name] = {
        'wasm': len(entries['app.wasm']),
        'package': sum(map(len, entries.values())),
        'http_gzip_sum': sum(len(gzip.compress(data, compresslevel=9, mtime=0)) for data in entries.values()),
        'tar_gzip': len(packed),
        'file_count': len(files),
    }
for path in sorted((root/'examples/media/assets').glob('*')):
    if path.is_file():
        report['embedded_assets'][path.name] = path.stat().st_size
report['embedded_assets_total'] = sum(report['embedded_assets'].values())
text = json.dumps(report, indent=2, ensure_ascii=False)+'\n'
(root/'dist/media-sizes.json').write_text(text, encoding='utf-8')
print(text)
