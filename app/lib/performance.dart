import 'dart:async';

import 'package:flutter/scheduler.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

/// Measures how long recent frames took, so slow displays can be detected
/// and shown to the user instead of guessed at.
class FrameStats {
  static final instance = FrameStats();
  static const _keep = 90;
  final _build = <int>[], _raster = <int>[];
  bool _started = false;

  void start() {
    if (_started) return;
    _started = true;
    SchedulerBinding.instance.addTimingsCallback((timings) {
      for (final t in timings) {
        _build.add(t.buildDuration.inMicroseconds);
        _raster.add(t.rasterDuration.inMicroseconds);
      }
      if (_build.length > _keep) {
        _build.removeRange(0, _build.length - _keep);
        _raster.removeRange(0, _raster.length - _keep);
      }
    });
  }

  int get samples => _build.length;
  static double _average(List<int> values) => values.isEmpty
      ? 0
      : values.reduce((a, b) => a + b) / values.length / 1000;

  /// Average milliseconds spent preparing a frame in the app.
  double get buildMs => _average(_build);

  /// Average milliseconds the graphics engine took to draw a frame.
  double get rasterMs => _average(_raster);

  /// True when frames take far longer than a display refresh.
  bool get slow => samples >= 30 && buildMs + rasterMs > 45;
}

const _display = MethodChannel('canvas_studio/display');

const _modes = <(String, String, String)>[
  ('opengl', 'OpenGL', 'Recommended. Works best on most large panels.'),
  ('vulkan', 'Vulkan', 'Newer engine. Faster on some recent phones.'),
  ('classic', 'Compatibility', 'Try this if both others are slow.'),
];

/// Shows measured drawing speed and lets the user switch graphics engine,
/// which restarts the app. [beforeRestart] saves any open work first.
Future<void> showDisplayPanel(
  BuildContext context, {
  Future<Object?> Function()? beforeRestart,
}) => showDialog<void>(
  context: context,
  builder: (_) => _DisplayDialog(beforeRestart: beforeRestart),
);

class _DisplayDialog extends StatefulWidget {
  const _DisplayDialog({this.beforeRestart});
  final Future<Object?> Function()? beforeRestart;
  @override
  State<_DisplayDialog> createState() => _DisplayDialogState();
}

class _DisplayDialogState extends State<_DisplayDialog> {
  Map<Object?, Object?>? info;
  String? mode, current, error;
  Timer? _refresh;

  @override
  void initState() {
    super.initState();
    _load();
    _refresh = Timer.periodic(const Duration(seconds: 1), (_) {
      if (mounted) setState(() {});
    });
  }

  @override
  void dispose() {
    _refresh?.cancel();
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final value = await _display.invokeMapMethod<Object?, Object?>('info');
      if (!mounted || value == null) return;
      setState(() {
        info = value;
        mode = current = value['graphics'] as String? ?? 'opengl';
      });
    } catch (_) {
      // Only Android reports display details; the timings below still work.
    }
  }

  Future<void> _apply() async {
    try {
      await widget.beforeRestart?.call();
      await _display.invokeMethod<void>('setGraphics', {'mode': mode});
    } catch (_) {
      if (mounted) {
        setState(() => error = 'Could not change the graphics engine.');
      }
    }
  }

  Widget _row(String label, String value) => Padding(
    padding: const EdgeInsets.symmetric(vertical: 2),
    child: Row(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        SizedBox(
          width: 130,
          child: Text(label, style: const TextStyle(color: Color(0xff8b9098))),
        ),
        Expanded(child: Text(value)),
      ],
    ),
  );

  @override
  Widget build(BuildContext context) {
    final stats = FrameStats.instance, i = info;
    final frame = stats.buildMs + stats.rasterMs;
    return AlertDialog(
      title: const Text('Display and speed'),
      content: SizedBox(
        width: 440,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              _row(
                'Frame time',
                stats.samples == 0
                    ? 'Measuring…'
                    : '${frame.toStringAsFixed(1)} ms  '
                          '(app ${stats.buildMs.toStringAsFixed(1)}, '
                          'graphics ${stats.rasterMs.toStringAsFixed(1)})',
              ),
              _row(
                'Verdict',
                stats.samples == 0
                    ? '…'
                    : frame <= 20
                    ? 'Smooth'
                    : frame <= 45
                    ? 'A little slow'
                    : 'Slow: try another graphics engine below',
              ),
              if (i != null) ...[
                _row('Device', '${i['device']} · Android ${i['android']}'),
                _row(
                  'Screen',
                  '${i['width']} × ${i['height']} at '
                      '${(i['refresh'] as num?)?.round() ?? '?'} Hz',
                ),
                _row('Chip', '${i['hardware']}'),
                const Divider(height: 24),
                const Text(
                  'Graphics engine',
                  style: TextStyle(fontWeight: FontWeight.w600),
                ),
                for (final m in _modes)
                  ListTile(
                    key: ValueKey('graphics-${m.$1}'),
                    dense: true,
                    contentPadding: EdgeInsets.zero,
                    leading: Icon(
                      mode == m.$1
                          ? Icons.radio_button_checked
                          : Icons.radio_button_unchecked,
                    ),
                    title: Text(
                      m.$1 == current ? '${m.$2} (in use)' : m.$2,
                    ),
                    subtitle: Text(m.$3),
                    onTap: () => setState(() => mode = m.$1),
                  ),
              ],
              if (error != null)
                Text(error!, style: const TextStyle(color: Color(0xffe03131))),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Close'),
        ),
        if (i != null && mode != current)
          FilledButton(
            key: const ValueKey('graphics-apply'),
            onPressed: _apply,
            child: const Text('Apply and restart'),
          ),
      ],
    );
  }
}
