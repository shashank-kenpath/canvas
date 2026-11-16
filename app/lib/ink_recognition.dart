import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/services.dart';
import 'package:google_mlkit_digital_ink_recognition/google_mlkit_digital_ink_recognition.dart'
    as ml;

import 'canvas_document.dart';

class InkRecognitionException implements Exception {
  const InkRecognitionException(this.message);
  final String message;
  @override
  String toString() => message;
}

/// Models are downloaded only after [prepare] or [recognize] is requested.
/// Recognition runs on-device once the selected language model is available.
class HandwritingRecognition {
  /// One recognizer for the whole app, so a model is loaded only once.
  static final shared = HandwritingRecognition();

  final _manager = ml.DigitalInkRecognizerModelManager();
  final _recognizers = <String, ml.DigitalInkRecognizer>{};
  final _preparing = <String, Future<void>>{};
  bool _disposed = false;

  String _language(String value) => switch (value.toLowerCase()) {
    'en' || 'en-in' => 'en-IN',
    'en-us' => 'en-US',
    'en-gb' => 'en-GB',
    'hi' || 'hi-in' => 'hi',
    _ => throw const InkRecognitionException(
      'Choose English or Hindi for handwriting recognition.',
    ),
  };

  /// True when [language] can be recognized immediately.
  bool isReady(String language) =>
      _recognizers.containsKey(_language(language));

  Future<void> prepare(String language) {
    if (_disposed) {
      return Future.error(
        const InkRecognitionException(
          'The handwriting recognizer has been closed.',
        ),
      );
    }
    final code = _language(language);
    if (_recognizers.containsKey(code)) return Future.value();
    return _preparing.putIfAbsent(
      code,
      // A block body: returning the removed future here would make it wait
      // on itself forever.
      () => _prepare(code).whenComplete(() {
        _preparing.remove(code);
      }),
    );
  }

  Future<void> _prepare(String code) async {
    Object? failure;
    // The first download can be slow or interrupted on a fresh install, so
    // it is retried before the user is told anything went wrong.
    for (var attempt = 0; attempt < 3; attempt++) {
      if (attempt > 0) {
        await Future<void>.delayed(Duration(seconds: 2 * attempt));
      }
      try {
        var downloaded = false;
        try {
          downloaded = await _manager
              .isModelDownloaded(code)
              .timeout(const Duration(seconds: 20));
        } on TimeoutException {
          // Fall through to the download, which also reports readiness.
        }
        if (!downloaded) {
          final ready = await _manager
              .downloadModel(code, isWifiRequired: false)
              .timeout(const Duration(minutes: 6));
          if (!ready) throw StateError('the model was not saved');
        }
        if (_disposed) {
          throw const InkRecognitionException(
            'The handwriting recognizer has been closed.',
          );
        }
        _recognizers[code] = ml.DigitalInkRecognizer(languageCode: code);
        return;
      } on InkRecognitionException {
        rethrow;
      } on MissingPluginException {
        throw const InkRecognitionException(
          'Handwriting recognition is not available on this device.',
        );
      } catch (error) {
        failure = error;
      }
    }
    final detail = failure is TimeoutException
        ? 'the download timed out'
        : failure is PlatformException
        ? (failure.message ?? failure.code)
        : '$failure';
    throw InkRecognitionException(
      'Could not download the handwriting model ($detail). It needs '
      'internet access once; it will be tried again automatically.',
    );
  }

