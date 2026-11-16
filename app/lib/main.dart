import 'dart:async';

import 'package:material_ui/material_ui.dart';

import 'app_updates.dart';
import 'board_store.dart';
import 'brand_logo.dart';
import 'canvas_painting.dart';
import 'editor.dart';
import 'ink_recognition.dart';
import 'performance.dart';
import 'share_panel.dart';
import 'update_page.dart';

export 'editor.dart'
    show InkRecognizer, NativeEditor, NativeEditorState, askText;

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  FrameStats.instance.start();
  runApp(const CanvasStudio());
  // Fetch the handwriting model in the background on first launch so the
  // Write tool works straight away. Failures are retried when it is used.
  unawaited(
    HandwritingRecognition.shared.prepare('en').then((_) {}, onError: (_) {}),
  );
}

const purple = Color(0xff6750d8);
const ink = Color(0xff272536);

class CanvasStudio extends StatelessWidget {
  const CanvasStudio({super.key});
  @override
  Widget build(BuildContext context) => MaterialApp(
    title: 'Canvas Studio',
    debugShowCheckedModeBanner: false,
    theme: ThemeData(
      colorSchemeSeed: purple,
      useMaterial3: true,
      scaffoldBackgroundColor: const Color(0xfff6f5fa),
    ),
    home: BoardLibrary(store: BoardStore()),
  );
}

class BoardLibrary extends StatefulWidget {
  const BoardLibrary({super.key, required this.store});
  final BoardStore store;
  @override
  State<BoardLibrary> createState() => _BoardLibraryState();
}

class _BoardLibraryState extends State<BoardLibrary> {
  final updater = UpdateController(UpdateService());
  List<BoardData>? boards;
  String? error;
  @override
  void initState() {
    super.initState();
    load();
    unawaited(updater.check());
  }

  @override
  void dispose() {
    updater.dispose();
    super.dispose();
  }

  Future<void> load() async {
    try {
      final data = await widget.store.list();
      if (mounted) {
        setState(() {
          boards = data;
          error = null;
        });
      }
    } catch (_) {
      if (mounted) {
        setState(() => error = 'Could not read your boards. Please retry.');
      }
    }
  }

