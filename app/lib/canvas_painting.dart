import 'dart:math' as math;
import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:material_ui/material_ui.dart';

import 'canvas_document.dart';

const selectionBlue = Color(0xff2f80ed);
const canvasBackground = Color(0xfff9fafb);
const panelBorder = Color(0xffe8e8ec);
const inkBlack = Color(0xff1d1d1d);

/// Board palette (name, ARGB).
const palette = <(String, int)>[
  ('Black', 0xff1d1d1d),
  ('Grey', 0xff9fa8b2),
  ('Light violet', 0xffe085f4),
  ('Violet', 0xffae3ec9),
  ('Blue', 0xff4465e9),
  ('Light blue', 0xff4ba1f1),
  ('Yellow', 0xfff1ac4b),
  ('Orange', 0xffe16919),
  ('Green', 0xff099268),
  ('Light green', 0xff4cb05e),
  ('Light red', 0xfff87777),
  ('Red', 0xffe03131),
];

/// Stroke sizes (label, width).
const strokeSizes = <(String, double)>[
  ('S', 2),
  ('M', 3.5),
  ('L', 5),
  ('XL', 10),
];

Rect? sceneBounds(Iterable<CanvasShape> shapes) {
  Rect? bounds;
  for (final s in shapes) {
    bounds = bounds == null ? s.bounds : bounds.expandToInclude(s.bounds);
  }
  return bounds;
}

void drawShape(Canvas canvas, CanvasShape s, {double alpha = 1}) {
  if (s.points.isEmpty) {
    return;
  }
  final color = Color(s.color).withValues(alpha: alpha);
  final paint = Paint()
    ..color = color
    ..strokeWidth = s.width
    ..strokeCap = StrokeCap.round
    ..strokeJoin = StrokeJoin.round
    ..style = PaintingStyle.stroke;
  final a = s.points.first, b = s.points.last;
  switch (s.tool) {
    case CanvasTool.rectangle:
    case CanvasTool.ellipse:
      final rect = Rect.fromPoints(a, b);
      final round = s.tool == CanvasTool.rectangle
          ? RRect.fromRectAndRadius(
              rect,
              Radius.circular(math.min(6, rect.shortestSide / 4)),
            )
          : null;
      if (s.filled) {
        final fill = Paint()..color = color.withValues(alpha: alpha * .18);
        round != null
            ? canvas.drawRRect(round, fill)
            : canvas.drawOval(rect, fill);
      }
      round != null
          ? canvas.drawRRect(round, paint)
          : canvas.drawOval(rect, paint);
    case CanvasTool.arrow:
      canvas.drawLine(a, b, paint);
      final angle = math.atan2(b.dy - a.dy, b.dx - a.dx),
          length = math.min(
            math.max(14.0, s.width * 4),
            math.max(4.0, (b - a).distance * .5),
          );
      canvas.drawLine(
        b,
        b - Offset(math.cos(angle - .5), math.sin(angle - .5)) * length,
        paint,
      );
      canvas.drawLine(
        b,
        b - Offset(math.cos(angle + .5), math.sin(angle + .5)) * length,
        paint,
      );
    case CanvasTool.text:
      final text = TextPainter(
        text: TextSpan(
          text: s.text,
          style: boardTextStyle(s.fontSize, color: color),
        ),
        textDirection: TextDirection.ltr,
      )..layout();
      text.paint(canvas, a);
      text.dispose();
    case CanvasTool.pen:
      if (s.isDot) {
        canvas.drawCircle(a, s.width / 2, paint..style = PaintingStyle.fill);
      } else {
        canvas.drawPath(s.penPath, paint);
      }
    case CanvasTool.select:
    case CanvasTool.hand:
    case CanvasTool.eraser:
      break;
  }
}

/// Paints background, dot grid and every shape that touches the viewport.
void drawScene(
  Canvas canvas,
  Size size, {
  required List<CanvasShape> shapes,
  required Offset camera,
  required double zoom,
  required bool grid,
  Set<String> hidden = const {},
}) {
  canvas.drawRect(Offset.zero & size, Paint()..color = canvasBackground);
  if (grid) {
    drawDotGrid(canvas, size, camera, zoom);
  }
  canvas.save();
  canvas.translate(camera.dx, camera.dy);
  canvas.scale(zoom);
  final visible = Rect.fromLTWH(
    -camera.dx / zoom,
    -camera.dy / zoom,
    size.width / zoom,
    size.height / zoom,
  );
  for (final s in shapes) {
    if (hidden.contains(s.id)) continue;
    if (!s.bounds.inflate(s.width + 2).overlaps(visible)) continue;
    drawShape(canvas, s);
  }
  canvas.restore();
}

/// One batched draw call instead of thousands of circles per frame.
void drawDotGrid(Canvas canvas, Size size, Offset camera, double zoom) {
  var spacing = 24 * zoom;
  while (spacing < 14) {
    spacing *= 2;
  }
  final columns = (size.width / spacing).ceil() + 1,
      rows = (size.height / spacing).ceil() + 1;
  if (columns * rows > 40000) return;
  final startX = camera.dx % spacing, startY = camera.dy % spacing;
  final points = Float32List(columns * rows * 2);
  var i = 0;
  for (var x = 0; x < columns; x++) {
    for (var y = 0; y < rows; y++) {
      points[i++] = startX + x * spacing;
      points[i++] = startY + y * spacing;
    }
  }
  canvas.drawRawPoints(
    ui.PointMode.points,
    points,
    Paint()
      ..color = const Color(0xffd9dce1)
      ..strokeWidth = 2
      ..strokeCap = StrokeCap.round,
  );
}

class ThumbnailPainter extends CustomPainter {
  ThumbnailPainter(this.shapes);
  final List<CanvasShape> shapes;
  @override
  void paint(Canvas canvas, Size size) {
    final bounds = sceneBounds(shapes);
    if (bounds == null) {
      return;
    }
    final scale = math.min(
      1.5,
      math.min(
        size.width / math.max(1, bounds.width + 32),
        size.height / math.max(1, bounds.height + 32),
      ),
    );
    canvas.save();
    canvas.clipRect(Offset.zero & size);
    canvas.translate(size.width / 2, size.height / 2);
    canvas.scale(scale);
    canvas.translate(-bounds.center.dx, -bounds.center.dy);
    for (final s in shapes) {
      drawShape(canvas, s);
    }
    canvas.restore();
  }

  @override
  bool shouldRepaint(covariant ThumbnailPainter oldDelegate) =>
      !identical(oldDelegate.shapes, shapes);
}