  Future<List<String>> recognize(
    List<CanvasShape> strokes, {
    required String language,
  }) async {
    final inkStrokes = strokes
        .where((s) => s.tool == CanvasTool.pen && s.points.isNotEmpty)
        .toList();
    if (inkStrokes.isEmpty) {
      throw const InkRecognitionException(
        'Write a few words with the pen, then recognize them.',
      );
    }
    final pointCount = inkStrokes.fold<int>(0, (n, s) => n + s.points.length);
    if (inkStrokes.length > 200 || pointCount > 50000) {
      throw const InkRecognitionException(
        'Recognize a smaller group of handwritten words at a time.',
      );
    }
    await prepare(language);
    if (_disposed) {
      throw const InkRecognitionException(
        'The handwriting recognizer has been closed.',
      );
    }
    final code = _language(language);
    // Translate negative canvas coordinates into an ordinary writing area.
    var minX = double.infinity, minY = double.infinity;
    for (final stroke in inkStrokes) {
      for (final point in stroke.points) {
        minX = math.min(minX, point.dx);
        minY = math.min(minY, point.dy);
      }
    }
    final ink = ml.Ink();
    var timestamp = 0;
    for (final shape in inkStrokes) {
      final stroke = ml.Stroke();
      for (final point in shape.points) {
        // Existing saved strokes do not carry event times. Stable increasing
        // sample times preserve pen order without claiming measured timing.
        stroke.points.add(
          ml.StrokePoint(
            x: point.dx - minX + 16,
            y: point.dy - minY + 16,
            t: timestamp,
          ),
        );
        timestamp += 8;
      }
      ink.strokes.add(stroke);
      timestamp += 80;
    }
    try {
      final candidates = await _recognizers[code]!
          .recognize(ink)
          .timeout(const Duration(seconds: 20));
      final text = candidates
          .map((c) => c.text.trim())
          .where((text) => text.isNotEmpty)
          .toSet()
          .take(5)
          .toList();
      if (text.isEmpty) {
        throw const InkRecognitionException(
          'No handwriting was recognized. Try a smaller group of words. Your ink has been kept.',
        );
      }
      return text;
    } on InkRecognitionException {
      rethrow;
    } catch (_) {
      throw const InkRecognitionException(
        'Could not recognize this handwriting. Your ink has been kept; please try again.',
      );
    }
  }

  Future<void> dispose() async {
    _disposed = true;
    final recognizers = _recognizers.values.toList();
    _recognizers.clear();
    for (final recognizer in recognizers) {
      try {
        await recognizer.close().timeout(const Duration(seconds: 3));
      } catch (_) {
        /* A stopped native recognizer may already be closed. */
      }
    }
  }
}

