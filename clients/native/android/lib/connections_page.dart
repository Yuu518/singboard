import 'dart:async';

import 'package:material_ui/material_ui.dart';

import 'catalog_page.dart' show Item;
import 'config_page.dart' show configDirectory;
import 'glass_ui.dart';
import 'native_stream.dart';

enum _RowState { live, closing, ended }

class _ConnRow {
  final Item conn;
  final _RowState state;
  const _ConnRow(this.conn, this.state);
}

Future<(int, int, Object?)> disconnectSelected(
  Iterable<String> ids,
  Future<void> Function(String) disconnect,
) async {
  var succeeded = 0, failed = 0;
  Object? reason;
  for (final id in ids) {
    try {
      await disconnect(id);
      succeeded++;
    } catch (e) {
      failed++;
      reason = e;
    }
  }
  return (succeeded, failed, reason);
}

class ConnectionsPage extends StatefulWidget {
  final String endpoint, secret, status, sort;
  final ValueChanged<String> onSort;
  final List<Item> active, history;
  final num download, upload;
  const ConnectionsPage({
    super.key,
    required this.sort,
    required this.onSort,
    required this.endpoint,
    required this.secret,
    required this.status,
    required this.active,
    required this.history,
    required this.download,
    required this.upload,
  });
  @override
  State<ConnectionsPage> createState() => _ConnectionsPageState();
}

class _ConnectionsPageState extends State<ConnectionsPage> {
  final _searchCtrl = TextEditingController();
  bool closed = false, busy = false;
  String query = '';
  static const sorts = {
    'recent': '最新',
    'duration': '时长最长',
    'download': '下载速度',
    'upload': '上传速度',
    'downloadTotal': '下载总量',
    'uploadTotal': '上传总量',
    'host': '域名',
  };
  late String sort = sorts.containsKey(widget.sort) ? widget.sort : 'recent';
  (List<Item>, List<Item>, num, num)? frozen;
  final _closingIds = <String>{};
  final _closingTimers = <String, Timer>{};
  bool _touchHeld = false, _listHovered = false;
  Timer? _touchReleaseTimer;
  List<(String, Item)>? _frozenRows;
  (bool, String, String)? _frozenKey;

  bool get _holdingRows => _touchHeld || _listHovered;

  void _markClosing(Iterable<String> ids) {
    setState(() {
      for (final id in ids) {
        if (id.isEmpty) continue;
        _closingIds.add(id);
        _closingTimers[id]?.cancel();
        _closingTimers[id] = Timer(const Duration(seconds: 4), () {
          _closingTimers.remove(id);
          if (mounted && _closingIds.remove(id)) setState(() {});
        });
      }
    });
  }

  void _unmarkClosing(Iterable<String> ids) {
    var changed = false;
    for (final id in ids) {
      _closingTimers.remove(id)?.cancel();
      if (_closingIds.remove(id)) changed = true;
    }
    if (changed && mounted) setState(() {});
  }

  void _onTouchStart() {
    _touchReleaseTimer?.cancel();
    _touchReleaseTimer = null;
    if (!_touchHeld) setState(() => _touchHeld = true);
  }

  void _releaseTouchLater() {
    if (!_touchHeld) return;
    _touchReleaseTimer?.cancel();
    _touchReleaseTimer = Timer(const Duration(milliseconds: 1200), () {
      _touchReleaseTimer = null;
      if (!mounted) return;
      setState(() {
        _touchHeld = false;
        if (!_listHovered) _frozenRows = null;
      });
    });
  }

  void _setHovered(bool hovered) {
    if (_listHovered == hovered) return;
    setState(() {
      _listHovered = hovered;
      if (!_holdingRows) _frozenRows = null;
    });
  }

