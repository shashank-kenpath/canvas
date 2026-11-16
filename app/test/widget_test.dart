import 'dart:convert';

import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:canvas_studio/main.dart';
import 'package:canvas_studio/board_store.dart';
import 'package:canvas_studio/canvas_document.dart';
import 'package:canvas_studio/share_panel.dart';

class MemoryStore extends BoardStore {
  final Map<String, BoardData> boards = {};
  @override
  Future<void> save(BoardData board) async {
    boards[board.id] = BoardData.fromJson(
      jsonDecode(jsonEncode(board.toJson())) as Map<String, dynamic>,
    );
  }

  @override
  Future<List<BoardData>> list() async => boards.values.toList();
}

BoardData emptyBoard([String id = 'test']) => BoardData(
  id: id,
  title: 'Native board',
  updated: DateTime.now(),
  shapes: [],
);

const canvasKey = ValueKey('native-canvas');

Future<NativeEditorState> open(
  WidgetTester tester,
  BoardStore store,
  BoardData board, {
  Size size = const Size(1280, 800),
  InkRecognizer? recognize,
  Future<Map<String, dynamic>> Function(
    String,
    Uri,
    Map<String, String>,
    Map<String, dynamic>?,
  )?
  fetch,
}) async {
  tester.view.physicalSize = size;
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.resetPhysicalSize);
  addTearDown(tester.view.resetDevicePixelRatio);
  await tester.pumpWidget(
    MaterialApp(
      home: NativeEditor(
        board: board,
        store: store,
        recognize: recognize,
        collaborationFetch: fetch,
      ),
    ),
  );
  await tester.pumpAndSettle();
  return tester.state<NativeEditorState>(find.byType(NativeEditor));
}

Future<void> close(WidgetTester tester) async {
  await tester.pump(const Duration(milliseconds: 500));
  await tester.pumpWidget(const SizedBox());
  await tester.pump(const Duration(seconds: 2));
}

/// Draws one pen stroke through [points] (canvas-local offsets).
Future<void> stroke(
  WidgetTester tester,
  List<Offset> points, {
  Duration hold = Duration.zero,
}) async {
  final origin = tester.getRect(find.byKey(canvasKey)).topLeft;
  final gesture = await tester.startGesture(origin + points.first);
  for (final point in points.skip(1)) {
    await gesture.moveTo(origin + point);
  }
  await tester.pump();
  if (hold > Duration.zero) await tester.pump(hold);
  await gesture.up();
  await tester.pump();
}

