import 'dart:async';

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'glass_ui.dart';
import 'native_stream.dart';

const _documents = MethodChannel('singboard/config');
typedef ConfigData = Map<String, dynamic>;

String _errorMessage(Object error) => switch (error) {
  PlatformException(:final message) => message ?? '文件操作失败',
  StateError(:final message) => message,
  _ => error.toString(),
};

Future<String> configDirectory() async =>
    (await _documents.invokeMethod<String>('directory'))!;

Future<bool> _confirm(
  BuildContext context,
  String title,
  String message,
  String action, {
  bool danger = false,
}) async =>
    await showGlassDialog<bool>(
      context: context,
      builder: (ctx) {
        final c = SingboardColors.of(ctx);
        return GlassConfirmDialog(
          title: Text(title, style: const TextStyle(fontSize: 17)),
          content: Text(message, style: const TextStyle(fontSize: 14)),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: const Text('取消'),
            ),
            FilledButton(
              style: danger
                  ? FilledButton.styleFrom(
                      backgroundColor: c.error,
                      foregroundColor: Colors.white,
                    )
                  : null,
              onPressed: () => Navigator.pop(ctx, true),
              child: Text(action),
            ),
          ],
        );
      },
    ) ??
    false;

class ConfigPage extends StatefulWidget {
  const ConfigPage({super.key});
  @override
  State<ConfigPage> createState() => _ConfigPageState();
}

class _ConfigPageState extends State<ConfigPage> {
  String directory = '', message = '';
  bool busy = false;
  List<ConfigData> profiles = [];

  @override
  void initState() {
    super.initState();
    run(() async {
      directory = await configDirectory();
    });
  }

  Future<void> run(Future<void> Function() operation) async {
    if (busy) return;
    setState(() {
      busy = true;
      message = '';
    });
    try {
      if (directory.isEmpty) directory = await configDirectory();
      await operation();
      final list = await configRequest(directory, 'list') as List;
      if (mounted) setState(() => profiles = list.cast<ConfigData>());
    } catch (e) {
      if (mounted) setState(() => message = _errorMessage(e));
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  String _hostOf(String url) {
    final u = Uri.tryParse(url);
    return (u != null && u.host.isNotEmpty) ? u.host : url;
  }

  Future<void> editSubscription([ConfigData? profile]) async {
    String name = profile?['name'] ?? '', url = profile?['url'] ?? '';
    String hours = (profile?['hours'] ?? 0).toString();
    final form = GlobalKey<FormState>();
    final save = await showGlassDialog<bool>(
      context: context,
      builder: (ctx) {
        final c = SingboardColors.of(ctx);
        InputDecoration fieldDecoration(String hint) => InputDecoration(
          hintText: hint,
          hintStyle: TextStyle(fontSize: 14, color: c.muted),
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
        );
        return GlassConfirmDialog(
          title: Text(profile == null ? '添加远程配置' : '修改信息'),
          content: SingleChildScrollView(
            child: Form(
              key: form,
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Text('配置名称', style: TextStyle(fontSize: 12, color: c.muted)),
                  const SizedBox(height: 4),
                  TextFormField(
                    initialValue: name,
                    style: TextStyle(fontSize: 14, color: c.ink),
                    decoration: fieldDecoration('例如：主订阅'),
                    onChanged: (v) => name = v.trim(),
                    validator: (v) => v!.trim().isEmpty ? '请填写名称' : null,
                  ),
                  const SizedBox(height: 12),
                  Text('远程地址（URL）', style: TextStyle(fontSize: 12, color: c.muted)),
                  const SizedBox(height: 4),
                  TextFormField(
                    initialValue: url,
                    style: TextStyle(fontSize: 14, color: c.ink, fontFamily: 'monospace'),
                    decoration: fieldDecoration('https://example.com/config.json'),
                    keyboardType: TextInputType.url,
                    autocorrect: false,
                    onChanged: (v) => url = v.trim(),
                    validator: (v) {
                      final u = Uri.tryParse(v!.trim());
                      return u != null &&
                              u.host.isNotEmpty &&
                              ['http', 'https'].contains(u.scheme)
                          ? null
                          : '请输入有效的 HTTP(S) 订阅地址';
                    },
                  ),
                  const SizedBox(height: 12),
                  Text(
                    '自动更新间隔（小时，0 为不自动更新，最长 576）',
                    style: TextStyle(fontSize: 12, color: c.muted),
                  ),
                  const SizedBox(height: 4),
                  TextFormField(
                    initialValue: hours,
                    style: TextStyle(fontSize: 14, color: c.ink, fontFamily: 'monospace'),
                    decoration: fieldDecoration('0'),
                    keyboardType: TextInputType.number,
                    onChanged: (v) => hours = v.trim(),
                    validator: (v) {
                      final h = int.tryParse(v!.trim());
                      return h != null && h >= 0 && h <= 576
                          ? null
                          : '请输入 0–576 的整数';
                    },
                  ),
                ],
              ),
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(ctx, false),
              child: const Text('取消'),
            ),
            FilledButton(
              onPressed: () {
                if (form.currentState!.validate()) Navigator.pop(ctx, true);
              },
              child: Text(profile == null ? '添加' : '保存'),
            ),
          ],
        );
      },
    );
    if (save != true || !mounted) return;
    await run(() async {
      await configRequest(directory, profile == null ? 'add' : 'update', {
        'name': name,
        'url': url,
        'hours': int.parse(hours),
        if (profile != null) 'id': profile['id'],
      });
      message = profile == null ? '已添加远程配置「$name」' : '已保存「$name」';
    });
  }

