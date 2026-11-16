from pathlib import Path
from zipfile import ZipFile
import json

root = Path('/home/ubuntu/canvas-studio/app')
apk = root / 'build/app/outputs/flutter-apk/app-release.apk'
with ZipFile(apk) as archive:
    names = archive.namelist()
    assert not any('assets/editor/' in name for name in names), 'Legacy web assets packaged'
    assert not any(name.endswith(('.html', '.jsx', '.js')) for name in names), 'Unexpected web bundle'
    assert any(name.endswith('/libapp.so') for name in names), 'Missing compiled native Dart application'
    print('Native release verified: compiled libapp.so present; no HTML/JS editor assets.')
plugins = json.loads((root / '.flutter-plugins-dependencies').read_text())
assert 'webview' not in json.dumps(plugins).lower(), 'WebView plugin still present'
print('No WebView plugin registered.')
