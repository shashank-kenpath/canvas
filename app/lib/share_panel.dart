import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'board_store.dart';
import 'canvas_document.dart';
import 'canvas_painting.dart';
import 'collaboration.dart';
import 'editor.dart';

/// Collaboration server used for new shared boards and 6-digit board codes.
/// Override at build time with --dart-define=COLLAB_SERVER_URL=https://…
const collaborationServer = String.fromEnvironment(
  'COLLAB_SERVER_URL',
  defaultValue: 'https://annotated-remedies-property-binding.trycloudflare.com',
);

Widget _copyRow(
  BuildContext context,
  String label,
  String value, {
  Key? key,
}) => Padding(
  padding: const EdgeInsets.only(top: 8),
  child: Row(
    children: [
      Expanded(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              label,
              style: const TextStyle(fontSize: 12, color: Color(0xff8b9098)),
            ),
            Text(
              value,
              key: key,
              maxLines: 3,
              overflow: TextOverflow.ellipsis,
              style: const TextStyle(fontSize: 13, fontFamily: 'monospace'),
            ),
          ],
        ),
      ),
      IconButton(
        tooltip: 'Copy $label',
        onPressed: () {
          Clipboard.setData(ClipboardData(text: value));
          ScaffoldMessenger.of(context)
            ..hideCurrentSnackBar()
            ..showSnackBar(SnackBar(content: Text('$label copied')));
        },
        icon: const Icon(Icons.copy, size: 18),
      ),
    ],
  ),
);

/// Lets Claude read and draw on the board. Kept apart from sharing with
/// people, which only ever needs the 6-digit code.
Future<void> showClaudePanel(BuildContext context, NativeEditorState editor) =>
    showDialog<void>(
      context: context,
      builder: (_) => _ClaudeDialog(editor: editor),
    );

class _ClaudeDialog extends StatefulWidget {
  const _ClaudeDialog({required this.editor});
  final NativeEditorState editor;
  @override
  State<_ClaudeDialog> createState() => _ClaudeDialogState();
}

class _ClaudeDialogState extends State<_ClaudeDialog> {
  bool working = false;
  String? error;

  Future<void> _connect() async {
    setState(() {
      working = true;
      error = null;
    });
    try {
      await widget.editor.startSharing();
    } catch (_) {
      error = 'Could not reach the server. Check the internet and retry.';
    }
    if (mounted) setState(() => working = false);
  }

  @override
  Widget build(BuildContext context) {
    final link = widget.editor.shareLink;
    final mcp = link?.mcpUri.toString();
    return AlertDialog(
      title: const Text('Connect Claude'),
      content: SizedBox(
        width: 460,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (mcp == null)
                const Text(
                  'Claude can read this board and draw on it while you watch. '
                  'Connecting puts the board online so Claude can reach it.',
                )
              else ...[
                const Text(
                  'Claude Code: run this command in a terminal.',
                  style: TextStyle(fontSize: 13),
                ),
                _copyRow(
                  context,
                  'Claude Code command',
                  'claude mcp add --transport http canvas-studio $mcp',
                ),
                const SizedBox(height: 12),
                const Text(
                  'Claude Desktop: Settings → Connectors → Add custom '
                  'connector, and paste this address.',
                  style: TextStyle(fontSize: 13),
                ),
                _copyRow(
                  context,
                  'Connector address',
                  mcp,
                  key: const ValueKey('mcp-url'),
                ),
                const SizedBox(height: 8),
                const Text(
                  'Keep this address private: anyone who has it can edit '
                  'this board.',
                  style: TextStyle(fontSize: 12, color: Color(0xff8b9098)),
                ),
              ],
              if (working)
                const Padding(
                  padding: EdgeInsets.only(top: 16),
                  child: LinearProgressIndicator(),
                ),
              if (error != null)
                Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: Text(
                    error!,
                    style: const TextStyle(color: Color(0xffe03131)),
                  ),
                ),
            ],
          ),
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: Text(mcp == null ? 'Cancel' : 'Done'),
        ),
        if (mcp == null)
          FilledButton(
            key: const ValueKey('claude-connect'),
            onPressed: working ? null : _connect,
            child: const Text('Connect'),
          ),
      ],
    );
  }
}

