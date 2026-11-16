import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';

const updateManifestUrl = String.fromEnvironment(
  'UPDATE_MANIFEST_URL',
  defaultValue: 'https://annotated-remedies-property-binding.trycloudflare.com/update.json',
);
const appPackage = 'com.canvasstudio.canvas_studio';
const maxApkBytes = 200 * 1024 * 1024;

class AppRelease {
  const AppRelease({
    required this.version,
    required this.build,
    required this.url,
    required this.sha256Hex,
    required this.size,
    required this.notes,
  });
  final String version, sha256Hex, notes;
  final int build, size;
  final Uri url;
  factory AppRelease.parse(Map<String, dynamic> data, Uri manifest) {
    if (data['schema'] != 1 || data['packageName'] != appPackage) {
      throw const FormatException('This update is for a different app.');
    }
    final version = data['versionName'];
    final build = data['versionCode'];
    final size = data['size'];
    final hash = data['sha256'];
    final notes = data['notes'] ?? '';
    final url = Uri.tryParse(
      data['url'] is String ? data['url'] as String : '',
    );
    if (version is! String ||
        version.isEmpty ||
        version.length > 100 ||
        build is! int ||
        build < 1 ||
        size is! int ||
        size < 1 ||
        size > maxApkBytes ||
        hash is! String ||
        !RegExp(r'^[a-fA-F0-9]{64}$').hasMatch(hash) ||
        notes is! String ||
        notes.length > 4000) {
      throw const FormatException('The update details are invalid.');
    }
    if (manifest.scheme != 'https' ||
        url == null ||
        url.scheme != 'https' ||
        url.host.isEmpty ||
        url.origin != manifest.origin ||
        url.userInfo.isNotEmpty ||
        url.hasFragment) {
      throw const FormatException('The update download address is invalid.');
    }
    return AppRelease(
      version: version,
      build: build,
      url: url,
      sha256Hex: hash.toLowerCase(),
      size: size,
      notes: notes,
    );
  }
}

class UpdateResponse {
  UpdateResponse(this.status, this.body);
  final int status;
  final Stream<List<int>> body;
}

typedef UpdateFetch = Future<UpdateResponse> Function(Uri uri);

class UpdateCancelled implements Exception {}

class UpdateService {
  UpdateService({
    Uri? manifest,
    this._fetch,
    Future<Directory> Function()? temporaryDirectory,
  }) : manifest = manifest ?? Uri.parse(updateManifestUrl),
       _directory = temporaryDirectory ?? getTemporaryDirectory;
  final Uri manifest;
  final UpdateFetch? _fetch;
  final Future<Directory> Function() _directory;
  final HttpClient _client = HttpClient()
    ..connectionTimeout = const Duration(seconds: 15);
  HttpClientRequest? _request;
  bool _cancelled = false;
  Future<UpdateResponse> _get(Uri uri) async {
    if (uri.scheme != 'https') {
      throw const FormatException('Updates require HTTPS.');
    }
    if (_fetch != null) {
      return _fetch(uri);
    }
    final request = await _client
        .getUrl(uri)
        .timeout(const Duration(seconds: 15));
    _request = request;
    request.followRedirects = false;
    request.headers.set(HttpHeaders.cacheControlHeader, 'no-cache');
    final response = await request.close().timeout(const Duration(seconds: 15));
    return UpdateResponse(response.statusCode, response);
  }

  Future<AppRelease?> check(int installedBuild) async {
    final response = await _get(manifest);
    if (response.status != 200) {
      await response.body.drain<void>();
      throw const HttpException('The update server is unavailable.');
    }
    final bytes = <int>[];
    await for (final chunk in response.body.timeout(
      const Duration(seconds: 20),
    )) {
      bytes.addAll(chunk);
      if (bytes.length > 64 * 1024) {
        throw const FormatException('The update response is too large.');
      }
    }
    final release = AppRelease.parse(
      jsonDecode(utf8.decode(bytes)) as Map<String, dynamic>,
      manifest,
    );
    return release.build > installedBuild ? release : null;
  }