/// Turns a single hand-drawn pen stroke into the clean shape it resembles:
/// a straight line, an arrow, a rectangle or an ellipse. Marks that look like
/// writing or scribbles return null and stay as ink.
CanvasShape? recognizeShape(CanvasShape stroke) {
  if (stroke.tool != CanvasTool.pen || stroke.points.length < 2) return null;
  // Recognition is bounded independently of raw sampling rate on touch panels.
  final step = math.max(1, (stroke.points.length / 256).ceil());
  final points = <Offset>[];
  for (var i = 0; i < stroke.points.length; i += step) {
    final p = stroke.points[i];
    if (!p.dx.isFinite || !p.dy.isFinite) return null;
    if (points.isEmpty || (p - points.last).distance >= .5) points.add(p);
  }
  if (points.isEmpty) return null;
  if (points.last != stroke.points.last) points.add(stroke.points.last);
  if (points.length < 2) return null;
  var left = points.first.dx, right = left, top = points.first.dy, bottom = top;
  var length = 0.0;
  for (var i = 0; i < points.length; i++) {
    final p = points[i];
    left = math.min(left, p.dx);
    right = math.max(right, p.dx);
    top = math.min(top, p.dy);
    bottom = math.max(bottom, p.dy);
    if (i > 0) length += (p - points[i - 1]).distance;
  }
  final bounds = Rect.fromLTRB(left, top, right, bottom);
  final diagonal = math.sqrt(
    bounds.width * bounds.width + bounds.height * bounds.height,
  );
  if (diagonal < 24 || length < 24) return null;
  final a = points.first, b = points.last, chord = (b - a).distance;
  CanvasShape result(CanvasTool tool, List<Offset> shapePoints) => CanvasShape(
    id: stroke.id,
    tool: tool,
    points: shapePoints,
    color: stroke.color,
    width: stroke.width,
    filled: stroke.filled,
    text: stroke.text,
  );

  // Straight line: little detour and no point far from the chord.
  if (chord / length >= .90 &&
      points.every(
        (p) => _segmentDistance(p, a, b) <= math.max(4, chord * .07),
      )) {
    return result(CanvasTool.pen, [a, b]);
  }
  final arrow = _arrowEndpoints(points, diagonal);
  if (arrow != null) return result(CanvasTool.arrow, arrow);

  // Closed outline: the ends meet, allowing a gap or a small overshoot.
  if (chord > math.max(16, diagonal * .30)) return null;
  if (bounds.width < 20 || bounds.height < 20) return null;
  // Reject retraced scribbles and writing, which cover far more or far less
  // path than one trip around the outline.
  final perimeter = 2 * (bounds.width + bounds.height);
  if (length < perimeter * .55 || length > perimeter * 1.45) return null;

  final center = bounds.center, rx = bounds.width / 2, ry = bounds.height / 2;
  var area = 0.0, radialError = 0.0, winding = 0.0, nearEdge = 0;
  double? previousAngle;
  final edgeBand = math.min(bounds.width, bounds.height) * .14 + 2;
  for (var i = 0; i < points.length; i++) {
    final p = points[i], q = points[(i + 1) % points.length];
    area += p.dx * q.dy - q.dx * p.dy;
    final edge = math.min(
      math.min((p.dx - left).abs(), (p.dx - right).abs()),
      math.min((p.dy - top).abs(), (p.dy - bottom).abs()),
    );
    if (edge <= edgeBand) nearEdge++;
    final x = (p.dx - center.dx) / rx, y = (p.dy - center.dy) / ry;
    radialError += (math.sqrt(x * x + y * y) - 1).abs();
    final angle = math.atan2(y, x);
    if (previousAngle != null) {
      var delta = angle - previousAngle;
      while (delta > math.pi) {
        delta -= 2 * math.pi;
      }
      while (delta < -math.pi) {
        delta += 2 * math.pi;
      }
      winding += delta;
    }
    previousAngle = angle;
  }
  // One trip around the centre; figure-eights and spirals are not shapes.
  if (winding.abs() < 1.5 * math.pi || winding.abs() > 2.7 * math.pi) {
    return null;
  }
  // How much of its bounding box the outline fills: a box fills nearly all
  // of it, an ellipse about 79 %, a triangle or diamond only half.
  final fill = area.abs() / 2 / (bounds.width * bounds.height);
  final edgeShare = nearEdge / points.length;
  radialError /= points.length;
  final box = [bounds.topLeft, bounds.bottomRight];
  if (fill >= .84 && edgeShare >= .70) {
    return result(CanvasTool.rectangle, box);
  }
  if (fill >= .62 && fill <= .88 && radialError <= .20) {
    return result(CanvasTool.ellipse, box);
  }
  if (fill >= .80 && edgeShare >= .60) {
    return result(CanvasTool.rectangle, box);
  }
  return null;
}

double _segmentDistance(Offset p, Offset a, Offset b) {
  final delta = b - a, squared = delta.dx * delta.dx + delta.dy * delta.dy;
  if (squared == 0) return (p - a).distance;
  final t = (((p.dx - a.dx) * delta.dx + (p.dy - a.dy) * delta.dy) / squared)
      .clamp(0.0, 1.0);
  return (p - (a + delta * t)).distance;
}

List<Offset> _simplify(List<Offset> points, double tolerance) {
  if (points.length <= 2) return points;
  var greatest = 0.0, index = 0;
  for (var i = 1; i < points.length - 1; i++) {
    final distance = _segmentDistance(points[i], points.first, points.last);
    if (distance > greatest) {
      greatest = distance;
      index = i;
    }
  }
  if (greatest <= tolerance) return [points.first, points.last];
  final a = _simplify(points.sublist(0, index + 1), tolerance);
  final b = _simplify(points.sublist(index), tolerance);
  return [...a.take(a.length - 1), ...b];
}

