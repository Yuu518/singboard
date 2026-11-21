import 'dart:ui' as ui;

import 'package:material_ui/material_ui.dart';

import 'config_page.dart';
import 'glass_ui.dart';
import 'native_stream.dart';

class SetupWizardOverlay extends StatefulWidget {
  final Map<String, dynamic> preferences;
  final String initialUrl;
  final String initialSecret;
  final VoidCallback onCompleted;
  final VoidCallback onGoToSettings;

  const SetupWizardOverlay({
    super.key,
    required this.preferences,
    required this.initialUrl,
    required this.initialSecret,
    required this.onCompleted,
    required this.onGoToSettings,
  });

  @override
  State<SetupWizardOverlay> createState() => _SetupWizardOverlayState();
}

class _SetupWizardOverlayState extends State<SetupWizardOverlay> {
  late final _urlCtrl = TextEditingController(
    text: widget.initialUrl.isEmpty
        ? 'http://127.0.0.1:9090'
        : widget.initialUrl,
  );
  late final _secretCtrl = TextEditingController(text: widget.initialSecret);
  late final _workDirCtrl = TextEditingController(
    text: (widget.preferences['workingDir'] as String? ?? ''),
  );
  late bool _rootMode = widget.preferences['nativeRootEnabled'] == true;
  bool _saving = false;
  String _error = '';

  @override
  void dispose() {
    _urlCtrl.dispose();
    _secretCtrl.dispose();
    _workDirCtrl.dispose();
    super.dispose();
  }

