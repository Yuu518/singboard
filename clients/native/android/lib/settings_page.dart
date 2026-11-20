import 'dart:math' as math;

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'config_page.dart';
import 'glass_ui.dart';
import 'maintenance_page.dart';
import 'native_stream.dart';
import 'wallpaper_editor.dart';

class SettingsPage extends StatefulWidget {
  final Map<String, dynamic> preferences;
  final Map<String, dynamic> summary;
  final String endpoint;
  final String rootStatus;
  final bool rootRunning;
  final bool probing;
  final ThemeMode themeMode;
  final double glassTint;
  final String? wallpaperPath;
  final Future<void> Function() onReload;
  final Future<void> Function(String mode) onSetMode;
  final Future<void> Function() onProbeRoot;
  final Future<void> Function(String action, String label) onRootAction;
  final Future<void> Function([int initial]) onOpenTools;
  final ValueChanged<ThemeMode> onThemeChanged;
  final ValueChanged<double> onTintChanged;
  final ValueChanged<String?> onWallpaperChanged;

  const SettingsPage({
    super.key,
    required this.preferences,
    required this.summary,
    required this.endpoint,
    required this.rootStatus,
    required this.rootRunning,
    required this.probing,
    required this.themeMode,
    required this.glassTint,
    required this.wallpaperPath,
    required this.onReload,
    required this.onSetMode,
    required this.onProbeRoot,
    required this.onRootAction,
    required this.onOpenTools,
    required this.onThemeChanged,
    required this.onTintChanged,
    required this.onWallpaperChanged,
  });

