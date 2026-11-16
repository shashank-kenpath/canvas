from pathlib import Path
from datetime import datetime, timezone
import hashlib
import json
import os
import re
import shutil
import subprocess

root = Path('/home/ubuntu/canvas-studio')
artifacts = root / 'app/build/app/outputs/flutter-apk'
downloads = root / 'downloads'
downloads.mkdir(exist_ok=True)
pubspec = (root / 'app/pubspec.yaml').read_text()
version, build = re.search(r'^version:\s*([^+\s]+)\+(\d+)\s*$', pubspec, re.MULTILINE).groups()
apk = artifacts / 'app-release.apk'
apksigner = '/home/ubuntu/Android/sdk/build-tools/36.0.0/apksigner'
def certificate(path):
    output = subprocess.check_output([apksigner, 'verify', '--print-certs', str(path)], text=True)
    return re.findall(r'Signer #\d+ certificate SHA-256 digest: (\w+)', output)
new_certificate = certificate(apk)
if not new_certificate:
    raise RuntimeError('APK signing certificate is missing')
previous = downloads / 'canvas-studio-native.apk'
if previous.exists() and certificate(previous) != new_certificate:
    raise RuntimeError('Signing key changed; refusing to publish an incompatible update')
with apk.open('rb') as source:
    release_hash = hashlib.file_digest(source, 'sha256').hexdigest()
immutable_name = f'canvas-studio-{build}-{release_hash[:12]}.apk'
feed = downloads / 'update.json'
if feed.exists():
    old = json.loads(feed.read_text())
    if old['versionCode'] > int(build) or (old['versionCode'] == int(build) and old['sha256'] != release_hash):
        raise RuntimeError('Increment the pubspec version code before publishing a changed APK')
names = {
    'canvas-studio-native.apk': 'app-release.apk',
    'canvas-studio.apk': 'app-release.apk',
    'canvas-studio-arm64.apk': 'app-arm64-v8a-release.apk',
    immutable_name: 'app-release.apk',
}
checksums = []
for name, source in names.items():
    temp = downloads / (name + '.tmp')
    shutil.copyfile(artifacts / source, temp)
    temp.replace(downloads / name)
    with (downloads / name).open('rb') as source:
        sha = hashlib.file_digest(source, 'sha256').hexdigest()
    checksums.append(f'{sha}  {name}\n')
    print(name, (downloads / name).stat().st_size, sha)
checksum_temp = downloads / 'SHA256SUMS.tmp'
checksum_temp.write_text(''.join(checksums))
checksum_temp.replace(downloads / 'SHA256SUMS')
base = os.environ.get('PUBLIC_BASE_URL', 'https://annotated-remedies-property-binding.trycloudflare.com').rstrip('/')
if not base.startswith('https://'):
    raise RuntimeError('Public update host must use HTTPS')
manifest = {
    'schema': 1, 'packageName': 'com.canvasstudio.canvas_studio',
    'versionName': version, 'versionCode': int(build),
    'url': f'{base}/{immutable_name}', 'size': apk.stat().st_size,
    'sha256': release_hash, 'publishedAt': datetime.now(timezone.utc).isoformat(),
    'notes': os.environ.get('RELEASE_NOTES', 'New Canvas Studio logo and launcher icon. In-app update checks, verified downloads, and Android installation. Saved boards stay on your device.'),
}
manifest_temp = downloads / 'update.json.tmp'
manifest_temp.write_text(json.dumps(manifest, indent=2) + '\n')
manifest_temp.replace(downloads / 'update.json')
print('Published update feed:', base + '/update.json', 'version', version, 'build', build)