  Future<void> _goToSettings() async {
    if (_saving) return;
    setState(() {
      _saving = true;
      _error = '';
    });
    try {
      await configRequest(await configDirectory(), 'tool', {
        'action': 'skip_setup',
        'values': <String, String>{},
        'servers': <String>[],
      });
      if (mounted) widget.onGoToSettings();
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  Future<void> _save() async {
    if (_saving) return;
    final apiUrl = _urlCtrl.text.trim();
    final apiSecret = _secretCtrl.text;
    final workDir = _workDirCtrl.text.trim().replaceAll(RegExp(r'/+$'), '');

    if (!RegExp(r'^https?://.+').hasMatch(apiUrl)) {
      setState(() => _error = '请填写以 http:// 或 https:// 开头的 Clash API 地址。');
      return;
    }
    if (_rootMode && !workDir.startsWith('/')) {
      setState(() => _error = '请填写 sing-box 工作目录的绝对路径，例如 /data/adb/sing-box。');
      return;
    }

    setState(() {
      _saving = true;
      _error = '';
    });
    try {
      final dir = await configDirectory();
      if (_rootMode) {
        await configRequest(dir, 'tool', {
          'action': 'root_enable',
          'values': {'workdir': workDir},
          'servers': <String>[],
        });
        await configRequest(dir, 'tool', {
          'action': 'root_detect',
          'values': {'workdir': workDir},
          'servers': <String>[],
        });
      }
      final apis = (widget.preferences['clashApis'] as List? ?? [])
          .cast<Map<String, dynamic>>();
      final active = apis
          .where((a) => a['id'] == widget.preferences['activeClashApiId'])
          .firstOrNull;
      await configRequest(dir, 'tool', {
        'action': 'setup',
        'values': {
          'id': active?['id']?.toString() ?? '',
          'name': active?['name']?.toString() ?? '默认 API',
          'url': apiUrl,
          'secret': apiSecret,
          'workdir': _rootMode ? workDir : '',
        },
        'servers': <String>[],
      });
      if (mounted) widget.onCompleted();
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  Widget _fieldLabel(String text, SingboardColors c) => Padding(
    padding: const EdgeInsets.only(bottom: 5),
    child: Text(text, style: TextStyle(fontSize: 12, color: c.muted)),
  );

  Widget _styledField({
    required TextEditingController controller,
    required String hint,
    required SingboardColors c,
    bool obscure = false,
    bool mono = false,
    TextInputType? keyboardType,
  }) => Container(
    height: 32,
    decoration: BoxDecoration(
      color: c.fill,
      borderRadius: BorderRadius.circular(11),
      border: Border.all(color: c.hairline, width: 0.6),
    ),
    child: TextField(
      controller: controller,
      obscureText: obscure,
      keyboardType: keyboardType,
      autocorrect: false,
      enabled: !_saving,
      style: TextStyle(
        fontSize: 14,
        fontFamily: mono ? 'monospace' : null,
        color: c.ink,
      ),
      decoration: InputDecoration(
        isDense: true,
        border: InputBorder.none,
        contentPadding: const EdgeInsets.symmetric(horizontal: 12, vertical: 5),
        hintText: hint,
        hintStyle: TextStyle(fontSize: 14, color: c.muted),
      ),
    ),
  );

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    return Positioned.fill(
      child: TweenAnimationBuilder<double>(
        tween: Tween(begin: 0, end: 1),
        duration: const Duration(milliseconds: 180),
        curve: Curves.ease,
        builder: (context, t, child) => Opacity(opacity: t, child: child),
        child: BackdropFilter(
          filter: ui.ImageFilter.blur(sigmaX: 3, sigmaY: 3),
          child: Material(
            color: (c.dark ? Colors.black : Colors.white).withValues(
              alpha: c.dark ? .35 : .22,
            ),
            child: SafeArea(
              child: Center(
                child: SingleChildScrollView(
                  padding: EdgeInsets.fromLTRB(
                    16,
                    16,
                    16,
                    16 + MediaQuery.viewInsetsOf(context).bottom,
                  ),
                  child: ConstrainedBox(
                    constraints: const BoxConstraints(maxWidth: 576),
                    child: TweenAnimationBuilder<double>(
                      tween: Tween(begin: .94, end: 1),
                      duration: const Duration(milliseconds: 220),
                      curve: const Cubic(.32, .72, 0, 1),
                      builder: (context, t, child) =>
                          Transform.scale(scale: t, child: child),
                      child: GlassCard(
                        radius: 18,
                        popover: true,
                        padding: const EdgeInsets.all(20),
                        child: Column(
                          mainAxisSize: MainAxisSize.min,
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              '初始化向导',
                              style: TextStyle(
                                fontSize: 18,
                                fontWeight: FontWeight.w600,
                                color: c.ink,
                              ),
                            ),
                            const SizedBox(height: 6),
                            Text(
                              '填写要连接的 Clash API。sing-box 可以运行在路由器、电脑或本机上。',
                              style: TextStyle(fontSize: 14, color: c.muted),
                            ),
                            const SizedBox(height: 16),
                            _fieldLabel('API 地址', c),
                            _styledField(
                              controller: _urlCtrl,
                              hint: 'http://127.0.0.1:9090',
                              c: c,
                              keyboardType: TextInputType.url,
                            ),
                            const SizedBox(height: 12),
                            _fieldLabel('密钥 (Secret)', c),
                            _styledField(
                              controller: _secretCtrl,
                              hint: '留空表示无密钥',
                              c: c,
                              obscure: true,
                            ),
                            const SizedBox(height: 14),
                            Container(
                              padding: const EdgeInsets.symmetric(
                                horizontal: 12,
                                vertical: 10,
                              ),
                              decoration: BoxDecoration(
                                color: c.fill,
                                borderRadius: BorderRadius.circular(14),
                              ),
                              child: Row(
                                children: [
                                  Expanded(
                                    child: Column(
                                      crossAxisAlignment:
                                          CrossAxisAlignment.start,
                                      children: [
                                        Text(
                                          '使用 root 管理本机 sing-box',
                                          style: TextStyle(
                                            fontSize: 14,
                                            fontWeight: FontWeight.w500,
                                            color: c.ink,
                                          ),
                                        ),
                                        const SizedBox(height: 2),
                                        Text(
                                          '需要 Magisk / KernelSU / APatch。支持启动、停止、重启、开机自启和核心更新。',
                                          style: TextStyle(
                                            fontSize: 12,
                                            color: c.muted,
                                          ),
                                        ),
                                      ],
                                    ),
                                  ),
                                  const SizedBox(width: 10),
                                  GlassToggle(
                                    value: _rootMode,
                                    onChanged: _saving
                                        ? null
                                        : (v) => setState(() => _rootMode = v),
                                  ),
                                ],
                              ),
                            ),
                            if (_rootMode) ...[
                              const SizedBox(height: 12),
                              _fieldLabel('sing-box 工作目录', c),
                              _styledField(
                                controller: _workDirCtrl,
                                hint: '/data/adb/sing-box',
                                c: c,
                                mono: true,
                              ),
                              const SizedBox(height: 4),
                              Text(
                                '会在目录及子目录中自动查找核心（优先 bin 文件夹）、配置文件和规则，日志写入 logs 或 log 文件夹。',
                                style: TextStyle(
                                  fontSize: 12,
                                  color: c.muted,
                                ),
                              ),
                            ],
                            if (_error.isNotEmpty) ...[
                              const SizedBox(height: 12),
                              Text(
                                _error,
                                style: TextStyle(
                                  fontSize: 13,
                                  color: c.dangerInk,
                                ),
                              ),
                            ],
                            const SizedBox(height: 18),
                            GlassDialogButtons(
                              child: SizedBox(
                                width: double.infinity,
                                child: Wrap(
                                  alignment: WrapAlignment.end,
                                  spacing: 8,
                                  runSpacing: 8,
                                  crossAxisAlignment: WrapCrossAlignment.center,
                                  children: [
                                    TextButton(
                                      onPressed: _saving ? null : _goToSettings,
                                      child: const Text('稍后在设置中配置'),
                                    ),
                                    FilledButton(
                                      onPressed: _saving ? null : _save,
                                      style: FilledButton.styleFrom(
                                        backgroundColor: c.primary,
                                        foregroundColor: Colors.white,
                                        shape: const StadiumBorder(),
                                      ),
                                      child: Text(_saving ? '保存中…' : '完成'),
                                    ),
                                  ],
                                ),
                              ),
                            ),
                          ],
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