List<Offset>? _arrowEndpoints(List<Offset> points, double diagonal) {
  final simple = _simplify(points, diagonal * .05);
  // Shaft start -> tip -> wing -> (back to tip) -> other wing. Some people
  // go straight from one wing to the other, which leaves four points.
  if (simple.length != 5 && simple.length != 4) return null;
  final start = simple[0], tip = simple[1], wingA = simple[2];
  final wingB = simple.last;
  if (simple.length == 5 && (tip - simple[3]).distance > diagonal * .14) {
    return null;
  }
  final shaft = tip - start, distance = shaft.distance;
  if (distance < diagonal * .60) return null;
  final unit = shaft / distance, da = wingA - tip, db = wingB - tip;
  if (da.distance < distance * .10 ||
      db.distance < distance * .10 ||
      da.distance > distance * .55 ||
      db.distance > distance * .55) {
    return null;
  }
  final projectionA = -(da.dx * unit.dx + da.dy * unit.dy),
      projectionB = -(db.dx * unit.dx + db.dy * unit.dy);
  final crossA = da.dx * unit.dy - da.dy * unit.dx,
      crossB = db.dx * unit.dy - db.dy * unit.dx;
  if (projectionA < distance * .05 ||
      projectionB < distance * .05 ||
      crossA * crossB >= 0 ||
      crossA.abs() < distance * .05 ||
      crossB.abs() < distance * .05) {
    return null;
  }
  return [start, tip];
}

/// Splits handwriting into text lines (top to bottom) so each line can be
/// recognized separately. Strokes keep their writing order within a line.
/// Small marks such as dots and matras join the nearest line.
List<List<CanvasShape>> groupInkLines(List<CanvasShape> strokes) {
  final ink = strokes
      .where((s) => s.tool == CanvasTool.pen && s.points.isNotEmpty)
      .toList();
  if (ink.length < 2) return ink.isEmpty ? [] : [ink];
  final heights = ink.map((s) => s.bounds.height).toList()..sort();
  final typical = math.max(8.0, heights[(heights.length * .75).floor()]);
  final order = {for (var i = 0; i < ink.length; i++) ink[i].id: i};
  final large = ink.where((s) => s.bounds.height >= typical * .4).toList()
    ..sort((a, b) => a.bounds.center.dy.compareTo(b.bounds.center.dy));
  final lines = <({List<CanvasShape> strokes, double top, double bottom})>[];
  for (final stroke in large) {
    final b = stroke.bounds;
    var placed = false;
    for (var i = 0; i < lines.length; i++) {
      final line = lines[i];
      final overlap =
          math.min(line.bottom, b.bottom) - math.max(line.top, b.top);
      if (overlap >= math.min(b.height, line.bottom - line.top) * .35) {
        lines[i] = (
          strokes: line.strokes..add(stroke),
          top: math.min(line.top, b.top),
          bottom: math.max(line.bottom, b.bottom),
        );
        placed = true;
        break;
      }
    }
    if (!placed) lines.add((strokes: [stroke], top: b.top, bottom: b.bottom));
  }
  if (lines.isEmpty) return [ink];
  for (final stroke in ink) {
    if (stroke.bounds.height >= typical * .4) continue;
    final y = stroke.bounds.center.dy;
    var best = 0;
    var bestDistance = double.infinity;
    for (var i = 0; i < lines.length; i++) {
      final line = lines[i];
      final distance = y < line.top
          ? line.top - y
          : y > line.bottom
          ? y - line.bottom
          : 0.0;
      if (distance < bestDistance) {
        bestDistance = distance;
        best = i;
      }
    }
    lines[best].strokes.add(stroke);
  }
  lines.sort((a, b) => a.top.compareTo(b.top));
  return [
    for (final line in lines)
      line.strokes..sort((a, b) => order[a.id]!.compareTo(order[b.id]!)),
  ];
}
