import 'dart:convert';
import 'dart:io';

import 'package:canvas_studio/board_store.dart';
import 'package:canvas_studio/canvas_document.dart';
import 'package:flutter_test/flutter_test.dart';

BoardData board(String id, {String title = 'Ideas', DateTime? updated}) =>
    BoardData(
      id: id,
      title: title,
      updated: updated ?? DateTime.utc(2026, 11, 16),
      shapes: [
        CanvasShape(
          id: 'stroke-$id',
          tool: CanvasTool.pen,
          points: const [Offset(-15, 2), Offset(3, 20), Offset(31, 5)],
          color: 0xff6254d9,
          width: 5,
        ),
        CanvasShape(
          id: 'text-$id',
          tool: CanvasTool.text,
          points: const [Offset(50, 80)],
          text: 'Sketch → plan\nनमस्ते',
        ),
      ],
    );

void main() {
  late Directory temp;
  late BoardStore store;

  setUp(() async {
    temp = await Directory.systemTemp.createTemp('canvas-studio-store-test-');
    store = BoardStore(directory: temp);
  });

  tearDown(() async {
    await temp.delete(recursive: true);
  });

  test('Saved board reopens with its complete native drawing', () async {
    final original = board('a', title: 'Project notes');
    await store.save(original);

    // A new store instance verifies persistence independently of memory state.
    final reopened = await BoardStore(directory: temp).list();
    expect(reopened, hasLength(1));
    expect(reopened.single.toJson(), original.toJson());
    final document = CanvasDocument()..load(reopened.single.shapes);
    expect(document.shapes.last.text, 'Sketch → plan\nनमस्ते');
    expect(document.shapes.first.points.first, const Offset(-15, 2));
    expect(document.canUndo, isFalse);
  });

  test(
    'Atomic overwrite replaces content without leaving a temporary file',
    () async {
      final original = board('a');
      await store.save(original);
      original.title = 'Updated board';
      original.updated = DateTime.utc(2026, 11, 17);
      original.shapes = [original.shapes.first.copyWith(color: 0xffe5484d)];
      await store.save(original);

      final files = await temp.list().toList();
      expect(files, hasLength(1));
      expect(files.single.path.endsWith('a.json'), isTrue);
      expect(await File('${temp.path}/a.tmp').exists(), isFalse);
      final diskContent = jsonDecode(
        await File('${temp.path}/a.json').readAsString(),
      );
      expect(diskContent, original.toJson());
      final reopened = await store.list();
      expect(reopened.single.title, 'Updated board');
      expect(reopened.single.shapes, hasLength(1));
      expect(reopened.single.shapes.single.color, 0xffe5484d);
    },
  );

  test(
    'Deletion removes only the selected board and is safe to repeat',
    () async {
      final first = board('a');
      final second = board('b');
      await store.save(first);
      await store.save(second);
      await store.delete(first);
      await store.delete(first);

      expect(await File('${temp.path}/a.json').exists(), isFalse);
      final remaining = await BoardStore(directory: temp).list();
      expect(remaining.map((value) => value.id), ['b']);
      expect(remaining.single.toJson(), second.toJson());
    },
  );

  test('Library lists the most recently edited board first', () async {
    await store.save(board('older', updated: DateTime.utc(2026, 11, 15)));
    await store.save(board('newer', updated: DateTime.utc(2026, 11, 17)));
    await store.save(board('middle', updated: DateTime.utc(2026, 11, 16)));
    expect((await store.list()).map((value) => value.id), [
      'newer',
      'middle',
      'older',
    ]);
  });
}
