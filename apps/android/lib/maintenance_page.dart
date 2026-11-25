import 'dart:async';

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'config_page.dart';
import 'glass_ui.dart';
import 'native_stream.dart';

const _channel = MethodChannel('singboard/config');

bool _panelUpdateVisible = false;
bool _coreUpdateVisible = false;

Future<Map<String, dynamic>> checkPanelUpdate(String dir) async {
  final path = await _channel.invokeMethod<String>('installedApk');
  final info = await configRequest(dir, 'check_update', {
    'installed_apk': path,
  });
  return Map<String, dynamic>.from(info);
}

Future<void> showPanelUpdate(
  BuildContext context,
  Map<String, dynamic> info,
) async {
  if (_panelUpdateVisible) return;
  _panelUpdateVisible = true;
  try {
    await showGlassDialog<void>(
      context: context,
      barrierDismissible: false,
      requestFocus: true,
      builder: (_) => _PanelUpdateDialog(info),
    );
  } finally {
    _panelUpdateVisible = false;
  }
}

class _PanelUpdateDialog extends StatefulWidget {
  const _PanelUpdateDialog(this.info);
  final Map<String, dynamic> info;
  @override
  State<_PanelUpdateDialog> createState() => _PanelUpdateDialogState();
}

class _PanelUpdateDialogState extends State<_PanelUpdateDialog> {
  String current = '', message = '';
  String? package;
  int received = 0;
  bool busy = false, polling = false;
  bool cancelling = false;
  String? downloadTicket;
  Timer? timer;

  @override
  void initState() {
    super.initState();
    _channel
        .invokeMethod<String>('version')
        .then((v) {
          if (mounted) setState(() => current = v ?? '');
        })
        .catchError((Object _) {});
  }

  @override
  void dispose() {
    timer?.cancel();
    super.dispose();
  }

