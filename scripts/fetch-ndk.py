"""Download the pinned official Windows NDK into the checkout, without changing the SDK."""
from pathlib import Path
import hashlib
import urllib.request
import zipfile

root = Path(__file__).resolve().parents[1] / '.tools'
root.mkdir(exist_ok=True)
archive = root / 'android-ndk-r30-windows.zip'
expected = '9bf167a1985fa7d4a036186b78f702eab9179408'
if not archive.exists():
    temporary = archive.with_suffix('.download')
    print('Downloading Android NDK r30 (728 MB)', flush=True)
    urllib.request.urlretrieve('https://dl.google.com/android/repository/android-ndk-r30-windows.zip', temporary)
    temporary.replace(archive)
if hashlib.file_digest(archive.open('rb'), 'sha1').hexdigest() != expected:
    raise SystemExit('NDK checksum mismatch; remove the archive and retry')
print('Extracting verified NDK', flush=True)
with zipfile.ZipFile(archive) as package:
    for entry in package.infolist():
        if not (root / entry.filename).resolve().is_relative_to(root.resolve()):
            raise SystemExit('Unsafe archive entry')
    package.extractall(root)
print(root / 'android-ndk-r30', flush=True)