void main() {
  for (final size in [const Size(390, 844), const Size(1280, 800)]) {
    testWidgets('Editor fits $size and a tap leaves a dot', (tester) async {
      final editor = await open(
        tester,
        MemoryStore(),
        emptyBoard(),
        size: size,
      );
      await tester.tapAt(tester.getCenter(find.byKey(canvasKey)));
      await tester.pump(const Duration(milliseconds: 400));
      expect(editor.document.shapes.single.tool, CanvasTool.pen);
      // Selecting ink shows its actions without overflowing a phone.
      editor.document.selectMany(editor.document.shapes.map((s) => s.id));
      await tester.pump();
      expect(find.byTooltip('Convert handwriting to text'), findsOneWidget);
      expect(tester.takeException(), isNull);
      await close(tester);
    });
  }

  testWidgets('Draw, undo, redo, save and reopen a board', (tester) async {
    final store = MemoryStore();
    var editor = await open(tester, store, emptyBoard());
    await tester.tap(find.byKey(const ValueKey('tool-rectangle')));
    final canvas = tester.getRect(find.byKey(canvasKey));
    await tester.dragFrom(
      canvas.center - const Offset(80, 50),
      const Offset(130, 100),
    );
    await tester.pump();
    expect(editor.document.shapes.single.tool, CanvasTool.rectangle);
    // Like tldraw, the new shape is selected and the tool returns to Select.
    expect(editor.tool, CanvasTool.select);
    expect(editor.document.selectedIds, hasLength(1));
    await tester.tap(find.byTooltip('Undo'));
    await tester.pump();
    expect(editor.document.shapes, isEmpty);
    await tester.tap(find.byTooltip('Redo'));
    await tester.pump();
    expect(editor.document.shapes, hasLength(1));
    await tester.pump(const Duration(milliseconds: 900));
    await tester.pump();
    expect(find.text('Saved'), findsOneWidget);
    final boards = await store.list();
    expect(boards.single.shapes, hasLength(1));
    await close(tester);
    editor = await open(tester, store, boards.single);
    expect(editor.document.shapes.single.tool, CanvasTool.rectangle);
    await close(tester);
  });

  testWidgets('A pen stroke keeps every sample and ignores a resting palm', (
    tester,
  ) async {
    final editor = await open(tester, MemoryStore(), emptyBoard());
    final origin = tester.getRect(find.byKey(canvasKey)).topLeft;
    final pen = await tester.startGesture(origin + const Offset(300, 300));
    for (var i = 1; i <= 20; i++) {
      await pen.moveTo(origin + Offset(300 + i * 8, 300 + (i.isEven ? 6 : -6)));
    }
    await tester.pump(const Duration(milliseconds: 300));
    // A palm landing mid-stroke must not turn the stroke into a pinch.
    final palm = await tester.startGesture(
      origin + const Offset(700, 600),
      pointer: 9,
    );
    await palm.moveBy(const Offset(30, 10));
    for (var i = 21; i <= 30; i++) {
      await pen.moveTo(origin + Offset(300 + i * 8, 300));
    }
    await palm.up();
    await pen.up();
    await tester.pump();
    final shape = editor.document.shapes.single;
    expect(shape.tool, CanvasTool.pen);
    expect(shape.points, hasLength(31));
    expect(editor.zoom, 1);
    expect(editor.camera, Offset.zero);
    expect(tester.takeException(), isNull);
    await close(tester);
  });

  testWidgets('Two fingers pinch to zoom without drawing', (tester) async {
    final editor = await open(tester, MemoryStore(), emptyBoard());
    final origin = tester.getRect(find.byKey(canvasKey)).topLeft;
    final a = await tester.startGesture(origin + const Offset(500, 400));
    final b = await tester.startGesture(
      origin + const Offset(600, 400),
      pointer: 5,
    );
    await a.moveTo(origin + const Offset(400, 400));
    await b.moveTo(origin + const Offset(700, 400));
    await tester.pump();
    await a.up();
    await b.up();
    await tester.pump();
    expect(editor.zoom, closeTo(3, .01));
    expect(editor.document.shapes, isEmpty);
    await close(tester);
  });

  testWidgets('Holding the pen still snaps a rough box to a rectangle', (
    tester,
  ) async {
    final editor = await open(tester, MemoryStore(), emptyBoard());
    final box = <Offset>[
      for (var x = 300.0; x <= 500; x += 10) Offset(x, 300),
      for (var y = 310.0; y <= 420; y += 10) Offset(500, y),
      for (var x = 490.0; x >= 300; x -= 10) Offset(x, 420),
      for (var y = 410.0; y >= 300; y -= 10) Offset(300, y),
    ];
    await stroke(tester, box, hold: const Duration(milliseconds: 700));
    expect(editor.document.shapes.single.tool, CanvasTool.rectangle);
    // Without the pause the same stroke stays as handwriting.
    await stroke(tester, [for (final p in box) p + const Offset(260, 0)]);
    expect(editor.document.shapes.last.tool, CanvasTool.pen);
    // Undo brings back nothing surprising: one step per stroke.
    editor.document.undo();
    editor.document.undo();
    expect(editor.document.shapes, isEmpty);
    await close(tester);
  });

  testWidgets('Write tool turns handwriting into typed text after a pause', (
    tester,
  ) async {
    final editor = await open(
      tester,
      MemoryStore(),
      emptyBoard(),
      recognize: (strokes, {required language}) async => ['hello'],
    );
    await tester.tap(find.byKey(const ValueKey('tool-write')));
    await tester.pump();
    expect(find.byKey(const ValueKey('pen-mode-hint')), findsOneWidget);
    await stroke(tester, const [
      Offset(300, 300),
      Offset(310, 340),
      Offset(320, 300),
    ]);
    await tester.pump(const Duration(milliseconds: 500));
    // Still writing: the next stroke restarts the wait.
    await stroke(tester, const [
      Offset(340, 300),
      Offset(350, 340),
      Offset(360, 300),
    ]);
    await tester.pump(const Duration(milliseconds: 600));
    expect(editor.document.shapes.map((s) => s.tool).toSet(), {CanvasTool.pen});
    await tester.pump(const Duration(milliseconds: 500));
    await tester.pump(const Duration(milliseconds: 100));
    expect(editor.document.shapes.single.tool, CanvasTool.text);
    expect(editor.document.shapes.single.text, 'hello');
    await tester.tap(find.byKey(const ValueKey('tool-pen')));
    await tester.pump();
    await close(tester);
  });

  testWidgets('Smart shape tool cleans up a rough box when the pen lifts', (
    tester,
  ) async {
    final editor = await open(tester, MemoryStore(), emptyBoard());
    await tester.tap(find.byKey(const ValueKey('tool-smart')));
    await tester.pump();
    // A wobbly, slightly open box drawn in one go, with no pause.
    final box = <Offset>[
      for (var x = 300.0; x <= 500; x += 10) Offset(x, 300 + (x % 20) / 4),
      for (var y = 310.0; y <= 420; y += 10) Offset(500 - (y % 20) / 4, y),
      for (var x = 490.0; x >= 300; x -= 10) Offset(x, 420 - (x % 20) / 4),
      for (var y = 410.0; y >= 318; y -= 10) Offset(300 + (y % 20) / 4, y),
    ];
    await stroke(tester, box);
    expect(editor.document.shapes.single.tool, CanvasTool.rectangle);
    // Writing-like marks are left alone.
    await stroke(tester, const [
      Offset(600, 300),
      Offset(620, 360),
      Offset(640, 300),
      Offset(660, 360),
    ]);
    expect(editor.document.shapes.last.tool, CanvasTool.pen);
    expect(editor.document.shapes.last.points.length, greaterThan(2));
    await tester.tap(find.byKey(const ValueKey('tool-pen')));
    await tester.pump();
    await close(tester);
  });

  testWidgets('Selected handwriting converts to text and undo restores ink', (
    tester,
  ) async {
    final seen = <int>[];
    final editor = await open(
      tester,
      MemoryStore(),
      emptyBoard(),
      recognize: (strokes, {required language}) async {
        seen.add(strokes.length);
        return [language == 'en' ? 'hello' : 'नमस्ते'];
      },
    );
    await stroke(tester, const [
      Offset(300, 300),
      Offset(310, 340),
      Offset(320, 300),
    ]);
    await stroke(tester, const [
      Offset(340, 300),
      Offset(350, 340),
      Offset(360, 300),
    ]);
    // A second line of writing well below the first.
    await stroke(tester, const [
      Offset(300, 420),
      Offset(310, 460),
      Offset(320, 420),
    ]);
    expect(editor.document.shapes, hasLength(3));
    await tester.tap(find.byKey(const ValueKey('tool-select')));
    await tester.pump();
    // Marquee-select all three strokes.
    await tester.dragFrom(
      tester.getRect(find.byKey(canvasKey)).topLeft + const Offset(250, 250),
      const Offset(200, 260),
    );
    await tester.pump();
    expect(editor.document.selectedIds, hasLength(3));
    await tester.tap(find.byTooltip('Convert handwriting to text'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    expect(seen, [2, 1]);
    final texts = editor.document.shapes;
    expect(texts.map((s) => s.tool).toSet(), {CanvasTool.text});
    expect(texts.map((s) => s.text), ['hello', 'hello']);
    expect(texts.first.points.first.dy, lessThan(texts.last.points.first.dy));
    await tester.tap(find.byTooltip('Undo'));
    await tester.pump();
    expect(editor.document.shapes.map((s) => s.tool).toSet(), {CanvasTool.pen});
    expect(editor.document.shapes, hasLength(3));
    await close(tester);
  });

  testWidgets('Eraser removes touched shapes as one undo step; move works', (
    tester,
  ) async {
    final editor = await open(tester, MemoryStore(), emptyBoard());
    final origin = tester.getRect(find.byKey(canvasKey)).topLeft;
    await stroke(tester, const [Offset(300, 300), Offset(400, 300)]);
    await stroke(tester, const [Offset(300, 340), Offset(400, 340)]);
    await stroke(tester, const [Offset(300, 500), Offset(400, 500)]);
    await tester.tap(find.byKey(const ValueKey('tool-eraser')));
    await tester.pump();
    await tester.dragFrom(origin + const Offset(350, 280), const Offset(0, 80));
    await tester.pump();
    expect(editor.document.shapes, hasLength(1));
    editor.document.undo();
    expect(editor.document.shapes, hasLength(3));
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('tool-select')));
    await tester.pump();
    final before = editor.document.shapes.last.points.first;
    await tester.dragFrom(
      origin + const Offset(350, 500),
      const Offset(60, 40),
    );
    await tester.pump();
    final after = editor.document.shapes.last.points.first;
    expect((after - before).dx, closeTo(60, 1));
    expect((after - before).dy, closeTo(40, 1));
    expect(tester.takeException(), isNull);
    await close(tester);
  });

  testWidgets('Sharing shows a board code and Claude setup, then syncs edits', (
    tester,
  ) async {
    final calls = <String>[];
    final sent = <Map<String, dynamic>>[];
    var revision = 1;
    const room = 'abcdefabcdef';
    Future<Map<String, dynamic>> fetch(
      String method,
      Uri uri,
      Map<String, String> headers,
      Map<String, dynamic>? body,
    ) async {
      calls.add('$method ${uri.path}');
      if (uri.path == '/api/rooms') {
        return {
          'roomId': room,
          'token': 'k' * 43,
          'name': body!['name'],
          'revision': revision,
          'shapes': body['shapes'],
        };
      }
      if (uri.path.endsWith('/code')) {
        return {'code': '123456', 'expiresInSeconds': 600};
      }
      if (uri.path.endsWith('/ops')) {
        sent.add(body!);
        return {'roomId': room, 'revision': ++revision, 'ack': true};
      }
      return {'roomId': room, 'revision': revision, 'unchanged': true};
    }

    final store = MemoryStore();
    final editor = await open(tester, store, emptyBoard(), fetch: fetch);
    await stroke(tester, const [Offset(300, 300), Offset(400, 300)]);
    await tester.tap(find.byKey(const ValueKey('share-button')));
    await tester.pumpAndSettle();
    await tester.tap(find.byKey(const ValueKey('start-sharing')));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 100));
    await tester.pump(const Duration(milliseconds: 100));
    expect(find.text('123 456'), findsOneWidget);
    // Sharing with people is by code only: no links, no Claude setup.
    expect(find.byKey(const ValueKey('mcp-url')), findsNothing);
    expect(find.textContaining('http'), findsNothing);
    expect(editor.isShared, isTrue);
    await tester.tap(find.text('Done'));
    await tester.pumpAndSettle();
    // Claude has its own entry in the menu.
    await tester.tap(find.byTooltip('Menu'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Connect Claude…'));
    await tester.pumpAndSettle();
    final mcp = tester.widget<Text>(find.byKey(const ValueKey('mcp-url'))).data;
    expect(mcp, endsWith('/mcp/$room/${'k' * 43}'));
    await tester.tap(find.text('Done'));
    await tester.pumpAndSettle();
    expect(find.text('Shared'), findsOneWidget);
    await stroke(tester, const [Offset(300, 400), Offset(400, 400)]);
    await tester.pump(const Duration(milliseconds: 1200));
    await tester.pump(const Duration(milliseconds: 1200));
    expect(calls, contains('POST /api/rooms/$room/ops'));
    expect((sent.single['operations'] as List).single['type'], 'put');
    expect(editor.status, 'Shared • synced');
    final saved = (await store.list()).single;
    expect(saved.share!['roomId'], room);
    expect(saved.synced, hasLength(2));
    await close(tester);
  });

  testWidgets('A board code joins a shared board and stores it locally', (
    tester,
  ) async {
    final store = MemoryStore();
    BoardData? joined;
    await tester.pumpWidget(
      MaterialApp(
        home: Builder(
          builder: (context) => TextButton(
            onPressed: () async => joined = await showJoinDialog(
              context,
              store,
              fetch: (method, uri, headers, body) async {
                expect(uri.path, '/api/join-code');
                expect(body, {'code': '654321'});
                return {
                  'roomId': 'abcdefabcdef',
                  'token': 'k' * 43,
                  'name': 'Wall display',
                  'revision': 4,
                  'shapes': [
                    {
                      'id': 'a',
                      'tool': 'text',
                      'points': [
                        [1, 2],
                      ],
                      'text': 'Hi',
                      'fontSize': 40,
                    },
                  ],
                };
              },
            ),
            child: const Text('open'),
          ),
        ),
      ),
    );
    await tester.tap(find.text('open'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byKey(const ValueKey('join-input')), '654 321');
    await tester.tap(find.byKey(const ValueKey('join-confirm')));
    await tester.pumpAndSettle();
    expect(joined!.title, 'Wall display');
    expect(joined!.shapes.single.fontSize, 40);
    expect((await store.list()).single.share!['roomId'], 'abcdefabcdef');
  });
}