  @override
  State<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends State<SettingsPage> {
  String _startPage(Map<String, dynamic> prefs) {
    final value = prefs['startPage'];
    return _startPages.any((page) => page.$1 == value)
        ? value as String
        : '/overview';
  }

  static const _channel = MethodChannel('singboard/config');
  static const _startPages = [
    ('/overview', '概览'),
    ('/proxies', '代理'),
    ('/connections', '连接'),
    ('/logs', '日志'),
    ('/rules', '规则'),
    ('/settings', '设置'),
  ];

  late double _tint = widget.glassTint;
  bool _saving = false;
  final _inputs = <String, TextEditingController>{};
  final _open = <String>{};
  final _busy = <String>{};
  final _messages = <String, String>{};
  String _record = 'A', _resolver = 'system', _version = '';
  late bool _geo = widget.preferences['dnsGeoEnabled'] == true;
  late List<String> _dnsHistory = List<String>.from(
    widget.preferences['dnsHistory'] ?? [],
  );
  Map<String, dynamic>? _dnsAnswer;
  bool _showDnsHistory = false;
  bool? _newApi, _boot;
  String _protocol = 'http';

  @override
  void initState() {
    super.initState();
    _channel
        .invokeMethod<String>('version')
        .then((v) {
          if (mounted) setState(() => _version = v ?? '');
        })
        .catchError((Object _) {});
    if (widget.preferences['nativeRootEnabled'] == true) _loadBoot();
  }

  @override
  void didUpdateWidget(SettingsPage old) {
    super.didUpdateWidget(old);
    if (old.glassTint != widget.glassTint) {
      _tint = widget.glassTint;
    }
    for (final (key, pref) in [
      ('test_url', 'latencyTestUrl'),
      ('workdir', 'workingDir'),
      ('repo', 'coreUpdateCustomRepo'),
      ('mirror', 'coreUpdateMirror'),
    ]) {
      final previous = old.preferences[pref] as String? ?? '';
      final value = widget.preferences[pref] as String? ?? '';
      final input = _inputs[key];
      if (previous != value && input != null && input.text == previous) {
        input.text = value;
      }
    }
    if (old.preferences['activeClashApiId'] !=
        widget.preferences['activeClashApiId']) {
      _newApi = null;
    }
    if (old.preferences['nativeRootEnabled'] !=
        widget.preferences['nativeRootEnabled']) {
      _boot = null;
      if (widget.preferences['nativeRootEnabled'] == true) _loadBoot();
    }
  }

  @override
  void dispose() {
    for (final c in _inputs.values) {
      c.dispose();
    }
    super.dispose();
  }

  String _tintLabel(double t) {
    if (t == 0) return '通透';
    if (t == 0.5) return '适中';
    if (t == 1) return '模糊';
    return t < 0.5 ? '偏通透' : '偏模糊';
  }

  TextEditingController _input(String key, [String initial = '']) =>
      _inputs.putIfAbsent(key, () => TextEditingController(text: initial));

  Widget _glassInput(
    String key,
    String hint, {
    String initial = '',
    bool secret = false,
    bool mono = false,
  }) {
    final c = SingboardColors.of(context);
    return TextField(
      controller: _input(key, initial),
      onTap: key == 'domain'
          ? () => setState(() => _showDnsHistory = true)
          : null,
      obscureText: secret,
      autocorrect: false,
      style: TextStyle(
        fontSize: 13,
        color: c.ink,
        fontFamily: mono ? 'monospace' : null,
      ),
      decoration: InputDecoration(
        hintText: hint,
        hintStyle: TextStyle(fontSize: 13, color: c.muted),
        isDense: true,
        filled: true,
        fillColor: c.fill,
        contentPadding: const EdgeInsets.symmetric(
          horizontal: 12,
          vertical: 10,
        ),
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(10),
          borderSide: BorderSide.none,
        ),
        focusedBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(10),
          borderSide: BorderSide(
            color: c.primary.withValues(alpha: 0.55),
            width: 1,
          ),
        ),
      ),
    );
  }

  Widget _routeBtn(String label, VoidCallback? onTap) {
    final c = SingboardColors.of(context);
    final disabled = onTap == null;
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 7),
        decoration: BoxDecoration(
          color: disabled ? c.fill : c.primary,
          borderRadius: BorderRadius.circular(10),
        ),
        child: Text(
          label,
          style: TextStyle(
            fontSize: 12.5,
            fontWeight: FontWeight.w500,
            color: disabled ? c.muted.withValues(alpha: 0.55) : Colors.white,
          ),
        ),
      ),
    );
  }

  Widget _settingsBtn(
    String label,
    VoidCallback? onTap, {
    bool danger = false,
    bool ghost = false,
  }) {
    final c = SingboardColors.of(context);
    final disabled = onTap == null;
    return GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 7),
        decoration: BoxDecoration(
          color: ghost
              ? Colors.transparent
              : (c.dark ? const Color(0x4D767680) : const Color(0x80FFFFFF)),
          borderRadius: BorderRadius.circular(10),
          border: ghost ? null : Border.all(color: c.hairline, width: 0.75),
        ),
        child: Text(
          label,
          style: TextStyle(
            fontSize: 12.5,
            fontWeight: FontWeight.w500,
            color: disabled
                ? c.muted.withValues(alpha: 0.55)
                : (danger ? c.error : c.primary),
          ),
        ),
      ),
    );
  }

  Widget _iconBtn(
    String icon,
    String tooltip,
    VoidCallback? onTap, {
    bool selected = false,
    bool danger = false,
  }) {
    final c = SingboardColors.of(context);
    return Tooltip(
      message: tooltip,
      child: GestureDetector(
        onTap: onTap,
        child: Container(
          width: 32,
          height: 32,
          decoration: BoxDecoration(
            color: selected ? c.primarySoft : Colors.transparent,
            borderRadius: BorderRadius.circular(8),
          ),
          child: Center(
            child: PanelIcon(
              icon,
              size: 16,
              color: onTap == null
                  ? c.muted.withValues(alpha: 0.4)
                  : (selected
                        ? c.primary
                        : (danger ? c.error : c.ink.withValues(alpha: 0.65))),
            ),
          ),
        ),
      ),
    );
  }

  Widget _glassSelect(
    String hint,
    String value,
    Map<String, String> options,
    ValueChanged<String>? changed, {
    bool mono = false,
  }) {
    return _SettingsSelect(
      hint: hint,
      value: value,
      options: options,
      changed: changed,
      mono: mono,
    );
  }

  Future<void> _savePreference({
    ThemeMode? theme,
    String? startPage,
    bool? pageSwipe,
    bool? autoCheck,
  }) async {
    if (_saving) return;
    setState(() => _saving = true);
    try {
      final dir = await configDirectory();
      final prefs = widget.preferences;
      final nextTheme = theme ?? widget.themeMode;
      final nextStart = startPage ?? _startPage(prefs);
      final nextSwipe = pageSwipe ?? (prefs['pageSwipe'] == true);
      await configRequest(dir, 'tool', {
        'action': 'preferences',
        'values': {
          'start': nextStart,
          'autocheck': (autoCheck ?? prefs['panelAutoCheckUpdate'] != false)
              ? 'on'
              : 'off',
          'swipe': nextSwipe ? 'on' : 'off',
          'theme': switch (nextTheme) {
            ThemeMode.light => 'light',
            ThemeMode.dark => 'dark',
            ThemeMode.system => 'auto',
          },
        },
        'servers': <String>[],
      });
      if (theme != null) widget.onThemeChanged(theme);
      await widget.onReload();
    } catch (e) {
      if (mounted) {
        showGlassToast(context, e.toString());
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  Future<void> _saveTint(double val) async {
    try {
      await configRequest(await configDirectory(), 'set_glass_tint', {
        'tint': val,
      });
    } catch (_) {}
  }

  Future<void> _clearWallpaper() async {
    try {
      await _channel.invokeMethod('clearWallpaper');
      widget.onWallpaperChanged(null);
    } catch (e) {
      if (mounted) {
        showGlassToast(context, '清除壁纸失败：$e');
      }
    }
  }

  Future<void> _openWallpaperEditor() async {
    try {
      final uri = await _channel.invokeMethod<String>('pickImage');
      if (!mounted || uri == null) return;
      final bytes = await _channel.invokeMethod<Uint8List>('readImageBytes', {
        'uri': uri,
      });
      if (!mounted || bytes == null) return;
      final path = await showGlassDialog<String>(
        context: context,
        barrierDismissible: false,
        builder: (_) => GlassScope(
          tint: _tint,
          wallpaperPath: widget.wallpaperPath,
          wallpaperRevision: 0,
          child: WallpaperEditorDialog(bytes: bytes),
        ),
      );
      if (mounted && path != null) widget.onWallpaperChanged(path);
    } catch (e) {
      if (mounted)
        showGlassToast(context, '读取图片失败：$e');
    }
  }

  Future<void> _loadBoot() async {
    try {
      final value = await configRequest(await configDirectory(), 'tool', {
        'action': 'root_boot_status',
        'values': <String, String>{},
      });
      if (mounted) setState(() => _boot = value['text'] == 'on');
    } catch (e) {
      if (mounted) setState(() => _messages['root'] = e.toString());
    }
  }

  Future<bool> _run(
    String group,
    String action,
    Map<String, String> values,
  ) async {
    if (_busy.contains(group)) return false;
    setState(() {
      _busy.add(group);
      _messages[group] = '正在处理…';
    });
    try {
      final servers = action == 'dns' && values['resolver'] == 'system'
          ? await _channel.invokeListMethod<String>('dnsServers')
          : null;
      final reply = await configRequest(await configDirectory(), 'tool', {
        'action': action,
        'values': values,
        'servers': servers ?? <String>[],
      });
      if (reply['refresh'] == true) await widget.onReload();
      if (mounted) setState(() => _messages[group] = reply['text'] as String);
      return true;
    } catch (e) {
      if (mounted) setState(() => _messages[group] = e.toString());
      return false;
    } finally {
      if (mounted) setState(() => _busy.remove(group));
    }
  }

  Widget _note(String group) {
    if (!_messages.containsKey(group)) return const SizedBox.shrink();
    final c = SingboardColors.of(context);
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 0, 16, 12),
      child: Align(
        alignment: Alignment.centerLeft,
        child: SelectableText(
          _messages[group]!,
          style: TextStyle(fontSize: 12, color: c.muted),
        ),
      ),
    );
  }

  Widget _card(List<Widget> rows) => GlassCard(
    padding: EdgeInsets.zero,
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        for (var i = 0; i < rows.length; i++) ...[
          if (i > 0)
            Divider(
              height: 0.5,
              thickness: 0.5,
              indent: 16,
              color: SingboardColors.of(context).hairline,
            ),
          rows[i],
        ],
      ],
    ),
  );

  Widget _fold(String key, String title, String summary) => SettingsRow(
    title: title,
    subtitle: summary,
    inline: true,
    trailing: AnimatedRotation(
      turns: _open.contains(key) ? 0.5 : 0,
      duration: const Duration(milliseconds: 180),
      child: PanelIcon(
        'chevron-down',
        size: 16,
        color: SingboardColors.of(context).muted,
      ),
    ),
    onTap: () => setState(() {
      if (!_open.remove(key)) _open.add(key);
    }),
  );

  void _editApi(bool add, Map<String, dynamic> api) {
    if (_newApi == add) {
      setState(() => _newApi = null);
      return;
    }
    for (final k in ['name', 'secret']) {
      _input(k).text = add ? '' : api[k]?.toString() ?? '';
    }
    final address = add ? '' : api['url']?.toString() ?? '';
    final parts = RegExp(
      r'^(https?|wss?)://(\[[^\]]+\]|[^/:?#]+)(?::([0-9]+))?(.*)$',
    ).firstMatch(address);
    _protocol = {'https', 'wss'}.contains(parts?.group(1)) ? 'https' : 'http';
    _input('host').text = parts == null
        ? address
        : (parts.group(2)! +
              (parts.group(4) == '/' ? '' : parts.group(4) ?? ''));
    _input('port').text = parts?.group(3) ?? '';
    setState(() {
      _newApi = add;
      _messages.remove('api');
    });
  }

  Future<bool> _confirmDialog(
    String title,
    String message,
    String confirmLabel,
  ) async {
    final c = SingboardColors.of(context);
    final yes = await showGlassDialog<bool>(
      context: context,
      builder: (ctx) => Dialog(
        backgroundColor: Colors.transparent,
        child: GlassCard(
          popover: true,
          padding: const EdgeInsets.all(20),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                title,
                style: TextStyle(
                  fontSize: 16,
                  fontWeight: FontWeight.w600,
                  color: c.ink,
                ),
              ),
              const SizedBox(height: 8),
              Text(
                message,
                style: TextStyle(fontSize: 13, height: 1.5, color: c.muted),
              ),
              const SizedBox(height: 16),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  _settingsBtn(
                    '取消',
                    () => Navigator.pop(ctx, false),
                    ghost: true,
                  ),
                  const SizedBox(width: 8),
                  GestureDetector(
                    onTap: () => Navigator.pop(ctx, true),
                    child: Container(
                      padding: const EdgeInsets.symmetric(
                        horizontal: 14,
                        vertical: 7,
                      ),
                      decoration: BoxDecoration(
                        color: c.error,
                        borderRadius: BorderRadius.circular(10),
                      ),
                      child: Text(
                        confirmLabel,
                        style: const TextStyle(
                          fontSize: 12.5,
                          fontWeight: FontWeight.w500,
                          color: Colors.white,
                        ),
                      ),
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
    return yes == true;
  }

  Future<void> _openTokenDialog(String current) async {
    final ctrl = TextEditingController(text: current);
    var reveal = false;
    var err = '';
    final saved = await showGlassDialog<String?>(
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setModal) {
          final c = SingboardColors.of(ctx);
          return Dialog(
            backgroundColor: Colors.transparent,
            child: GlassCard(
              popover: true,
              padding: const EdgeInsets.all(20),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text(
                    'GitHub Token',
                    style: TextStyle(
                      fontSize: 16,
                      fontWeight: FontWeight.w600,
                      color: c.ink,
                    ),
                  ),
                  const SizedBox(height: 8),
                  Text(
                    '填写后，检查核心和面板更新都会带上这个 Token，不受匿名限流影响。Token 只保存在本机，只发给 api.github.com。',
                    style: TextStyle(
                      fontSize: 12.5,
                      height: 1.5,
                      color: c.muted,
                    ),
                  ),
                  const SizedBox(height: 12),
                  Row(
                    children: [
                      Expanded(
                        child: TextField(
                          controller: ctrl,
                          obscureText: !reveal,
                          autocorrect: false,
                          style: TextStyle(
                            fontSize: 13,
                            fontFamily: 'monospace',
                            color: c.ink,
                          ),
                          decoration: InputDecoration(
                            hintText: 'github_pat_… 或 ghp_…',
                            hintStyle: TextStyle(fontSize: 13, color: c.muted),
                            isDense: true,
                            filled: true,
                            fillColor: c.fill,
                            contentPadding: const EdgeInsets.symmetric(
                              horizontal: 12,
                              vertical: 10,
                            ),
                            border: OutlineInputBorder(
                              borderRadius: BorderRadius.circular(10),
                              borderSide: BorderSide.none,
                            ),
                          ),
                          onChanged: (_) {
                            if (err.isNotEmpty) setModal(() => err = '');
                          },
                        ),
                      ),
                      const SizedBox(width: 8),
                      _settingsBtn(
                        reveal ? '隐藏' : '显示',
                        () => setModal(() => reveal = !reveal),
                      ),
                    ],
                  ),
                  if (err.isNotEmpty) ...[
                    const SizedBox(height: 8),
                    Text(err, style: TextStyle(fontSize: 12, color: c.error)),
                  ],
                  const SizedBox(height: 16),
                  Row(
                    children: [
                      if (current.isNotEmpty)
                        _settingsBtn(
                          '清除 Token',
                          () => Navigator.pop(ctx, ''),
                          danger: true,
                          ghost: true,
                        ),
                      const Spacer(),
                      _settingsBtn('取消', () => Navigator.pop(ctx), ghost: true),
                      const SizedBox(width: 8),
                      _routeBtn('保存', () {
                        final v = ctrl.text.trim();
                        if (v.isNotEmpty &&
                            !RegExp(r'^[A-Za-z0-9_]{20,255}$').hasMatch(v)) {
                          setModal(() => err = 'Token 格式或长度不正确，请重新复制完整的 Token');
                          return;
                        }
                        Navigator.pop(ctx, v);
                      }),
                    ],
                  ),
                ],
              ),
            ),
          );
        },
      ),
    );
    ctrl.dispose();
    if (saved != null && mounted) {
      await _run('panel', 'save_token', {'token': saved});
    }
  }

  Widget _backends(Map<String, dynamic> prefs) {
    final c = SingboardColors.of(context);
    final apis = (prefs['clashApis'] as List? ?? [])
        .cast<Map<String, dynamic>>();
    final api = apis.firstWhere(
      (a) => a['id'] == prefs['activeClashApiId'],
      orElse: () => apis.isEmpty ? <String, dynamic>{} : apis.first,
    );
    final id = api['id']?.toString() ?? '';
    final busy = _busy.contains('api');

    Future<void> submit(String action) async {
      if (action == 'delete_api') {
        final ok = await _confirmDialog(
          '删除后端',
          '确定删除后端：${api['name'] ?? ''} ?',
          '删除',
        );
        if (!ok || !mounted) return;
      }
      final ok = await _run('api', action, {
        'id': id,
        'protocol': _protocol,
        for (final k in ['name', 'host', 'port', 'secret']) k: _input(k).text,
      });
      if (ok && action != 'probe' && mounted) setState(() => _newApi = null);
    }

    return _card([
      SettingsRow(
        title: '控制端点',
        subtitle: '面板连接的 Clash API，共 ${apis.length} 个。',
        trailing: Row(
          children: [
            Expanded(
              child: _glassSelect(
                '选择后端',
                id,
                {for (final a in apis) a['id'] as String: a['name'] as String},
                busy
                    ? null
                    : (v) async {
                        if (await _run('api', 'switch_api', {'id': v}) &&
                            mounted) {
                          setState(() => _newApi = null);
                        }
                      },
                mono: true,
              ),
            ),
            const SizedBox(width: 4),
            _iconBtn(
              'edit',
              '编辑当前后端',
              busy || apis.isEmpty ? null : () => _editApi(false, api),
              selected: _newApi == false,
            ),
            _iconBtn(
              'plus',
              '新增后端',
              busy ? null : () => _editApi(true, api),
              selected: _newApi == true,
            ),
          ],
        ),
      ),
      if (_newApi != null)
        Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                _newApi! ? '新增控制端点' : '编辑当前端点',
                style: TextStyle(
                  fontSize: 13,
                  fontWeight: FontWeight.w500,
                  color: c.ink,
                ),
              ),
              const SizedBox(height: 2),
              Text(
                _newApi! ? '新增后会自动切换到这个端点。' : '保存后面板会立即重新连接。',
                style: TextStyle(fontSize: 12, color: c.muted),
              ),
              const SizedBox(height: 12),
              Text('名称', style: TextStyle(fontSize: 12, color: c.muted)),
              const SizedBox(height: 6),
              _glassInput('name', _newApi! ? '后端 2' : '默认后端'),
              const SizedBox(height: 10),
              Row(
                children: [
                  SizedBox(
                    width: 92,
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          '协议',
                          style: TextStyle(fontSize: 12, color: c.muted),
                        ),
                        const SizedBox(height: 6),
                        _glassSelect(
                          '协议',
                          _protocol,
                          {'http': 'http', 'https': 'https'},
                          (s) => setState(() => _protocol = s),
                          mono: true,
                        ),
                      ],
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          '主机',
                          style: TextStyle(fontSize: 12, color: c.muted),
                        ),
                        const SizedBox(height: 6),
                        _glassInput('host', '127.0.0.1', mono: true),
                      ],
                    ),
                  ),
                  const SizedBox(width: 8),
                  SizedBox(
                    width: 82,
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          '端口',
                          style: TextStyle(fontSize: 12, color: c.muted),
                        ),
                        const SizedBox(height: 6),
                        _glassInput('port', '9090', mono: true),
                      ],
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 10),
              Text('访问密钥', style: TextStyle(fontSize: 12, color: c.muted)),
              const SizedBox(height: 6),
              _glassInput('secret', '留空表示无密钥', secret: true, mono: true),
              const SizedBox(height: 12),
              Row(
                children: [
                  if (!_newApi!)
                    _settingsBtn(
                      '删除端点',
                      busy || apis.length <= 1
                          ? null
                          : () => submit('delete_api'),
                      danger: true,
                      ghost: true,
                    ),
                  const Spacer(),
                  _routeBtn(
                    _newApi! ? '新增并切换' : '保存',
                    busy
                        ? null
                        : () => submit(_newApi! ? 'add_api' : 'save_api'),
                  ),
                ],
              ),
            ],
          ),
        ),
      if (_messages.containsKey('api')) _note('api'),
    ]);
  }

  Widget _network(Map<String, dynamic> prefs) {
    final c = SingboardColors.of(context);
    final ipv6 = prefs['ipv6TestEnabled'] == true;
    final groups = Map<String, dynamic>.from(
      prefs['groupTestUrls'] as Map? ?? {},
    );
    final test = _input('test_url', prefs['latencyTestUrl'] as String? ?? '');
    Future<bool> save({
      bool? enabled,
      String group = '',
      String url = '',
      bool saveDefault = true,
    }) => _run('network', 'test_settings', {
      'test_url': saveDefault
          ? test.text
          : prefs['latencyTestUrl'] as String? ?? '',
      'ipv6': (enabled ?? ipv6) ? 'on' : 'off',
      'group': group,
      'group_url': url,
    });
    return _card([
      Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              '默认测速地址',
              style: TextStyle(
                fontSize: 13,
                fontWeight: FontWeight.w500,
                color: c.ink,
              ),
            ),
            const SizedBox(height: 2),
            Text(
              '没有单独指定测试地址的代理组会使用这里的 URL。',
              style: TextStyle(fontSize: 12, color: c.muted),
            ),
            const SizedBox(height: 10),
            Row(
              children: [
                Expanded(
                  child: _glassInput(
                    'test_url',
                    'https://www.gstatic.com/generate_204',
                    mono: true,
                  ),
                ),
                const SizedBox(width: 8),
                _settingsBtn('保存', () => save()),
              ],
            ),
          ],
        ),
      ),
      _fold(
        'groups',
        '代理组专用地址',
        groups.isEmpty ? '暂未配置' : '已配置 ${groups.length} 个代理组',
      ),
      if (_open.contains('groups'))
        Padding(
          padding: const EdgeInsets.fromLTRB(16, 4, 16, 16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              for (final name in groups.keys.toList()..sort())
                Padding(
                  padding: const EdgeInsets.only(bottom: 8),
                  child: Row(
                    children: [
                      Container(
                        constraints: const BoxConstraints(maxWidth: 112),
                        padding: const EdgeInsets.symmetric(
                          horizontal: 10,
                          vertical: 4,
                        ),
                        decoration: BoxDecoration(
                          color: c.primarySoft,
                          borderRadius: BorderRadius.circular(999),
                        ),
                        child: Text(
                          name,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            fontSize: 12,
                            fontWeight: FontWeight.w500,
                            color: c.primary,
                          ),
                        ),
                      ),
                      const SizedBox(width: 6),
                      Expanded(
                        child: Text(
                          groups[name].toString(),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            fontSize: 12,
                            fontFamily: 'monospace',
                            color: c.muted,
                          ),
                        ),
                      ),
                      _iconBtn('edit', '编辑', () {
                        _input('group').text = name;
                        _input('group_url').text = groups[name].toString();
                      }),
                      _iconBtn(
                        'trash',
                        '删除',
                        () => save(group: name, saveDefault: false),
                        danger: true,
                      ),
                    ],
                  ),
                ),
              _glassInput('group', '代理组名称'),
              const SizedBox(height: 8),
              Row(
                children: [
                  Expanded(child: _glassInput('group_url', '测速地址', mono: true)),
                  const SizedBox(width: 8),
                  _settingsBtn('添加', () {
                    final group = _input('group').text.trim();
                    final url = _input('group_url').text.trim();
                    if (group.isEmpty || url.isEmpty) {
                      setState(() => _messages['network'] = '请填写代理组名称和测速地址');
                    } else {
                      save(group: group, url: url, saveDefault: false);
                    }
                  }),
                ],
              ),
            ],
          ),
        ),
      SettingsRow(
        title: 'IPv6 连通性测试',
        subtitle: '测速时额外检查节点的 IPv6 可用性。',
        inline: true,
        trailing: GlassToggle(
          value: ipv6,
          onChanged: _busy.contains('network')
              ? null
              : (v) => save(enabled: v, saveDefault: false),
        ),
      ),
      if (_messages.containsKey('network')) _note('network'),
    ]);
  }

  Future<void> _lookup() async {
    if (_busy.contains('dns')) return;
    setState(() {
      _busy.add('dns');
      _messages.remove('dns');
      _dnsAnswer = null;
      _showDnsHistory = false;
    });
    try {
      final domain = _input('domain').text.trim();
      if (domain.isEmpty) throw StateError('请输入域名。');
      final custom = _resolver == 'custom';
      final server = custom ? _input('server').text.trim() : _resolver;
      if (custom && server.isEmpty) throw StateError('请输入自定义解析器地址。');
      final system = _resolver == 'system';
      final servers = system
          ? await _channel.invokeListMethod<String>('dnsServers')
          : null;
      final result = await configRequest(
        await configDirectory(),
        'dns_lookup',
        {
          'domain': domain,
          'record': _record,
          'server': system ? null : server,
          'servers': servers ?? <String>[],
          'geo': _geo,
        },
      );
      if (mounted)
        setState(() {
          _dnsAnswer = Map<String, dynamic>.from(result);
          _dnsHistory = List<String>.from(result['history']);
        });
    } catch (e) {
      if (mounted)
        setState(
          () => _messages['dns'] = e is StateError ? e.message : e.toString(),
        );
    } finally {
      if (mounted) setState(() => _busy.remove('dns'));
    }
  }

  Future<void> _removeDns(String domain) async {
    try {
      final items = await configRequest(
        await configDirectory(),
        'remove_dns_history',
        {'domain': domain},
      );
      if (mounted) setState(() => _dnsHistory = List<String>.from(items));
    } catch (e) {
      if (mounted) setState(() => _messages['dns'] = e.toString());
    }
  }

  Widget _dns() {
    final c = SingboardColors.of(context);
    final busy = _busy.contains('dns');
    final answer = _dnsAnswer;
    final records = (answer?['records'] as List? ?? [])
        .whereType<Map>()
        .toList();
    return _card([
      Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Row(
              crossAxisAlignment: CrossAxisAlignment.end,
              children: [
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        '域名',
                        style: TextStyle(fontSize: 12, color: c.muted),
                      ),
                      const SizedBox(height: 6),
                      _glassInput('domain', 'www.google.com', mono: true),
                    ],
                  ),
                ),
                const SizedBox(width: 10),
                SizedBox(
                  width: 86,
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        '记录',
                        style: TextStyle(fontSize: 12, color: c.muted),
                      ),
                      const SizedBox(height: 6),
                      _glassSelect(
                        '记录',
                        _record,
                        {
                          for (final v in ['A', 'AAAA', 'CNAME']) v: v,
                        },
                        (v) => setState(() => _record = v),
                        mono: true,
                      ),
                    ],
                  ),
                ),
              ],
            ),
            if (_showDnsHistory)
              for (final item in _dnsHistory)
                Row(
                  children: [
                    Expanded(
                      child: InkWell(
                        onTap: () => setState(() {
                          _input('domain').text = item;
                          _showDnsHistory = false;
                        }),
                        child: Padding(
                          padding: const EdgeInsets.all(8),
                          child: Text(
                            item,
                            style: TextStyle(
                              fontFamily: 'monospace',
                              fontSize: 12,
                              color: c.ink,
                            ),
                          ),
                        ),
                      ),
                    ),
                    IconButton(
                      tooltip: '删除这条查询记录',
                      onPressed: () => _removeDns(item),
                      icon: PanelIcon('trash', size: 14, color: c.dangerInk),
                    ),
                  ],
                ),
            const SizedBox(height: 10),
            Text('解析器', style: TextStyle(fontSize: 12, color: c.muted)),
            const SizedBox(height: 6),
            _glassSelect(
              '解析器',
              _resolver,
              {
                'system': '本机 DNS',
                '8.8.8.8': 'Google (8.8.8.8)',
                '1.1.1.1': 'Cloudflare (1.1.1.1)',
                '223.5.5.5': 'AliDNS (223.5.5.5)',
                '119.29.29.29': 'DNSPod (119.29.29.29)',
                'custom': '自定义',
              },
              (v) => setState(() => _resolver = v),
              mono: true,
            ),
            if (_resolver == 'custom') ...[
              const SizedBox(height: 8),
              _glassInput('server', 'IP 或 IP:端口', mono: true),
            ],
            const SizedBox(height: 10),
            SizedBox(
              width: double.infinity,
              child: Center(
                child: _settingsBtn(
                  busy ? '查询中…' : '查询',
                  busy ? null : _lookup,
                ),
              ),
            ),
            if (_messages.containsKey('dns')) _note('dns'),
            if (answer != null) ...[
              const SizedBox(height: 12),
              DecoratedBox(
                decoration: BoxDecoration(
                  color: c.fill,
                  borderRadius: BorderRadius.circular(14),
                ),
                child: Padding(
                  padding: const EdgeInsets.all(12),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      if (records.isEmpty)
                        Text(
                          '无记录',
                          style: TextStyle(fontSize: 12, color: c.muted),
                        ),
                      for (final record in records) ...[
                        Row(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Container(
                              padding: const EdgeInsets.symmetric(
                                vertical: 2,
                                horizontal: 6,
                              ),
                              decoration: BoxDecoration(
                                color: c.primarySoft,
                                borderRadius: BorderRadius.circular(6),
                              ),
                              child: Text(
                                record['record_type'] ?? '',
                                style: TextStyle(
                                  fontSize: 11,
                                  color: c.primary,
                                ),
                              ),
                            ),
                            const SizedBox(width: 8),
                            Expanded(
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  SelectableText(
                                    record['name'] ?? '',
                                    style: TextStyle(
                                      color: c.ink,
                                      fontSize: 12,
                                    ),
                                  ),
                                  Text(
                                    "TTL ${record['ttl']}",
                                    style: TextStyle(
                                      color: c.muted,
                                      fontSize: 11,
                                    ),
                                  ),
                                  if ((record['geo'] ?? '').isNotEmpty)
                                    Text(
                                      record['geo'],
                                      style: TextStyle(
                                        color: c.muted,
                                        fontSize: 11,
                                      ),
                                    ),
                                ],
                              ),
                            ),
                            const SizedBox(width: 6),
                            Flexible(
                              child: SelectableText(
                                record['value'] ?? '',
                                style: TextStyle(
                                  fontSize: 12,
                                  color: c.ink,
                                  fontFamily: 'monospace',
                                ),
                              ),
                            ),
                          ],
                        ),
                        Divider(color: c.hairline),
                      ],
                      Text(
                        answer['source'] == 'custom'
                            ? "指定 DNS · ${answer['server']}"
                            : '本机当前 DNS',
                        style: TextStyle(fontSize: 11, color: c.muted),
                      ),
                    ],
                  ),
                ),
              ),
            ],
          ],
        ),
      ),
      SettingsRow(
        title: '显示 IP 归属地',
        subtitle: '开启后会把查询到的 IP 发送给 api.ip.sb 查询归属地。',
        inline: true,
        trailing: GlassToggle(
          value: _geo,
          onChanged: (v) async {
            try {
              await configRequest(await configDirectory(), 'set_dns_geo', {
                'enabled': v,
              });
              if (mounted) setState(() => _geo = v);
            } catch (e) {
              if (mounted) setState(() => _messages['dns'] = e.toString());
            }
          },
        ),
      ),
    ]);
  }

  Map<String, String> _updateValues() {
    final p = widget.preferences;
    String selected(String key, String fallback) =>
        (p[key] as String? ?? '').isEmpty ? fallback : p[key] as String;
    return {
      'source': selected('coreUpdateSource', 'official'),
      'channel': selected('coreUpdateChannel', 'stable'),
      'repo': p['coreUpdateCustomRepo'] as String? ?? '',
      'mirror': p['coreUpdateMirror'] as String? ?? '',
      'token': p['githubToken'] as String? ?? '',
      'core': p['singboxPath'] as String? ?? '',
    };
  }

  Widget _coreUpdate(String version) {
    final c = SingboardColors.of(context);
    final values = _updateValues();
    final busy = _busy.contains('coreUpdate');
    Future<bool> save(String key, String value) =>
        _run('coreUpdate', 'update_preferences', {...values, key: value});

    Widget stackedEdit(
      String key,
      String title,
      String desc,
      String placeholder,
    ) => Padding(
      padding: const EdgeInsets.all(16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            title,
            style: TextStyle(
              fontSize: 13,
              fontWeight: FontWeight.w500,
              color: c.ink,
            ),
          ),
          const SizedBox(height: 2),
          Text(desc, style: TextStyle(fontSize: 12, color: c.muted)),
          const SizedBox(height: 10),
          Row(
            children: [
              Expanded(
                child: _glassInput(
                  key,
                  placeholder,
                  initial: values[key]!,
                  mono: true,
                ),
              ),
              const SizedBox(width: 8),
              _settingsBtn('保存', () => save(key, _input(key).text)),
            ],
          ),
        ],
      ),
    );

    return _card([
      SettingsRow(
        title: 'sing-box 核心',
        subtitle:
            _messages['coreUpdate'] ?? (version.isEmpty ? '尚未检查上游版本' : version),
        inline: true,
        trailing: _settingsBtn(
          busy ? '检查中' : '检查核心更新',
          busy ? null : () => _run('coreUpdate', 'core_check', values),
        ),
      ),
      SettingsRow(
        title: '更新源',
        trailing: _glassSelect('更新源', values['source']!, {
          'official': '官方核心 (SagerNet/sing-box)',
          'ref1nd': 'reF1nd 核心',
          'custom': '自定义仓库',
        }, busy ? null : (v) => save('source', v)),
      ),
      if (values['source'] == 'custom')
        stackedEdit(
          'repo',
          'GitHub 仓库',
          '写成 owner/repo，从它的 Releases 下载核心。',
          'SagerNet/sing-box',
        ),
      SettingsRow(
        title: '版本通道',
        trailing: _glassSelect('版本通道', values['channel']!, {
          'stable': '稳定版',
          'testing': '测试版',
        }, busy ? null : (v) => save('channel', v)),
      ),
      stackedEdit(
        'mirror',
        '下载镜像前缀',
        '可选。下载核心和面板更新时都会把它加在 GitHub 地址前面；留空直连 GitHub。',
        'https://mirror.example.com/',
      ),
    ]);
  }

  Future<void> _checkPanel() async {
    if (_busy.contains('panel')) return;
    setState(() {
      _busy.add('panel');
      _messages['panel'] = '正在检查上游版本…';
    });
    try {
      final result = await checkPanelUpdate(await configDirectory());
      if (mounted) {
        setState(() => _messages['panel'] = result['message'] as String);
        if (result['available'] == true || result['reinstall'] == true) {
          await showPanelUpdate(context, Map<String, dynamic>.from(result));
        }
      }
    } catch (e) {
      if (mounted) setState(() => _messages['panel'] = e.toString());
    } finally {
      if (mounted) setState(() => _busy.remove('panel'));
    }
  }

  Widget _panelUpdate(Map<String, dynamic> prefs) {
    final c = SingboardColors.of(context);
    final busy = _busy.contains('panel');
    final token = (prefs['githubToken'] as String? ?? '').trim();
    final hasToken = token.isNotEmpty;
    return _card([
      SettingsRow(
        title: _version.isEmpty ? 'singboard 面板' : 'singboard 面板  $_version',
        subtitle: _messages['panel'] ?? '尚未检查上游版本',
        inline: true,
        trailing: _settingsBtn(
          busy ? '检查中' : '检查面板更新',
          busy ? null : _checkPanel,
        ),
      ),
      SettingsRow(
        title: '启动时自动检查',
        subtitle: '打开面板后静默检查是否有新版本。',
        inline: true,
        trailing: GlassToggle(
          value: prefs['panelAutoCheckUpdate'] != false,
          onChanged: _saving ? null : (v) => _savePreference(autoCheck: v),
        ),
      ),
      Padding(
        padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
        child: Row(
          children: [
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Text(
                        'GitHub Token',
                        style: TextStyle(
                          fontSize: 13,
                          fontWeight: FontWeight.w500,
                          color: c.ink,
                        ),
                      ),
                      if (hasToken) ...[
                        const SizedBox(width: 8),
                        Container(
                          padding: const EdgeInsets.symmetric(
                            horizontal: 8,
                            vertical: 1,
                          ),
                          decoration: BoxDecoration(
                            color: c.fill,
                            borderRadius: BorderRadius.circular(999),
                          ),
                          child: Text(
                            '已填写',
                            style: TextStyle(fontSize: 11, color: c.muted),
                          ),
                        ),
                      ],
                    ],
                  ),
                  const SizedBox(height: 2),
                  Text(
                    hasToken
                        ? '检查核心和面板更新时带上 Token，面板下载优先走 GitHub API。'
                        : '可选。填写后检查核心和面板更新不受匿名限流影响。',
                    style: TextStyle(fontSize: 12, color: c.muted),
                  ),
                ],
              ),
            ),
            const SizedBox(width: 12),
            _settingsBtn(hasToken ? '修改' : '填写', () => _openTokenDialog(token)),
          ],
        ),
      ),
    ]);
  }

  Future<void> _maintenance() async {
    final result = await Navigator.push<String>(
      context,
      MaterialPageRoute(
        builder: (_) => MaintenancePage(preferences: widget.preferences),
      ),
    );
    if (mounted && result != null) {
      await widget.onReload();
      if (mounted) {
        showGlassToast(context, result, error: false);
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final prefs = widget.preferences;
    final config = widget.summary['config'] as Map<String, dynamic>? ?? {};
    final modes = ((config['mode-list'] ?? config['modes']) as List? ?? [])
        .whereType<String>()
        .toList();
    final currentMode = config['mode'] as String? ?? '';
    final root = prefs['nativeRootEnabled'] == true;
    final version = widget.summary['version']?.toString() ?? '';
    final hasWallpaper = widget.wallpaperPath?.isNotEmpty == true;
    final start = _startPage(prefs);
    final work = prefs['workingDir'] as String? ?? '';
    final isRunning = root ? widget.rootRunning : version.isNotEmpty;
    final dotColor = isRunning ? c.success : c.error;
    final haloColor = isRunning ? c.successSoft : c.dangerSoft;

    return ListView(
      padding: EdgeInsets.fromLTRB(
        panelGutter(context),
        0,
        panelGutter(context),
        112,
      ),
      children: [
        Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 760),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Padding(
                  padding: const EdgeInsets.only(bottom: 20),
                  child: Builder(
                    builder: (context) {
                      final scope = GlassScope.of(context);
                      return Text(
                        '设置',
                        style: TextStyle(
                          fontSize: 26,
                          fontWeight: FontWeight.bold,
                          color: scope.backdropInk(c),
                          shadows: scope.backdropShadows(),
                        ),
                      );
                    },
                  ),
                ),
                const SettingsSectionHeader('核心与服务', first: true),
                _card([
                  SettingsRow(
                    title: 'Root 管理本机核心',
                    subtitle:
                        '需要 Magisk / KernelSU / APatch。关闭时仅作为远程面板连接 Clash API。',
                    inline: true,
                    trailing: GlassToggle(
                      value: root,
                      onChanged: _busy.contains('root')
                          ? null
                          : (v) async {
                              if (await _run(
                                    'root',
                                    v ? 'root_enable' : 'root_disable',
                                    {},
                                  ) &&
                                  v &&
                                  mounted) {
                                await widget.onProbeRoot();
                              }
                            },
                    ),
                  ),
                  Padding(
                    padding: const EdgeInsets.symmetric(
                      horizontal: 16,
                      vertical: 12,
                    ),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        Row(
                          children: [
                            Container(
                              width: 14,
                              height: 14,
                              decoration: BoxDecoration(
                                shape: BoxShape.circle,
                                color: haloColor,
                              ),
                              alignment: Alignment.center,
                              child: Container(
                                width: 8,
                                height: 8,
                                decoration: BoxDecoration(
                                  shape: BoxShape.circle,
                                  color: dotColor,
                                ),
                              ),
                            ),
                            const SizedBox(width: 12),
                            Expanded(
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  Text(
                                    root
                                        ? widget.rootStatus
                                        : (version.isEmpty ? '未连接' : '已连接'),
                                    style: TextStyle(
                                      fontSize: 14,
                                      fontWeight: FontWeight.w600,
                                      color: c.ink,
                                    ),
                                  ),
                                  Text(
                                    version.isEmpty
                                        ? '未检测'
                                        : 'sing-box $version',
                                    maxLines: 1,
                                    overflow: TextOverflow.ellipsis,
                                    style: TextStyle(
                                      fontSize: 12,
                                      fontFamily: 'monospace',
                                      color: c.muted,
                                    ),
                                  ),
                                ],
                              ),
                            ),
                            if (!root)
                              _settingsBtn(
                                '重新连接',
                                widget.probing ? null : widget.onReload,
                              ),
                          ],
                        ),
                        if (root) ...[
                          const SizedBox(height: 10),
                          Wrap(
                            spacing: 6,
                            runSpacing: 6,
                            children: [
                              _routeBtn(
                                '启动',
                                widget.probing || isRunning
                                    ? null
                                    : () => widget.onRootAction(
                                        'root_start',
                                        '启动',
                                      ),
                              ),
                              _settingsBtn(
                                '重启',
                                widget.probing || !isRunning
                                    ? null
                                    : () => widget.onRootAction(
                                        'root_restart',
                                        '重启',
                                      ),
                              ),
                              _settingsBtn(
                                '停止',
                                widget.probing || !isRunning
                                    ? null
                                    : () => widget.onRootAction(
                                        'root_stop',
                                        '停止',
                                      ),
                                danger: true,
                              ),
                            ],
                          ),
                        ],
                      ],
                    ),
                  ),
                  SettingsRow(
                    title: '代理模式',
                    subtitle: '核心当前采用的流量处理策略。',
                    trailing: _glassSelect(
                      modes.isEmpty ? '未检测' : '选择模式',
                      currentMode,
                      {for (final m in modes) m: m},
                      modes.isEmpty ? null : widget.onSetMode,
                    ),
                  ),
                  if (root) ...[
                    Padding(
                      padding: const EdgeInsets.all(16),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [
                          Text(
                            '工作目录',
                            style: TextStyle(
                              fontSize: 13,
                              fontWeight: FontWeight.w500,
                              color: c.ink,
                            ),
                          ),
                          const SizedBox(height: 2),
                          Text(
                            '自动在目录及其子目录中查找核心（优先 bin 文件夹）、配置文件和规则。',
                            style: TextStyle(fontSize: 12, color: c.muted),
                          ),
                          const SizedBox(height: 10),
                          Row(
                            children: [
                              Expanded(
                                child: _glassInput(
                                  'workdir',
                                  '/data/adb/sing-box',
                                  initial: work,
                                  mono: true,
                                ),
                              ),
                              const SizedBox(width: 8),
                              _routeBtn(
                                '识别',
                                () => _run('root', 'root_detect', {
                                  'workdir': _input('workdir').text,
                                }),
                              ),
                            ],
                          ),
                        ],
                      ),
                    ),
                    SettingsRow(
                      title: '开机自启动',
                      subtitle: '通过 /data/adb/service.d 在开机完成后启动核心。',
                      inline: true,
                      trailing: GlassToggle(
                        value: _boot == true,
                        onChanged: _boot == null || widget.probing
                            ? null
                            : (v) async {
                                await widget.onRootAction(
                                  v ? 'root_boot' : 'root_unboot',
                                  v ? '启用开机启动' : '关闭开机启动',
                                );
                                if (mounted) await _loadBoot();
                              },
                      ),
                    ),
                  ],
                  if (_messages.containsKey('root')) _note('root'),
                ]),
                const SettingsSectionHeader('后端'),
                _backends(prefs),
                const SettingsSectionHeader('网络测试'),
                _network(prefs),
                const SettingsSectionHeader('DNS 查询'),
                _dns(),
                const SettingsSectionHeader('应用'),
                _card([
                  SettingsRow(
                    title: '界面主题',
                    trailing: GlassSegmented<ThemeMode>(
                      items: const [
                        (ThemeMode.system, '跟随系统'),
                        (ThemeMode.light, '浅色'),
                        (ThemeMode.dark, '深色'),
                      ],
                      value: widget.themeMode,
                      onChanged: (m) => _savePreference(theme: m),
                    ),
                  ),
                  SettingsRow(
                    title: '默认启动页',
                    subtitle: '启动面板时首先打开的页面，下次启动生效。',
                    trailing: _glassSelect('启动页', start, {
                      for (final p in _startPages) p.$1: p.$2,
                    }, (v) => _savePreference(startPage: v)),
                  ),
                  SettingsRow(
                    title: '左右滑动切换页面',
                    subtitle: '关闭时只能点底栏切换页面，避免和页面里的横向滚动冲突。',
                    inline: true,
                    trailing: GlassToggle(
                      value: prefs['pageSwipe'] == true,
                      onChanged: _saving
                          ? null
                          : (v) => _savePreference(pageSwipe: v),
                    ),
                  ),
                  _fold(
                    'appearance',
                    '个性化',
                    '${_tintLabel(_tint)} · ${hasWallpaper ? '已设置背景' : '默认背景'}',
                  ),
                  if (_open.contains('appearance'))
                    Padding(
                      padding: const EdgeInsets.fromLTRB(16, 0, 16, 14),
                      child: Container(
                        decoration: BoxDecoration(
                          color: c.fill,
                          borderRadius: BorderRadius.circular(14),
                        ),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            Padding(
                              padding: const EdgeInsets.all(14),
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.stretch,
                                children: [
                                  Text(
                                    '玻璃效果',
                                    style: TextStyle(
                                      fontSize: 13,
                                      fontWeight: FontWeight.w500,
                                      color: c.ink,
                                    ),
                                  ),
                                  const SizedBox(height: 2),
                                  Text(
                                    '作用于顶部栏、底栏、卡片和弹窗。往左更通透，往右底色更重、文字更易读。',
                                    style: TextStyle(
                                      fontSize: 12,
                                      color: c.muted,
                                    ),
                                  ),
                                  const SizedBox(height: 8),
                                  GlassTintSliderWidget(
                                    value: _tint,
                                    onChanged: (v) {
                                      setState(() => _tint = v);
                                      widget.onTintChanged(v);
                                    },
                                    onChangeEnd: _saveTint,
                                  ),
                                ],
                              ),
                            ),
                            Divider(
                              height: 0.5,
                              thickness: 0.5,
                              indent: 14,
                              color: c.hairline,
                            ),
                            Padding(
                              padding: const EdgeInsets.symmetric(
                                horizontal: 14,
                                vertical: 10,
                              ),
                              child: Row(
                                children: [
                                  Expanded(
                                    child: Column(
                                      crossAxisAlignment:
                                          CrossAxisAlignment.start,
                                      children: [
                                        Text(
                                          '背景图片',
                                          style: TextStyle(
                                            fontSize: 13,
                                            fontWeight: FontWeight.w500,
                                            color: c.ink,
                                          ),
                                        ),
                                        const SizedBox(height: 2),
                                        Text(
                                          hasWallpaper
                                              ? '显示在应用背后，玻璃越通透越能看清它。'
                                              : '未设置，当前使用默认流光背景。',
                                          style: TextStyle(
                                            fontSize: 12,
                                            color: c.muted,
                                          ),
                                        ),
                                      ],
                                    ),
                                  ),
                                  const SizedBox(width: 8),
                                  _settingsBtn(
                                    hasWallpaper ? '更换' : '选择图片',
                                    _openWallpaperEditor,
                                  ),
                                  if (hasWallpaper) ...[
                                    const SizedBox(width: 4),
                                    _iconBtn(
                                      'trash',
                                      '删除背景',
                                      _clearWallpaper,
                                      danger: true,
                                    ),
                                  ],
                                ],
                              ),
                            ),
                          ],
                        ),
                      ),
                    ),
                ]),
                const SettingsSectionHeader('更新'),
                if (root) ...[_coreUpdate(version), const SizedBox(height: 10)],
                _panelUpdate(prefs),
                const SizedBox(height: 10),
                _card([
                  if (root)
                    SettingsRow(
                      title: '配置文件与订阅',
                      subtitle: '管理本地与远程 sing-box 配置档案。',
                      inline: true,
                      trailing: _settingsBtn(
                        '管理',
                        () => Navigator.push(
                          context,
                          MaterialPageRoute(builder: (_) => const ConfigPage()),
                        ),
                      ),
                    ),
                  SettingsRow(
                    title: '配置迁移与维护',
                    subtitle: '导入旧版设置或管理本地安装包。',
                    inline: true,
                    trailing: _settingsBtn('打开', _maintenance),
                  ),
                ]),
              ],
            ),
          ),
        ),
      ],
    );
  }
}

