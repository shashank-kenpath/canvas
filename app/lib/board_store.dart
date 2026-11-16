import 'dart:convert';
import 'dart:io';
import 'dart:isolate';
import 'dart:typed_data';
import 'dart:ui' show Offset;

import 'package:path_provider/path_provider.dart';

import 'canvas_document.dart';

class BoardData {
  BoardData({
    required this.id,
    required this.title,
    required this.updated,
    required this.shapes,
    this.share,
    this.synced,
  });
  final String id;
  String title;
  DateTime updated;
  List<CanvasShape> shapes;

  /// Shared-board link (server, room, token) when this board is collaborative.
  Map<String, dynamic>? share;

  /// Last state acknowledged by the collaboration server, used to rebuild
  /// unsent edits after a restart.
  List<CanvasShape>? synced;
  Map<String, dynamic> toJson() => {
    'version': 1,
    'id': id,
    'title': title,
    'updated': updated.toIso8601String(),
    'shapes': shapes.map((s) => s.toJson()).toList(),
    if (share != null) 'share': share,
    if (share != null && synced != null)
      'synced': synced!.map((s) => s.toJson()).toList(),
  };
  factory BoardData.fromJson(Map<String, dynamic> json) => BoardData(
    id: json['id'] as String,
    title: json['title'] as String,
    updated: DateTime.parse(json['updated'] as String),
    shapes: (json['shapes'] as List)
        .map((s) => CanvasShape.fromJson(Map<String, dynamic>.from(s as Map)))
        .toList(),
    share: json['share'] == null
        ? null
        : Map<String, dynamic>.from(json['share'] as Map),
    synced: (json['synced'] as List?)
        ?.map((s) => CanvasShape.fromJson(Map<String, dynamic>.from(s as Map)))
        .toList(),
  );
}

class BoardStore {
  BoardStore({this._directory});
  Directory? _directory;
  Future<Directory> get directory async {
    final dir = _directory ??= Directory(
      '${(await getApplicationDocumentsDirectory()).path}/native_boards',
    );
    await dir.create(recursive: true);
    return dir;
  }

  Future<List<BoardData>> list() async {
    final result = <BoardData>[];
    await for (final file in (await directory).list()) {
      if (file is File && file.path.endsWith('.json')) {
        result.add(
          BoardData.fromJson(
            jsonDecode(await file.readAsString()) as Map<String, dynamic>,
          ),
        );
      }
    }
    result.sort((a, b) => b.updated.compareTo(a.updated));
    return result;
  }

  Future<void> save(BoardData board) async {
    final dir = await directory;
    final temp = File('${dir.path}/${board.id}.tmp');
    final target = '${dir.path}/${board.id}.json';
    var points = 0;
    for (final shape in board.shapes) {
      points += shape.points.length;
    }
    if (points < 4000) {
      await temp.writeAsString(jsonEncode(board.toJson()), flush: true);
      await temp.rename(target);
      return;
    }
    // A full board is megabytes of JSON. Encoding and writing it on the UI
    // thread would freeze drawing, so it is handed to a helper isolate as
    // flat number lists, which are cheap to copy.
    final header = board.toJson()
      ..remove('shapes')
      ..remove('synced');
    final shapes = _pack(board.shapes);
    final synced = board.share != null && board.synced != null
        ? _pack(board.synced!)
        : null;
    final path = temp.path;
    await Isolate.run(() {
      final json = {
        ...header,
        'shapes': [for (final s in shapes) _unpack(s).toJson()],
        if (synced != null)
          'synced': [for (final s in synced) _unpack(s).toJson()],
      };
      File(path)
        ..writeAsStringSync(jsonEncode(json), flush: true)
        ..renameSync(target);
    });
  }

  static List<_Packed> _pack(List<CanvasShape> shapes) => [
    for (final s in shapes)
      (
        s.id,
        s.tool.index,
        Float64List.fromList([
          for (final p in s.points) ...[p.dx, p.dy],
        ]),
        s.color,
        s.width,
        s.filled,
        s.text,
        s.fontSize,
      ),
  ];

  static CanvasShape _unpack(_Packed s) => CanvasShape(
    id: s.$1,
    tool: CanvasTool.values[s.$2],
    points: [
      for (var i = 0; i < s.$3.length; i += 2) Offset(s.$3[i], s.$3[i + 1]),
    ],
    color: s.$4,
    width: s.$5,
    filled: s.$6,
    text: s.$7,
    fontSize: s.$8,
  );

  Future<void> delete(BoardData board) async {
    final file = File('${(await directory).path}/${board.id}.json');
    if (await file.exists()) {
      await file.delete();
    }
  }
}

typedef _Packed = (String, int, Float64List, int, double, bool, String, double);
