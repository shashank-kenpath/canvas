import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';

import 'canvas_document.dart';

typedef CollaborationFetch = Future<Map<String, dynamic>> Function(
  String method,
  Uri uri,
  Map<String, String> headers,
  Map<String, dynamic>? body,
);

class CollaborationException implements Exception {
  const CollaborationException(this.message, {this.statusCode});
  final String message;
  final int? statusCode;
  @override
  String toString() => message;
}

class CollaborationLink {
  CollaborationLink({
    required Uri server,
    required this.roomId,
    required this.token,
  }) : server = _validateServer(server) {
    if (!RegExp(r'^[A-Za-z0-9_-]{4,80}$').hasMatch(roomId) ||
        !RegExp(r'^[A-Za-z0-9_-]{8,256}$').hasMatch(token)) {
      throw const FormatException('This shared-board link is invalid.');
    }
  }

  final Uri server;
  final String roomId;
  final String token;

  Uri get inviteUri => server.replace(
    path: '/join',
    queryParameters: {'room': roomId, 'key': token},
  );
  String get joinCode => inviteUri.toString();
  Uri get mcpUri => server.replace(pathSegments: ['mcp', roomId, token]);

  Map<String, dynamic> toJson() => {
    'server': server.toString(),
    'roomId': roomId,
    'token': token,
  };
  factory CollaborationLink.fromJson(Map<String, dynamic> data) =>
      CollaborationLink(
        server: Uri.parse(data['server'] as String),
        roomId: data['roomId'] as String,
        token: data['token'] as String,
      );

  static CollaborationLink parseInvite(String invite) {
    final uri = Uri.tryParse(invite.trim());
    if (uri == null ||
        uri.path != '/join' ||
        uri.hasFragment ||
        uri.userInfo.isNotEmpty ||
        uri.queryParametersAll['room']?.length != 1 ||
        uri.queryParametersAll['key']?.length != 1) {
      throw const FormatException(
        'Paste the complete shared-board invite link.',
      );
    }
    return CollaborationLink(
      server: Uri(
        scheme: uri.scheme,
        host: uri.host,
        port: uri.hasPort ? uri.port : null,
      ),
      roomId: uri.queryParameters['room'] ?? '',
      token: uri.queryParameters['key'] ?? '',
    );
  }

  static Uri _validateServer(Uri uri) {
    if (uri.scheme != 'https' ||
        uri.host.isEmpty ||
        uri.userInfo.isNotEmpty ||
        (uri.path.isNotEmpty && uri.path != '/') ||
        uri.hasQuery ||
        uri.hasFragment) {
      throw const FormatException(
        'Shared boards require a valid HTTPS server.',
      );
    }
    return Uri(
      scheme: uri.scheme,
      host: uri.host,
      port: uri.hasPort ? uri.port : null,
    );
  }
}

class _PendingOperation {
  const _PendingOperation(this.generation, this.shape);
  final int generation;
  final CanvasShape? shape;
  Map<String, dynamic> json(String id) => shape == null
      ? {'type': 'delete', 'id': id}
      : {'type': 'put', 'shape': shape!.toJson()};
}

/// Shape-level synchronization: independent shapes merge; the server's last
/// committed mutation wins when two people edit the same shape.
///
/// Pending edits survive transient network failures while this client lives.
/// Persist [syncedShapes] beside the local drawing and pass it as [start]'s
/// lastSynced value to reconstruct unacknowledged edits after an app restart.
class CollaborationClient extends ChangeNotifier {
  CollaborationClient({
    required this.link,
    required this.clientId,
    HttpClient? client,
    this._fetch,
    this.pollInterval = const Duration(seconds: 1),
  }) : _client = client ?? HttpClient(),
       _ownsClient = client == null {
    if (clientId.isEmpty || clientId.length > 128) {
      throw ArgumentError('A client identifier is required.');
    }
    _client.connectionTimeout = const Duration(seconds: 15);
  }

  final CollaborationLink link;
  final String clientId;
  final Duration pollInterval;
  final HttpClient _client;
  final bool _ownsClient;
  final CollaborationFetch? _fetch;
  final Map<String, _PendingOperation> _pending = {};
  Map<String, CanvasShape> _view = {};
  List<CanvasShape> _syncedShapes = const [];
  Timer? _timer;
  Future<void>? _inFlight;
  void Function(List<CanvasShape>)? _onRemote;
  int _generation = 0;
  bool _disposed = false, _started = false;
  bool applyingRemote = false;
  String status = 'Connecting…';
  int revision = 0;
  int get pendingCount => _pending.length;
  bool get hasPending => _pending.isNotEmpty;
  List<CanvasShape> get syncedShapes => _syncedShapes;

