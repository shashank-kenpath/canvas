import 'dart:convert';
import 'dart:math' as math;

import 'package:flutter/foundation.dart';
import 'package:flutter/painting.dart';

enum CanvasTool { select, hand, pen, rectangle, ellipse, arrow, text, eraser }

/// Font used for text on boards. Falls back to the system font (including
/// Devanagari) when a glyph is missing.
const boardFontFamily = 'ShantellSans';

TextStyle boardTextStyle(double fontSize, {Color? color}) => TextStyle(
  color: color,
  fontSize: fontSize,
  height: 1.3,
  fontFamily: boardFontFamily,
);

class CanvasShape {
  CanvasShape({
    required this.id,
    required this.tool,
    required List<Offset> points,
    this.color = 0xff1d1d1d,
    this.width = 3,
    this.filled = false,
    this.text = '',
    this.fontSize = 24,
  }) : points = List.unmodifiable(points);

  final String id;
  final CanvasTool tool;
  final List<Offset> points;
  final int color;
  final double width;
  final bool filled;
  final String text;
  final double fontSize;

  /// Computed once per (immutable) shape: hit testing, culling and the
  /// selection overlay all read this on every pointer sample.
  late final Rect bounds = _computeBounds();

  /// Smoothed freehand outline, built once and reused by every repaint.
  late final Path penPath = _buildPenPath();

  bool get isDot {
    final a = points.first;
    for (final p in points) {
      if ((p - a).distanceSquared > .01) return false;
    }
    return true;
  }

  Path _buildPenPath() {
    final path = Path();
    if (points.isEmpty) return path;
    final a = points.first;
    path.moveTo(a.dx, a.dy);
    for (var i = 1; i < points.length - 1; i++) {
      final p = points[i], next = points[i + 1];
      path.quadraticBezierTo(
        p.dx,
        p.dy,
        (p.dx + next.dx) / 2,
        (p.dy + next.dy) / 2,
      );
    }
    path.lineTo(points.last.dx, points.last.dy);
    return path;
  }

  Rect _computeBounds() {
    if (points.isEmpty) return Rect.zero;
    if (tool == CanvasTool.text) {
      final painter = TextPainter(
        text: TextSpan(text: text, style: boardTextStyle(fontSize)),
        textDirection: TextDirection.ltr,
      )..layout();
      final rect = Rect.fromLTWH(
        points.first.dx,
        points.first.dy,
        math.max(fontSize, painter.width),
        math.max(fontSize * 1.3, painter.height),
      );
      painter.dispose();
      return rect;
    }
    var minX = points.first.dx;
    var maxX = minX;
    var minY = points.first.dy;
    var maxY = minY;
    for (var i = 1; i < points.length; i++) {
      final point = points[i];
      if (point.dx < minX) minX = point.dx;
      if (point.dx > maxX) maxX = point.dx;
      if (point.dy < minY) minY = point.dy;
      if (point.dy > maxY) maxY = point.dy;
    }
    return Rect.fromLTRB(minX, minY, maxX, maxY);
  }

  CanvasShape copyWith({
    List<Offset>? points,
    int? color,
    double? width,
    bool? filled,
    String? text,
    double? fontSize,
  }) => CanvasShape(
    id: id,
    tool: tool,
    points: points ?? this.points,
    color: color ?? this.color,
    width: width ?? this.width,
    filled: filled ?? this.filled,
    text: text ?? this.text,
    fontSize: fontSize ?? this.fontSize,
  );

  Map<String, dynamic> toJson() => {
    'id': id,
    'tool': tool.name,
    'points': points.map((point) => [point.dx, point.dy]).toList(),
    'color': color,
    'width': width,
    'filled': filled,
    'text': text,
    if (fontSize != 24) 'fontSize': fontSize,
  };

  factory CanvasShape.fromJson(Map<String, dynamic> json) {
    final tool = CanvasTool.values.byName(json['tool'] as String);
    final points = (json['points'] as List).map((value) {
      final pair = value as List;
      final x = (pair[0] as num).toDouble();
      final y = (pair[1] as num).toDouble();
      if (!x.isFinite || !y.isFinite) {
        throw const FormatException('Shape coordinates must be finite.');
      }
      return Offset(x, y);
    }).toList();
    final width = (json['width'] as num? ?? 3).toDouble();
    if (!width.isFinite || width <= 0) {
      throw const FormatException('Stroke width must be positive and finite.');
    }
    final fontSize = (json['fontSize'] as num? ?? 24).toDouble();
    if (!fontSize.isFinite || fontSize <= 0) {
      throw const FormatException('Font size must be positive and finite.');
    }
    return CanvasShape(
      id: json['id'] as String,
      tool: tool,
      points: points,
      color: (json['color'] as num? ?? 0xff1d1d1d).toInt(),
      width: width,
      filled: json['filled'] as bool? ?? false,
      text: json['text'] as String? ?? '',
      fontSize: fontSize,
    );
  }
}

