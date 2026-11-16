import 'package:canvas_studio/canvas_document.dart';
import 'package:flutter_test/flutter_test.dart';

CanvasShape rectangle(String id, {bool filled = false}) => CanvasShape(
  id: id,
  tool: CanvasTool.rectangle,
  points: const [Offset.zero, Offset(100, 100)],
  filled: filled,
);

void main() {
  test('Undo restores edits and deletion; new edit discards redo', () {
    final document = CanvasDocument();
    final shape = rectangle('a');
    document.add(shape);
    document.replace(shape.copyWith(color: 0xff00ff00));
    document.remove('a');
    document.undo();
    expect(document.shapes.single.color, 0xff00ff00);
    document.undo();
    expect(document.shapes.single.color, shape.color);
    document.redo();
    expect(document.shapes.single.color, 0xff00ff00);
    document.undo();
    document.add(rectangle('b'));
    expect(document.canRedo, isFalse);
  });

  test(
    'JSON round-trip preserves text, fill, negative coordinates and stroke',
    () {
      final document = CanvasDocument();
      document.add(
        CanvasShape(
          id: 'text',
          tool: CanvasTool.text,
          points: const [Offset(-20, 35)],
          text: 'Hello\nworld',
          width: 5,
          color: 0xff123456,
          filled: true,
        ),
      );
      final restored = CanvasDocument()
        ..load(CanvasDocument.decode(document.encode()));
      expect(restored.shapes.single.toJson(), document.shapes.single.toJson());
      expect(restored.canUndo, isFalse);
      expect(() => restored.shapes.clear(), throwsUnsupportedError);
      expect(
        () => restored.shapes.single.points.clear(),
        throwsUnsupportedError,
      );
    },
  );

  test('Hit tests outlines, filled interiors and topmost shape', () {
    final document = CanvasDocument()..add(rectangle('outline'));
    expect(document.hitTest(const Offset(50, 50)), isNull);
    expect(document.hitTest(const Offset(2, 50))?.id, 'outline');
    document.add(rectangle('filled', filled: true));
    expect(document.hitTest(const Offset(50, 50))?.id, 'filled');
    expect(document.hitTest(const Offset(2, 50))?.id, 'filled');
  });

  test('Line hit testing uses segments rather than bounding rectangles', () {
    final document = CanvasDocument()
      ..add(
        CanvasShape(
          id: 'line',
          tool: CanvasTool.pen,
          points: const [Offset.zero, Offset(100, 100)],
        ),
      );
    expect(document.hitTest(const Offset(50, 52))?.id, 'line');
    expect(document.hitTest(const Offset(10, 90)), isNull);
  });

  test('Ellipse outline excludes its center, fill includes it', () {
    final document = CanvasDocument();
    final ellipse = CanvasShape(
      id: 'ellipse',
      tool: CanvasTool.ellipse,
      points: const [Offset.zero, Offset(100, 60)],
    );
    document.add(ellipse);
    expect(document.hitTest(const Offset(50, 30)), isNull);
    expect(document.hitTest(const Offset(99, 30))?.id, 'ellipse');
    document.replace(ellipse.copyWith(filled: true));
    expect(document.hitTest(const Offset(50, 30))?.id, 'ellipse');
  });

  test('History is bounded and removing selection clears it', () {
    final document = CanvasDocument();
    for (var i = 0; i < 105; i++) {
      document.add(rectangle('$i'));
    }
    for (var i = 0; i < 100; i++) {
      document.undo();
    }
    expect(document.shapes.length, 5);
    expect(document.canUndo, isFalse);
    document.select('0');
    document.remove('0');
    expect(document.selectedId, isNull);
  });
}