  List<_ConnRow> _prepareRows(List<Item> live, (bool, String, String) key) {
    final active = [
      for (final item in live)
        if (!_closingIds.contains(item['id']?.toString() ?? '')) item,
    ];
    if (!_holdingRows) {
      _frozenRows = null;
      return [for (final item in active) _ConnRow(item, _RowState.live)];
    }
    if (_frozenRows == null || _frozenKey != key) {
      _frozenRows = [
        for (final item in active) (item['id']?.toString() ?? '', item),
      ];
      _frozenKey = key;
    }
    final pending = <String, Item>{
      for (final item in active) item['id']?.toString() ?? '': item,
    };
    final out = <_ConnRow>[];
    for (final (id, snapshot) in _frozenRows!) {
      final current = pending.remove(id);
      if (current != null) {
        out.add(_ConnRow(current, _RowState.live));
      } else {
        out.add(
          _ConnRow(
            snapshot,
            _closingIds.contains(id) ? _RowState.closing : _RowState.ended,
          ),
        );
      }
    }
    for (final item in active) {
      final id = item['id']?.toString() ?? '';
      if (pending.remove(id) != null) {
        out.add(_ConnRow(item, _RowState.live));
      }
    }
    return out;
  }

  @override
  void didUpdateWidget(ConnectionsPage old) {
    super.didUpdateWidget(old);
    if (frozen == null && _closingIds.isNotEmpty) {
      final present = {
        for (final item in widget.active) item['id']?.toString() ?? '',
      };
      final gone = [
        for (final id in _closingIds)
          if (!present.contains(id)) id,
      ];
      for (final id in gone) {
        _closingTimers.remove(id)?.cancel();
        _closingIds.remove(id);
      }
    }
  }

  String get effectiveSort =>
      closed && (sort == 'download' || sort == 'upload') ? 'recent' : sort;

  bool matches(Item row) {
    final m = row['metadata'] as Item? ?? {};
    return query.isEmpty ||
        [
          for (final k in [
            'host',
            'sniffHost',
            'destinationIP',
            'sourceIP',
            'destinationPort',
            'network',
            'process',
            'processPath',
          ])
            m[k],
          (row['chains'] as List? ?? []).join(' '),
          row['rule'],
          row['rulePayload'],
        ].any(
          (field) => field?.toString().toLowerCase().contains(query) == true,
        );
  }

  @override
  void dispose() {
    _touchReleaseTimer?.cancel();
    for (final timer in _closingTimers.values) {
      timer.cancel();
    }
    _closingTimers.clear();
    _searchCtrl.dispose();
    super.dispose();
  }

  Future<void> selectSort(String key) async {
    try {
      await configRequest(await configDirectory(), 'tool', {
        'action': 'connection_sort',
        'values': {'key': key},
      });
      if (mounted) {
        widget.onSort(key);
        setState(() => sort = key);
      }
    } catch (e) {
      if (mounted) {
        showGlassToast(context, e.toString());
      }
    }
  }

  String host(Item r) =>
      ['host', 'sniffHost', 'destinationIP']
          .map((k) => r['metadata']?[k])
          .whereType<String>()
          .where((s) => s.isNotEmpty)
          .firstOrNull ??
      '未知目标';

  String durationLabel(Item r) {
    final start = (r['startMs'] as num?)?.toInt() ?? 0;
    if (start == 0) return '';
    final end = closed
        ? ((r['closedAt'] as num?)?.toInt() ?? start)
        : DateTime.now().millisecondsSinceEpoch;
    final secs = ((end - start) ~/ 1000).clamp(0, 1 << 30);
    if (secs < 60) return '${secs}s';
    if (secs < 3600) return '${secs ~/ 60}m ${secs % 60}s';
    return '${secs ~/ 3600}h ${(secs % 3600) ~/ 60}m';
  }