class _Change {
  const _Change(this.id, this.before, this.after, this.index);
  final String id;
  final CanvasShape? before, after;

  /// Stacking position the shape had when it was added or removed, so undo
  /// and redo put it back in the same layer.
  final int index;
}

/// Shapes plus selection and undo history.
///
/// History stores per-shape changes rather than whole-board snapshots, so
/// undoing your own edit never reverts shapes that collaborators changed.
class CanvasDocument extends ChangeNotifier {
  List<CanvasShape> _shapes = const [];
  final List<List<_Change>> _past = [];
  final List<List<_Change>> _future = [];
  final Set<String> _selected = {};

  /// Incremented whenever the shape list changes; used to invalidate caches.
  int revision = 0;

  List<CanvasShape> get shapes => _shapes;
  Set<String> get selectedIds => _selected;
  String? get selectedId => _selected.isEmpty ? null : _selected.first;
  CanvasShape? get selected {
    if (_selected.isEmpty) return null;
    final id = _selected.first;
    for (final shape in _shapes) {
      if (shape.id == id) return shape;
    }
    return null;
  }

  List<CanvasShape> get selectedShapes => _selected.isEmpty
      ? const []
      : _shapes.where((shape) => _selected.contains(shape.id)).toList();

  bool contains(String id) => _shapes.any((shape) => shape.id == id);
  bool get canUndo => _past.isNotEmpty;
  bool get canRedo => _future.isNotEmpty;

  List<_Change> _apply(
    Map<String, CanvasShape?> changes, [
    Map<String, int> positions = const {},
  ]) {
    final applied = <_Change>[];
    final next = <CanvasShape>[];
    final seen = <String>{};
    for (var i = 0; i < _shapes.length; i++) {
      final shape = _shapes[i];
      if (changes.containsKey(shape.id)) {
        seen.add(shape.id);
        final after = changes[shape.id];
        if (identical(after, shape)) {
          next.add(shape);
          continue;
        }
        applied.add(_Change(shape.id, shape, after, i));
        if (after != null) next.add(after);
      } else {
        next.add(shape);
      }
    }
    final added = [
      for (final entry in changes.entries)
        if (!seen.contains(entry.key) && entry.value != null) entry,
    ];
    // Restored shapes return to their old layer, lowest first so later
    // positions stay valid; brand new shapes go on top.
    final restored = [
      for (final entry in added)
        if (positions.containsKey(entry.key)) entry,
    ]..sort((a, b) => positions[a.key]!.compareTo(positions[b.key]!));
    for (final entry in restored) {
      final index = math.min(positions[entry.key]!, next.length);
      next.insert(index, entry.value!);
      applied.add(_Change(entry.key, null, entry.value, index));
    }
    for (final entry in added) {
      if (positions.containsKey(entry.key)) continue;
      applied.add(_Change(entry.key, null, entry.value, next.length));
      next.add(entry.value!);
    }
    if (applied.isEmpty) return applied;
    _shapes = List.unmodifiable(next);
    revision++;
    _pruneSelection();
    return applied;
  }

  void _pruneSelection() {
    if (_selected.isEmpty) return;
    final ids = {for (final shape in _shapes) shape.id};
    _selected.removeWhere((id) => !ids.contains(id));
  }

  void _commit(Map<String, CanvasShape?> changes) {
    final applied = _apply(changes);
    if (applied.isEmpty) return;
    _past.add(applied);
    if (_past.length > 100) _past.removeAt(0);
    _future.clear();
    notifyListeners();
  }

  void add(CanvasShape shape) {
    if (contains(shape.id)) {
      throw ArgumentError.value(shape.id, 'shape.id', 'Already exists');
    }
    _commit({shape.id: shape});
  }

  void replace(CanvasShape shape) {
    if (!contains(shape.id)) return;
    _commit({shape.id: shape});
  }

  /// Replaces several existing shapes as one undo step.
  void replaceMany(Iterable<CanvasShape> shapes) => _commit({
    for (final shape in shapes)
      if (contains(shape.id)) shape.id: shape,
  });

  void remove(String id) => removeMany([id]);

  void removeMany(Iterable<String> ids) => _commit({
    for (final id in ids)
      if (contains(id)) id: null,
  });

  /// Removes [remove] and adds [add] as one undo step, for example replacing
  /// ink with recognized text.
  void swap(Iterable<String> remove, Iterable<CanvasShape> add) => _commit({
    for (final id in remove)
      if (contains(id)) id: null,
    for (final shape in add) shape.id: shape,
  });

  void select(String? id) => selectMany(id == null ? const [] : [id]);

