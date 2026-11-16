import 'dart:math' as math;

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:canvas_studio/canvas_document.dart';
import 'package:canvas_studio/ink_recognition.dart';

CanvasShape ink(List<Offset> points) => CanvasShape(
  id: 'ink',
  tool: CanvasTool.pen,
  points: points,
  color: 0xffc03030,
  width: 6,
);
List<Offset> segment(Offset a, Offset b) =>
    List.generate(21, (i) => a + (b - a) * (i / 20));

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  test('Recognizes a closed rectangle without changing identity or style', () {
    final stroke = ink([
      ...segment(const Offset(20, 30), const Offset(160, 30)),
      ...segment(const Offset(160, 30), const Offset(160, 110)),
      ...segment(const Offset(160, 110), const Offset(20, 110)),
      ...segment(const Offset(20, 110), const Offset(20, 30)),
    ]);
    final shape = recognizeShape(stroke)!;
    expect(shape.tool, CanvasTool.rectangle);
    expect(shape.id, stroke.id);
    expect(shape.color, stroke.color);
    expect(shape.width, stroke.width);
    expect(shape.points, [const Offset(20, 30), const Offset(160, 110)]);
  });
  test('Recognizes a circle and does not mistake it for a rectangle', () {
    final stroke = ink(
      List.generate(101, (i) {
        final angle = i * math.pi * 2 / 100;
        return Offset(100 + math.cos(angle) * 60, 120 + math.sin(angle) * 60);
      }),
    );
    expect(recognizeShape(stroke)!.tool, CanvasTool.ellipse);
  });
  test('Straightens a line but never adds an arrowhead to it', () {
    final stroke = ink(
      List.generate(80, (i) => Offset(i * 3.0, i * 1.5 + math.sin(i) * .4)),
    );
    final shape = recognizeShape(stroke)!;
    expect(shape.tool, CanvasTool.pen);
    expect(shape.points.length, 2);
    expect(shape.points.first, stroke.points.first);
    expect(shape.points.last, stroke.points.last);
  });
  test('Recognizes a clearly traced arrow', () {
    final stroke = ink([
      ...segment(Offset.zero, const Offset(200, 0)),
      ...segment(const Offset(200, 0), const Offset(150, -35)),
      ...segment(const Offset(150, -35), const Offset(200, 0)),
      ...segment(const Offset(200, 0), const Offset(150, 35)),
    ]);
    expect(recognizeShape(stroke)!.tool, CanvasTool.arrow);
  });
  test(
    'Keeps scribbles, tiny marks, open curves and existing shapes unchanged',
    () {
      expect(
        recognizeShape(
          ink(const [
            Offset(0, 0),
            Offset(70, 100),
            Offset(120, 10),
            Offset(15, 80),
            Offset(150, 90),
            Offset(30, 20),
            Offset(120, 120),
          ]),
        ),
        isNull,
      );
      expect(recognizeShape(ink(const [Offset(0, 0), Offset(3, 2)])), isNull);
      expect(
        recognizeShape(
          ink(
            List.generate(
              40,
              (i) => Offset(i * 3.0, math.sin(i / 39 * math.pi) * 80),
            ),
          ),
        ),
        isNull,
      );
      expect(
        recognizeShape(
          CanvasShape(
            id: 'box',
            tool: CanvasTool.rectangle,
            points: const [Offset.zero, Offset(100, 100)],
          ),
        ),
        isNull,
      );
    },
  );
  test('Language model is lazy, English/Hindi preparation is cached, and ink order is preserved', () async {
    const channel = MethodChannel('google_mlkit_digital_ink_recognizer');
    final calls = <MethodCall>[];
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          calls.add(call);
          if (call.method == 'vision#manageInkModels') {
            final args = call.arguments as Map;
            return args['task'] == 'check' ? false : 'success';
          }
          if (call.method == 'vision#startDigitalInkRecognizer') {
            return [
              {'text': 'Hello', 'score': 0.0},
              {'text': 'Hello', 'score': 1.0},
            ];
          }
          return null;
        });
    addTearDown(
      () => TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
          .setMockMethodCallHandler(channel, null),
    );
    final recognizer = HandwritingRecognition();
    expect(calls, isEmpty);
    await recognizer.prepare('en');
    await recognizer.prepare('en-IN');
    await recognizer.prepare('hi-IN');
    final downloads = calls
        .where(
          (c) =>
              c.method == 'vision#manageInkModels' &&
              (c.arguments as Map)['task'] == 'download',
        )
        .toList();
    expect(downloads.length, 2);
    expect(downloads.map((c) => (c.arguments as Map)['model']), [
      'en-IN',
      'hi',
    ]);
    final candidates = await recognizer.recognize([
      ink(const [Offset(-5, -2), Offset(20, 30)]),
      ink(const [Offset(40, 40), Offset(50, 60)]),
    ], language: 'en');
    expect(candidates, ['Hello']);
    final arguments = calls.last.arguments as Map;
    final strokes = (arguments['ink'] as Map)['strokes'] as List;
    final points = [for (final s in strokes) ...((s as Map)['points'] as List)];
    final times = points.map((p) => (p as Map)['t'] as int).toList();
    expect(times.every((t) => t >= 0), isTrue);
    expect(times, [...times]..sort());
    expect((points.first as Map)['x'], 16);
    expect((points.first as Map)['y'], 16);
    await recognizer.dispose();
  });
}
