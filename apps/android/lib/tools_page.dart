import 'dart:async';

import 'package:material_ui/material_ui.dart';

import 'config_page.dart';
import 'native_stream.dart';
import 'glass_ui.dart';

class ToolsPage extends StatefulWidget {
  final int initial;
  final String tag, query;
  final Future<void> Function()? onSaved;
  const ToolsPage({
    super.key,
    this.onSaved,
    this.tag = "",
    this.query = "",
    this.initial = 0,
  });
  @override
  State<ToolsPage> createState() => _ToolsPageState();
}

class _ToolsPageState extends State<ToolsPage> {
  final inputs = <String, TextEditingController>{};
  List<Map<String, dynamic>> forms = [];
  int selected = 0;
  String message = '';
  bool hideResult = false;
  bool busy = false;

  @override
  void initState() {
    super.initState();
    load(widget.initial);
  }

  Future<void> load(int index) async {
    try {
      final raw = await configRequest(await configDirectory(), 'tools');
      if (!mounted) return;
      forms = (raw as List).cast<Map<String, dynamic>>();
      select(index);
      if (index == 5) {
        if (widget.tag.isNotEmpty) inputs["tag"]?.text = widget.tag;
        if (widget.query.isNotEmpty) inputs["query"]?.text = widget.query;
      }
    } catch (e) {
      if (mounted) setState(() => message = e.toString());
    }
  }

  void select(int index) {
    final old = Map.of(inputs);
    inputs.clear();
    selected = index.clamp(0, forms.length - 1);
    for (final f in forms[selected]['fields']) {
      inputs[f['key']] = TextEditingController(text: f['value']);
    }
    setState(() => message = '');
    WidgetsBinding.instance.addPostFrameCallback((_) {
      for (final input in old.values) {
        input.dispose();
      }
    });
  }

  Future<bool> confirm(String label, String detail) async {
    return await showGlassDialog<bool>(
          context: context,
          builder: (c) => GlassConfirmDialog(
            title: Text(label),
            content: Text(detail),
            actions: [
              TextButton(
                onPressed: () => Navigator.pop(c, false),
                child: const Text('取消'),
              ),
              FilledButton(
                onPressed: () => Navigator.pop(c, true),
                child: const Text('确认'),
              ),
            ],
          ),
        ) ??
        false;
  }

  Future<void> run(String action, String label) async {
    if (busy) return;
    Timer? progress;
    final values = inputs.map((k, v) => MapEntry(k, v.text));
    setState(() {
      busy = true;
      message = '';
    });
    try {
      if ({
            'delete_api',
            'root_install',
            'root_stop',
            'root_restart',
            'root_boot',
            'root_unboot',
          }.contains(action) &&
          !await confirm(label, '确认执行「$label」？'))
        return;
      final dir = await configDirectory();
      final args = <String, dynamic>{
        'action': action,
        'values': values,
      };
      var reply = await configRequest(dir, 'tool', args) as Map;
      if (!mounted) return;
      if (reply['ticket'] case final String ticket) {
        if (!await confirm('发现新核心版本', reply['text'])) return;
        progress = Timer.periodic(const Duration(milliseconds: 700), (_) async {
          try {
            final text = await configRequest(dir, 'core_progress');
            if (mounted && busy) setState(() => message = text);
          } catch (_) {}
        });
        reply = await configRequest(dir, 'tool', {
          ...args,
          'ticket': ticket,
        }) as Map;
      }
      if (!mounted) return;
      if ({'setup', 'skip_setup'}.contains(action) && mounted) {
        Navigator.pop(context);
        return;
      }
      if (reply['refresh'] == true) {
        await widget.onSaved?.call();
        if (mounted) await load(selected);
      }
      if (mounted) setState(() => message = reply['text']);
    } catch (e) {
      if (mounted) setState(() => message = e.toString());
    } finally {
      progress?.cancel();
      if (mounted) setState(() => busy = false);
    }
  }

  @override
  void dispose() {
    for (final input in inputs.values) {
      input.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => GlassBackdrop(child: Scaffold(
    backgroundColor: Colors.transparent,
    appBar: AppBar(backgroundColor: Colors.transparent, leading: IconButton(tooltip: '返回', onPressed: () => Navigator.maybePop(context), icon: PanelIcon('back', size: 18, color: SingboardColors.of(context).ink)), title: const Text('面板设置与工具')),
    body: ListView(
      padding: const EdgeInsets.all(16),
      children: [
        if (forms.isNotEmpty) ...[
          DropdownButton<int>(
            isExpanded: true,
            value: selected,
            onChanged: busy ? null : (i) => select(i!),
            items: [
              for (var i = 0; i < forms.length; i++)
                DropdownMenuItem(value: i, child: Text(forms[i]['title'])),
            ],
          ),
          for (final f in forms[selected]['fields'])
            if (f['label'] != '')
              Padding(
                padding: const EdgeInsets.symmetric(vertical: 6),
                child: (f['choices'] as List).isEmpty
                    ? TextField(
                        key: ValueKey('$selected/${f['key']}'),
                        controller: inputs[f['key']],
                        decoration: InputDecoration(labelText: f['label']),
                        obscureText: f['secret'],
                        enabled: !busy,
                      )
                    : InputDecorator(
                        decoration: InputDecoration(labelText: f['label']),
                        child: DropdownButton<String>(
                          isExpanded: true,
                          value: inputs[f['key']]!.text,
                          onChanged: busy
                              ? null
                              : (v) =>
                                    setState(() => inputs[f['key']]!.text = v!),
                          items: [
                            for (final entry in f['choices'])
                              DropdownMenuItem<String>(
                                value: entry[0],
                                child: Text(entry[1]),
                              ),
                          ],
                        ),
                      ),
              ),
          Wrap(
            spacing: 8,
            children: [
              for (final a in forms[selected]['actions'])
                TextButton(
                  onPressed: busy ? null : () => run(a[0], a[1]),
                  child: Text(a[1]),
                ),
            ],
          ),
        ],
        if (busy) const LinearProgressIndicator(),
        if (message.isNotEmpty && selected == 3)
          TextButton(
            onPressed: () => setState(() => hideResult = !hideResult),
            child: Text(hideResult ? '显示结果' : '隐藏结果'),
          ),
        if (message.isNotEmpty && (selected != 3 || !hideResult))
          SelectableText(message),
      ],
    ),
  ));
}
