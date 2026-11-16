import 'package:material_ui/material_ui.dart';

import 'app_updates.dart';

class UpdatePage extends StatefulWidget {
  const UpdatePage({super.key, required this.controller});
  final UpdateController controller;
  @override
  State<UpdatePage> createState() => _UpdatePageState();
}

class _UpdatePageState extends State<UpdatePage> {
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && !widget.controller.busy) {
        widget.controller.check();
      }
    });
  }

  @override
  Widget build(BuildContext context) => Scaffold(
    appBar: AppBar(title: const Text('App updates')),
    body: SafeArea(
      child: AnimatedBuilder(
        animation: widget.controller,
        builder: (context, _) {
          final c = widget.controller, release = c.release;
          final downloaded = c.file != null;
          return ListView(
            padding: const EdgeInsets.all(24),
            children: [
              const Icon(
                Icons.system_update_rounded,
                size: 64,
                color: Color(0xff6750d8),
              ),
              const SizedBox(height: 20),
              Text(
                'Keep your canvas up to date',
                textAlign: TextAlign.center,
                style: Theme.of(context).textTheme.headlineSmall,
              ),
              const SizedBox(height: 8),
              Text(
                c.installed.isEmpty
                    ? 'Canvas Studio'
                    : 'Installed: ${c.installed}',
                textAlign: TextAlign.center,
              ),
              const SizedBox(height: 24),
              if (c.state == UpdateState.checking) ...[
                const LinearProgressIndicator(),
                const SizedBox(height: 12),
                const Text('Checking for updates…'),
              ],
              if (c.state == UpdateState.current)
                const Card(
                  child: Padding(
                    padding: EdgeInsets.all(20),
                    child: Text('You have the latest version.'),
                  ),
                ),
              if (release != null) ...[
                Card(
                  child: Padding(
                    padding: const EdgeInsets.all(20),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          'Version ${release.version}',
                          style: Theme.of(context).textTheme.titleLarge,
                        ),
                        const SizedBox(height: 8),
                        Text(
                          '${(release.size / 1000000).toStringAsFixed(1)} MB download',
                        ),
                        if (release.notes.isNotEmpty) ...[
                          const SizedBox(height: 12),
                          Text(release.notes),
                        ],
                      ],
                    ),
                  ),
                ),
                const SizedBox(height: 16),
                if (c.state == UpdateState.downloading) ...[
                  LinearProgressIndicator(
                    value: c.total > 0 ? c.received / c.total : null,
                  ),
                  const SizedBox(height: 12),
                  Text(
                    '${(c.received / 1000000).toStringAsFixed(1)} / ${(c.total / 1000000).toStringAsFixed(1)} MB',
                  ),
                  TextButton(
                    onPressed: c.cancelDownload,
                    child: const Text('Cancel download'),
                  ),
                ] else if (downloaded) ...[
                  const Text(
                    'Download verified. Android will ask you to confirm installation. Your saved boards stay on this device.',
                  ),
                  const SizedBox(height: 16),
                  if (c.state == UpdateState.permission) ...[
                    const Text(
                      'Allow Canvas Studio to install updates in Android settings. Then return here and tap Install update.',
                    ),
                    const SizedBox(height: 12),
                    OutlinedButton(
                      onPressed: c.requestPermission,
                      child: const Text('Allow updates in settings'),
                    ),
                  ],
                  FilledButton.icon(
                    onPressed: c.install,
                    icon: const Icon(Icons.install_mobile),
                    label: const Text('Install update'),
                  ),
                  if (c.state == UpdateState.installer)
                    const Text(
                      'Finish the update in the Android installer. If you cancelled, tap Install update to try again.',
                    ),
                ] else if (!c.busy)
                  FilledButton.icon(
                    onPressed: c.download,
                    icon: const Icon(Icons.download),
                    label: const Text('Download update'),
                  ),
              ],
              if (c.error.isNotEmpty) ...[
                const SizedBox(height: 16),
                Text(
                  c.error,
                  style: TextStyle(color: Theme.of(context).colorScheme.error),
                ),
              ],
              if (!c.busy) ...[
                const SizedBox(height: 16),
                OutlinedButton(
                  onPressed: c.check,
                  child: const Text('Check for updates'),
                ),
              ],
              const SizedBox(height: 24),
              const Text(
                'Updates install over the existing app. Keep the app installed to retain your boards.',
                style: TextStyle(color: Colors.grey, fontSize: 12),
              ),
            ],
          );
        },
      ),
    ),
  );
}