  void message(String text) {
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text(text)));
    }
  }

  Future<void> open(BoardData board) async {
    await Navigator.push(
      context,
      MaterialPageRoute<void>(
        builder: (_) => NativeEditor(board: board, store: widget.store),
      ),
    );
    await load();
  }

  Future<void> create() async {
    final name = await askText(context, 'A new space for ideas');
    if (name == null) {
      return;
    }
    final board = BoardData(
      id: DateTime.now().microsecondsSinceEpoch.toString(),
      title: name.isEmpty ? 'Untitled board' : name,
      updated: DateTime.now(),
      shapes: [],
    );
    try {
      await widget.store.save(board);
      if (mounted) {
        await open(board);
      }
    } catch (_) {
      message('Could not create this board. Please retry.');
    }
  }

  Future<void> join() async {
    final board = await showJoinDialog(context, widget.store);
    if (board != null && mounted) {
      await open(board);
    }
  }

  Future<void> manage(BoardData board, String action) async {
    try {
      if (action == 'rename') {
        final name = await askText(
          context,
          'Rename board',
          initial: board.title,
        );
        if (name == null || name.isEmpty) {
          return;
        }
        board.title = name;
        await widget.store.save(board);
      } else {
        final remove = await showDialog<bool>(
          context: context,
          builder: (context) => AlertDialog(
            title: const Text('Delete this board?'),
            content: Text('“${board.title}” will be removed from this device.'),
            actions: [
              TextButton(
                onPressed: () => Navigator.pop(context, false),
                child: const Text('Keep'),
              ),
              FilledButton(
                onPressed: () => Navigator.pop(context, true),
                child: const Text('Delete'),
              ),
            ],
          ),
        );
        if (remove != true) {
          return;
        }
        await widget.store.delete(board);
      }
      await load();
    } catch (_) {
      message('Could not update this board. Please retry.');
    }
  }

  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: AppBar(
      title: const Row(
        children: [
          BrandLogo(size: 36),
          SizedBox(width: 10),
          Text('Canvas Studio'),
        ],
      ),
      backgroundColor: const Color(0xfff6f5fa),
      actions: [
        TextButton.icon(
          key: const ValueKey('join-board'),
          onPressed: join,
          icon: const Icon(Icons.group_add_outlined),
          label: const Text('Join shared board'),
        ),
        IconButton(
          tooltip: 'Display and speed',
          onPressed: () => showDisplayPanel(context),
          icon: const Icon(Icons.speed_outlined),
        ),
        AnimatedBuilder(
          animation: updater,
          builder: (context, _) => IconButton(
            tooltip: 'App updates',
            onPressed: () => Navigator.push(
              context,
              MaterialPageRoute<void>(
                builder: (_) => UpdatePage(controller: updater),
              ),
            ),
            icon: Badge(
              isLabelVisible: updater.release != null,
              child: const Icon(Icons.system_update_outlined),
            ),
          ),
        ),
      ],
    ),
    floatingActionButton: FloatingActionButton.extended(
      onPressed: create,
      icon: const Icon(Icons.add),
      label: const Text('New board'),
    ),
    body: error != null
        ? Center(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Text(error!),
                TextButton(onPressed: load, child: const Text('Retry')),
              ],
            ),
          )
        : boards == null
        ? const Center(child: CircularProgressIndicator())
        : LayoutBuilder(
            builder: (context, size) => Padding(
              padding: EdgeInsets.all(size.maxWidth >= 600 ? 36 : 20),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    'Make room for ideas.',
                    style: Theme.of(context).textTheme.headlineLarge
                        ?.copyWith(fontWeight: FontWeight.w700, color: ink),
                  ),
                  const SizedBox(height: 8),
                  const Text('A blank canvas. Endless possibilities.'),
                  const SizedBox(height: 28),
                  Expanded(
                    child: boards!.isEmpty
                        ? Center(
                            child: Column(
                              mainAxisSize: MainAxisSize.min,
                              children: [
                                const Icon(
                                  Icons.draw_outlined,
                                  size: 64,
                                  color: purple,
                                ),
                                const SizedBox(height: 20),
                                Text(
                                  'Your next idea starts here',
                                  style: Theme.of(context).textTheme.titleLarge,
                                ),
                                const SizedBox(height: 8),
                                const Text(
                                  'Create a board and make your first mark.',
                                ),
                              ],
                            ),
                          )
                        : GridView.builder(
                            padding: const EdgeInsets.only(bottom: 90),
                            itemCount: boards!.length,
                            gridDelegate:
                                SliverGridDelegateWithFixedCrossAxisCount(
                                  crossAxisCount: size.maxWidth >= 1000
                                      ? 4
                                      : size.maxWidth >= 600
                                      ? 3
                                      : 2,
                                  childAspectRatio: .95,
                                  crossAxisSpacing: 14,
                                  mainAxisSpacing: 14,
                                ),
                            itemBuilder: (context, i) {
                              final board = boards![i];
                              return Card(
                                color: Colors.white,
                                clipBehavior: Clip.antiAlias,
                                child: InkWell(
                                  onTap: () => open(board),
                                  child: Column(
                                    crossAxisAlignment:
                                        CrossAxisAlignment.start,
                                    children: [
                                      Expanded(
                                        child: Padding(
                                          padding: const EdgeInsets.all(14),
                                          child: SizedBox.expand(
                                            child: CustomPaint(
                                              painter: ThumbnailPainter(
                                                board.shapes,
                                              ),
                                            ),
                                          ),
                                        ),
                                      ),
                                      Padding(
                                        padding: const EdgeInsets.only(
                                          left: 14,
                                        ),
                                        child: Row(
                                          children: [
                                            Expanded(
                                              child: Column(
                                                crossAxisAlignment:
                                                    CrossAxisAlignment.start,
                                                children: [
                                                  Text(
                                                    board.title,
                                                    maxLines: 1,
                                                    overflow:
                                                        TextOverflow.ellipsis,
                                                    style: const TextStyle(
                                                      fontWeight:
                                                          FontWeight.w600,
                                                    ),
                                                  ),
                                                  Text(
                                                    board.share == null
                                                        ? '${board.shapes.length} marks'
                                                        : 'Shared • ${board.shapes.length} marks',
                                                    style: const TextStyle(
                                                      fontSize: 12,
                                                      color: Colors.grey,
                                                    ),
                                                  ),
                                                ],
                                              ),
                                            ),
                                            PopupMenuButton<String>(
                                              onSelected: (s) =>
                                                  manage(board, s),
                                              itemBuilder: (_) => const [
                                                PopupMenuItem(
                                                  value: 'rename',
                                                  child: Text('Rename'),
                                                ),
                                                PopupMenuItem(
                                                  value: 'delete',
                                                  child: Text('Delete'),
                                                ),
                                              ],
                                            ),
                                          ],
                                        ),
                                      ),
                                      const SizedBox(height: 8),
                                    ],
                                  ),
                                ),
                              );
                            },
                          ),
                  ),
                ],
              ),
            ),
          ),
  );
}