Future<void> showSharePanel(BuildContext context, NativeEditorState editor) =>
    showDialog<void>(
      context: context,
      builder: (_) => _ShareDialog(editor: editor),
    );

class _ShareDialog extends StatefulWidget {
  const _ShareDialog({required this.editor});
  final NativeEditorState editor;
  @override
  State<_ShareDialog> createState() => _ShareDialogState();
}

class _ShareDialogState extends State<_ShareDialog> {
  bool working = false;
  String? error, code;

  NativeEditorState get editor => widget.editor;

  Future<void> _run(Future<void> Function() action, String failure) async {
    setState(() {
      working = true;
      error = null;
    });
    try {
      await action();
    } catch (_) {
      error = failure;
    }
    if (mounted) setState(() => working = false);
  }

  Future<void> _start() => _run(() async {
    await editor.startSharing();
    await _fetchCode();
  }, 'Could not reach the sharing server. Check the internet and retry.');

  Future<void> _fetchCode() async {
    final link = editor.shareLink;
    if (link == null) return;
    final result = await CollaborationClient.pairingCode(
      link,
      fetch: editor.widget.collaborationFetch,
    );
    code = result.code;
  }

  List<Widget> _shared() {
    return [
      Text(
        editor.status,
        style: const TextStyle(fontSize: 12, color: Color(0xff8b9098)),
      ),
      const SizedBox(height: 12),
      const Text(
        'Board code',
        style: TextStyle(fontWeight: FontWeight.w600),
      ),
      const SizedBox(height: 4),
      if (code != null) ...[
        Center(
          child: SelectableText(
            '${code!.substring(0, 3)} ${code!.substring(3)}',
            key: const ValueKey('board-code'),
            style: const TextStyle(
              fontSize: 44,
              fontWeight: FontWeight.w700,
              letterSpacing: 4,
              color: inkBlack,
            ),
          ),
        ),
        const Center(
          child: Text(
            'On the other device: Boards → Join shared board, then '
            'type this code. It works for 10 minutes.',
            textAlign: TextAlign.center,
            style: TextStyle(fontSize: 12, color: Color(0xff8b9098)),
          ),
        ),
      ] else
        OutlinedButton.icon(
          onPressed: working
              ? null
              : () => _run(
                  _fetchCode,
                  'Could not get a board code. Check the internet and retry.',
                ),
          icon: const Icon(Icons.pin_outlined),
          label: const Text('Show 6-digit board code'),
        ),
      const SizedBox(height: 12),
      const Text(
        'Anyone with the code can edit this board.',
        style: TextStyle(fontSize: 12, color: Color(0xff8b9098)),
      ),
    ];
  }

  @override
  Widget build(BuildContext context) {
    final link = editor.shareLink;
    return AlertDialog(
      title: Text(link == null ? 'Share this board' : 'Shared board'),
      content: SizedBox(
        width: 440,
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (link == null)
                const Text(
                  'Work on this board together from several devices. You '
                  'get a 6-digit code to type on the other device; the '
                  'board stays in sync while it is open.',
                )
              else
                ..._shared(),
              if (working)
                const Padding(
                  padding: EdgeInsets.only(top: 16),
                  child: LinearProgressIndicator(),
                ),
              if (error != null)
                Padding(
                  padding: const EdgeInsets.only(top: 12),
                  child: Text(
                    error!,
                    style: const TextStyle(color: Color(0xffe03131)),
                  ),
                ),
            ],
          ),
        ),
      ),
      actions: [
        if (link != null)
          TextButton(
            onPressed: working
                ? null
                : () async {
                    await editor.stopSharing();
                    if (context.mounted) Navigator.pop(context);
                  },
            child: const Text('Stop sharing on this device'),
          ),
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: Text(link == null ? 'Cancel' : 'Done'),
        ),
        if (link == null)
          FilledButton(
            key: const ValueKey('start-sharing'),
            onPressed: working ? null : _start,
            child: const Text('Start sharing'),
          ),
      ],
    );
  }
}