  Future<void> openEditor(
    String type, {
    String? id,
    String? uri,
    String? draft,
    String? title,
  }) async {
    if (type == 'file' && uri == null) {
      uri = await _documents.invokeMethod<String>('pick');
      if (uri == null) return;
    }
    final value = type == 'file'
        ? Map<String, dynamic>.from(
            (await _documents.invokeMethod<Map>('read', {'uri': uri}))!,
          )
        : await configRequest(
            directory,
            type == 'root' ? 'root_open' : 'open',
            {if (id != null) 'id': id},
          ) as ConfigData;
    if (!mounted) return;
    await Navigator.push<void>(
      context,
      MaterialPageRoute(
        builder: (_) => ConfigEditor(
          directory: directory,
          type: type,
          id: id,
          uri: uri,
          title: title,
          document: value,
          draft: draft,
        ),
      ),
    );
  }

  Future<void> act(String action, ConfigData profile) async {
    final id = profile['id'] as String;
    final name = profile['name']?.toString() ?? '';
    if (action == 'settings') {
      await editSubscription(profile);
      return;
    }
    if (action == 'remove' &&
        !await _confirm(
          context,
          '删除配置',
          '确定删除配置「$name」？\n同时会删除已下载的远程配置文件和它的上一版本。',
          '删除',
          danger: true,
        )) {
      return;
    }
    if (!mounted) return;
    await run(() async {
      switch (action) {
        case 'edit':
          await openEditor('profile', id: id, title: name);
        case 'refresh':
          final changed = await configRequest(directory, 'refresh', {'id': id});
          message = changed == true ? '远程配置「$name」有新内容，已更新' : '远程配置「$name」已是最新';
        case 'remove':
          await configRequest(directory, 'remove', {'id': id});
          message = '已删除「$name」';
        case 'apply':
          final remote = await configRequest(directory, 'open', {'id': id});
          await openEditor(
            'root',
            draft: remote['text'] as String,
            title: name,
          );
      }
    });
  }