  Future<void> install() async {
    if (busy) return;
    setState(() {
      busy = true;
      received = 0;
      message = '';
    });
    try {
      final dir = await configDirectory();
      if (package == null) {
        downloadTicket = DateTime.now().microsecondsSinceEpoch.toString();
        if (mounted) setState(() {});
        timer = Timer.periodic(const Duration(milliseconds: 350), (_) async {
          if (polling) return;
          polling = true;
          try {
            final bytes = await configRequest(dir, 'update_progress') as int;
            if (mounted && busy && package == null)
              setState(() => received = bytes);
          } catch (_) {
            timer?.cancel();
          } finally {
            polling = false;
          }
        });
        package = await configRequest(dir, 'download_update', {
          'version': widget.info['version'],
          'ticket': downloadTicket,
        }) as String;
        timer?.cancel();
        downloadTicket = null;
        if (!mounted || cancelling) return;
        setState(() {});
      }
      message =
          await _channel.invokeMethod<String>('installUpdate', {
            'path': package,
          }) ??
          '';
    } catch (e) {
      message = e is PlatformException ? (e.message ?? e.code) : e.toString();
    } finally {
      timer?.cancel();
      downloadTicket = null;
      if (mounted)
        setState(() {
          busy = false;
          cancelling = false;
        });
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final total = widget.info['size'] as int? ?? 0;
    final published = DateTime.tryParse(
      widget.info['published_at'] as String? ?? '',
    )?.toLocal();
    String mb(int bytes) => (bytes / 1048576).toStringAsFixed(1);
    return PopScope(
      canPop: !busy,
      child: Dialog(
        insetPadding: const EdgeInsets.all(16),
        constraints: const BoxConstraints(maxWidth: 384),
        backgroundColor: Colors.transparent,
        child: SingleChildScrollView(
          child: GlassCard(
            popover: true,
            padding: const EdgeInsets.all(20),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  busy
                      ? '正在更新面板'
                      : widget.info['reinstall'] == true
                      ? '本地面板和官方发布的不同'
                      : '发现新面板版本',
                  style: TextStyle(
                    color: c.ink,
                    fontSize: 18,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                const SizedBox(height: 12),
                for (final entry in [
                  ('当前', current),
                  ('最新', '${widget.info['version']}'),
                  if (published != null)
                    ('发布时间', published.toString().split('.').first),
                  if ((widget.info['mirror'] ?? '').isNotEmpty)
                    ('下载经由', widget.info['mirror'].toString()),
                ])
                  Padding(
                    padding: const EdgeInsets.only(bottom: 5),
                    child: Row(
                      children: [
                        SizedBox(
                          width: 70,
                          child: Text(
                            entry.$1,
                            style: TextStyle(fontSize: 12, color: c.muted),
                          ),
                        ),
                        Expanded(
                          child: Text(
                            entry.$2,
                            style: TextStyle(
                              fontSize: 12,
                              color: c.ink,
                              fontFamily: 'monospace',
                            ),
                          ),
                        ),
                      ],
                    ),
                  ),
                const SizedBox(height: 8),
                if (busy) ...[
                  Text(
                    package != null
                        ? '正在打开系统安装器…'
                        : (received > 0 && received >= total
                              ? '正在校验…'
                              : '下载中… ${mb(received)} MB / ${mb(total)} MB'),
                    style: TextStyle(fontSize: 12, color: c.muted),
                  ),
                  const SizedBox(height: 6),
                  LinearProgressIndicator(
                    value: package == null && total > 0
                        ? (received / total).clamp(0.0, 1.0).toDouble()
                        : null,
                  ),
                  const SizedBox(height: 10),
                ],
                Text(
                  message.isEmpty
                      ? widget.info['reinstall'] == true
                            ? '版本相同，但本地文件和官方发布的文件不同。重新安装会覆盖当前应用。'
                            : '下载并校验后会打开系统安装器，代理服务不受影响。'
                      : message,
                  style: TextStyle(fontSize: 14, color: c.muted),
                ),
                const SizedBox(height: 14),
                SizedBox(
                  width: double.infinity,
                  child: Wrap(
                    alignment: WrapAlignment.end,
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      TextButton(
                        onPressed: busy
                            ? null
                            : () async {
                                try {
                                  await _channel.invokeMethod<void>(
                                    'openChangelog',
                                  );
                                } catch (e) {
                                  if (mounted)
                                    setState(() => message = e.toString());
                                }
                              },
                        style: TextButton.styleFrom(
                          padding: EdgeInsets.zero,
                          foregroundColor: c.selectionInk,
                        ),
                        child: Row(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            const Text('查看更新日志'),
                            const SizedBox(width: 4),
                            PanelIcon(
                              'external-link',
                              size: 11,
                              color: c.selectionInk,
                            ),
                          ],
                        ),
                      ),
                      TextButton(
                        autofocus: true,
                        onPressed: !busy
                            ? () => Navigator.pop(context)
                            : downloadTicket == null || cancelling
                            ? null
                            : () async {
                                final ticket = downloadTicket!;
                                setState(() => cancelling = true);
                                try {
                                  await configRequest(
                                    await configDirectory(),
                                    'cancel_update',
                                    {'ticket': ticket},
                                  );
                                } catch (e) {
                                  if (mounted)
                                    setState(() {
                                      message = e.toString();
                                      cancelling = false;
                                    });
                                }
                              },
                        child: Text(busy && package == null ? '取消下载' : '稍后'),
                      ),
                      FilledButton(
                        onPressed: busy ? null : install,
                        child: Text(
                          package == null
                              ? widget.info['reinstall'] == true
                                    ? '重新安装'
                                    : '立即更新'
                              : '安装已校验的更新',
                        ),
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

Future<String?> showCoreUpdate(
  BuildContext context, {
  required String dir,
  required String summary,
  required String target,
  required String ticket,
  required Map<String, String> values,
}) async {
  if (_coreUpdateVisible) return null;
  _coreUpdateVisible = true;
  try {
    return await showGlassDialog<String>(
      context: context,
      barrierDismissible: false,
      requestFocus: true,
      builder: (_) => _CoreUpdateDialog(
        dir: dir,
        summary: summary,
        target: target,
        ticket: ticket,
        values: values,
      ),
    );
  } finally {
    _coreUpdateVisible = false;
  }
}

class _CoreUpdateDialog extends StatefulWidget {
  const _CoreUpdateDialog({
    required this.dir,
    required this.summary,
    required this.target,
    required this.ticket,
    required this.values,
  });
  final String dir;
  final String summary;
  final String target;
  final String ticket;
  final Map<String, String> values;

  @override
  State<_CoreUpdateDialog> createState() => _CoreUpdateDialogState();
}

class _CoreUpdateDialogState extends State<_CoreUpdateDialog> {
  bool busy = false;
  bool polling = false;
  String progress = '正在准备核心更新…';
  String error = '';
  Timer? timer;

  @override
  void dispose() {
    timer?.cancel();
    super.dispose();
  }

  Future<void> _confirm() async {
    if (busy) return;
    setState(() {
      busy = true;
      error = '';
      progress = '正在准备核心更新…';
    });
    timer = Timer.periodic(const Duration(milliseconds: 350), (_) async {
      if (polling) return;
      polling = true;
      try {
        final text = await configRequest(widget.dir, 'core_progress');
        if (mounted && busy && text is String && text.isNotEmpty) {
          setState(() => progress = text);
        }
      } catch (_) {
        timer?.cancel();
      } finally {
        polling = false;
      }
    });
    try {
      final reply = await configRequest(widget.dir, 'tool', {
        'action': 'core_check',
        'values': widget.values,
        'ticket': widget.ticket,
      }) as Map;
      timer?.cancel();
      if (!mounted) return;
      Navigator.pop(context, reply['text'] as String? ?? '核心已更新');
    } catch (e) {
      timer?.cancel();
      if (mounted) {
        setState(() {
          busy = false;
          error = e is StateError ? e.message : e.toString();
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    return PopScope(
      canPop: !busy,
      child: Dialog(
        insetPadding: const EdgeInsets.all(16),
        constraints: const BoxConstraints(maxWidth: 384),
        backgroundColor: Colors.transparent,
        child: SingleChildScrollView(
          child: GlassCard(
            popover: true,
            padding: const EdgeInsets.all(20),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  '发现核心更新',
                  style: TextStyle(
                    color: c.ink,
                    fontSize: 18,
                    fontWeight: FontWeight.w600,
                  ),
                ),
                const SizedBox(height: 12),
                Container(
                  width: double.infinity,
                  padding: const EdgeInsets.all(12),
                  decoration: BoxDecoration(
                    color: c.fill,
                    borderRadius: BorderRadius.circular(10),
                  ),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        widget.summary,
                        style: TextStyle(
                          fontSize: 12,
                          color: c.ink,
                          fontFamily: 'monospace',
                        ),
                      ),
                      if (widget.target.isNotEmpty) ...[
                        const SizedBox(height: 4),
                        Text(
                          '目标：${widget.target}',
                          style: TextStyle(
                            fontSize: 12,
                            color: c.muted,
                            fontFamily: 'monospace',
                          ),
                        ),
                      ],
                    ],
                  ),
                ),
                const SizedBox(height: 12),
                Text(
                  busy
                      ? progress
                      : '更新会先停止核心，替换后自动重启，期间代理短暂中断。',
                  style: TextStyle(fontSize: 14, color: c.muted),
                ),
                if (error.isNotEmpty) ...[
                  const SizedBox(height: 8),
                  Text(error, style: TextStyle(fontSize: 12, color: c.error)),
                ],
                const SizedBox(height: 14),
                SizedBox(
                  width: double.infinity,
                  child: Wrap(
                    alignment: WrapAlignment.end,
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      if (!busy)
                        TextButton(
                          autofocus: true,
                          onPressed: () => Navigator.pop(context),
                          child: const Text('稍后'),
                        ),
                      FilledButton(
                        onPressed: busy ? null : _confirm,
                        child: Text(busy ? '更新中…' : '立即更新'),
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