/// Asks for a board code or invite link, joins the room and stores it as a
/// local board. Returns the new board, or null when cancelled.
Future<BoardData?> showJoinDialog(
  BuildContext context,
  BoardStore store, {
  CollaborationFetch? fetch,
}) => showDialog<BoardData>(
  context: context,
  builder: (_) => _JoinDialog(store: store, fetch: fetch),
);

class _JoinDialog extends StatefulWidget {
  const _JoinDialog({required this.store, this.fetch});
  final BoardStore store;
  final CollaborationFetch? fetch;
  @override
  State<_JoinDialog> createState() => _JoinDialogState();
}

class _JoinDialogState extends State<_JoinDialog> {
  final controller = TextEditingController();
  bool working = false;
  String? error;

  @override
  void dispose() {
    controller.dispose();
    super.dispose();
  }

  Future<void> _join() async {
    final input = controller.text.trim();
    final digits = input.replaceAll(RegExp(r'[\s-]'), '');
    setState(() {
      working = true;
      error = null;
    });
    try {
      final CollaborationLink link;
      final List<CanvasShape> shapes;
      String name;
      const clientId = 'join';
      if (RegExp(r'^\d{6}$').hasMatch(digits)) {
        final joined = await CollaborationClient.joinWithCode(
          server: Uri.parse(collaborationServer),
          code: digits,
          fetch: widget.fetch,
        );
        link = joined.link;
        shapes = joined.shapes;
        name = joined.name;
      } else {
        final joined = await CollaborationClient.join(
          invite: input,
          clientId: clientId,
          fetch: widget.fetch,
        );
        link = joined.link;
        shapes = joined.shapes;
        name = 'Shared board';
        try {
          name = await CollaborationClient.roomName(link, fetch: widget.fetch);
        } catch (_) {
          // The name is only a label.
        }
      }
      final board = BoardData(
        id: DateTime.now().microsecondsSinceEpoch.toString(),
        title: name,
        updated: DateTime.now(),
        shapes: shapes,
        share: link.toJson(),
        synced: shapes,
      );
      await widget.store.save(board);
      if (mounted) Navigator.pop(context, board);
    } on FormatException catch (e) {
      error = e.message;
    } on CollaborationException catch (e) {
      error = e.statusCode == 404 || e.statusCode == 401 || e.statusCode == 403
          ? 'That code is not valid any more. Ask for a new one.'
          : e.statusCode == 429
          ? 'Too many attempts. Wait a minute and try again.'
          : 'Could not reach the sharing server. Check the internet and retry.';
    } catch (_) {
      error =
          'Could not reach the sharing server. Check the internet and retry.';
    }
    if (mounted) setState(() => working = false);
  }

  @override
  Widget build(BuildContext context) => AlertDialog(
    title: const Text('Join shared board'),
    content: SizedBox(
      width: 400,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const Text(
            'Enter the 6-digit board code shown under Share on the other '
            'device.',
          ),
          const SizedBox(height: 12),
          TextField(
            key: const ValueKey('join-input'),
            controller: controller,
            autofocus: true,
            enabled: !working,
            decoration: const InputDecoration(
              hintText: '123 456',
              border: OutlineInputBorder(),
            ),
            onSubmitted: (_) => _join(),
          ),
          if (working)
            const Padding(
              padding: EdgeInsets.only(top: 12),
              child: LinearProgressIndicator(),
            ),
          if (error != null)
            Padding(
              padding: const EdgeInsets.only(top: 12),
              child: Text(
                error!,
                style: const TextStyle(color: Color(0xffe03131)),
              ),
            ),
        ],
      ),
    ),
    actions: [
      TextButton(
        onPressed: () => Navigator.pop(context),
        child: const Text('Cancel'),
      ),
      FilledButton(
        key: const ValueKey('join-confirm'),
        onPressed: working ? null : _join,
        child: const Text('Join'),
      ),
    ],
  );
}
