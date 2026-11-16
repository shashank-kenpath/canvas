import 'dart:async';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';
import 'package:share_plus/share_plus.dart';

import 'board_store.dart';
import 'canvas_document.dart';
import 'canvas_painting.dart';
import 'collaboration.dart';
import 'ink_recognition.dart';
import 'performance.dart';
import 'share_panel.dart';

Future<String?> askText(
  BuildContext context,
  String title, {
  String initial = '',
  String? hint,
  bool multiline = false,
}) async {
  final controller = TextEditingController(text: initial);
  final result = await showDialog<String>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(title),
      content: TextField(
        controller: controller,
        autofocus: true,
        minLines: multiline ? 2 : 1,
        maxLines: multiline ? 6 : 1,
        maxLength: multiline ? 2000 : 300,
        decoration: InputDecoration(
          hintText: hint ?? (multiline ? 'Type a note…' : 'Give it a name'),
          counterText: '',
        ),
        onSubmitted: multiline
            ? null
            : (value) => Navigator.pop(context, value.trim()),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed: () => Navigator.pop(context, controller.text.trim()),
          child: const Text('Done'),
        ),
      ],
    ),
  );
  Future<void>.delayed(const Duration(seconds: 1), controller.dispose);
  return result;
}

typedef InkRecognizer = Future<List<String>> Function(
  List<CanvasShape> strokes, {
  required String language,
});

class NativeEditor extends StatefulWidget {
  const NativeEditor({
    super.key,
    required this.board,
    required this.store,
    this.recognize,
    this.collaborationFetch,
  });
  final BoardData board;
  final BoardStore store;

  /// Handwriting recognizer override (tests). Defaults to on-device ML Kit.
  final InkRecognizer? recognize;
  final CollaborationFetch? collaborationFetch;
  @override
  State<NativeEditor> createState() => NativeEditorState();
}

/// What the pen does with a finished stroke.
enum PenMode {
  /// Keeps the stroke as ink.
  ink,

  /// Replaces handwriting with typed text after a short pause.
  text,

  /// Replaces a rough outline with a clean shape as soon as the pen lifts.
  shape,
}

enum _Mode {
  none,
  draw,
  shape,
  move,
  resize,
  marquee,
  erase,
  pan,
  pinch,
  text,
  blocked,
}

