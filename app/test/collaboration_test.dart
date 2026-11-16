import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:canvas_studio/canvas_document.dart';
import 'package:canvas_studio/collaboration.dart';

final link = CollaborationLink(
  server: Uri.parse('https://boards.example.test'),
  roomId: 'room1234',
  token: 'private_token_12345678',
);
CanvasShape shape(String id, [double x = 0]) => CanvasShape(
  id: id,
  tool: CanvasTool.pen,
  points: [Offset(x, 0), Offset(x + 10, 10)],
);
Map<String, dynamic> snapshot(List<CanvasShape> shapes, [int revision = 1]) => {
  'roomId': link.roomId,
  'revision': revision,
  'name': 'Ideas',
  'shapes': shapes.map((s) => s.toJson()).toList(),
};

void main() {
  test('Invite and persisted links round-trip without changing capability', () {
    final restored = CollaborationLink.parseInvite(link.inviteUri.toString());
    expect(restored.server, link.server);
    expect(restored.roomId, link.roomId);
    expect(restored.token, link.token);
    expect(
      CollaborationLink.fromJson(link.toJson()).mcpUri.path,
      '/mcp/room1234/private_token_12345678',
    );
    expect(
      () => CollaborationLink.parseInvite(
        'http://boards.example.test/join?room=room1234&key=private_token_12345678',
      ),
      throwsFormatException,
    );
    expect(
      () => CollaborationLink.parseInvite(
        'https://user:password@boards.example.test/join?room=room1234&key=private_token_12345678',
      ),
      throwsFormatException,
    );
    expect(
      () => CollaborationLink.parseInvite('${link.inviteUri}&room=other'),
      throwsFormatException,
    );
    expect(
      () => CollaborationLink.parseInvite(
        'https://boards.example.test/join?room=room1234',
      ),
      throwsFormatException,
    );
  });

  test('Create and join pass the shared-board API contract', () async {
    final created = await CollaborationClient.create(
      server: link.server,
      name: 'Ideas',
      shapes: [shape('first')],
      clientId: 'device1',
      fetch: (method, uri, headers, body) async {
        expect(method, 'POST');
        expect(uri.path, '/api/rooms');
        expect(body!['name'], 'Ideas');
        return {
          ...snapshot([shape('first')]),
          'token': link.token,
        };
      },
    );
    expect(created.link.token, link.token);
    expect(created.shapes.single.id, 'first');
    final joined = await CollaborationClient.join(
      invite: link.joinCode,
      clientId: 'device2',
      fetch: (method, uri, headers, body) async {
        expect(method, 'GET');
        expect(headers['Authorization'], 'Bearer ${link.token}');
        return snapshot([shape('first')]);
      },
    );
    expect(joined.shapes.single.id, 'first');
  });

  test('Pending local edits survive an in-flight remote snapshot', () async {
    final response = Completer<Map<String, dynamic>>();
    final client = CollaborationClient(
      link: link,
      clientId: 'device1',
      pollInterval: const Duration(days: 1),
      fetch: (_, _, _, _) => response.future,
    );
    addTearDown(client.dispose);
    List<CanvasShape> visible = [shape('local')];
    client.start(
      initial: visible,
      onRemote: (shapes) {
        visible = shapes;
        client.submit(shapes);
      },
    );
    client.submit([shape('local', 20)]);
    response.complete(snapshot([shape('local'), shape('remote', 50)]));
    await client.flush();
    expect(visible.map((s) => s.id), containsAll(['local', 'remote']));
    expect(visible.firstWhere((s) => s.id == 'local').points.first.dx, 20);
    expect(client.pendingCount, 1);
  });

  test('Newer edit during POST remains queued after acknowledgement', () async {
    var calls = 0;
    final response = Completer<Map<String, dynamic>>();
    final client = CollaborationClient(
      link: link,
      clientId: 'device1',
      pollInterval: const Duration(days: 1),
      fetch: (method, uri, headers, body) async {
        calls++;
        if (calls == 1) {
          return snapshot([shape('a')]);
        }
        expect(method, 'POST');
        return response.future;
      },
    );
    addTearDown(client.dispose);
    List<CanvasShape> visible = [shape('a')];
    client.start(initial: visible, onRemote: (s) => visible = s);
    await client.flush();
    client.submit([shape('a', 10)]);
    final flush = client.flush();
    client.submit([shape('a', 20)]);
    response.complete(snapshot([shape('a', 10)], 2));
    await flush;
    expect(client.pendingCount, 1);
    // The local edit is already visible in the caller; a remote callback is only
    // needed when other server content differs. It must never be overwritten.
    expect(client.status, 'Syncing…');
  });

  test(
    'Failed writes retain operations and retry deletes without resurrection',
    () async {
      var fail = false;
      var serverShapes = [shape('a'), shape('b')];
      List<Map<String, dynamic>> lastOps = [];
      final client = CollaborationClient(
        link: link,
        clientId: 'device1',
        pollInterval: const Duration(days: 1),
        fetch: (method, uri, headers, body) async {
          if (fail) {
            throw const CollaborationException('offline');
          }
          if (method == 'POST') {
            lastOps = (body!['operations'] as List)
                .cast<Map<String, dynamic>>();
            for (final op in lastOps) {
              if (op['type'] == 'delete') {
                serverShapes.removeWhere((s) => s.id == op['id']);
              }
            }
          }
          return snapshot(serverShapes);
        },
      );
      addTearDown(client.dispose);
      client.start(initial: serverShapes, onRemote: (_) {});
      await client.flush();
      client.submit([shape('b')]);
      fail = true;
      await client.flush();
      expect(client.pendingCount, 1);
      expect(client.status, 'Offline • changes queued');
      fail = false;
      await client.flush();
      expect(lastOps.single, {'type': 'delete', 'id': 'a'});
      expect(client.pendingCount, 0);
      expect(serverShapes.map((s) => s.id), ['b']);
    },
  );

  test('Disposal prevents late remote callbacks', () async {
    final response = Completer<Map<String, dynamic>>();
    final client = CollaborationClient(
      link: link,
      clientId: 'device1',
      pollInterval: const Duration(days: 1),
      fetch: (_, _, _, _) => response.future,
    );
    var notified = false;
    client.start(initial: [], onRemote: (_) => notified = true);
    final flush = client.flush();
    client.dispose();
    response.complete(snapshot([shape('remote')]));
    await flush;
    expect(notified, isFalse);
  });

  test(
    'Persisted baseline reconstructs offline mutations after restart',
    () async {
      List<Map<String, dynamic>> operations = [];
      final client = CollaborationClient(
        link: link,
        clientId: 'device1',
        pollInterval: const Duration(days: 1),
        fetch: (method, uri, headers, body) async {
          expect(method, 'POST');
          operations = (body!['operations'] as List)
              .cast<Map<String, dynamic>>();
          return snapshot([shape('a', 20), shape('remote', 50)], 4);
        },
      );
      addTearDown(client.dispose);
      List<CanvasShape> visible = [shape('a', 20)];
      client.start(
        initial: visible,
        lastSynced: [shape('a'), shape('deleted')],
        onRemote: (s) => visible = s,
      );
      await client.flush();
      expect(
        operations.map((op) => op['type']),
        containsAll(['put', 'delete']),
      );
      expect(
        operations.singleWhere((op) => op['type'] == 'delete')['id'],
        'deleted',
      );
      expect(client.pendingCount, 0);
      expect(client.syncedShapes.map((s) => s.id), ['a', 'remote']);
      expect(visible.map((s) => s.id), ['a', 'remote']);
    },
  );
}