  void selectMany(Iterable<String> ids) {
    final next = ids.where(contains).toSet();
    if (next.length == _selected.length && next.containsAll(_selected)) return;
    _selected
      ..clear()
      ..addAll(next);
    notifyListeners();
  }

  void undo() {
    if (!canUndo) return;
    final entry = _past.removeLast();
    _apply(
      {for (final change in entry) change.id: change.before},
      {for (final change in entry) change.id: change.index},
    );
    _future.add(entry);
    notifyListeners();
  }

  void redo() {
    if (!canRedo) return;
    final entry = _future.removeLast();
    _apply(
      {for (final change in entry) change.id: change.after},
      {for (final change in entry) change.id: change.index},
    );
    _past.add(entry);
    notifyListeners();
  }

  void load(List<CanvasShape> shapes) {
    if (shapes.map((shape) => shape.id).toSet().length != shapes.length) {
      throw const FormatException('Duplicate shape IDs.');
    }
    _shapes = List.unmodifiable(shapes);
    revision++;
    _past.clear();
    _future.clear();
    _selected.clear();
    notifyListeners();
  }

  /// Applies collaborator state without touching the local undo history.
  void applyRemote(List<CanvasShape> shapes) {
    _shapes = List.unmodifiable(shapes);
    revision++;
    _pruneSelection();
    notifyListeners();
  }

  /// Shapes whose bounds touch [area], in drawing order.
  List<CanvasShape> shapesIn(Rect area) => _shapes
      .where((shape) => shape.bounds.inflate(.5).overlaps(area))
      .toList();

  String encode() => jsonEncode({
    'version': 1,
    'shapes': _shapes.map((shape) => shape.toJson()).toList(),
  });

  static List<CanvasShape> decode(String source) {
    final json = jsonDecode(source) as Map<String, dynamic>;
    if (json['version'] != 1) {
      throw const FormatException('Unsupported board version.');
    }
    final shapes = (json['shapes'] as List)
        .map((value) => CanvasShape.fromJson(value as Map<String, dynamic>))
        .toList();
    if (shapes.map((shape) => shape.id).toSet().length != shapes.length) {
      throw const FormatException('Duplicate shape IDs.');
    }
    return shapes;
  }

  CanvasShape? hitTest(Offset point, {double tolerance = 10}) {
    for (final shape in _shapes.reversed) {
      if (hits(shape, point, tolerance)) return shape;
    }
    return null;
  }

  static bool hits(CanvasShape shape, Offset point, double tolerance) {
    if (shape.points.isEmpty) return false;
    final radius = tolerance + shape.width / 2;
    final bounds = shape.bounds;
    if (!bounds.inflate(radius).contains(point)) return false;
    switch (shape.tool) {
      case CanvasTool.text:
        return bounds.inflate(tolerance).contains(point);
      case CanvasTool.rectangle:
        if (shape.filled && bounds.contains(point)) return true;
        final corners = [
          bounds.topLeft,
          bounds.topRight,
          bounds.bottomRight,
          bounds.bottomLeft,
        ];
        for (var i = 0; i < corners.length; i++) {
          if (_distance(point, corners[i], corners[(i + 1) % 4]) <= radius) {
            return true;
          }
        }
        return false;
      case CanvasTool.ellipse:
        final rx = bounds.width / 2;
        final ry = bounds.height / 2;
        if (rx < .001 || ry < .001) {
          return _distance(point, bounds.topLeft, bounds.bottomRight) <= radius;
        }
        final dx = point.dx - bounds.center.dx;
        final dy = point.dy - bounds.center.dy;
        final normalized = math.sqrt(dx * dx / (rx * rx) + dy * dy / (ry * ry));
        if (shape.filled && normalized <= 1) return true;
        if (normalized < .000001) return math.min(rx, ry) <= radius;
        final intersection =
            bounds.center + Offset(dx / normalized, dy / normalized);
        return (intersection - point).distance <= radius;
      case CanvasTool.pen:
      case CanvasTool.arrow:
        if (shape.points.length == 1) {
          return (shape.points.first - point).distance <= radius;
        }
        for (var i = 1; i < shape.points.length; i++) {
          if (_distance(point, shape.points[i - 1], shape.points[i]) <=
              radius) {
            return true;
          }
        }
        return false;
      case CanvasTool.select:
      case CanvasTool.hand:
      case CanvasTool.eraser:
        return false;
    }
  }

  static double _distance(Offset point, Offset start, Offset end) {
    final delta = end - start;
    final lengthSquared = delta.distanceSquared;
    if (lengthSquared == 0) return (point - start).distance;
    final relative = point - start;
    final t =
        ((relative.dx * delta.dx + relative.dy * delta.dy) / lengthSquared)
            .clamp(0.0, 1.0);
    return (point - (start + delta * t)).distance;
  }
}

typedef Document = CanvasDocument;