  Widget _actionChip(
    String label,
    VoidCallback? onTap,
    SingboardColors c, {
    bool danger = false,
    bool accent = false,
  }) =>
      GestureDetector(
        onTap: onTap,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 5),
          child: Text(
            label,
            style: TextStyle(
              fontSize: 14,
              fontWeight: accent ? FontWeight.w600 : FontWeight.w500,
              color: danger
                  ? c.dangerInk
                  : accent
                      ? c.selectionInk
                      : c.ink.withValues(alpha: 0.85),
            ),
          ),
        ),
      );

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final scope = GlassScope.of(context);
    return GlassBackdrop(
      child: Scaffold(
        backgroundColor: Colors.transparent,
        appBar: AppBar(
          backgroundColor: Colors.transparent,
          foregroundColor: scope.backdropInk(c),
          elevation: 0,
          scrolledUnderElevation: 0,
          title: Text(
            '配置',
            style: TextStyle(
              fontSize: 26,
              fontWeight: FontWeight.w700,
              color: scope.backdropInk(c),
              shadows: scope.backdropShadows(),
            ),
          ),
          actions: [
            IconButton(
              tooltip: '添加远程配置',
              onPressed: busy ? null : () => editSubscription(),
              icon: PanelIcon('plus', size: 20, color: scope.backdropInk(c)),
            ),
          ],
        ),
        body: SafeArea(
          child: ListView(
            padding: const EdgeInsets.fromLTRB(16, 4, 16, 28),
            children: [
              Row(
                children: [
                  Expanded(
                    child: GestureDetector(
                      onTap: busy ? null : () => run(() => openEditor('file')),
                      child: GlassCard(
                        radius: 14,
                        padding: const EdgeInsets.symmetric(
                          horizontal: 12,
                          vertical: 11,
                        ),
                        child: Row(
                          mainAxisAlignment: MainAxisAlignment.center,
                          children: [
                            PanelIcon('folder', size: 17, color: c.selectionInk),
                            const SizedBox(width: 6),
                            Text(
                              '本地文件',
                              style: TextStyle(
                                fontSize: 14,
                                fontWeight: FontWeight.w500,
                                color: c.ink,
                              ),
                            ),
                          ],
                        ),
                      ),
                    ),
                  ),
                  const SizedBox(width: 10),
                  Expanded(
                    child: GestureDetector(
                      onTap: busy ? null : () => editSubscription(),
                      child: GlassCard(
                        radius: 14,
                        padding: const EdgeInsets.symmetric(
                          horizontal: 12,
                          vertical: 11,
                        ),
                        child: Row(
                          mainAxisAlignment: MainAxisAlignment.center,
                          children: [
                            PanelIcon('profile-remote', size: 17, color: c.selectionInk),
                            const SizedBox(width: 6),
                            Text(
                              '添加远程配置',
                              style: TextStyle(
                                fontSize: 14,
                                fontWeight: FontWeight.w500,
                                color: c.ink,
                              ),
                            ),
                          ],
                        ),
                      ),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 10),
              GestureDetector(
                onTap: busy ? null : () => run(() => openEditor('root', title: 'Root 服务配置')),
                child: GlassCard(
                  radius: 14,
                  padding: const EdgeInsets.symmetric(
                    horizontal: 14,
                    vertical: 11,
                  ),
                  child: Row(
                    children: [
                      PanelIcon('terminal', size: 17, color: c.muted),
                      const SizedBox(width: 8),
                      Expanded(
                        child: Text(
                          '编辑本机 Root 服务配置',
                          style: TextStyle(
                            fontSize: 14,
                            fontWeight: FontWeight.w500,
                            color: c.ink,
                          ),
                        ),
                      ),
                      PanelIcon('chevron', size: 18, color: c.muted),
                    ],
                  ),
                ),
              ),
              if (busy)
                const Padding(
                  padding: EdgeInsets.only(top: 10),
                  child: LinearProgressIndicator(minHeight: 2),
                ),
              if (message.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.only(top: 10),
                  child: GlassCard(
                    radius: 12,
                    padding: const EdgeInsets.symmetric(
                      horizontal: 12,
                      vertical: 8,
                    ),
                    child: Text(
                      message,
                      style: TextStyle(fontSize: 13, color: c.ink),
                    ),
                  ),
                ),
              const SizedBox(height: 14),
              if (!busy && profiles.isEmpty)
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 40),
                  child: Column(
                    children: [
                      Text(
                        '还没有远程配置',
                        style: TextStyle(
                          fontSize: 14.5,
                          fontWeight: FontWeight.w500,
                          color: c.ink,
                        ),
                      ),
                      const SizedBox(height: 4),
                      Text(
                        '点击上方「添加远程配置」填入订阅地址下载配置。',
                        style: TextStyle(fontSize: 13, color: c.muted),
                      ),
                    ],
                  ),
                ),
              for (final p in profiles)
                Padding(
                  padding: const EdgeInsets.only(bottom: 10),
                  child: GlassCard(
                    radius: 18,
                    padding: const EdgeInsets.all(16),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          children: [
                            Expanded(
                              child: Text(
                                p['name']?.toString() ?? '',
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: TextStyle(
                                  fontSize: 15,
                                  fontWeight: FontWeight.w600,
                                  color: c.ink,
                                ),
                              ),
                            ),
                            const SizedBox(width: 8),
                            GestureDetector(
                              onTap: busy ? null : () => act('apply', p),
                              child: Container(
                                padding: const EdgeInsets.symmetric(
                                  horizontal: 11,
                                  vertical: 4,
                                ),
                                decoration: BoxDecoration(
                                  color: c.fill,
                                  borderRadius: BorderRadius.circular(999),
                                ),
                                child: Text(
                                  '应用到核心',
                                  style: TextStyle(
                                    fontSize: 12,
                                    fontWeight: FontWeight.w600,
                                    color: c.selectionInk,
                                  ),
                                ),
                              ),
                            ),
                          ],
                        ),
                        const SizedBox(height: 6),
                        Row(
                          children: [
                            Container(
                              padding: const EdgeInsets.symmetric(
                                horizontal: 7,
                                vertical: 2,
                              ),
                              decoration: BoxDecoration(
                                color: c.fill,
                                borderRadius: BorderRadius.circular(999),
                              ),
                              child: Text(
                                '远程',
                                style: TextStyle(fontSize: 11, color: c.muted),
                              ),
                            ),
                            const SizedBox(width: 6),
                            Expanded(
                              child: Text(
                                _hostOf(p['url']?.toString() ?? ''),
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: TextStyle(
                                  fontSize: 11.5,
                                  fontFamily: 'monospace',
                                  color: c.muted,
                                ),
                              ),
                            ),
                          ],
                        ),
                        const SizedBox(height: 4),
                        Text(
                          (p['hours'] ?? 0) == 0
                              ? '手动更新'
                              : '每 ${p['hours']} 小时自动更新',
                          style: TextStyle(fontSize: 12, color: c.muted),
                        ),
                        const SizedBox(height: 8),
                        Row(
                          children: [
                            _actionChip('编辑', busy ? null : () => act('edit', p), c),
                            _actionChip('更新', busy ? null : () => act('refresh', p), c),
                            _actionChip('修改信息', busy ? null : () => act('settings', p), c),
                            const Spacer(),
                            _actionChip(
                              '删除',
                              busy ? null : () => act('remove', p),
                              c,
                              danger: true,
                            ),
                          ],
                        ),
                      ],
                    ),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }
}

class ConfigEditor extends StatefulWidget {
  const ConfigEditor({
    super.key,
    required this.directory,
    required this.type,
    required this.document,
    this.id,
    this.uri,
    this.draft,
    this.title,
  });
  final String directory, type;
  final String? id, uri, draft, title;
  final ConfigData document;
  @override
  State<ConfigEditor> createState() => _ConfigEditorState();
}

class _ConfigEditorState extends State<ConfigEditor> {
  late String saved, reference, _normalizedReference, _lastText;
  late bool _isDirty;
  late final TextEditingController input;
  bool busy = false, hasBackup = false, leaving = false;
  String message = '';
  String normalize(String value) =>
      value.replaceFirst(RegExp(r'^\uFEFF'), '').replaceAll('\r\n', '\n');
  String prefix = '', suffix = '', module = '';
  final undo = UndoHistoryController();
  String get full => prefix + input.text + suffix;
  String get content {
    final value = full;
    if (_normalizedReference == value) return reference;
    return (reference.startsWith('\uFEFF') ? '\uFEFF' : '') +
        (reference.contains('\r\n') ? value.replaceAll('\n', '\r\n') : value);
  }

  bool get dirty => _isDirty;

  void _syncReference(String next) {
    reference = next;
    _normalizedReference = normalize(next);
    _recomputeDirty();
  }

  void _recomputeDirty() {
    _lastText = input.text;
    _isDirty = content != saved;
  }

  @override
  void initState() {
    super.initState();
    saved = widget.document['text'];
    reference = widget.draft ?? saved;
    _normalizedReference = normalize(reference);
    hasBackup = widget.document['backup'] == true;
    input = TextEditingController(text: _normalizedReference);
    _recomputeDirty();
    input.addListener(changed);
    if (widget.draft != null) message = '已载入订阅，保存前会使用服务核心校验';
  }

  void changed() {
    if (input.text == _lastText) return;
    _recomputeDirty();
    if (mounted) setState(() {});
  }

  @override
  void dispose() {
    undo.dispose();
    input.removeListener(changed);
    input.dispose();
    super.dispose();
  }

  Future<void> edit(String op) async {
    try {
      if (op == 'module') {
        final source = full;
        if (module.isNotEmpty) {
          prefix = suffix = module = '';
          input.text = source;
          return;
        }
        final parts = await configRequest(widget.directory, 'modules', {
          'text': source,
        }) as List;
        if (!mounted) return;
        final part = await showGlassDialog<Map>(
          context: context,
          builder: (ctx) => SimpleDialog(
            title: const Text('选择配置模块'),
            children: [
              for (final p in parts)
                SimpleDialogOption(
                  onPressed: () => Navigator.pop(ctx, p),
                  child: Text(p['name']),
                ),
            ],
          ),
        );
        if (part == null || !mounted || source != full) return;
        prefix = source.substring(0, part['start']);
        suffix = source.substring(part['end']);
        module = part['name'];
        input.text = source.substring(part['start'], part['end']);
        return;
      }
      if (op == 'format') {
        final before = input.text;
        final result = await configRequest(widget.directory, 'format', {
          'text': before,
        });
        if (mounted && before == input.text) input.text = result;
        return;
      }
      final text = input.text, selection = input.selection;
      final start = selection.isValid ? selection.start : 0,
          end = selection.isValid ? selection.end : 0;
      final left = start == 0 ? 0 : text.lastIndexOf('\n', start - 1) + 1;
      final right = text.indexOf(
        '\n',
        end > start && text[end - 1] == '\n' ? end - 1 : end,
      );
      final last = right < 0 ? text.length : right;
      final lines = text.substring(left, last).split('\n');
      final nonEmpty = lines.where((v) => v.trim().isNotEmpty).toList();
      final remove = nonEmpty.isNotEmpty &&
          nonEmpty.every((v) => v.trimLeft().startsWith('//'));
      final changed = lines
          .map(
            (v) => remove
                ? v.replaceFirstMapped(RegExp(r'^(\s*)// ?'), (m) => m[1]!)
                : '// $v',
          )
          .join('\n');
      input.value = TextEditingValue(
        text: text.replaceRange(left, last, changed),
        selection: TextSelection(
          baseOffset: left,
          extentOffset: left + changed.length,
        ),
      );
    } catch (e) {
      if (mounted) setState(() => message = _errorMessage(e));
    }
  }

  Future<void> execute(String op) async {
    if (busy) return;
    if (op == 'backup' &&
        dirty &&
        !await _confirm(context, '载入上一版', '用上一版替换未保存的编辑？', '载入')) {
      return;
    }
    if (!mounted) return;
    setState(() {
      busy = true;
      message = '';
    });
    try {
      final root = widget.type == 'root', file = widget.type == 'file';
      final args = <String, dynamic>{
        if (widget.id != null) 'id': widget.id,
        if (widget.uri != null) 'uri': widget.uri,
        'path': widget.document['path'],
        'text': content,
        'expected': saved,
      };
      if (op == 'backup') {
        final previous = file
            ? await _documents.invokeMethod<Map>('backup', args)
            : await configRequest(
                widget.directory,
                root ? 'root_backup' : 'backup',
                args,
              ) as Map;
        if (!mounted) return;
        prefix = suffix = module = '';
        _syncReference(previous!['text'] as String);
        input.text = _normalizedReference;
        _recomputeDirty();
        message = '已载入上一版，保存后生效';
      } else if (op == 'validate') {
        message = await configRequest(
          widget.directory,
          root ? 'root_check' : 'validate',
          args,
        );
      } else {
        final previous = saved, submitted = content;
        if (file) {
          await configRequest(widget.directory, 'validate', {
            'text': submitted,
          });
          message = (await _documents.invokeMethod<String>('save', args))!;
        } else {
          message = await configRequest(
            widget.directory,
            root ? 'root_save' : 'save',
            args,
          );
        }
        saved = submitted;
        _syncReference(saved);
        hasBackup = hasBackup || previous != saved;
      }
    } catch (e) {
      message = _errorMessage(e);
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  Widget _toolPill(String label, VoidCallback? onTap, SingboardColors c) =>
      GestureDetector(
        onTap: onTap,
        child: Container(
          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 5),
          decoration: BoxDecoration(
            color: c.fill,
            borderRadius: BorderRadius.circular(999),
          ),
          child: Text(
            label,
            style: TextStyle(fontSize: 12, color: c.ink),
          ),
        ),
      );

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final scope = GlassScope.of(context);
    final headerTitle = widget.title?.isNotEmpty == true
        ? widget.title!
        : '配置编辑';
    return PopScope(
      canPop: leaving || (!busy && !dirty),
      onPopInvokedWithResult: (didPop, result) async {
        if (didPop || busy || leaving) return;
        if (await _confirm(
              context,
              '有未保存的改动',
              '「$headerTitle」还有没保存的改动，离开后这些改动会丢失。',
              '放弃改动',
              danger: true,
            ) &&
            mounted) {
          setState(() => leaving = true);
          WidgetsBinding.instance.addPostFrameCallback((_) {
            if (mounted) Navigator.pop(context);
          });
        }
      },
      child: GlassBackdrop(
        child: Scaffold(
          backgroundColor: Colors.transparent,
          appBar: AppBar(
            backgroundColor: Colors.transparent,
            foregroundColor: scope.backdropInk(c),
            elevation: 0,
            scrolledUnderElevation: 0,
            title: Row(
              children: [
                Flexible(
                  child: Text(
                    headerTitle,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 18,
                      fontWeight: FontWeight.w700,
                      color: scope.backdropInk(c),
                      shadows: scope.backdropShadows(),
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                Text(
                  dirty
                      ? '未保存'
                      : widget.type == 'profile'
                          ? '远程副本'
                          : '本地文件',
                  style: TextStyle(
                    fontSize: 12,
                    color: dirty ? c.warningInk : scope.backdropMuted(c),
                    shadows: scope.backdropShadows(),
                  ),
                ),
              ],
            ),
            actions: [
              Padding(
                padding: const EdgeInsets.only(right: 12),
                child: FilledButton(
                  style: FilledButton.styleFrom(
                    backgroundColor: c.primary,
                    foregroundColor: Colors.white,
                    padding: const EdgeInsets.symmetric(horizontal: 14),
                    minimumSize: const Size(0, 32),
                  ),
                  onPressed: busy ? null : () => execute('save'),
                  child: const Text('保存', style: TextStyle(fontSize: 14)),
                ),
              ),
            ],
          ),
          body: SafeArea(
            child: Padding(
              padding: const EdgeInsets.fromLTRB(14, 0, 14, 14),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  if (widget.type == 'profile')
                    Padding(
                      padding: const EdgeInsets.only(bottom: 8),
                      child: GlassCard(
                        radius: 12,
                        padding: const EdgeInsets.symmetric(
                          horizontal: 12,
                          vertical: 7,
                        ),
                        child: Text(
                          '下次更新（手动或自动）会用订阅内容覆盖这里的改动。',
                          style: TextStyle(fontSize: 12, color: c.muted),
                        ),
                      ),
                    ),
                  SingleChildScrollView(
                    scrollDirection: Axis.horizontal,
                    child: Row(
                      children: [
                        _toolPill(
                          widget.type == 'root' ? 'sing-box 校验' : 'JSONC 校验',
                          busy ? null : () => execute('validate'),
                          c,
                        ),
                        const SizedBox(width: 6),
                        _toolPill(
                          module.isEmpty ? '分模块编辑' : '返回全文 · $module',
                          busy ? null : () => edit('module'),
                          c,
                        ),
                        const SizedBox(width: 6),
                        _toolPill('格式化', busy ? null : () => edit('format'), c),
                        const SizedBox(width: 6),
                        _toolPill('切换注释', busy ? null : () => edit('comment'), c),
                        if (hasBackup) ...[
                          const SizedBox(width: 6),
                          _toolPill('载入上一版', busy ? null : () => execute('backup'), c),
                        ],
                        IconButton(
                          onPressed: busy ? null : undo.undo,
                          icon: PanelIcon('rotate-left', size: 19, color: c.muted),
                          tooltip: '撤销',
                        ),
                        IconButton(
                          onPressed: busy ? null : undo.redo,
                          icon: PanelIcon('rotate-right', size: 19, color: c.muted),
                          tooltip: '重做',
                        ),
                      ],
                    ),
                  ),
                  const SizedBox(height: 8),
                  Expanded(
                    child: GlassCard(
                      radius: 16,
                      padding: const EdgeInsets.all(10),
                      child: TextField(
                        key: ValueKey(module),
                        undoController: undo,
                        controller: input,
                        readOnly: busy,
                        expands: true,
                        minLines: null,
                        maxLines: null,
                        textAlignVertical: TextAlignVertical.top,
                        keyboardType: TextInputType.multiline,
                        autocorrect: false,
                        enableSuggestions: false,
                        style: TextStyle(
                          fontFamily: 'monospace',
                          fontSize: 13.5,
                          color: c.ink,
                        ),
                        decoration: const InputDecoration(
                          border: InputBorder.none,
                        ),
                      ),
                    ),
                  ),
                  if (busy)
                    const Padding(
                      padding: EdgeInsets.only(top: 6),
                      child: LinearProgressIndicator(minHeight: 2),
                    ),
                  if (message.isNotEmpty)
                    Padding(
                      padding: const EdgeInsets.only(top: 8),
                      child: GlassCard(
                        radius: 12,
                        padding: const EdgeInsets.symmetric(
                          horizontal: 12,
                          vertical: 8,
                        ),
                        child: ConstrainedBox(
                          constraints: const BoxConstraints(maxHeight: 90),
                          child: SingleChildScrollView(
                            child: SelectableText(
                              message,
                              style: TextStyle(fontSize: 12, color: c.ink),
                            ),
                          ),
                        ),
                      ),
                    ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}