class NativeEditorState extends State<NativeEditor>
    with WidgetsBindingObserver {
  // Preferences kept for the app session so every board opens the same way.
  static bool _snapPreference = true;
  static PenMode _penModePreference = PenMode.ink;
  static bool _slowHintShown = false;
  static String _languagePreference = 'en';
  static int _sequence = 0;

  final document = CanvasDocument();
  final _sceneTick = ValueNotifier<int>(0);
  final _liveTick = ValueNotifier<int>(0);
  final _zoomLabel = ValueNotifier<double>(1);
  final _clientId =
      'd${math.Random().nextInt(0x7fffffff).toRadixString(36)}${DateTime.now().millisecondsSinceEpoch.toRadixString(36)}';

  CanvasTool tool = CanvasTool.pen;
  int color = palette.first.$2;
  double width = strokeSizes[1].$2;
  bool filled = false, grid = true, styleOpen = false;
  bool snapShapes = _snapPreference;
  PenMode penMode = _penModePreference;
  String language = _languagePreference;
  bool allowPop = false, closing = false;
  String saveStatus = 'Saved';
  String? busy;

  // Camera.
  double zoom = 1;
  Offset camera = Offset.zero;
  Size viewport = Size.zero;
  double _dpr = 1;
  bool _navigating = false;

  // Pointer state.
  final _pointers = <int, Offset>{};
  final _ignored = <int>{};
  _Mode _mode = _Mode.none;
  int? _primary;
  Duration _downStamp = Duration.zero;
  double _travel = 0;
  Offset _startWorld = Offset.zero, _anchorWorld = Offset.zero;
  double _startZoom = 1, _pinchDistance = 1;

  // In-progress ink. Mutated in place: no per-sample list copies or rebuilds.
  List<Offset>? _draft;
  Path _draftPath = Path();
  String _draftId = '';
  Offset _lastScreen = Offset.zero, _stillAnchor = Offset.zero;
  CanvasShape? _snapped;
  Timer? _snapTimer;
  Offset _draftEnd = Offset.zero;

  // Selection transforms and erasing.
  List<CanvasShape> _moving = const [];
  bool _dragging = false;
  Offset _delta = Offset.zero, _resizeOrigin = Offset.zero;
  Rect _resizeBounds = Rect.zero;
  int _handle = 0;
  double _scaleX = 1, _scaleY = 1;
  Rect? _marquee;
  Set<String> _hidden = {};
  int _hiddenVersion = 0;
  Offset _lastErase = Offset.zero;
  String? _lastTapId;
  DateTime _lastTapTime = DateTime.fromMillisecondsSinceEpoch(0);

  // Rasterized scene: all committed shapes for the current camera.
  ui.Image? _cache;
  int _cacheRevision = -1, _cacheHidden = -1;
  Offset _cacheCamera = Offset.zero;
  double _cacheZoom = 1, _cacheDpr = 1;
  Size _cacheSize = Size.zero;
  bool _cacheGrid = true;

  // Handwriting.
  final _pendingInk = <String>[];
  Timer? _autoTimer;

  // Saving and collaboration.
  Timer? _debounce;
  Future<void> _saveQueue = Future.value();
  CollaborationClient? _collab;
  CollaborationLink? shareLink;

  bool get isShared => shareLink != null;
  String get status => saveStatus.startsWith('Save failed')
      ? saveStatus
      : (_collab?.status ?? saveStatus);

  @override
  void initState() {
    super.initState();
    document.load(widget.board.shapes);
    document.addListener(_changed);
    WidgetsBinding.instance.addObserver(this);
    final share = widget.board.share;
    if (share != null) {
      try {
        _startCollaboration(
          CollaborationLink.fromJson(share),
          lastSynced: widget.board.synced,
        );
      } catch (_) {
        // A corrupt link leaves the board usable as a local board.
      }
    }
    if (penMode == PenMode.text) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) unawaited(_prepareRecognition());
      });
    }
  }

  @override
  void dispose() {
    _debounce?.cancel();
    _snapTimer?.cancel();
    _autoTimer?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    document.removeListener(_changed);
    _collab?.removeListener(_collabChanged);
    _collab?.dispose();
    _cache?.dispose();
    document.dispose();
    _sceneTick.dispose();
    _liveTick.dispose();
    _zoomLabel.dispose();
    super.dispose();
  }

  // ---------------------------------------------------------------- saving

  void _changed() {
    if (!mounted) {
      return;
    }
    _collab?.submit(document.shapes);
    _sceneTick.value++;
    _liveTick.value++;
    if (saveStatus != 'Saving…') setState(() => saveStatus = 'Saving…');
    _scheduleSave();
  }

  /// Saves shortly after the last edit, but never while a finger or pen is
  /// down: writing the board to storage must not compete with drawing.
  void _scheduleSave() {
    _debounce?.cancel();
    _debounce = Timer(const Duration(milliseconds: 700), () {
      if (_pointers.isNotEmpty) {
        _scheduleSave();
      } else {
        save();
      }
    });
  }

  Future<bool> save() async {
    _debounce?.cancel();
    final snapshot = BoardData(
      id: widget.board.id,
      title: widget.board.title,
      updated: DateTime.now(),
      shapes: document.shapes,
      share: shareLink?.toJson(),
      synced: _collab?.syncedShapes,
    );
    var success = true;
    _saveQueue = _saveQueue
        .then((_) => widget.store.save(snapshot))
        .then((_) {
          widget.board
            ..shapes = snapshot.shapes
            ..updated = snapshot.updated
            ..share = snapshot.share
            ..synced = snapshot.synced;
          if (mounted && saveStatus != 'Saved') {
            setState(() => saveStatus = 'Saved');
          }
        })
        .catchError((Object _) {
          success = false;
          if (mounted) {
            setState(() => saveStatus = 'Save failed • tap to retry');
          }
        });
    await _saveQueue;
    return success;
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state != AppLifecycleState.resumed) {
      _cancelAction();
      save();
      unawaited(_collab?.flush());
    }
  }

  Future<void> leave() async {
    if (closing) {
      return;
    }
    closing = true;
    _cancelAction();
    await _collab?.flush();
    final ok = await save();
    if (!mounted) {
      return;
    }
    if (ok) {
      setState(() => allowPop = true);
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) {
          Navigator.pop(context);
        }
      });
    } else {
      closing = false;
    }
  }

  void message(String text) {
    if (mounted) {
      ScaffoldMessenger.of(context)
        ..hideCurrentSnackBar()
        ..showSnackBar(SnackBar(content: Text(text)));
    }
  }

  // --------------------------------------------------------- collaboration

  void _startCollaboration(
    CollaborationLink link, {
    List<CanvasShape>? lastSynced,
  }) {
    final client = CollaborationClient(
      link: link,
      clientId: _clientId,
      fetch: widget.collaborationFetch,
    );
    client.addListener(_collabChanged);
    shareLink = link;
    _collab = client;
    client.start(
      initial: document.shapes,
      lastSynced: lastSynced,
      onRemote: document.applyRemote,
    );
  }

  void _collabChanged() {
    if (mounted) {
      setState(() {});
    }
  }

  /// Publishes this board to the collaboration server and starts syncing.
  Future<void> startSharing() async {
    if (isShared) {
      return;
    }
    final created = await CollaborationClient.create(
      server: Uri.parse(collaborationServer),
      name: widget.board.title,
      shapes: document.shapes,
      clientId: _clientId,
      fetch: widget.collaborationFetch,
    );
    if (!mounted) {
      return;
    }
    _startCollaboration(created.link, lastSynced: created.shapes);
    setState(() {});
    await save();
  }

  /// Stops syncing on this device. The room and other participants are kept.
  Future<void> stopSharing() async {
    _collab?.removeListener(_collabChanged);
    _collab?.dispose();
    _collab = null;
    shareLink = null;
    if (mounted) {
      setState(() {});
    }
    await save();
  }

  // ------------------------------------------------------------- geometry

  Offset world(Offset point) => (point - camera) / zoom;
  String nextId() =>
      '$_clientId-${DateTime.now().microsecondsSinceEpoch.toRadixString(36)}-${_sequence++}';

  void _viewChanged() {
    _zoomLabel.value = zoom;
    _sceneTick.value++;
    _liveTick.value++;
  }

  void fit() {
    final bounds = sceneBounds(document.shapes);
    if (bounds == null || viewport.isEmpty) {
      zoom = 1;
      camera = Offset.zero;
    } else {
      zoom = math
          .min(
            (viewport.width - 160) / math.max(bounds.width, 1),
            (viewport.height - 200) / math.max(bounds.height, 1),
          )
          .clamp(.1, 2.0);
      camera = viewport.center(Offset.zero) - bounds.center * zoom;
    }
    _viewChanged();
  }

  void zoomTo(double next, [Offset? focus]) {
    final point = focus ?? viewport.center(Offset.zero);
    final anchor = world(point);
    zoom = next.clamp(.1, 8.0);
    camera = point - anchor * zoom;
    _viewChanged();
  }

  static CanvasShape _scaled(
    CanvasShape s,
    Offset origin,
    double sx,
    double sy,
  ) => s.copyWith(
    points: [
      for (final p in s.points)
        Offset(
          origin.dx + (p.dx - origin.dx) * sx,
          origin.dy + (p.dy - origin.dy) * sy,
        ),
    ],
    fontSize: s.tool == CanvasTool.text
        ? (s.fontSize * sy).clamp(6.0, 2000.0)
        : null,
  );

  static List<Offset> _corners(Rect r) => [
    r.topLeft,
    r.topRight,
    r.bottomLeft,
    r.bottomRight,
  ];

  // ------------------------------------------------------------- pointers

  void _pointerDown(PointerDownEvent e) {
    if (_pointers.isEmpty) {
      _ignored.clear();
      if (_mode == _Mode.blocked) _mode = _Mode.none;
    }
    if (_mode == _Mode.blocked || _pointers.length >= 2) {
      _ignored.add(e.pointer);
      return;
    }
    if (_pointers.length == 1) {
      // A second touch only becomes a pinch if it lands right as the gesture
      // starts. Later contacts (a resting palm or sleeve on a large touch
      // panel) are ignored so they cannot interrupt a stroke.
      final early =
          e.timeStamp - _downStamp < const Duration(milliseconds: 220) &&
          _travel < 36;
      final interruptible =
          _mode == _Mode.pan ||
          _mode == _Mode.none ||
          _mode == _Mode.text ||
          _mode == _Mode.marquee;
      if (!interruptible && !early) {
        _ignored.add(e.pointer);
        return;
      }
      _cancelAction();
      _pointers[e.pointer] = e.localPosition;
      final touches = _pointers.values.toList();
      _pinchDistance = math.max(24, (touches[0] - touches[1]).distance);
      _anchorWorld = world((touches[0] + touches[1]) / 2);
      _startZoom = zoom;
      _mode = _Mode.pinch;
      _navigating = true;
      return;
    }
    _pointers[e.pointer] = e.localPosition;
    _primary = e.pointer;
    _downStamp = e.timeStamp;
    _travel = 0;
    final mousePan =
        e.kind == PointerDeviceKind.mouse && e.buttons != kPrimaryMouseButton;
    _begin(e.localPosition, forcePan: mousePan);
  }

  void _pointerMove(PointerMoveEvent e) {
    final previous = _pointers[e.pointer];
    if (previous == null || _ignored.contains(e.pointer)) {
      return;
    }
    _pointers[e.pointer] = e.localPosition;
    if (_mode == _Mode.pinch) {
      if (_pointers.length < 2) return;
      final touches = _pointers.values.toList();
      final focal = (touches[0] + touches[1]) / 2;
      zoom = (_startZoom * (touches[0] - touches[1]).distance / _pinchDistance)
          .clamp(.1, 8.0);
      camera = focal - _anchorWorld * zoom;
      _viewChanged();
      return;
    }
    if (e.pointer != _primary) {
      return;
    }
    _travel += (e.localPosition - previous).distance;
    _continue(e.localPosition);
  }

  void _pointerEnd(PointerEvent e, {required bool cancelled}) {
    if (_ignored.remove(e.pointer)) {
      return;
    }
    if (_pointers.remove(e.pointer) == null) {
      return;
    }
    if (_mode == _Mode.pinch) {
      _mode = _pointers.isEmpty ? _Mode.none : _Mode.blocked;
      _navigating = false;
      _viewChanged();
      return;
    }
    if (_mode == _Mode.blocked) {
      if (_pointers.isEmpty) _mode = _Mode.none;
      return;
    }
    if (e.pointer == _primary) {
      if (cancelled) {
        _cancelAction();
      } else {
        _finish(e.localPosition);
      }
      _mode = _Mode.none;
      _primary = null;
      if (_pendingInk.isNotEmpty) {
        _autoTimer?.cancel();
        _autoTimer = Timer(
          const Duration(milliseconds: 1000),
          _convertPendingInk,
        );
      }
    }
  }

  void _begin(Offset position, {bool forcePan = false}) {
    final w = world(position);
    _startWorld = w;
    _autoTimer?.cancel();
    if (forcePan || tool == CanvasTool.hand) {
      _mode = _Mode.pan;
      _anchorWorld = w;
      _navigating = true;
      return;
    }
    switch (tool) {
      case CanvasTool.pen:
        _mode = _Mode.draw;
        _draftId = nextId();
        _draft = [w];
        _draftPath = Path()..moveTo(w.dx, w.dy);
        _lastScreen = position;
        _snapped = null;
        _resetStill(position);
        _liveTick.value++;
      case CanvasTool.rectangle:
      case CanvasTool.ellipse:
      case CanvasTool.arrow:
        _mode = _Mode.shape;
        _draftEnd = w;
      case CanvasTool.text:
        _mode = _Mode.text;
      case CanvasTool.eraser:
        _mode = _Mode.erase;
        _lastErase = w;
        _eraseAt(w);
      case CanvasTool.select:
        _beginSelect(position, w);
      case CanvasTool.hand:
        break;
    }
  }

  void _beginSelect(Offset position, Offset w) {
    final selected = document.selectedShapes;
    final bounds = sceneBounds(selected);
    _dragging = false;
    _delta = Offset.zero;
    _scaleX = _scaleY = 1;
    if (bounds != null) {
      final frame = bounds.inflate(6 / zoom);
      final corners = _corners(frame);
      for (var i = 0; i < 4; i++) {
        if ((corners[i] * zoom + camera - position).distance <= 22) {
          _mode = _Mode.resize;
          _handle = i;
          _resizeBounds = frame;
          _resizeOrigin = corners[3 - i];
          _moving = selected;
          return;
        }
      }
    }
    final hit = document.hitTest(w, tolerance: 12 / zoom);
    if (hit != null) {
      if (!document.selectedIds.contains(hit.id)) {
        document.select(hit.id);
      }
      _moving = document.selectedShapes;
      _mode = _Mode.move;
    } else if (bounds != null &&
        selected.length > 1 &&
        bounds.inflate(6 / zoom).contains(w)) {
      _moving = selected;
      _mode = _Mode.move;
    } else {
      document.select(null);
      _mode = _Mode.marquee;
      _marquee = Rect.fromPoints(w, w);
    }
  }

  void _continue(Offset position) {
    final w = world(position);
    switch (_mode) {
      case _Mode.draw:
        final draft = _draft;
        if (draft == null || _snapped != null) return;
        if ((position - _lastScreen).distanceSquared < 1) return;
        final previous = draft.last;
        if (draft.length >= 2) {
          _draftPath.quadraticBezierTo(
            previous.dx,
            previous.dy,
            (previous.dx + w.dx) / 2,
            (previous.dy + w.dy) / 2,
          );
        }
        draft.add(w);
        _lastScreen = position;
        if ((position - _stillAnchor).distance > 16) _resetStill(position);
        _liveTick.value++;
      case _Mode.shape:
        _draftEnd = w;
        _liveTick.value++;
      case _Mode.erase:
        final steps = math.max(
          1,
          ((w - _lastErase).distance * zoom / 8).ceil(),
        );
        for (var i = 1; i <= steps; i++) {
          _eraseAt(_lastErase + (w - _lastErase) * (i / steps));
        }
        _lastErase = w;
      case _Mode.move:
        _delta = w - _startWorld;
        _startDragging();
        _liveTick.value++;
      case _Mode.resize:
        final corner = _corners(_resizeBounds)[_handle];
        final dx = corner.dx - _resizeOrigin.dx,
            dy = corner.dy - _resizeOrigin.dy;
        _scaleX = dx.abs() < 1
            ? 1
            : ((w.dx - _resizeOrigin.dx) / dx).clamp(.05, 100.0);
        _scaleY = dy.abs() < 1
            ? 1
            : ((w.dy - _resizeOrigin.dy) / dy).clamp(.05, 100.0);
        _startDragging();
        _liveTick.value++;
      case _Mode.marquee:
        _marquee = Rect.fromPoints(_startWorld, w);
        _liveTick.value++;
      case _Mode.pan:
        camera = position - _anchorWorld * zoom;
        _viewChanged();
      case _Mode.none:
      case _Mode.pinch:
      case _Mode.text:
      case _Mode.blocked:
        break;
    }
  }

  void _startDragging() {
    if (_dragging || _travel < 4) return;
    _dragging = true;
    _hidden = {for (final shape in _moving) shape.id};
    _hiddenVersion++;
    _sceneTick.value++;
  }

  void _finish(Offset position) {
    final w = world(position);
    switch (_mode) {
      case _Mode.draw:
        _snapTimer?.cancel();
        final draft = _draft;
        if (draft == null) break;
        final raw = _draftShape();
        final snapped =
            _snapped ?? (penMode == PenMode.shape ? recognizeShape(raw) : null);
        final shape = snapped ?? raw;
        _draft = null;
        _snapped = null;
        _addFast(shape);
        if (snapped == null && penMode == PenMode.text) {
          _pendingInk.add(shape.id);
        }
        _hintIfSlow();
      case _Mode.shape:
        if ((_draftEnd - _startWorld).distance * zoom > 6) {
          final shape = CanvasShape(
            id: nextId(),
            tool: tool,
            points: [_startWorld, _draftEnd],
            color: color,
            width: width,
            filled: filled,
          );
          _addFast(shape);
          // Like tldraw, drawing a shape returns to Select with it selected.
          tool = CanvasTool.select;
          document.select(shape.id);
        }
      case _Mode.text:
        if (_travel < 16) {
          unawaited(editText(w));
        }
      case _Mode.erase:
        final ids = _hidden;
        _hidden = {};
        _hiddenVersion++;
        document.removeMany(ids);
      case _Mode.move:
        if (_dragging) {
          final delta = _delta;
          final moved = [
            for (final s in _moving)
              s.copyWith(points: [for (final p in s.points) p + delta]),
          ];
          _clearTransform();
          document.replaceMany(moved);
        } else {
          final hit = document.hitTest(w, tolerance: 12 / zoom);
          final now = DateTime.now();
          if (hit != null &&
              hit.tool == CanvasTool.text &&
              hit.id == _lastTapId &&
              now.difference(_lastTapTime) <
                  const Duration(milliseconds: 450)) {
            unawaited(editText(hit.points.first, existing: hit));
          }
          _lastTapId = hit?.id;
          _lastTapTime = now;
          _clearTransform();
        }
      case _Mode.resize:
        if (_dragging) {
          final resized = [
            for (final s in _moving)
              _scaled(s, _resizeOrigin, _scaleX, _scaleY),
          ];
          _clearTransform();
          document.replaceMany(resized);
        } else {
          _clearTransform();
        }
      case _Mode.marquee:
        final area = _marquee;
        _marquee = null;
        if (area != null && area.longestSide * zoom > 6) {
          document.selectMany(document.shapesIn(area).map((s) => s.id));
        }
      case _Mode.pan:
        _navigating = false;
      case _Mode.none:
      case _Mode.pinch:
      case _Mode.blocked:
        break;
    }
    _sceneTick.value++;
    _liveTick.value++;
    if (mounted) {
      setState(() {});
    }
  }

  void _clearTransform() {
    _moving = const [];
    _dragging = false;
    _delta = Offset.zero;
    _scaleX = _scaleY = 1;
    if (_hidden.isNotEmpty) {
      _hidden = {};
      _hiddenVersion++;
    }
  }

  /// Abandons the in-progress action without changing the document.
  void _cancelAction() {
    _snapTimer?.cancel();
    _draft = null;
    _snapped = null;
    _marquee = null;
    _clearTransform();
    if (_mode == _Mode.pan) _navigating = false;
    if (_mode != _Mode.pinch && _mode != _Mode.blocked) _mode = _Mode.none;
    _sceneTick.value++;
    _liveTick.value++;
  }

  CanvasShape _draftShape() => CanvasShape(
    id: _draftId,
    tool: CanvasTool.pen,
    points: _draft!,
    color: color,
    width: width,
  );

  void _eraseAt(Offset w) {
    var found = false;
    final tolerance = 12 / zoom;
    for (final shape in document.shapes) {
      if (_hidden.contains(shape.id)) continue;
      if (CanvasDocument.hits(shape, w, tolerance)) {
        _hidden.add(shape.id);
        found = true;
      }
    }
    if (found) {
      _hiddenVersion++;
      _sceneTick.value++;
      _liveTick.value++;
    }
  }

  // ------------------------------------------------------ shape snapping

  void _resetStill(Offset position) {
    _stillAnchor = position;
    _snapTimer?.cancel();
    if (snapShapes) {
      _snapTimer = Timer(const Duration(milliseconds: 500), _trySnap);
    }
  }

  /// Holding the pen still at the end of a stroke replaces it with the clean
  /// shape it resembles (line, arrow, rectangle or ellipse).
  void _trySnap() {
    final draft = _draft;
    if (!mounted || _mode != _Mode.draw || draft == null || draft.length < 6) {
      return;
    }
    final shape = recognizeShape(_draftShape());
    if (shape != null) {
      _snapped = shape;
      unawaited(HapticFeedback.selectionClick());
      _liveTick.value++;
    }
  }

  // ---------------------------------------------------------- handwriting

  InkRecognizer get _recognize =>
      widget.recognize ?? HandwritingRecognition.shared.recognize;

  /// Tells the user once when this display draws frames too slowly, and
  /// offers the graphics setting that usually fixes it.
  void _hintIfSlow() {
    if (_slowHintShown || !FrameStats.instance.slow || !mounted) return;
    _slowHintShown = true;
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(
        SnackBar(
          duration: const Duration(seconds: 12),
          content: const Text('Drawing is slow on this display.'),
          action: SnackBarAction(
            label: 'Fix',
            onPressed: () => showDisplayPanel(context, beforeRestart: save),
          ),
        ),
      );
  }

  void _convertPendingInk() {
    _autoTimer?.cancel();
    final strokes = document.shapes
        .where((s) => _pendingInk.contains(s.id))
        .toList();
    _pendingInk.clear();
    if (strokes.isNotEmpty) {
      unawaited(convertToText(strokes, quiet: true));
    }
  }

  /// Replaces handwritten strokes with typed text, one text shape per line.
  Future<void> convertToText(
    List<CanvasShape> strokes, {
    bool quiet = false,
  }) async {
    final lines = groupInkLines(strokes);
    if (lines.isEmpty) {
      if (!quiet) message('Select handwriting drawn with the pen first.');
      return;
    }
    if (!quiet) setState(() => busy = 'Reading handwriting…');
    try {
      final texts = <String>[];
      for (final line in lines) {
        texts.add((await _recognize(line, language: language)).first);
      }
      if (!mounted) return;
      final ids = [for (final line in lines) ...line.map((s) => s.id)];
      if (!ids.every(document.contains)) return;
      var height = 0.0;
      for (final line in lines) {
        height = math.max(height, sceneBounds(line)!.height);
      }
      final fontSize = (height * .8).clamp(14.0, 800.0);
      final replacement = <CanvasShape>[];
      for (var i = 0; i < lines.length; i++) {
        final bounds = sceneBounds(lines[i])!;
        replacement.add(
          CanvasShape(
            id: nextId(),
            tool: CanvasTool.text,
            points: [Offset(bounds.left, bounds.center.dy - fontSize * .65)],
            color: lines[i].first.color,
            text: texts[i],
            fontSize: fontSize,
          ),
        );
      }
      document.swap(ids, replacement);
      if (!quiet) {
        document.selectMany(replacement.map((s) => s.id));
        message('Handwriting converted. Undo restores your ink.');
      }
    } on InkRecognitionException catch (error) {
      if (!quiet || error.message.contains('model')) message(error.message);
    } catch (_) {
      if (!quiet) message('Could not read this handwriting. Ink was kept.');
    } finally {
      if (mounted && busy != null) setState(() => busy = null);
    }
  }

  Future<void> _setLanguage(String next) async {
    _languagePreference = next;
    setState(() => language = next);
    await _prepareRecognition(announce: true);
  }

  /// Makes sure the handwriting model for the current language is on the
  /// device, downloading it if needed. Nothing is shown when it is ready.
  Future<void> _prepareRecognition({bool announce = false}) async {
    if (widget.recognize != null) return;
    final shared = HandwritingRecognition.shared;
    if (shared.isReady(language)) return;
    final name = language == 'hi' ? 'Hindi' : 'English';
    setState(() => busy = 'Getting $name handwriting ready…');
    try {
      await shared.prepare(language);
      if (announce) message('$name handwriting is ready.');
    } on InkRecognitionException catch (error) {
      message(error.message);
    } finally {
      if (mounted) setState(() => busy = null);
    }
  }

  // --------------------------------------------------------------- edits

  Future<void> editText(Offset position, {CanvasShape? existing}) async {
    final text = await askText(
      context,
      existing == null ? 'Add text' : 'Edit text',
      initial: existing?.text ?? '',
      multiline: true,
    );
    if (!mounted || text == null || text.isEmpty) {
      return;
    }
    if (existing != null) {
      document.replace(existing.copyWith(text: text));
    } else {
      final shape = CanvasShape(
        id: nextId(),
        tool: CanvasTool.text,
        points: [position],
        color: color,
        text: text,
        fontSize: (28 / zoom).clamp(12.0, 400.0),
      );
      document.add(shape);
      setState(() => tool = CanvasTool.select);
      document.select(shape.id);
    }
  }

  void selectTool(CanvasTool next, [PenMode mode = PenMode.ink]) {
    _cancelAction();
    if (_pendingInk.isNotEmpty) _convertPendingInk();
    setState(() {
      tool = next;
      if (next == CanvasTool.pen) _penModePreference = penMode = mode;
    });
    if (next == CanvasTool.pen && mode == PenMode.text) {
      unawaited(_prepareRecognition());
    }
    if (next != CanvasTool.select) {
      document.select(null);
    }
    _liveTick.value++;
  }

  void applyStyle({int? nextColor, double? nextWidth, bool? nextFilled}) {
    setState(() {
      color = nextColor ?? color;
      width = nextWidth ?? width;
      filled = nextFilled ?? filled;
    });
    final selected = document.selectedShapes;
    if (selected.isNotEmpty) {
      document.replaceMany([
        for (final s in selected)
          s.copyWith(color: nextColor, width: nextWidth, filled: nextFilled),
      ]);
    }
  }

  void deleteSelection() => document.removeMany(document.selectedIds.toList());

  Future<void> export() async {
    try {
      final bounds =
          (sceneBounds(document.shapes) ?? const Rect.fromLTWH(0, 0, 600, 400))
              .inflate(32);
      final scale = math.min(2.0, 4096 / math.max(bounds.width, bounds.height));
      final recorder = ui.PictureRecorder();
      final canvas = Canvas(recorder);
      canvas.scale(scale);
      canvas.drawRect(
        Rect.fromLTWH(0, 0, bounds.width, bounds.height),
        Paint()..color = Colors.white,
      );
      canvas.translate(-bounds.left, -bounds.top);
      for (final s in document.shapes) {
        drawShape(canvas, s);
      }
      final picture = recorder.endRecording();
      final image = await picture.toImage(
        math.max(1, (bounds.width * scale).ceil()),
        math.max(1, (bounds.height * scale).ceil()),
      );
      final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
      image.dispose();
      picture.dispose();
      if (bytes == null) {
        throw StateError('No image');
      }
      await SharePlus.instance.share(
        ShareParams(
          files: [
            XFile.fromData(bytes.buffer.asUint8List(), mimeType: 'image/png'),
          ],
          fileNameOverrides: ['canvas-studio.png'],
          title: widget.board.title,
        ),
      );
    } catch (_) {
      message('Could not export this image. Please retry.');
    }
  }

  // ------------------------------------------------------------ painting

  bool get _cacheValid =>
      _cache != null &&
      _cacheRevision == document.revision &&
      _cacheHidden == _hiddenVersion &&
      _cacheCamera == camera &&
      _cacheZoom == zoom &&
      _cacheSize == viewport &&
      _cacheDpr == _dpr &&
      _cacheGrid == grid;

  void _markCache() {
    _cacheRevision = document.revision;
    _cacheHidden = _hiddenVersion;
    _cacheCamera = camera;
    _cacheZoom = zoom;
    _cacheSize = viewport;
    _cacheDpr = _dpr;
    _cacheGrid = grid;
  }

  /// Adds a finished shape by painting it on top of the cached scene, so
  /// lifting the pen costs one small draw instead of repainting the board.
  void _addFast(CanvasShape shape) {
    final previous = _cacheValid ? _cache : null;
    document.add(shape);
    if (previous == null) {
      return;
    }
    final recorder = ui.PictureRecorder();
    final canvas = Canvas(recorder);
    canvas.drawImage(previous, Offset.zero, Paint());
    canvas.scale(_dpr);
    canvas.translate(camera.dx, camera.dy);
    canvas.scale(zoom);
    drawShape(canvas, shape);
    final picture = recorder.endRecording();
    _cache = picture.toImageSync(previous.width, previous.height);
    picture.dispose();
    previous.dispose();
    _markCache();
  }

  void _paintScene(Canvas canvas, Size size) {
    if (size.isEmpty) {
      return;
    }
    viewport = size;
    if (!_cacheValid) {
      if (_navigating) {
        final cache = _cache;
        // Large boards pan and zoom by transforming the last raster; small
        // ones are cheap enough to redraw crisply every frame.
        if (cache != null &&
            document.shapes.length > 250 &&
            _cacheRevision == document.revision &&
            _cacheHidden == _hiddenVersion &&
            _cacheSize == size) {
          final ratio = zoom / _cacheZoom;
          canvas.drawRect(
            Offset.zero & size,
            Paint()..color = canvasBackground,
          );
          canvas.save();
          canvas.translate(
            camera.dx - _cacheCamera.dx * ratio,
            camera.dy - _cacheCamera.dy * ratio,
          );
          canvas.scale(ratio / _cacheDpr);
          canvas.drawImage(
            cache,
            Offset.zero,
            Paint()..filterQuality = FilterQuality.low,
          );
          canvas.restore();
          return;
        }
        drawScene(
          canvas,
          size,
          shapes: document.shapes,
          camera: camera,
          zoom: zoom,
          grid: grid,
          hidden: _hidden,
        );
        return;
      }
      final recorder = ui.PictureRecorder();
      final target = Canvas(recorder);
      target.scale(_dpr);
      drawScene(
        target,
        size,
        shapes: document.shapes,
        camera: camera,
        zoom: zoom,
        grid: grid,
        hidden: _hidden,
      );
      final picture = recorder.endRecording();
      _cache?.dispose();
      _cache = picture.toImageSync(
        math.max(1, (size.width * _dpr).ceil()),
        math.max(1, (size.height * _dpr).ceil()),
      );
      picture.dispose();
      _markCache();
    }
    canvas.save();
    canvas.scale(1 / _dpr);
    canvas.drawImage(_cache!, Offset.zero, Paint());
    canvas.restore();
  }

  void _paintLive(Canvas canvas, Size size) {
    canvas.save();
    canvas.clipRect(Offset.zero & size);
    canvas.translate(camera.dx, camera.dy);
    canvas.scale(zoom);
    final transforming =
        _dragging && (_mode == _Mode.move || _mode == _Mode.resize);
    if (_mode == _Mode.erase && _hidden.isNotEmpty) {
      for (final s in document.shapes) {
        if (_hidden.contains(s.id)) drawShape(canvas, s, alpha: .22);
      }
    }
    if (transforming) {
      canvas.save();
      _applyTransform(canvas);
      for (final s in _moving) {
        drawShape(canvas, s);
      }
      canvas.restore();
    }
    final snapped = _snapped, draft = _draft;
    if (snapped != null) {
      drawShape(canvas, snapped);
    } else if (draft != null) {
      final paint = Paint()
        ..color = Color(color)
        ..strokeWidth = width
        ..strokeCap = StrokeCap.round
        ..strokeJoin = StrokeJoin.round
        ..style = PaintingStyle.stroke;
      if (draft.length == 1) {
        canvas.drawCircle(
          draft.first,
          width / 2,
          paint..style = PaintingStyle.fill,
        );
      } else {
        canvas.drawPath(_draftPath, paint);
        final last = draft.last, before = draft[draft.length - 2];
        canvas.drawLine(
          draft.length == 2 ? before : (before + last) / 2,
          last,
          paint,
        );
      }
    }
    if (_mode == _Mode.shape) {
      drawShape(
        canvas,
        CanvasShape(
          id: 'draft',
          tool: tool,
          points: [_startWorld, _draftEnd],
          color: color,
          width: width,
          filled: filled,
        ),
      );
    }
    if (tool == CanvasTool.select && document.selectedIds.isNotEmpty) {
      final outline = Paint()
        ..color = selectionBlue
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1.5 / zoom;
      Rect map(Rect r) {
        if (!transforming) return r;
        if (_mode == _Mode.move) return r.shift(_delta);
        Offset scale(Offset p) => Offset(
          _resizeOrigin.dx + (p.dx - _resizeOrigin.dx) * _scaleX,
          _resizeOrigin.dy + (p.dy - _resizeOrigin.dy) * _scaleY,
        );
        return Rect.fromPoints(scale(r.topLeft), scale(r.bottomRight));
      }

      final selected = transforming ? _moving : document.selectedShapes;
      Rect? all;
      for (final s in selected) {
        final r = map(s.bounds);
        all = all == null ? r : all.expandToInclude(r);
        if (selected.length > 1) {
          canvas.drawRect(r.inflate(2 / zoom), outline);
        }
      }
      if (all != null) {
        final frame = all.inflate(6 / zoom);
        canvas.drawRect(frame, outline);
        final half = 5 / zoom;
        final fill = Paint()..color = Colors.white;
        for (final corner in _corners(frame)) {
          final handle = Rect.fromCircle(center: corner, radius: half);
          final rounded = RRect.fromRectAndRadius(
            handle,
            Radius.circular(1.5 / zoom),
          );
          canvas.drawRRect(rounded, fill);
          canvas.drawRRect(rounded, outline);
        }
      }
    }
    final marquee = _marquee;
    if (marquee != null) {
      canvas.drawRect(
        marquee,
        Paint()..color = selectionBlue.withValues(alpha: .1),
      );
      canvas.drawRect(
        marquee,
        Paint()
          ..color = selectionBlue
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1 / zoom,
      );
    }
    canvas.restore();
  }

  void _applyTransform(Canvas canvas) {
    if (_mode == _Mode.move) {
      canvas.translate(_delta.dx, _delta.dy);
    } else {
      canvas.translate(_resizeOrigin.dx, _resizeOrigin.dy);
      canvas.scale(_scaleX, _scaleY);
      canvas.translate(-_resizeOrigin.dx, -_resizeOrigin.dy);
    }
  }

  // ------------------------------------------------------------------ UI

  Widget _panel({required Widget child, EdgeInsets? padding}) => Material(
    color: Colors.white,
    elevation: 3,
    shadowColor: const Color(0x33000000),
    shape: RoundedRectangleBorder(
      borderRadius: BorderRadius.circular(11),
      side: const BorderSide(color: panelBorder),
    ),
    clipBehavior: Clip.antiAlias,
    child: Padding(padding: padding ?? const EdgeInsets.all(4), child: child),
  );

  Widget _iconButton(
    IconData icon,
    String label,
    VoidCallback? onTap, {
    Key? key,
    bool active = false,
    double size = 40,
  }) => Tooltip(
    message: label,
    child: Semantics(
      button: true,
      selected: active,
      child: InkWell(
        key: key,
        onTap: onTap,
        borderRadius: BorderRadius.circular(8),
        child: Container(
          width: size,
          height: size,
          margin: const EdgeInsets.all(1),
          decoration: BoxDecoration(
            color: active ? selectionBlue : null,
            borderRadius: BorderRadius.circular(8),
          ),
          child: Icon(
            icon,
            size: size * .5,
            color: active
                ? Colors.white
                : onTap == null
                ? const Color(0xffc2c5cb)
                : inkBlack,
          ),
        ),
      ),
    ),
  );

  Widget _toolbar(double size) {
    const ink = PenMode.ink;
    const items = <(String, CanvasTool, PenMode, IconData, String)>[
      ('select', CanvasTool.select, ink, Icons.near_me_outlined, 'Select'),
      ('hand', CanvasTool.hand, ink, Icons.pan_tool_outlined, 'Hand'),
      ('pen', CanvasTool.pen, ink, Icons.edit_outlined, 'Draw'),
      (
        'write',
        CanvasTool.pen,
        PenMode.text,
        Icons.spellcheck,
        'Write: handwriting becomes typed text',
      ),
      (
        'smart',
        CanvasTool.pen,
        PenMode.shape,
        Icons.auto_awesome_outlined,
        'Smart shape: rough shapes become clean',
      ),
      ('eraser', CanvasTool.eraser, ink, Icons.auto_fix_normal_outlined, 'Eraser'),
      ('arrow', CanvasTool.arrow, ink, Icons.north_east, 'Arrow'),
      ('text', CanvasTool.text, ink, Icons.title, 'Text'),
      ('rectangle', CanvasTool.rectangle, ink, Icons.crop_square, 'Rectangle'),
      ('ellipse', CanvasTool.ellipse, ink, Icons.circle_outlined, 'Ellipse'),
    ];
    return _panel(
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final item in items)
              _iconButton(
                item.$4,
                item.$5,
                () => selectTool(item.$2, item.$3),
                key: ValueKey('tool-${item.$1}'),
                active:
                    tool == item.$2 &&
                    (tool != CanvasTool.pen || penMode == item.$3),
                size: size,
              ),
          ],
        ),
      ),
    );
  }

  Widget _stylePanel() {
    final selected = document.selectedShapes;
    final current = selected.isEmpty ? null : selected.first;
    final activeColor = current?.color ?? color;
    final activeWidth = current?.width ?? width;
    final activeFill = current?.filled ?? filled;
    return _panel(
      padding: const EdgeInsets.all(8),
      child: SizedBox(
        width: 168,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Wrap(
              children: [
                for (final entry in palette)
                  Semantics(
                    label: '${entry.$1} color',
                    button: true,
                    selected: activeColor == entry.$2,
                    child: InkWell(
                      key: ValueKey('color-${entry.$1}'),
                      borderRadius: BorderRadius.circular(8),
                      onTap: () => applyStyle(nextColor: entry.$2),
                      child: Container(
                        width: 42,
                        height: 42,
                        alignment: Alignment.center,
                        decoration: BoxDecoration(
                          color: activeColor == entry.$2
                              ? const Color(0xffeceef1)
                              : null,
                          borderRadius: BorderRadius.circular(8),
                        ),
                        child: Container(
                          width: 22,
                          height: 22,
                          decoration: BoxDecoration(
                            color: Color(entry.$2),
                            shape: BoxShape.circle,
                          ),
                        ),
                      ),
                    ),
                  ),
              ],
            ),
            const Divider(height: 14, color: panelBorder),
            Row(
              children: [
                for (final entry in strokeSizes)
                  Expanded(
                    child: InkWell(
                      key: ValueKey('size-${entry.$1}'),
                      borderRadius: BorderRadius.circular(8),
                      onTap: () => applyStyle(nextWidth: entry.$2),
                      child: Container(
                        height: 40,
                        alignment: Alignment.center,
                        decoration: BoxDecoration(
                          color: activeWidth == entry.$2
                              ? const Color(0xffeceef1)
                              : null,
                          borderRadius: BorderRadius.circular(8),
                        ),
                        child: Text(
                          entry.$1,
                          style: const TextStyle(
                            fontWeight: FontWeight.w600,
                            color: inkBlack,
                          ),
                        ),
                      ),
                    ),
                  ),
              ],
            ),
            const Divider(height: 14, color: panelBorder),
            Row(
              children: [
                const Padding(
                  padding: EdgeInsets.only(left: 6),
                  child: Text('Fill', style: TextStyle(color: inkBlack)),
                ),
                const Spacer(),
                _iconButton(
                  Icons.crop_square,
                  'No fill',
                  () => applyStyle(nextFilled: false),
                  active: !activeFill,
                  size: 36,
                ),
                _iconButton(
                  Icons.square_rounded,
                  'Tinted fill',
                  () => applyStyle(nextFilled: true),
                  active: activeFill,
                  size: 36,
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Widget _menu() => PopupMenuButton<String>(
    tooltip: 'Menu',
    icon: const Icon(Icons.menu, size: 20, color: inkBlack),
    onSelected: (value) {
      switch (value) {
        case 'export':
          export();
        case 'fit':
          fit();
        case 'grid':
          setState(() => grid = !grid);
          _sceneTick.value++;
        case 'snap':
          setState(() => _snapPreference = snapShapes = !snapShapes);
        case 'claude':
          showClaudePanel(context, this);
        case 'display':
          showDisplayPanel(context, beforeRestart: save);
        case 'en':
        case 'hi':
          unawaited(_setLanguage(value));
      }
    },
    itemBuilder: (_) => [
      const PopupMenuItem(value: 'export', child: Text('Export as PNG')),
      const PopupMenuItem(value: 'fit', child: Text('Zoom to fit')),
      CheckedPopupMenuItem(
        value: 'grid',
        checked: grid,
        child: const Text('Show grid'),
      ),
      const PopupMenuDivider(),
      CheckedPopupMenuItem(
        value: 'snap',
        checked: snapShapes,
        child: const Text('Hold to snap shapes'),
      ),
      const PopupMenuDivider(),
      CheckedPopupMenuItem(
        value: 'en',
        checked: language == 'en',
        child: const Text('Handwriting: English'),
      ),
      CheckedPopupMenuItem(
        value: 'hi',
        checked: language == 'hi',
        child: const Text('Handwriting: हिन्दी'),
      ),
      const PopupMenuDivider(),
      const PopupMenuItem(
        key: ValueKey('menu-claude'),
        value: 'claude',
        child: Text('Connect Claude…'),
      ),
      const PopupMenuItem(
        value: 'display',
        child: Text('Display and speed…'),
      ),
    ],
  );

  Widget _topLeft(bool wide) {
    final selection = document.selectedShapes;
    final hasInk = selection.any((s) => s.tool == CanvasTool.pen);
    final onlyText =
        selection.length == 1 && selection.first.tool == CanvasTool.text;
    final main = _panel(
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          _iconButton(Icons.arrow_back, 'Back to boards', leave),
          _menu(),
          if (wide)
            ConstrainedBox(
              constraints: const BoxConstraints(maxWidth: 220),
              child: Padding(
                padding: const EdgeInsets.symmetric(horizontal: 6),
                child: GestureDetector(
                  onTap: save,
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        widget.board.title,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(
                          fontSize: 14,
                          fontWeight: FontWeight.w600,
                          color: inkBlack,
                        ),
                      ),
                      Text(
                        status,
                        key: const ValueKey('board-status'),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(
                          fontSize: 11,
                          color: Color(0xff8b9098),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
          _iconButton(
            Icons.undo,
            'Undo',
            document.canUndo ? document.undo : null,
          ),
          _iconButton(
            Icons.redo,
            'Redo',
            document.canRedo ? document.redo : null,
          ),
        ],
      ),
    );
    if (selection.isEmpty) return main;
    // Actions for the current selection sit on their own row so the top bar
    // never grows into the Share button on phones.
    final actions = _panel(
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          _iconButton(Icons.delete_outline, 'Delete', deleteSelection),
          if (hasInk)
            _iconButton(
              Icons.text_fields,
              'Convert handwriting to text',
              () => convertToText(selection),
            ),
          if (onlyText)
            _iconButton(
              Icons.edit_note,
              'Edit text',
              () => editText(
                selection.first.points.first,
                existing: selection.first,
              ),
            ),
        ],
      ),
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [main, const SizedBox(height: 8), actions],
    );
  }

  Widget _topRight() => Row(
    mainAxisSize: MainAxisSize.min,
    children: [
      FilledButton.icon(
        key: const ValueKey('share-button'),
        style: FilledButton.styleFrom(
          backgroundColor: selectionBlue,
          foregroundColor: Colors.white,
          minimumSize: const Size(0, 44),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(10),
          ),
        ),
        onPressed: () => showSharePanel(context, this),
        icon: Icon(isShared ? Icons.group : Icons.group_add_outlined, size: 18),
        label: Text(isShared ? 'Shared' : 'Share'),
      ),
      const SizedBox(width: 8),
      _panel(
        child: Tooltip(
          message: 'Styles',
          child: InkWell(
            key: const ValueKey('style-toggle'),
            borderRadius: BorderRadius.circular(8),
            onTap: () => setState(() => styleOpen = !styleOpen),
            child: SizedBox(
              width: 40,
              height: 40,
              child: Center(
                child: Container(
                  width: 22,
                  height: 22,
                  decoration: BoxDecoration(
                    color: Color(color),
                    shape: BoxShape.circle,
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    ],
  );

  Widget _zoomPanel() => _panel(
    child: Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        _iconButton(Icons.remove, 'Zoom out', () => zoomTo(zoom / 1.25)),
        InkWell(
          onTap: () => zoomTo(1),
          borderRadius: BorderRadius.circular(8),
          child: SizedBox(
            width: 52,
            height: 40,
            child: Center(
              child: ValueListenableBuilder<double>(
                valueListenable: _zoomLabel,
                builder: (_, value, _) => Text(
                  '${(value * 100).round()}%',
                  style: const TextStyle(fontSize: 12, color: inkBlack),
                ),
              ),
            ),
          ),
        ),
        _iconButton(Icons.add, 'Zoom in', () => zoomTo(zoom * 1.25)),
        _iconButton(Icons.fit_screen_outlined, 'Zoom to fit', fit),
      ],
    ),
  );

  @override
  Widget build(BuildContext context) {
    _dpr = MediaQuery.devicePixelRatioOf(context);
    return PopScope(
      canPop: allowPop,
      onPopInvokedWithResult: (popped, result) {
        if (!popped) {
          leave();
        }
      },
      child: Scaffold(
        backgroundColor: canvasBackground,
        resizeToAvoidBottomInset: false,
        body: SafeArea(
          child: LayoutBuilder(
            builder: (context, size) {
              final wide = size.maxWidth >= 700;
              final large = size.maxWidth >= 1200;
              return Stack(
                children: [
                  Positioned.fill(
                    child: Listener(
                      key: const ValueKey('native-canvas'),
                      behavior: HitTestBehavior.opaque,
                      onPointerDown: _pointerDown,
                      onPointerMove: _pointerMove,
                      onPointerUp: (e) => _pointerEnd(e, cancelled: false),
                      onPointerCancel: (e) => _pointerEnd(e, cancelled: true),
                      onPointerSignal: (event) {
                        if (event is PointerScrollEvent) {
                          zoomTo(
                            zoom * math.exp(-event.scrollDelta.dy / 400),
                            event.localPosition,
                          );
                        }
                      },
                      child: Stack(
                        fit: StackFit.expand,
                        children: [
                          RepaintBoundary(
                            child: CustomPaint(
                              painter: _CallbackPainter(
                                _paintScene,
                                _sceneTick,
                              ),
                            ),
                          ),
                          RepaintBoundary(
                            child: CustomPaint(
                              painter: _CallbackPainter(_paintLive, _liveTick),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                  Positioned(left: 10, top: 10, child: _topLeft(wide)),
                  Positioned(
                    right: 10,
                    top: 10,
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.end,
                      children: [
                        _topRight(),
                        if (styleOpen || large) ...[
                          const SizedBox(height: 8),
                          _stylePanel(),
                        ],
                      ],
                    ),
                  ),
                  if (wide)
                    Positioned(left: 10, bottom: 10, child: _zoomPanel()),
                  Positioned(
                    left: 10,
                    right: 10,
                    bottom: 10,
                    child: Center(child: _toolbar(large ? 52 : 42)),
                  ),
                  if (busy == null &&
                      tool == CanvasTool.pen &&
                      penMode != PenMode.ink)
                    Positioned(
                      top: 64,
                      left: 0,
                      right: 0,
                      child: IgnorePointer(
                        child: Center(
                          child: _panel(
                            padding: const EdgeInsets.symmetric(
                              horizontal: 12,
                              vertical: 7,
                            ),
                            child: Text(
                              penMode == PenMode.text
                                  ? 'Write, then pause: it becomes typed '
                                        '${language == 'hi' ? 'Hindi' : 'English'}'
                                  : 'Draw a box, circle, line or arrow',
                              key: const ValueKey('pen-mode-hint'),
                              style: const TextStyle(
                                fontSize: 12,
                                color: Color(0xff5f6570),
                              ),
                            ),
                          ),
                        ),
                      ),
                    ),
                  if (busy != null)
                    Positioned(
                      top: 64,
                      left: 0,
                      right: 0,
                      child: Center(
                        child: _panel(
                          padding: const EdgeInsets.symmetric(
                            horizontal: 14,
                            vertical: 10,
                          ),
                          child: Row(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              const SizedBox(
                                width: 16,
                                height: 16,
                                child: CircularProgressIndicator(
                                  strokeWidth: 2,
                                ),
                              ),
                              const SizedBox(width: 10),
                              Text(busy!),
                            ],
                          ),
                        ),
                      ),
                    ),
                ],
              );
            },
          ),
        ),
      ),
    );
  }
}

class _CallbackPainter extends CustomPainter {
  _CallbackPainter(this.onPaint, Listenable repaint) : super(repaint: repaint);
  final void Function(Canvas, Size) onPaint;
  @override
  void paint(Canvas canvas, Size size) => onPaint(canvas, size);
  @override
  bool shouldRepaint(covariant _CallbackPainter oldDelegate) => false;
}