  Future<File> download(
    AppRelease release,
    void Function(int received, int total) progress,
  ) async {
    _cancelled = false;
    final dir = Directory('${(await _directory()).path}/updates');
    await dir.create(recursive: true);
    final partial = File('${dir.path}/update-${release.build}.part');
    final finalFile = File('${dir.path}/update-${release.build}.apk');
    IOSink? sink;
    try {
      final response = await _get(release.url);
      if (response.status != 200) {
        await response.body.drain<void>();
        throw const HttpException('The update could not be downloaded.');
      }
      if (_cancelled) {
        throw UpdateCancelled();
      }
      sink = partial.openWrite();
      var count = 0;
      await for (final chunk in response.body.timeout(
        const Duration(seconds: 30),
      )) {
        if (_cancelled) {
          throw UpdateCancelled();
        }
        count += chunk.length;
        if (count > release.size) {
          throw const FormatException(
            'The download size does not match the update.',
          );
        }
        sink.add(chunk);
        progress(count, release.size);
      }
      await sink.flush();
      await sink.close();
      sink = null;
      if (_cancelled) {
        throw UpdateCancelled();
      }
      if (count != release.size) {
        throw const FormatException(
          'The download is incomplete. Please retry.',
        );
      }
      final hash = await sha256.bind(partial.openRead()).first;
      if (hash.toString() != release.sha256Hex) {
        throw const FormatException(
          'The update verification failed. Please download again.',
        );
      }
      if (_cancelled) {
        throw UpdateCancelled();
      }
      return await partial.rename(finalFile.path);
    } catch (_) {
      if (sink != null) {
        await sink.close();
      }
      if (await partial.exists()) {
        await partial.delete();
      }
      if (_cancelled) {
        throw UpdateCancelled();
      }
      rethrow;
    }
  }

  void cancel() {
    _cancelled = true;
    _request?.abort();
  }

  void close() {
    cancel();
    _client.close(force: true);
  }
}

abstract class UpdatePlatform {
  Future<({String name, int build})> appInfo();
  Future<bool> canInstall();
  Future<void> requestPermission();
  Future<void> install(File file);
}

class AndroidUpdatePlatform implements UpdatePlatform {
  const AndroidUpdatePlatform();
  static const channel = MethodChannel('canvas_studio/updates');
  @override
  Future<({String name, int build})> appInfo() async {
    final data = await channel.invokeMapMethod<String, dynamic>('appInfo');
    if (data == null) {
      throw StateError('Cannot read installed version');
    }
    return (
      name: data['versionName'] as String,
      build: data['versionCode'] as int,
    );
  }

  @override
  Future<bool> canInstall() async =>
      await channel.invokeMethod<bool>('canInstall') ?? false;
  @override
  Future<void> requestPermission() =>
      channel.invokeMethod<void>('requestInstallPermission');
  @override
  Future<void> install(File file) =>
      channel.invokeMethod<void>('installUpdate', {'path': file.path});
}

enum UpdateState {
  idle,
  checking,
  current,
  available,
  downloading,
  downloaded,
  permission,
  installer,
  failed,
}

class UpdateController extends ChangeNotifier {
  UpdateController(
    this.service, {
    this.platform = const AndroidUpdatePlatform(),
  });
  final UpdateService service;
  final UpdatePlatform platform;
  UpdateState state = UpdateState.idle;
  String installed = '', error = '';
  AppRelease? release;
  File? file;
  int received = 0, total = 0;
  bool _disposed = false;
  bool get busy =>
      state == UpdateState.checking || state == UpdateState.downloading;
  void changed() {
    if (!_disposed) {
      notifyListeners();
    }
  }

  Future<void> check() async {
    if (busy) {
      return;
    }
    state = UpdateState.checking;
    error = '';
    changed();
    try {
      final info = await platform.appInfo();
      installed = '${info.name} (${info.build})';
      release = await service.check(info.build);
      file = null;
      state = release == null ? UpdateState.current : UpdateState.available;
    } catch (_) {
      error =
          'Could not check for updates. Check your connection and try again.';
      state = UpdateState.failed;
    }
    changed();
  }

  Future<void> download() async {
    if (busy || release == null) {
      return;
    }
    state = UpdateState.downloading;
    received = 0;
    total = release!.size;
    error = '';
    changed();
    try {
      file = await service.download(release!, (r, t) {
        received = r;
        total = t;
        changed();
      });
      state = UpdateState.downloaded;
    } on UpdateCancelled {
      state = UpdateState.available;
    } catch (_) {
      state = UpdateState.failed;
      error = 'Download or verification failed. Please try again.';
    }
    changed();
  }

  void cancelDownload() => service.cancel();
  Future<void> install() async {
    if (file == null || busy) {
      return;
    }
    try {
      if (!await platform.canInstall()) {
        state = UpdateState.permission;
        changed();
        return;
      }
      await platform.install(file!);
      state = UpdateState.installer;
      error = '';
    } on PlatformException catch (e) {
      state = UpdateState.failed;
      error = e.message ?? 'Android could not open the update. Try again.';
    } catch (_) {
      state = UpdateState.failed;
      error = 'Android could not open the update. Try again.';
    }
    changed();
  }

  Future<void> requestPermission() async {
    try {
      await platform.requestPermission();
    } catch (_) {
      error = 'Could not open install settings. Please retry.';
      changed();
    }
  }

  @override
  void dispose() {
    _disposed = true;
    service.close();
    super.dispose();
  }
}