  static Future<({CollaborationLink link, List<CanvasShape> shapes})> create({
    required Uri server,
    required String name,
    required List<CanvasShape> shapes,
    required String clientId,
    HttpClient? client,
    CollaborationFetch? fetch,
  }) async {
    final base = CollaborationLink._validateServer(server);
    final http = client ?? HttpClient();
    try {
      final data = await _request(
        http,
        fetch,
        'POST',
        base.resolve('/api/rooms'),
        {},
        {
          'name': name,
          'shapes': shapes.map((shape) => shape.toJson()).toList(),
          'clientId': clientId,
        },
      );
      final link = CollaborationLink(
        server: base,
        roomId: data['roomId'] as String,
        token: data['token'] as String,
      );
      return (link: link, shapes: _shapes(data));
    } finally {
      if (client == null) {
        http.close(force: true);
      }
    }
  }

  static Future<({CollaborationLink link, List<CanvasShape> shapes})> join({
    required String invite,
    required String clientId,
    HttpClient? client,
    CollaborationFetch? fetch,
  }) async {
    final link = CollaborationLink.parseInvite(invite);
    final http = client ?? HttpClient();
    try {
      final data = await _request(
        http,
        fetch,
        'GET',
        link.server.resolve('/api/rooms/${link.roomId}'),
        {'Authorization': 'Bearer ${link.token}'},
        null,
      );
      return (link: link, shapes: _shapes(data));
    } finally {
      if (client == null) {
        http.close(force: true);
      }
    }
  }

  /// Requests a short-lived numeric code for [link], so another device (for
  /// example a wall display) can join without typing the long invite link.
  static Future<({String code, int expiresInSeconds})> pairingCode(
    CollaborationLink link, {
    HttpClient? client,
    CollaborationFetch? fetch,
  }) async {
    final http = client ?? HttpClient();
    try {
      final data = await _request(
        http,
        fetch,
        'POST',
        link.server.resolve('/api/rooms/${link.roomId}/code'),
        {'Authorization': 'Bearer ${link.token}'},
        const {},
      );
      final code = data['code'], expires = data['expiresInSeconds'];
      if (code is! String ||
          !RegExp(r'^\d{6}$').hasMatch(code) ||
          expires is! int) {
        throw const FormatException('The pairing code response is invalid.');
      }
      return (code: code, expiresInSeconds: expires);
    } finally {
      if (client == null) {
        http.close(force: true);
      }
    }
  }

  /// Joins with a pairing code produced by [pairingCode].
  static Future<
    ({CollaborationLink link, List<CanvasShape> shapes, String name})
  >
  joinWithCode({
    required Uri server,
    required String code,
    HttpClient? client,
    CollaborationFetch? fetch,
  }) async {
    final base = CollaborationLink._validateServer(server);
    if (!RegExp(r'^\d{6}$').hasMatch(code)) {
      throw const FormatException('Enter the 6-digit board code.');
    }
    final http = client ?? HttpClient();
    try {
      final data = await _request(
        http,
        fetch,
        'POST',
        base.resolve('/api/join-code'),
        {},
        {'code': code},
      );
      final link = CollaborationLink(
        server: base,
        roomId: data['roomId'] as String,
        token: data['token'] as String,
      );
      return (
        link: link,
        shapes: _shapes(data),
        name: data['name'] as String? ?? 'Shared board',
      );
    } finally {
      if (client == null) {
        http.close(force: true);
      }
    }
  }

  /// Name the room was created with, for labelling a joined board.
  static Future<String> roomName(
    CollaborationLink link, {
    HttpClient? client,
    CollaborationFetch? fetch,
  }) async {
    final http = client ?? HttpClient();
    try {
      final data = await _request(
        http,
        fetch,
        'GET',
        link.server.resolve('/api/rooms/${link.roomId}'),
        {'Authorization': 'Bearer ${link.token}'},
        null,
      );
      return data['name'] as String? ?? 'Shared board';
    } finally {
      if (client == null) {
        http.close(force: true);
      }
    }
  }

