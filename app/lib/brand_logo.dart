import 'package:flutter/widgets.dart';

/// Canvas Studio's geometric paper-and-pencil mark, drawn natively.
class BrandLogo extends StatelessWidget {
  const BrandLogo({super.key, this.size = 36});
  final double size;

  @override
  Widget build(BuildContext context) => Semantics(
    label: 'Canvas Studio logo',
    image: true,
    child: SizedBox.square(
      dimension: size,
      child: CustomPaint(painter: _LogoPainter()),
    ),
  );
}

class _LogoPainter extends CustomPainter {
  @override
  void paint(Canvas canvas, Size size) {
    canvas.save();
    canvas.scale(size.width / 108, size.height / 108);
    void round(Rect rect, double radius, int color) => canvas.drawRRect(
      RRect.fromRectAndRadius(rect, Radius.circular(radius)),
      Paint()..color = Color(color),
    );
    void path(Path path, int color) =>
        canvas.drawPath(path, Paint()..color = Color(color));
    round(const Rect.fromLTWH(0, 0, 108, 108), 25, 0xff6750d8);
    round(const Rect.fromLTWH(28, 23, 52, 64), 8, 0xffffffff);
    round(const Rect.fromLTWH(38, 36, 23, 4), 2, 0xffe7e0ff);
    round(const Rect.fromLTWH(38, 46, 13, 4), 2, 0xffe7e0ff);
    path(
      Path()
        ..moveTo(40, 61)
        ..lineTo(64, 37)
        ..lineTo(76, 49)
        ..lineTo(52, 73)
        ..close(),
      0xffff8966,
    );
    path(
      Path()
        ..moveTo(64, 37)
        ..lineTo(68, 33)
        ..quadraticBezierTo(72, 29, 76, 33)
        ..lineTo(80, 37)
        ..quadraticBezierTo(84, 41, 80, 45)
        ..lineTo(76, 49)
        ..close(),
      0xffffb39b,
    );
    path(
      Path()
        ..moveTo(44, 65)
        ..lineTo(68, 41)
        ..lineTo(72, 45)
        ..lineTo(48, 69)
        ..close(),
      0xffffa184,
    );
    path(
      Path()
        ..moveTo(36, 77)
        ..lineTo(40, 61)
        ..lineTo(52, 73)
        ..close(),
      0xffffd8aa,
    );
    path(
      Path()
        ..moveTo(36, 77)
        ..lineTo(38, 69)
        ..lineTo(44, 75)
        ..close(),
      0xff453587,
    );
    canvas.restore();
  }

  @override
  bool shouldRepaint(covariant _LogoPainter oldDelegate) => false;
}
