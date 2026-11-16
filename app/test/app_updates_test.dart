import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:canvas_studio/app_updates.dart';

final manifest = Uri.parse('https://updates.example.test/update.json');
final apkBytes = utf8.encode('a signed APK fixture');
Map<String, dynamic> releaseData() => {
  'schema': 1,
  'packageName': appPackage,
  'versionName': '2.1.0',
  'versionCode': 3,
  'url': 'https://updates.example.test/v3.apk',
  'size': apkBytes.length,
  'sha256': sha256.convert(apkBytes).toString(),
  'notes': 'Logo and updates',
};
UpdateResponse reply(List<int> bytes) =>
    UpdateResponse(200, Stream.value(bytes));

class FakePlatform implements UpdatePlatform {
  bool allowed = false, requested = false, opened = false;
  @override
  Future<({String name, int build})> appInfo() async =>
      (name: '2.0.0', build: 2);
  @override
  Future<bool> canInstall() async => allowed;
  @override
  Future<void> requestPermission() async {
    requested = true;
  }

  @override
  Future<void> install(File file) async {
    opened = true;
  }
}

void main() {
  test('Update feed rejects wrong package, insecure or foreign URL and bad metadata', () {
    expect(
      () => AppRelease.parse({
        ...releaseData(),
        'packageName': 'other.app',
      }, manifest),
      throwsFormatException,
    );
    expect(
      () => AppRelease.parse({
        ...releaseData(),
        'url': 'http://updates.example.test/v3.apk',
      }, manifest),
      throwsFormatException,
    );
    expect(
      () => AppRelease.parse({
        ...releaseData(),
        'url': 'https://other.example.test/v3.apk',
      }, manifest),
      throwsFormatException,
    );
    expect(
      () => AppRelease.parse({...releaseData(), 'sha256': 'bad'}, manifest),
      throwsFormatException,
    );
    expect(
      () => AppRelease.parse({
        ...releaseData(),
        'size': maxApkBytes + 1,
      }, manifest),
      throwsFormatException,
    );
  });
  test('Checks build numbers and never offers a downgrade', () async {
    final service = UpdateService(
      manifest: manifest,
      fetch: (_) async => reply(utf8.encode(jsonEncode(releaseData()))),
    );
    addTearDown(service.close);
    expect((await service.check(2))!.build, 3);
    expect(await service.check(3), isNull);
    expect(await service.check(4), isNull);
  });
  test(
    'Download verifies bytes and hash before returning installable APK',
    () async {
      final dir = await Directory.systemTemp.createTemp('canvas_update');
      addTearDown(() => dir.delete(recursive: true));
      final service = UpdateService(
        manifest: manifest,
        temporaryDirectory: () async => dir,
        fetch: (_) async => reply(apkBytes),
      );
      addTearDown(service.close);
      var count = 0;
      final file = await service.download(
        AppRelease.parse(releaseData(), manifest),
        (r, t) {
          count = r;
          expect(t, apkBytes.length);
        },
      );
      expect(await file.readAsBytes(), apkBytes);
      expect(count, apkBytes.length);
      expect(file.path.endsWith('.apk'), isTrue);
      expect(await File('${dir.path}/updates/update-3.part').exists(), isFalse);
    },
  );
  test('Truncated or tampered APK is deleted and never returned', () async {
    final dir = await Directory.systemTemp.createTemp('canvas_update_bad');
    addTearDown(() => dir.delete(recursive: true));
    final data = releaseData();
    for (final bytes in [
      apkBytes.sublist(1),
      List<int>.filled(apkBytes.length, 0),
    ]) {
      final service = UpdateService(
        manifest: manifest,
        temporaryDirectory: () async => dir,
        fetch: (_) async => reply(bytes),
      );
      await expectLater(
        service.download(AppRelease.parse(data, manifest), (_, _) {}),
        throwsFormatException,
      );
      expect(await File('${dir.path}/updates/update-3.apk').exists(), isFalse);
      expect(await File('${dir.path}/updates/update-3.part').exists(), isFalse);
      service.close();
    }
  });
  test('Cancelling removes partial download', () async {
    final dir = await Directory.systemTemp.createTemp('canvas_update_cancel');
    addTearDown(() => dir.delete(recursive: true));
    final service = UpdateService(
      manifest: manifest,
      temporaryDirectory: () async => dir,
      fetch: (_) async => UpdateResponse(
        200,
        Stream.fromIterable([apkBytes.sublist(0, 5), apkBytes.sublist(5)]),
      ),
    );
    addTearDown(service.close);
    await expectLater(
      service.download(
        AppRelease.parse(releaseData(), manifest),
        (_, _) => service.cancel(),
      ),
      throwsA(isA<UpdateCancelled>()),
    );
    expect(await File('${dir.path}/updates/update-3.part').exists(), isFalse);
  });
  test(
    'Install requires explicit Android permission before opening installer',
    () async {
      final dir = await Directory.systemTemp.createTemp('canvas_update_flow');
      addTearDown(() => dir.delete(recursive: true));
      final service = UpdateService(
        manifest: manifest,
        temporaryDirectory: () async => dir,
        fetch: (uri) async => uri.path.endsWith('.json')
            ? reply(utf8.encode(jsonEncode(releaseData())))
            : reply(apkBytes),
      );
      final platform = FakePlatform();
      final controller = UpdateController(service, platform: platform);
      addTearDown(controller.dispose);
      await controller.check();
      expect(controller.state, UpdateState.available);
      await controller.download();
      expect(controller.state, UpdateState.downloaded);
      await controller.install();
      expect(controller.state, UpdateState.permission);
      expect(platform.opened, isFalse);
      await controller.requestPermission();
      expect(platform.requested, isTrue);
      platform.allowed = true;
      await controller.install();
      expect(controller.state, UpdateState.installer);
      expect(platform.opened, isTrue);
    },
  );
}