  void start({
    required List<CanvasShape> initial,
    List<CanvasShape>? lastSynced,
    required void Function(List<CanvasShape>) onRemote,
  }) {
    if (_disposed) {
      throw StateError('This shared-board session has closed.');
    }
    if (_started) {
      throw StateError('This shared-board session has already started.');
    }
    _started = true;
    _syncedShapes = List<CanvasShape>.unmodifiable(lastSynced ?? initial);
    _view = {for (final shape in _syncedShapes) shape.id: shape};
    _onRemote = onRemote;
    submit(initial);
    _timer = Timer.periodic(pollInterval, (_) {
      unawaited(flush());
    });
    unawaited(flush());
  }

  /// Submit committed document changes, never individual pointer samples.
  void submit(List<CanvasShape> shapes) {
    if (_disposed || applyingRemote) {
      return;
    }
    final next = {for (final shape in shapes) shape.id: shape};
    for (final entry in next.entries) {
      final old = _view[entry.key];
      if (old == null || !_same(old, entry.value)) {
        _pending[entry.key] = _PendingOperation(++_generation, entry.value);
      }
    }
    for (final id in _view.keys) {
      if (!next.containsKey(id)) {
        _pending[id] = _PendingOperation(++_generation, null);
      }
    }
    _view = next;
    if (_pending.isNotEmpty) {
      _setStatus('Syncing…');
    }
    // The timer batches rapid commits and ensures only one request is active.
  }

  /// Flush one batch or fetch the latest server state. Multiple callers share
  /// the current request; new mutations remain queued for the following batch.
  Future<void> flush() {
    if (_disposed || !_started) {
      return Future.value();
    }
    return _inFlight ??= _synchronize().whenComplete(() {
      _inFlight = null;
    });
  }

  Future<void> _synchronize() async {
    final sent = Map<String, _PendingOperation>.of(_pending);
    try {
      final room = link.server.resolve('/api/rooms/${link.roomId}');
      final data = await _request(
        _client,
        _fetch,
        sent.isEmpty ? 'GET' : 'POST',
        sent.isEmpty
            ? (revision > 0
                  ? room.replace(queryParameters: {'since': '$revision'})
                  : room)
            : link.server.resolve('/api/rooms/${link.roomId}/ops'),
        {'Authorization': 'Bearer ${link.token}'},
        sent.isEmpty
            ? null
            : {
                'clientId': clientId,
                'since': revision,
                'operations': [
                  for (final entry in sent.entries) entry.value.json(entry.key),
                ],
              },
      );
      if (_disposed) {
        return;
      }
      if (data['roomId'] != link.roomId) {
        throw const FormatException('The shared-board response is invalid.');
      }
      final nextRevision = data['revision'];
      if (nextRevision is! int || nextRevision < 0) {
        throw const FormatException('The shared-board revision is invalid.');
      }
      // The server omits the board when nothing else changed, so an idle poll
      // or an uncontended edit costs no decoding or comparison work.
      final List<CanvasShape> serverShapes;
      if (sent.isEmpty && data['unchanged'] == true) {
        serverShapes = _syncedShapes;
      } else if (sent.isNotEmpty && data['ack'] == true) {
        final acknowledged = {
          for (final shape in _syncedShapes) shape.id: shape,
        };
        for (final entry in sent.entries) {
          final shape = entry.value.shape;
          if (shape == null) {
            acknowledged.remove(entry.key);
          } else {
            acknowledged[entry.key] = shape;
          }
        }
        serverShapes = acknowledged.values.toList();
      } else {
        serverShapes = _shapes(data);
      }
      _syncedShapes = List<CanvasShape>.unmodifiable(serverShapes);
      for (final entry in sent.entries) {
        if (_pending[entry.key]?.generation == entry.value.generation) {
          _pending.remove(entry.key);
        }
      }
      // Reuse existing instances for unchanged shapes so cached geometry and
      // identity comparisons stay valid.
      final remote = <String, CanvasShape>{};
      for (final shape in serverShapes) {
        final current = _view[shape.id];
        remote[shape.id] = current != null && _same(current, shape)
            ? current
            : shape;
      }
      for (final entry in _pending.entries) {
        if (entry.value.shape == null) {
          remote.remove(entry.key);
        } else {
          remote[entry.key] = entry.value.shape!;
        }
      }
      revision = nextRevision;
      final changed = !_sameDocument(_view, remote);
      if (changed) {
        _view = remote;
        applyingRemote = true;
        try {
          _onRemote?.call(List<CanvasShape>.unmodifiable(_view.values));
        } finally {
          applyingRemote = false;
        }
      }
      _setStatus(_pending.isEmpty ? 'Shared • synced' : 'Syncing…');
    } catch (error) {
      if (!_disposed) {
        _setStatus(
          error is CollaborationException &&
                  (error.statusCode == 401 || error.statusCode == 403)
              ? 'Access denied • check the invite link'
              : 'Offline • changes queued',
        );
      }
    }
  }