class _SettingsSelect extends StatefulWidget {
  const _SettingsSelect({
    required this.hint,
    required this.value,
    required this.options,
    required this.changed,
    required this.mono,
  });
  final String hint, value;
  final Map<String, String> options;
  final ValueChanged<String>? changed;
  final bool mono;
  @override
  State<_SettingsSelect> createState() => _SettingsSelectState();
}

class _SettingsSelectState extends State<_SettingsSelect> {
  bool open = false;
  Future<void> _choose() async {
    if (open || widget.changed == null || widget.options.isEmpty) return;
    final anchor = context.findRenderObject() as RenderBox;
    final overlay =
        Navigator.of(
              context,
              rootNavigator: true,
            ).overlay!.context.findRenderObject()
            as RenderBox;
    final origin = anchor.localToGlobal(Offset.zero, ancestor: overlay);
    final c = SingboardColors.of(context);
    final room = MediaQuery.viewPaddingOf(context);
    final height = math.min(
      math.min(220.0, overlay.size.height * .42),
      widget.options.length * 36.0 + 8,
    );
    final below =
        overlay.size.height - room.bottom - origin.dy - anchor.size.height - 4;
    final above = below < height && origin.dy - room.top > below;
    final y = above
        ? origin.dy - 4 - height
        : origin.dy + anchor.size.height + 4;
    final width = math.min(anchor.size.width, overlay.size.width - 16);
    setState(() => open = true);
    try {
      final selected = await showMenu<String>(
        context: context,
        useRootNavigator: true,
        requestFocus: true,
        position: RelativeRect.fromRect(
          Rect.fromLTWH(origin.dx, y, width, 0),
          Offset.zero & overlay.size,
        ),
        constraints: BoxConstraints(
          minWidth: width,
          maxWidth: width,
          maxHeight: height,
        ),
        menuPadding: const EdgeInsets.all(4),
        color: c.surface.withValues(alpha: .96),
        surfaceTintColor: Colors.transparent,
        clipBehavior: Clip.antiAlias,
        shape: RoundedRectangleBorder(
          side: BorderSide(color: c.hairline, width: .5),
          borderRadius: BorderRadius.circular(12),
        ),
        popUpAnimationStyle: const AnimationStyle(
          duration: Duration(milliseconds: 150),
          reverseDuration: Duration(milliseconds: 100),
        ),
        items: [
          for (final entry in widget.options.entries)
            PopupMenuItem<String>(
              value: entry.key,
              height: 36,
              padding: const EdgeInsets.symmetric(horizontal: 12),
              child: Text(
                entry.value,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                  fontSize: 14,
                  color: c.ink,
                  fontFamily: widget.mono ? 'monospace' : null,
                ),
              ),
            ),
        ],
      );
      if (mounted && selected != null) widget.changed?.call(selected);
    } finally {
      if (mounted) setState(() => open = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    return Semantics(
      label: widget.hint,
      button: true,
      enabled: widget.changed != null,
      child: Material(
        type: MaterialType.transparency,
        child: InkWell(
          onTap: widget.changed == null ? null : _choose,
          borderRadius: BorderRadius.circular(10),
          child: Container(
            height: 32,
            padding: const EdgeInsets.symmetric(horizontal: 12),
            decoration: BoxDecoration(
              color: c.fill,
              borderRadius: BorderRadius.circular(10),
            ),
            child: Row(
              children: [
                Expanded(
                  child: Text(
                    widget.options[widget.value] ?? widget.hint,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                      fontSize: 13,
                      fontFamily: widget.mono ? 'monospace' : null,
                      color: c.ink,
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                AnimatedRotation(
                  turns: open ? 0.5 : 0,
                  duration: const Duration(milliseconds: 150),
                  child: PanelIcon('select-arrow', size: 12, color: c.muted),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