  int compare(Item a, Item b) {
    int n(Item r, String k) => (r[k] as num?)?.toInt() ?? 0;
    int key(Item r) => switch (effectiveSort) {
      'duration' => n(r, 'closedAt') - n(r, 'startMs'),
      'download' when !closed => n(r, 'downloadSpeed'),
      'upload' when !closed => n(r, 'uploadSpeed'),
      'downloadTotal' => n(r, 'download'),
      'uploadTotal' => n(r, 'upload'),
      _ => n(r, closed ? 'closedAt' : 'startMs'),
    };
    final diff = effectiveSort == 'host'
        ? host(a).compareTo(host(b))
        : key(b).compareTo(key(a));
    if (diff != 0) return diff;
    final start = n(b, 'startMs').compareTo(n(a, 'startMs'));
    return start != 0
        ? start
        : (a['id'] ?? '').toString().compareTo((b['id'] ?? '').toString());
  }

  Future<void> disconnect(
    List<Item> rows, {
    bool all = false,
    bool batch = false,
  }) async {
    final ids = rows
        .map((c) => c['id'])
        .whereType<String>()
        .where((id) => id.isNotEmpty)
        .toSet();
    if (busy || ids.isEmpty) return;
    if (batch) {
      final confirmed = await showGlassDialog<bool>(
        context: context,
        builder: (context) => GlassConfirmDialog(
          title: Text(all ? '断开全部连接' : '断开筛选结果'),
          content: Text(
            all
                ? '将断开当前全部 ${ids.length} 个连接，正在进行的下载、通话和视频都会中断。'
                : '将断开匹配“${_searchCtrl.text.trim()}”的 ${ids.length} 个连接。',
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context, false),
              child: const Text('取消'),
            ),
            FilledButton(
              style: FilledButton.styleFrom(backgroundColor: SingboardColors.of(context).error),
              onPressed: () => Navigator.pop(context, true),
              child: const Text('断开'),
            ),
          ],
        ),
      );
      if (confirmed != true || !mounted) return;
    }
    final endpoint = widget.endpoint;
    final secret = widget.secret;
    _markClosing(ids);
    setState(() => busy = true);
    try {
      if (all) {
        await clashRequest(endpoint, secret, 'disconnect_all');
      } else {
        final failedIds = <String>[];
        final (succeeded, failed, reason) = await disconnectSelected(ids, (
          id,
        ) async {
          try {
            await clashRequest(endpoint, secret, 'disconnect', {'id': id});
          } catch (_) {
            failedIds.add(id);
            rethrow;
          }
        });
        if (failedIds.isNotEmpty) _unmarkClosing(failedIds);
        if (mounted && (batch || failed > 0)) {
          showGlassToast(
            context,
            batch
                ? (failed == 0
                      ? '已断开 $succeeded 个连接。'
                      : '已断开 $succeeded 个连接，$failed 个未能断开。')
                : '断开连接失败：$reason',
            error: failed > 0,
          );
        }
      }
    } catch (e) {
      _unmarkClosing(ids);
      if (mounted) {
        showGlassToast(context, e.toString());
      }
    } finally {
      if (mounted) setState(() => busy = false);
    }
  }

  Future<Item?> _geo(String ip) async =>
      await configRequest(await configDirectory(), 'connection_geo', {'ip': ip})
          as Item?;

  void _showDetail(Item row, String target) {
    final m = row['metadata'] as Item? ?? {};
    final ip = m['destinationIP']?.toString() ?? '';
    var geo = ip.isEmpty ? null : _geo(ip);
    final c = SingboardColors.of(context);
    final start = DateTime.tryParse(row['start']?.toString() ?? '')?.toLocal();
    final open = !closed;
    String address(dynamic host, dynamic port) {
      final h = host?.toString() ?? '';
      if (port == null || port.toString().isEmpty) return h.isEmpty ? '—' : h;
      if (h.isEmpty) return '端口 $port';
      return h.contains(':') && !h.startsWith('[') ? '[$h]:$port' : '$h:$port';
    }

    Widget section(String title, Map<String, dynamic> fields) => Padding(
      padding: const EdgeInsets.only(bottom: 16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            title,
            style: TextStyle(
              fontSize: 14,
              fontWeight: FontWeight.w600,
              color: c.muted,
            ),
          ),
          const SizedBox(height: 8),
          for (final field in fields.entries)
            Padding(
              padding: const EdgeInsets.only(bottom: 4),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  SizedBox(
                    width: 86,
                    child: Text(
                      field.key,
                      style: TextStyle(fontSize: 13, color: c.muted),
                    ),
                  ),
                  Expanded(
                    child: SelectableText(
                      field.value?.toString().isNotEmpty == true
                          ? field.value.toString()
                          : '—',
                      style: TextStyle(fontSize: 13, color: c.ink),
                    ),
                  ),
                ],
              ),
            ),
        ],
      ),
    );
    showGlassDialog<void>(
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, refresh) => Dialog(
          backgroundColor: Colors.transparent,
          insetPadding: const EdgeInsets.symmetric(horizontal: 16),
          child: ConstrainedBox(
            constraints: BoxConstraints(
              maxWidth: 672,
              maxHeight: MediaQuery.sizeOf(ctx).height * .8,
            ),
            child: GlassCard(
              popover: true,
              padding: const EdgeInsets.fromLTRB(20, 12, 20, 14),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Expanded(
                        child: Text(
                          host(row),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            fontSize: 18,
                            fontWeight: FontWeight.bold,
                            color: c.ink,
                          ),
                        ),
                      ),
                      if (!open) Text('已关闭', style: TextStyle(color: c.muted)),
                      IconButton(
                        tooltip: '关闭详情',
                        icon: PanelIcon(
                          'close-detail',
                          size: 12,
                          color: c.muted,
                        ),
                        onPressed: () => Navigator.pop(ctx),
                      ),
                    ],
                  ),
                  Flexible(
                    child: SingleChildScrollView(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          section('基本信息', {
                            'ID': row['id'],
                            '网络': m['network']?.toString().toUpperCase(),
                            '入站': m['type'],
                            '主机': host(row),
                            '规则': row['rule']?.toString().isNotEmpty == true
                                ? row['rule']
                                : 'final',
                            if (row['rulePayload']?.toString().isNotEmpty ==
                                true)
                              '规则载荷': row['rulePayload'],
                            '链路': (row['chains'] as List? ?? []).reversed.join(
                              ' → ',
                            ),
                            '开始时间': start?.toString().split('.').first,
                            '持续时长': durationLabel(row),
                          }),
                          section('网络信息', {
                            '源地址': address(m['sourceIP'], m['sourcePort']),
                            '目标地址': address(ip, m['destinationPort']),
                            'DNS 模式': m['dnsMode'],
                            if (m['sniffHost']?.toString().isNotEmpty == true)
                              '嗅探主机': m['sniffHost'],
                            '进程':
                                m['process'] ??
                                m['processPath']
                                    ?.toString()
                                    .split(RegExp(r'[/\\]'))
                                    .last,
                            if (m['processPath']?.toString().isNotEmpty == true)
                              '进程路径': m['processPath'],
                          }),
                          if (geo != null)
                            FutureBuilder<Item?>(
                              future: geo,
                              builder: (_, state) {
                                if (state.hasError)
                                  return Column(
                                    crossAxisAlignment:
                                        CrossAxisAlignment.start,
                                    children: [
                                      section('IP 地理信息', {'查询失败': state.error}),
                                      TextButton(
                                        onPressed: () =>
                                            refresh(() => geo = _geo(ip)),
                                        child: const Text('重试'),
                                      ),
                                    ],
                                  );
                                if (state.connectionState !=
                                    ConnectionState.done)
                                  return section('IP 地理信息', {'': '正在查询…'});
                                final data = state.data;
                                if (data == null)
                                  return const SizedBox.shrink();
                                return section('IP 地理信息', {
                                  'IP': ip,
                                  'ASN': data['asn'] == null
                                      ? null
                                      : 'AS${data['asn']}',
                                  '运营商':
                                      data['asn_organization'] ??
                                      data['organization'],
                                  '位置':
                                      [
                                            data['city'],
                                            data['region'],
                                            data['country'],
                                          ]
                                          .whereType<String>()
                                          .where((v) => v.isNotEmpty)
                                          .join(', '),
                                });
                              },
                            ),
                          section('流量信息', {
                            '下载总量': formatBytes(row['download'] as num? ?? 0),
                            '上传总量': formatBytes(row['upload'] as num? ?? 0),
                            if (open) ...{
                              '下载速度':
                                  '${formatBytes(row['downloadSpeed'] as num? ?? 0)}/s',
                              '上传速度':
                                  '${formatBytes(row['uploadSpeed'] as num? ?? 0)}/s',
                            },
                          }),
                        ],
                      ),
                    ),
                  ),
                  Row(
                    mainAxisAlignment: MainAxisAlignment.end,
                    children: [
                      if (open)
                        TextButton(
                          onPressed: busy
                              ? null
                              : () {
                                  Navigator.pop(ctx);
                                  disconnect([row]);
                                },
                          child: Text(
                            '断开连接',
                            style: TextStyle(color: c.dangerInk),
                          ),
                        ),
                      TextButton(
                        onPressed: () => Navigator.pop(ctx),
                        child: const Text('完成'),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final scope = GlassScope.of(context);
    final activeList = frozen?.$1 ?? widget.active;
    final historyList = frozen?.$2 ?? widget.history;
    final activeMatches = activeList
        .where(
          (item) =>
              !_closingIds.contains(item['id']?.toString() ?? '') &&
              matches(item),
        )
        .toList();
    final historyMatches = historyList.where(matches).toList();
    final sorted = closed ? List<Item>.of(historyMatches) : List<Item>.of(activeMatches);
    sorted.sort(compare);
    final rows = _prepareRows(sorted, (closed, effectiveSort, query));
    final liveRows = [
      for (final r in rows)
        if (r.state == _RowState.live) r.conn,
    ];
    if (rows.isEmpty && _holdingRows) {
      _touchHeld = false;
      _listHovered = false;
      _frozenRows = null;
    }

    return Column(
      children: [
        Padding(
          padding: EdgeInsets.fromLTRB(
            panelGutter(context),
            0,
            panelGutter(context),
            12,
          ),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              LayoutBuilder(
                builder: (context, bounds) {
                  final title = Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Text(
                        '连接',
                        style: TextStyle(
                          fontSize: 26,
                          height: 1.25,
                          fontWeight: FontWeight.w700,
                          letterSpacing: -.65,
                          color: scope.backdropInk(c),
                          shadows: scope.backdropShadows(),
                        ),
                      ),
                      const SizedBox(width: 12),
                      GlassSegmented<bool>(
                        items: [
                          (false, '活跃 (${activeMatches.length})'),
                          (true, '已关闭 (${historyMatches.length})'),
                        ],
                        value: closed,
                        onChanged: (v) => setState(() => closed = v),
                      ),
                    ],
                  );
                  final actions = Wrap(
                    alignment: WrapAlignment.end,
                    spacing: 8,
                    crossAxisAlignment: WrapCrossAlignment.center,
                    children: [
                      Text(
                        '↓ ${formatBytes(frozen?.$3 ?? widget.download)} · ↑ ${formatBytes(frozen?.$4 ?? widget.upload)}',
                        style: TextStyle(
                          fontSize: 12,
                          color: scope.backdropMuted(c),
                          shadows: scope.backdropShadows(),
                        ),
                      ),
                      IconButton(
                        constraints: const BoxConstraints.tightFor(
                          width: 32,
                          height: 32,
                        ),
                        padding: EdgeInsets.zero,
                        tooltip: frozen == null ? '暂停刷新' : '继续刷新',
                        style: IconButton.styleFrom(
                          backgroundColor: frozen == null
                              ? Colors.transparent
                              : c.primarySoft,
                          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                        ),
                        onPressed: () => setState(
                          () => frozen = frozen == null
                              ? (
                                  List<Item>.of(widget.active),
                                  List<Item>.of(widget.history),
                                  widget.download,
                                  widget.upload,
                                )
                              : null,
                        ),
                        icon: PlaybackIcon(
                          paused: frozen != null,
                          color: frozen != null
                              ? c.selectionInk
                              : scope.backdropInk(c),
                        ),
                      ),
                      Visibility(
                        visible: !closed,
                        maintainSize: true,
                        maintainState: true,
                        maintainAnimation: true,
                        child: GlassControl(
                          radius: 999,
                          tint: c.dangerSoft,
                          child: TextButton(
                            onPressed: busy || liveRows.isEmpty
                                ? null
                                : () => disconnect(
                                    liveRows,
                                    all: query.isEmpty,
                                    batch: true,
                                  ),
                            style: TextButton.styleFrom(
                              backgroundColor: Colors.transparent,
                              foregroundColor: c.dangerInk,
                              minimumSize: const Size(0, 32),
                              padding: const EdgeInsets.symmetric(horizontal: 12),
                              tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                              textStyle: Theme.of(context).textTheme.bodyMedium
                                  ?.copyWith(
                                    fontSize: 14,
                                    fontWeight: FontWeight.w600,
                                  ),
                            ),
                            child: Text(
                              query.isEmpty
                                  ? '断开全部'
                                  : '断开筛选结果 (${liveRows.length})',
                            ),
                          ),
                        ),
                      ),
                    ],
                  );
                  return SizedBox(
                    width: bounds.maxWidth,
                    child: Wrap(
                      alignment: WrapAlignment.spaceBetween,
                      crossAxisAlignment: WrapCrossAlignment.center,
                      runSpacing: 8,
                      spacing: 12,
                      children: [
                        title,
                        Align(
                          widthFactor: bounds.maxWidth < 570 ? null : 1,
                          alignment: Alignment.centerRight,
                          child: actions,
                        ),
                      ],
                    ),
                  );
                },
              ),
              const SizedBox(height: 12),
              Row(
                children: [
                  Expanded(
                    child: GlassSearchField(
                      controller: _searchCtrl,
                      hint: '搜索域名、IP、端口、进程、规则或节点',
                      value: query,
                      onChanged: (v) =>
                          setState(() => query = v.trim().toLowerCase()),
                    ),
                  ),
                  const SizedBox(width: 8),
                  PopupMenuButton<String>(
                    initialValue: effectiveSort,
                    onSelected: selectSort,
                    itemBuilder: (_) => [
                      for (final e in sorts.entries)
                        if (!closed || !{'download', 'upload'}.contains(e.key))
                          PopupMenuItem(value: e.key, child: Text(e.value)),
                    ],
                    child: GlassControl(
                      radius: 10,
                      child: SizedBox(
                        height: 32,
                        width: 102,
                        child: Padding(
                          padding: const EdgeInsets.symmetric(horizontal: 11),
                          child: Row(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              Expanded(
                                child: Text(
                                  sorts[effectiveSort] ?? '最新',
                                  style: TextStyle(fontSize: 13, color: c.ink),
                                ),
                              ),
                              PanelIcon('select-caret', size: 16, color: c.ink),
                            ],
                          ),
                        ),
                      ),
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
        if (busy) const LinearProgressIndicator(minHeight: 2),
        if (frozen != null)
          Padding(
            padding: EdgeInsets.fromLTRB(
              panelGutter(context),
              0,
              panelGutter(context),
              12,
            ),
            child: GlassCard(
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      '已暂停刷新，显示的是暂停时的连接。',
                      style: TextStyle(fontSize: 12, color: c.ink),
                    ),
                  ),
                  InkWell(
                    onTap: () => setState(() => frozen = null),
                    child: Text(
                      '继续刷新',
                      style: TextStyle(fontSize: 12, color: c.selectionInk),
                    ),
                  ),
                ],
              ),
            ),
          ),
        if (frozen == null && widget.status != '已连接')
          Padding(
            padding: const EdgeInsets.only(bottom: 4),
            child: Text(
              widget.status,
              style: TextStyle(
                fontSize: 12,
                color: scope.backdropMuted(c),
                shadows: scope.backdropShadows(),
              ),
            ),
          ),
        Expanded(
          child: rows.isEmpty
              ? Center(
                  child: Text(
                    query.isEmpty
                        ? (closed ? '暂无已关闭的连接' : '暂无活跃连接')
                        : '没有找到匹配的连接',
                    style: TextStyle(
                      fontSize: 14,
                      color: scope.backdropMuted(c),
                      shadows: scope.backdropShadows(),
                    ),
                  ),
                )
              : MouseRegion(
                  onEnter: (_) => _setHovered(true),
                  onExit: (_) => _setHovered(false),
                  child: Listener(
                    onPointerDown: (_) => _onTouchStart(),
                    onPointerUp: (_) => _releaseTouchLater(),
                    onPointerCancel: (_) => _releaseTouchLater(),
                    child: NotificationListener<ScrollNotification>(
                      onNotification: (_) {
                        if (_touchReleaseTimer != null) _releaseTouchLater();
                        return false;
                      },
                      child: ListView.separated(
                        padding: EdgeInsets.fromLTRB(
                          panelGutter(context),
                          4,
                          panelGutter(context),
                          108,
                        ),
                        itemCount: rows.length,
                        separatorBuilder: (_, __) => const SizedBox(height: 8),
                        itemBuilder: (_, i) {
                          final entry = rows[i];
                          final row = entry.conn;
                          final isLive = entry.state == _RowState.live;
                          final stateText = entry.state == _RowState.closing
                              ? '正在断开…'
                              : '已结束';
                          final m = row['metadata'] as Item? ?? {};
                          final h = host(row);
                          final port = m['destinationPort']?.toString() ?? '';
                          final target = port.isEmpty
                              ? h
                              : h.contains(':') && !h.startsWith('[')
                              ? '[$h]:$port'
                              : '$h:$port';
                          final net = (m['network']?.toString() ?? '').toUpperCase();
                          final kind = (m['type']?.toString() ?? '')
                              .split('/')
                              .first
                              .toUpperCase();
                          final udp = net == 'UDP';
                          final badge = udp ? c.udp : c.tcp;
                          final rule = (row['rule']?.toString() ?? '').replaceAll(
                            RegExp(r'\brule_set='),
                            '',
                          );
                          final chain = (row['chains'] as List? ?? []).reversed.join(
                            ' → ',
                          );
                          final dl = formatBytes((row['download'] as num?) ?? 0);
                          final ul = formatBytes((row['upload'] as num?) ?? 0);
                          final rates = [
                            ('↓', (row['downloadSpeed'] as num?)?.toInt() ?? 0),
                            ('↑', (row['uploadSpeed'] as num?)?.toInt() ?? 0),
                          ];
                          final small = TextStyle(
                            fontSize: 12,
                            height: 1.5,
                            color: c.muted,
                            fontFeatures: const [FontFeature.tabularFigures()],
                          );

                          return AnimatedOpacity(
                            opacity: isLive ? 1 : .5,
                            duration: const Duration(milliseconds: 180),
                            child: GestureDetector(
                              onTap: () => _showDetail(row, target),
                              child: GlassCard(
                                padding: EdgeInsets.zero,
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Padding(
                                      padding: const EdgeInsets.fromLTRB(14, 11, 14, 0),
                                      child: Column(
                                        crossAxisAlignment: CrossAxisAlignment.start,
                                        children: [
                                          Row(
                                            children: [
                                              if (net.isNotEmpty ||
                                                  kind.isNotEmpty) ...[
                                                Container(
                                                  padding: const EdgeInsets.symmetric(
                                                    horizontal: 6,
                                                  ),
                                                  height: 18,
                                                  decoration: BoxDecoration(
                                                    color: udp ? c.udpSoft : c.tcpSoft,
                                                    borderRadius: BorderRadius.circular(
                                                      999,
                                                    ),
                                                  ),
                                                  child: DefaultTextStyle.merge(
                                                    style: TextStyle(
                                                      fontSize: 11,
                                                      height: 1.4,
                                                      fontWeight: FontWeight.w500,
                                                      color: badge,
                                                    ),
                                                    child: Row(
                                                      mainAxisSize: MainAxisSize.min,
                                                      children: [
                                                        if (kind.isNotEmpty) ...[
                                                          Text(kind),
                                                          Container(
                                                            margin:
                                                                const EdgeInsets.symmetric(
                                                                  horizontal: 4,
                                                                ),
                                                            width: 1,
                                                            height: 9,
                                                            color: badge.withValues(
                                                              alpha: .35,
                                                            ),
                                                          ),
                                                        ],
                                                        Text(net),
                                                      ],
                                                    ),
                                                  ),
                                                ),
                                                const SizedBox(width: 6),
                                              ],
                                              Expanded(
                                                child: Text(
                                                  target,
                                                  maxLines: 1,
                                                  overflow: TextOverflow.ellipsis,
                                                  style: TextStyle(
                                                    fontSize: 14,
                                                    height: 20 / 14,
                                                    fontWeight: FontWeight.w600,
                                                    color: c.ink,
                                                  ),
                                                ),
                                              ),
                                              const SizedBox(width: 8),
                                              Text(
                                                isLive ? durationLabel(row) : stateText,
                                                style: small,
                                              ),
                                            ],
                                          ),
                                          const SizedBox(height: 4),
                                          Text(
                                            rule.isEmpty ? 'final' : rule,
                                            maxLines: 1,
                                            overflow: TextOverflow.ellipsis,
                                            style: small.copyWith(height: 17 / 12),
                                          ),
                                          const SizedBox(height: 4),
                                          Text(
                                            chain.isEmpty ? '-' : chain,
                                            maxLines: 1,
                                            overflow: TextOverflow.ellipsis,
                                            style: TextStyle(
                                              fontSize: 13,
                                              height: 18 / 13,
                                              color: c.ink.withValues(alpha: .85),
                                            ),
                                          ),
                                        ],
                                      ),
                                    ),
                                    const SizedBox(height: 10),
                                    Container(
                                      constraints: const BoxConstraints(minHeight: 38),
                                      padding: const EdgeInsets.fromLTRB(14, 4, 8, 4),
                                      decoration: BoxDecoration(
                                        border: Border(
                                          top: BorderSide(color: c.hairline, width: .5),
                                        ),
                                      ),
                                      child: Row(
                                        children: [
                                          if (isLive && !closed)
                                            for (final (arrow, speed) in rates) ...[
                                              Text(
                                                '$arrow ${formatSpeedParts(speed).$1} ${formatSpeedParts(speed).$2}',
                                                maxLines: 1,
                                                overflow: TextOverflow.ellipsis,
                                                style: small.copyWith(
                                                  color: speed > 0 ? c.ink : c.muted,
                                                ),
                                              ),
                                              const SizedBox(width: 9),
                                            ],
                                          Expanded(
                                            child: Text(
                                              '$dl / $ul',
                                              textAlign: TextAlign.right,
                                              maxLines: 1,
                                              overflow: TextOverflow.ellipsis,
                                              style: small,
                                            ),
                                          ),
                                          if (!closed) ...[
                                            const SizedBox(width: 9),
                                            GestureDetector(
                                              onTap: !isLive || busy
                                                  ? null
                                                  : () => disconnect([row]),
                                              child: Container(
                                                width: 28,
                                                height: 28,
                                                alignment: Alignment.center,
                                                decoration: BoxDecoration(
                                                  shape: BoxShape.circle,
                                                  color: c.fill,
                                                ),
                                                child: PanelIcon(
                                                  'close-detail',
                                                  size: 15,
                                                  color: c.muted,
                                                ),
                                              ),
                                            ),
                                          ],
                                        ],
                                      ),
                                    ),
                                  ],
                                ),
                              ),
                            ),
                          );
                        },
                      ),
                    ),
                  ),
                ),
        ),
      ],
    );
  }
}