  void _setStatus(String value) {
    if (_disposed || status == value) {
      return;
    }
    status = value;
    notifyListeners();
  }

  static bool _same(CanvasShape a, CanvasShape b) {
    if (identical(a, b)) return true;
    if (a.id != b.id ||
        a.tool != b.tool ||
        a.color != b.color ||
        a.width != b.width ||
        a.filled != b.filled ||
        a.text != b.text ||
        a.fontSize != b.fontSize ||
        a.points.length != b.points.length) {
      return false;
    }
    for (var i = 0; i < a.points.length; i++) {
      if (a.points[i] != b.points[i]) return false;
    }
    return true;
  }

  static bool _sameDocument(
    Map<String, CanvasShape> a,
    Map<String, CanvasShape> b,
  ) {
    if (a.length != b.length) {
      return false;
    }
    // Shape order is meaningful for overlap, so compare insertion order too.
    final aEntries = a.entries.toList(), bEntries = b.entries.toList();
    for (var i = 0; i < aEntries.length; i++) {
      if (aEntries[i].key != bEntries[i].key ||
          !_same(aEntries[i].value, bEntries[i].value)) {
        return false;
      }
    }
    return true;
  }

  static List<CanvasShape> _shapes(Map<String, dynamic> data) {
    final raw = data['shapes'];
    if (raw is! List || raw.length > 10000) {
      throw const FormatException('The shared-board response is invalid.');
    }
    final shapes = raw
        .map(
          (shape) =>
              CanvasShape.fromJson(Map<String, dynamic>.from(shape as Map)),
        )
        .toList();
    if (shapes.map((shape) => shape.id).toSet().length != shapes.length) {
      throw const FormatException('The shared board has duplicate shapes.');
    }
    return shapes;
  }

  static Future<Map<String, dynamic>> _request(
    HttpClient client,
    CollaborationFetch? fetch,
    String method,
    Uri uri,
    Map<String, String> headers,
    Map<String, dynamic>? body,
  ) async {
    if (fetch != null) {
      return fetch(
        method,
        uri,
        headers,
        body,
      ).timeout(const Duration(seconds: 20));
    }
    final request = await client
        .openUrl(method, uri)
        .timeout(const Duration(seconds: 15));
    request.followRedirects = false;
    request.headers.set(HttpHeaders.acceptHeader, 'application/json');
    request.headers.set(HttpHeaders.cacheControlHeader, 'no-cache');
    headers.forEach(request.headers.set);
    if (body != null) {
      request.headers.contentType = ContentType.json;
      request.write(jsonEncode(body));
    }
    final response = await request.close().timeout(const Duration(seconds: 20));
    final bytes = <int>[];
    await for (final chunk in response.timeout(const Duration(seconds: 20))) {
      bytes.addAll(chunk);
      if (bytes.length > 8 * 1024 * 1024) {
        throw const FormatException('This shared board is too large.');
      }
    }
    if (response.statusCode < 200 || response.statusCode >= 300) {
      throw CollaborationException(
        'The shared board could not be synchronized.',
        statusCode: response.statusCode,
      );
    }
    final decoded = jsonDecode(utf8.decode(bytes));
    if (decoded is! Map<String, dynamic>) {
      throw const FormatException('The shared-board response is invalid.');
    }
    return decoded;
  }

  void close() => dispose();
  @override
  void dispose() {
    if (_disposed) {
      return;
    }
    _disposed = true;
    _timer?.cancel();
    _onRemote = null;
    if (_ownsClient) {
      _client.close(force: true);
    }
    super.dispose();
  }
}
