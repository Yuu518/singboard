import 'dart:async';
import 'dart:io' show InternetAddress;

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'glass_ui.dart';
import 'native_stream.dart';
import 'config_page.dart' show configDirectory;

typedef Item = Map<String, dynamic>;

class _CondToken {
  final String kind;
  final String text;
  final String full;
  const _CondToken(this.kind, this.text, [this.full = '']);
}

class _RuleAction {
  final String kind;
  final String label;
  final String? finalNode;
  final String? icon;
  const _RuleAction(this.kind, this.label, {this.finalNode, this.icon});
}

class CatalogPage extends StatefulWidget {
  final String endpoint, secret;
  final bool proxies, enabled, active;
  final Map<String, dynamic> preferences;
  final Map<String, dynamic> chainSpeeds;

  const CatalogPage({
    super.key,
    required this.endpoint,
    required this.secret,
    required this.proxies,
    this.enabled = true,
    this.active = true,
    this.preferences = const {},
    this.chainSpeeds = const {},
  });

  @override
  State<CatalogPage> createState() => _CatalogPageState();
}

class _CatalogPageState extends State<CatalogPage> {
  static const _fieldLabels = <String, String>{
    'domain': '域名',
    'domain_suffix': '域名后缀',
    'domain_keyword': '域名关键词',
    'domain_regex': '域名正则',
    'ip_cidr': 'IP 段',
    'source_ip_cidr': '来源 IP 段',
    'default_interface_address': '默认网卡地址',
    'port': '端口',
    'source_port': '来源端口',
    'port_range': '端口范围',
    'source_port_range': '来源端口范围',
    'protocol': '协议',
    'network': '网络',
    'process_name': '进程',
    'process_path': '进程路径',
    'package_name': '应用',
    'inbound': '入站',
    'clash_mode': '模式',
    'network_type': '网络类型',
    'wifi_ssid': 'Wi-Fi',
    'wifi_bssid': 'Wi-Fi BSSID',
    'query_type': 'DNS 查询类型',
    'process_path_regex': '进程路径正则',
    'package_name_regex': '应用正则',
    'network_is_expensive': '计费网络',
    'network_is_constrained': '受限网络',
    'user': '用户',
  };
  static const _monoFields = {
    'ip_cidr',
    'source_ip_cidr',
    'default_interface_address',
    'port',
    'source_port',
    'port_range',
    'source_port_range',
    'domain',
    'domain_suffix',
    'domain_regex',
    'process_path',
  };
  static const _upperFields = {'protocol', 'network'};

  final _searchCtrl = TextEditingController();
  List<Item> items = [];
  List<Item> providerItems = [];
  bool providersAvailable = false;
  final expanded = <String>{};
  final expandedProviders = <String>{};
  final collapsedSearch = <String>{};
  final selectedTiles = <String, GlobalKey>{};
  final _frozenOrder = <String, Map<String, int>>{};
  final _pointed = <String>{}, _focused = <String>{};
  Future<void> _saveQueue = Future.value();

  void _savePreference(String op, Item args) {
    _saveQueue = _saveQueue.then((_) async {
      await configRequest(await configDirectory(), op, args);
    }).catchError((Object error) {
      if (mounted) showGlassToast(context, '保存代理偏好失败 · $error');
    });
  }

  List<Item> _orderedNodes(List<Item> nodes) {
    int rank(Item n) {
      final delay = _delayOf(n);
      if (delay != null && delay > 0) return delay;
      return delay == null ? 0x7ffffffe : 0x7fffffff;
    }
    final original = {for (final (i, node) in nodes.indexed) node: i};
    return [...nodes]..sort((a, b) {
      final order = rank(a).compareTo(rank(b));
      return order == 0 ? original[a]!.compareTo(original[b]!) : order;
    });
  }

  void _holdOrder(Item group) {
    _frozenOrder.putIfAbsent(group['name'] as String, () => {
      for (final (i, node) in _orderedNodes((group['nodes'] as List).cast<Item>()).indexed)
        node['name'] as String: i,
    });
  }

  void _releaseOrder(String name) {
    if (!_pointed.contains(name) && !_focused.contains(name) && !testingGroups.contains(name)) _frozenOrder.remove(name);
  }

  void toggleGroup(String group) {
    setState(() {
      final state = query.isEmpty ? expanded : collapsedSearch;
      if (!state.remove(group)) state.add(group);
    });
    if (query.isNotEmpty) return;
    widget.preferences['expandedProxyGroups'] = expanded.toList();
    _savePreference('set_proxy_group_open', {'group': group, 'open': expanded.contains(group)});
    if (!expanded.contains(group)) return;
    Future.delayed(const Duration(milliseconds: 280), () {
      if (!mounted || !widget.enabled || !expanded.contains(group)) return;
      final target = selectedTiles[group]?.currentContext;
      if (target == null || !target.mounted) return;
      final reduce = MediaQuery.disableAnimationsOf(target);
      final scroll = Scrollable.maybeOf(target, axis: Axis.vertical);
      final render = target.findRenderObject();
      if (scroll == null || render == null) return;
      final position = scroll.position;
      final policy = (render as RenderBox).localToGlobal(Offset.zero).dy <
          (scroll.context.findRenderObject() as RenderBox).localToGlobal(Offset.zero).dy
          ? ScrollPositionAlignmentPolicy.keepVisibleAtStart : ScrollPositionAlignmentPolicy.keepVisibleAtEnd;
      position.ensureVisible(render, alignmentPolicy: policy,
          duration: reduce ? Duration.zero : const Duration(milliseconds: 220), curve: Curves.easeOut);
    });
  }
  final testingGroups = <String>{};
  final testingNodes = <String>{};
  final delays = <String, int>{};
  final v6Nodes = <String>{};
  (String group, String prev)? undoSwitch;
  Timer? undoTimer;
  Timer? ruleTimer;
  int matchRevision = 0;
  Set<String> matched = {};
  String matchError = '';
  bool matching = false;
  bool get readable => widget.preferences['nativeRootEnabled'] == true;

  bool providers = false, busy = false, updating = false, sortByLatency = false;
  int generation = 0;
  String query = '', error = '';

  @override
  void initState() {
    super.initState();
    sortByLatency = widget.preferences['proxySortLatency'] == true;
    expanded.addAll((widget.preferences['expandedProxyGroups'] as List? ?? []).whereType<String>());
    if (widget.enabled) load();
  }

  @override
  void didUpdateWidget(CatalogPage old) {
    super.didUpdateWidget(old);
    if (widget.enabled && !old.enabled) {
      load();
    } else if (widget.enabled && widget.active && !old.active) {
      load(silent: items.isNotEmpty);
    }
  }

  @override
  void dispose() {
    undoTimer?.cancel();
    ruleTimer?.cancel();
    _searchCtrl.dispose();
    super.dispose();
  }

  Future<void> load({bool silent = false}) async {
    final revision = ++generation;
    if (!silent) {
      setState(() {
        busy = true;
        error = '';
      });
    }
    try {
      final op = widget.proxies ? 'proxies' : 'rules';
      final response = await clashRequest(widget.endpoint, widget.secret, op);
      if (!mounted || revision != generation) return;
      final next = ((widget.proxies ? response : response['rules']) as List)
          .cast<Item>();
      if (widget.proxies) {
        final urls = widget.preferences['groupTestUrls'] as Map?;
        for (final group in next) {
          final url =
              urls?[group['name']] ?? widget.preferences['latencyTestUrl'];
          if (url is String && url.isNotEmpty) group['test_url'] = url;
        }
      }
      List<Item> nextProviders = providerItems;
      var providerApiReady = providersAvailable;
      try {
        final provResp = await clashRequest(
          widget.endpoint,
          widget.secret,
          widget.proxies ? 'proxy_providers' : 'rule_providers',
        );
        final source = provResp['providers'] as Map<String, dynamic>? ?? {};
        nextProviders = [
          for (final entry in source.entries)
            {...entry.value as Item, 'name': entry.key},
        ];
        providerApiReady = !widget.proxies || nextProviders.isNotEmpty;
      } catch (_) {
        if (widget.proxies || !providerApiReady) {
          nextProviders = [];
          providerApiReady = false;
        }
      }
      if (!mounted || revision != generation) return;
      setState(() {
        items = next;
        providerItems = nextProviders;
        providersAvailable = providerApiReady;
        if (!providersAvailable) providers = false;
        error = '';
      });
      if (!widget.proxies && query.isNotEmpty) scheduleMatch();
    } catch (e) {
      if (mounted && revision == generation && !silent) {
        setState(() {
          items = [];
          error = e.toString();
        });
      }
    } finally {
      if (mounted && revision == generation && !silent) {
        setState(() => busy = false);
      }
    }
  }

  void scheduleMatch() {
    final revision = ++matchRevision;
    ruleTimer?.cancel();
    final q = query;
    final parts = q.split('/');
    final addr = InternetAddress.tryParse(parts.first);
    final prefix = parts.length == 2 ? int.tryParse(parts.last) : null;
    final isIp = addr != null && (parts.length == 1 ||
        (parts.length == 2 && prefix != null && prefix >= 0 && prefix <= addr.rawAddress.length * 8));
    final isDomain = RegExp(r'^(?:[a-z0-9_-]+\.)+[a-z0-9-]{2,}\.?$', caseSensitive: false).hasMatch(q);
    final lookup = !widget.proxies && readable && q.isNotEmpty && (providers || isIp || isDomain) && providerItems.isNotEmpty;
    setState(() {
      matched = {};
      matchError = '';
      matching = lookup;
    });
    if (!lookup) return;
    final tags = providerItems.map((p) => p['name']).whereType<String>().toList();
    ruleTimer = Timer(const Duration(milliseconds: 400), () async {
      try {
        final r = await configRequest(await configDirectory(), 'rule_matches', {'tags': tags, 'query': q}) as Item;
        if (!mounted || revision != matchRevision) return;
        setState(() {
          matched = (r['hits'] as List).cast<String>().toSet();
          matchError = (r['errors'] as List).join('\n');
        });
      } catch (e) {
        if (mounted && revision == matchRevision) setState(() => matchError = e.toString());
      } finally {
        if (mounted && revision == matchRevision) setState(() => matching = false);
      }
    });
  }

  void rulesTool(String tag) {
    final provider = providerItems.where((p) => p['name'] == tag).firstOrNull;
    if (!readable || provider == null) return;
    showGlassDialog<void>(context: context, builder: (_) => _RuleDetail(
      provider: provider,
      uses: _ruleProviderUsage()[tag] ?? [],
    ));
  }

  Future<void> providerAction(Item item, String op) async {
    if (updating) return;
    setState(() => updating = true);
    try {
      await clashRequest(widget.endpoint, widget.secret, op, {
        if (op == 'update_providers') ...{
          'proxies': widget.proxies,
          'names': providerItems.map((v) => v['name']).toList(),
        } else
          'name': item['name'],
      });
      if (mounted) await load();
    } catch (e) {
      if (mounted) {
        showGlassToast(context, e.toString());
      }
    } finally {
      if (mounted) setState(() => updating = false);
    }
  }

  Future<void> selectNode(Item group, String nodeName, {bool undoable = true}) async {
    final gName = group['name'].toString();
    final prev = group['now']?.toString() ?? '';
    if (prev == nodeName) return;
    try {
      await clashRequest(widget.endpoint, widget.secret, 'select', {
        'group': gName,
        'name': nodeName,
      });
      if (!mounted) return;
      setState(() {
        group['now'] = nodeName;
        undoTimer?.cancel();
        if (undoable && prev.isNotEmpty) {
          undoSwitch = (gName, prev);
          undoTimer = Timer(const Duration(seconds: 8), () {
            if (mounted) setState(() => undoSwitch = null);
          });
        } else {
          undoSwitch = null;
        }
      });
    } catch (e) {
      if (mounted) {
        showGlassToast(context, '切换失败：$e');
      }
    }
  }

  Future<void> testGroup(Item group) async {
    final gName = group['name'].toString();
    if (testingGroups.contains(gName)) return;
    _holdOrder(group);
    setState(() => testingGroups.add(gName));
    try {
      final value = await clashRequest(widget.endpoint, widget.secret, 'group_delay', {
        'name': gName,
        'url': group['test_url'] ?? '',
        'ipv6': widget.preferences['ipv6TestEnabled'] == true,
      });
      if (!mounted) return;
      setState(() {
        final lats = ((value['latencies'] ?? value) as Map).cast<String, dynamic>();
        for (final e in lats.entries) {
          if (e.value is num) delays[e.key] = (e.value as num).toInt();
        }
        final v6Map = value['ipv6'] as Map?;
        if (v6Map != null) {
          for (final e in v6Map.entries) {
            if ((e.value as num? ?? 0) > 0) {
              v6Nodes.add(e.key.toString());
            } else {
              v6Nodes.remove(e.key.toString());
            }
          }
        }
      });
    } catch (e) {
      if (mounted) {
        showGlassToast(context, '测速失败：$e');
      }
    } finally {
      if (mounted) setState(() { testingGroups.remove(gName); _releaseOrder(gName); });
    }
  }

  Future<void> testNode(String testUrl, String nodeName) async {
    if (testingNodes.contains(nodeName)) return;
    setState(() => testingNodes.add(nodeName));
    try {
      final value = await clashRequest(widget.endpoint, widget.secret, 'delay', {
        'name': nodeName,
        'url': testUrl,
        'ipv6': widget.preferences['ipv6TestEnabled'] == true,
      });
      if (!mounted) return;
      setState(() {
        if (value['delay'] case final num d) delays[nodeName] = d.toInt();
        if (value['ipv6'] == true) {
          v6Nodes.add(nodeName);
        } else {
          v6Nodes.remove(nodeName);
        }
      });
    } catch (e) {
      if (mounted) {
        showGlassToast(context, '测速失败：$e');
      }
    } finally {
      if (mounted) setState(() => testingNodes.remove(nodeName));
    }
  }

  int? _delayOf(Item node) {
    final name = node['name']?.toString() ?? '';
    if (delays.containsKey(name)) return delays[name];
    final d = node['delay'];
    if (d is num) return d.toInt();
    final hist = node['history'] as List?;
    if (hist != null && hist.isNotEmpty) {
      final last = hist.last;
      if (last is Map && last['delay'] is num) return (last['delay'] as num).toInt();
    }
    return null;
  }

  String _formatLatency(int? delay) {
    if (delay == null) return '未测';
    if (delay == 0) return '失败';
    return '${delay}ms';
  }

  String _formatSpeedCompact(int bytes) {
    if (bytes <= 0) return '0 B/s';
    final v = bytes.toDouble();
    if (v < 1024) return '$bytes B/s';
    if (v < 1048576) return '${(v / 1024).toStringAsFixed(1)} KB/s';
    if (v < 1073741824) return '${(v / 1048576).toStringAsFixed(1)} MB/s';
    return '${(v / 1073741824).toStringAsFixed(1)} GB/s';
  }

  String _formatType(Item node, bool isV6) {
    final base = (node['kind'] ?? node['type'] ?? '')
        .toString()
        .toLowerCase()
        .replaceAll('shadowsocks', 'ss')
        .replaceAll('hysteria', 'hy')
        .replaceAll('wireguard', 'wg');
    final udp = node['udp'] == true
        ? (node['xudp'] == true ? 'xudp' : 'udp')
        : '';
    return [
      if (base.isNotEmpty) base,
      if (udp.isNotEmpty) udp,
      if (isV6) 'IPv6',
    ].join(' / ');
  }

  Color? _dotFill(int? delay, SingboardColors c) {
    if (delay == null) return null;
    if (delay == 0) return c.error;
    if (delay < 300) return c.success;
    if (delay < 800) return c.warning;
    return c.error;
  }

  (Color bg, Color fg) _latencyBadge(int? delay, SingboardColors c) {
    if (delay == null) return (c.fill, c.muted);
    if (delay == 0) return (c.dangerSoft, c.dangerInk);
    if (delay < 300) return (c.successSoft, c.successInk);
    if (delay < 800) return (c.warningSoft, c.warningInk);
    return (c.dangerSoft, c.dangerInk);
  }

  List<_CondToken> _itemTokens(String key, String raw) {
    final list = raw.startsWith('[') && raw.endsWith(']')
        ? raw.substring(1, raw.length - 1).split(RegExp(r'\s+')).where((s) => s.isNotEmpty).toList()
        : [raw];
    if (key == 'rule_set') {
      return [
        for (final full in list)
          _CondToken('set', full.split('/').last, full),
      ];
    }
    if (key == 'ip_is_private') {
      return [_CondToken('text', raw == 'true' ? '私有 IP' : '非私有 IP')];
    }
    if (key == 'ip_version') {
      return [_CondToken('text', 'IPv$raw')];
    }
    final label = _fieldLabels[key];
    if (label == null) return [_CondToken('mono', '$key=$raw')];
    final out = <_CondToken>[_CondToken('word', label)];
    for (var i = 0; i < list.length; i++) {
      if (i > 0) out.add(const _CondToken('word', '、'));
      final text = _upperFields.contains(key) ? list[i].toUpperCase() : list[i];
      out.add(_CondToken(_monoFields.contains(key) ? 'mono' : 'text', text));
    }
    return out;
  }

  List<_CondToken> _termTokens(String term) {
    var body = term.trim();
    final out = <_CondToken>[];
    if (body.startsWith('!(') && body.endsWith(')')) {
      out.add(const _CondToken('word', '不满足'));
      body = body.substring(2, body.length - 1);
    } else if (body.startsWith('(') && body.endsWith(')')) {
      body = body.substring(1, body.length - 1);
    }
    final matches = RegExp(r'([a-z_]+)=(\[[^\]]*\]|\S+)').allMatches(body).toList();
    if (matches.isEmpty) {
      out.add(_CondToken('mono', body));
      return out;
    }
    for (var i = 0; i < matches.length; i++) {
      if (i > 0) out.add(const _CondToken('word', '，'));
      out.addAll(_itemTokens(matches[i].group(1)!, matches[i].group(2)!));
    }
    return out;
  }

  List<_CondToken> _tokenizeRule(Item rule) {
    final payload = (rule['payload']?.toString() ?? '').trim();
    if (payload.isEmpty) return const [_CondToken('word', '所有流量')];
    final out = <_CondToken>[];
    final parts = payload.split(RegExp(r'\s(&&|\|\|)\s'));
    final ops = RegExp(r'\s(&&|\|\|)\s').allMatches(payload).map((m) => m.group(1)!).toList();
    for (var i = 0; i < parts.length; i++) {
      out.addAll(_termTokens(parts[i]));
      if (i < ops.length) {
        out.add(_CondToken('word', ops[i] == '&&' ? '且' : '或'));
      }
    }
    return out;
  }

  _RuleAction _parseAction(Item rule) {
    final proxy = rule['proxy']?.toString() ?? '';
    if (proxy == 'reject') {
      return const _RuleAction('reject', '拒绝', icon: 'reject');
    }
    if (proxy == 'reject(drop)') {
      return const _RuleAction('reject', '丢弃', icon: 'reject');
    }
    if (proxy.startsWith('reject')) {
      return const _RuleAction('reject', '拒绝', icon: 'reject');
    }
    if (proxy == 'hijack-dns') {
      return const _RuleAction('stop', '劫持 DNS', icon: 'hijack');
    }
    if (proxy.startsWith('sniff')) {
      return const _RuleAction('pass', '嗅探', icon: 'sniff');
    }
    if (proxy == 'resolve(match_only)') {
      return const _RuleAction('pass', '解析 · 仅匹配', icon: 'resolve');
    }
    if (proxy.startsWith('resolve')) {
      return const _RuleAction('pass', '解析', icon: 'resolve');
    }
    if (proxy.startsWith('route-options')) {
      return const _RuleAction('pass', '路由选项', icon: 'tune');
    }
    var outbound = proxy;
    final m = RegExp(r'^route\((.*)\)$').firstMatch(proxy);
    if (m != null) {
      outbound = m.group(1)!.split(',').first.trim();
    }
    final chain = (rule['chain'] as List? ?? []).whereType<String>().toList();
    final finalNode = chain.length > 1 ? chain.last : null;
    return _RuleAction('route', outbound, finalNode: finalNode);
  }

  Map<String, List<(int, String)>> _ruleProviderUsage() {
    final usage = <String, List<(int, String)>>{};
    final pat = RegExp(r'rule_set=(\[[^\]]*\]|[^\s\]]+)');
    for (var i = 0; i < items.length; i++) {
      final payload = items[i]['payload']?.toString() ?? '';
      final tags = <String>{};
      for (final m in pat.allMatches(payload)) {
        final raw = m.group(1)!;
        final body = raw.startsWith('[') ? raw.substring(1, raw.length - 1) : raw;
        tags.addAll(body.split(RegExp(r'\s+')).where((s) => s.isNotEmpty));
      }
      if (tags.isEmpty) continue;
      final act = _parseAction(items[i]);
      for (final tag in tags) {
        (usage[tag] ??= []).add((i + 1, act.label));
      }
    }
    return usage;
  }

  String _formatCount(num n) {
    final s = n.toInt().toString();
    final buf = StringBuffer();
    for (var i = 0; i < s.length; i++) {
      if (i > 0 && (s.length - i) % 3 == 0) buf.write(',');
      buf.write(s[i]);
    }
    return buf.toString();
  }

  Widget _buildDot(int? delay, bool selected, SingboardColors c, {VoidCallback? onTap}) {
    final fill = _dotFill(delay, c);
    final failed = delay == 0;
    return GestureDetector(
      onTap: onTap,
      child: Container(
        width: 14,
        height: 14,
        decoration: BoxDecoration(
          shape: BoxShape.circle,
          color: fill ?? Colors.transparent,
          border: fill == null
              ? Border.all(color: c.muted.withValues(alpha: 0.55), width: 1.2)
              : (selected && failed
                  ? Border.all(color: c.ink.withValues(alpha: 0.75), width: 1.8)
                  : null),
        ),
        alignment: Alignment.center,
        child: failed
            ? const PanelIcon('dot-failed', size: 8, color: Colors.white)
            : (selected
                ? Container(
                    width: 6,
                    height: 6,
                    decoration: BoxDecoration(
                      shape: BoxShape.circle,
                      color: fill != null ? Colors.white : c.muted,
                    ),
                  )
                : null),
      ),
    );
  }

  Widget _buildProxyGroupCard(Item group, SingboardColors c) {
    final gName = group['name']?.toString() ?? '';
    final gKind = group['kind']?.toString() ?? '';
    final now = group['now']?.toString() ?? '';
    final selectable = gKind.toLowerCase() == 'selector';
    final isOpen = query.isEmpty ? expanded.contains(gName) : !collapsedSearch.contains(gName);
    final isTesting = testingGroups.contains(gName);
    final rawNodes = (group['nodes'] as List? ?? []).cast<Item>();
    final wholeMatch = query.isEmpty || gName.toLowerCase().contains(query);
    var nodes = rawNodes
        .where((n) => wholeMatch || n['name'].toString().toLowerCase().contains(query))
        .toList();
    if (sortByLatency) {
      final order = _frozenOrder[gName];
      if (order == null) {
        nodes = _orderedNodes(nodes);
      } else {
        final current = {for (final n in nodes) n['name'] as String: n};
        nodes = [for (final name in order.keys) if (current.containsKey(name)) current[name]!,
          for (final n in nodes) if (!order.containsKey(n['name'])) n];
      }
    }
    final nowNode = rawNodes.where((n) => n['name'] == now).firstOrNull;
    final nowDelay = nowNode == null ? null : _delayOf(nowNode);
    final showNowBadge = now.isNotEmpty &&
        ((nowDelay != null && nowDelay > 0) ||
            (nowNode?['kind']?.toString().toLowerCase() != 'direct'));
    final (nowBg, nowFg) = _latencyBadge(nowDelay, c);
    final ipv6Enabled = widget.preferences['ipv6TestEnabled'] == true;
    final spd = widget.chainSpeeds[gName];
    final dlSpd = spd is List && spd.isNotEmpty ? (spd[0] as num).toInt() : 0;
    final ulSpd = spd is List && spd.length > 1 ? (spd[1] as num).toInt() : 0;
    final hasSpeed = dlSpd > 0 || ulSpd > 0;
    final countText = query.isNotEmpty
        ? '$gKind (${nodes.length}/${rawNodes.length})'
        : '$gKind (${rawNodes.length})';

    return GlassCard(
      radius: 18,
      padding: EdgeInsets.zero,
      child: AnimatedSize(alignment: Alignment.topCenter,
        duration: Duration(milliseconds: MediaQuery.disableAnimationsOf(context) ? 0 : 260),
        curve: const Cubic(.2,.8,.2,1), child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          InkWell(
            borderRadius: BorderRadius.circular(18),
            onTap: () => toggleGroup(gName),
            child: Padding(
              padding: const EdgeInsets.fromLTRB(16, 14, 14, 14),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          children: [
                            Flexible(
                              child: Text(
                                gName,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: TextStyle(
                                  fontSize: 16,
                                  fontWeight: FontWeight.w600,
                                  color: c.ink,
                                ),
                              ),
                            ),
                            const SizedBox(width: 6),
                            Text(
                              '$countText${selectable ? '' : ' · 自动'}',
                              style: TextStyle(fontSize: 12, color: c.muted),
                            ),
                            const SizedBox(width: 4),
                            AnimatedRotation(turns: isOpen ? .25 : 0, duration: const Duration(milliseconds: 200), child: PanelIcon('chevron', size: 14, color: c.muted)),
                          ],
                        ),
                        const SizedBox(height: 6),
                        Row(
                          children: [
                            PanelIcon('arrow-right', size: 14, color: c.muted),
                            const SizedBox(width: 4),
                            Flexible(
                              child: Text(
                                now,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: TextStyle(fontSize: 14, color: c.muted),
                              ),
                            ),
                            if (showNowBadge) ...[
                              const SizedBox(width: 6),
                              Container(
                                padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 2),
                                decoration: BoxDecoration(
                                  color: nowBg,
                                  borderRadius: BorderRadius.circular(999),
                                ),
                                child: Text(
                                  _formatLatency(nowDelay),
                                  style: TextStyle(
                                    fontSize: 12,
                                    fontWeight: FontWeight.w500,
                                    color: nowFg,
                                    fontFeatures: const [FontFeature.tabularFigures()],
                                  ),
                                ),
                              ),
                            ],
                            if (undoSwitch != null && undoSwitch!.$1 == gName) ...[
                              const SizedBox(width: 6),
                              GestureDetector(
                                onTap: () => selectNode(
                                  group,
                                  undoSwitch!.$2,
                                  undoable: false,
                                ),
                                child: Text(
                                  '撤销',
                                  style: TextStyle(
                                    fontSize: 12,
                                    fontWeight: FontWeight.w600,
                                    color: c.selectionInk,
                                  ),
                                ),
                              ),
                            ],
                          ],
                        ),
                        if (!isOpen && nodes.isNotEmpty) ...[
                          const SizedBox(height: 10),
                          Wrap(
                            spacing: 6,
                            runSpacing: 6,
                            children: [
                              for (final node in nodes)
                                _buildDot(
                                  _delayOf(node),
                                  node['name'] == now,
                                  c,
                                  onTap: selectable
                                      ? () => selectNode(group, node['name'].toString())
                                      : null,
                                ),
                            ],
                          ),
                        ],
                      ],
                    ),
                  ),
                  const SizedBox(width: 8),
                  Column(
                    crossAxisAlignment: CrossAxisAlignment.end,
                    children: [
                      GestureDetector(
                        onTap: isTesting ? null : () => testGroup(group),
                        child: Padding(
                          padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 3),
                          child: Text(
                            isTesting ? '测速中' : '延迟测试',
                            style: TextStyle(
                              fontSize: 12,
                              fontWeight: FontWeight.w500,
                              color: isTesting ? c.muted : c.ink,
                            ),
                          ),
                        ),
                      ),
                      const SizedBox(height: 2),
                      Visibility(
                        visible: hasSpeed,
                        maintainSize: true,
                        maintainAnimation: true,
                        maintainState: true,
                        child: Padding(
                          padding: const EdgeInsets.only(right: 4),
                          child: DefaultTextStyle(
                            style: DefaultTextStyle.of(context).style.copyWith(
                              fontSize: 12,
                              height: 16 / 12,
                              color: c.muted,
                              fontFeatures: const [FontFeature.tabularFigures()],
                            ),
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.end,
                              children: [
                                Text('↓${_formatSpeedCompact(dlSpd)}'),
                                const SizedBox(height: 2),
                                Text('↑${_formatSpeedCompact(ulSpd)}'),
                              ],
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
          if (isOpen)
            Padding(
              padding: const EdgeInsets.fromLTRB(14, 0, 14, 14),
              child: LayoutBuilder(
                builder: (_, constraints) {
                  final cols = ((constraints.maxWidth + 8) / 136).floor().clamp(1, 1000);
                  final tileW = (constraints.maxWidth - 8 * (cols - 1)) / cols;
                  return Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      for (final node in nodes)
                        Builder(
                          builder: (_) {
                            final nName = node['name'].toString();
                            final selected = nName == now;
                            final delay = _delayOf(node);
                            final testing = testingNodes.contains(nName);
                            final showLat = (delay != null && delay > 0) ||
                                (node['kind']?.toString().toLowerCase() != 'direct');
                            final (latBg, latFg) = _latencyBadge(delay, c);
                            final isV6 = ipv6Enabled && v6Nodes.contains(nName);
                            return GestureDetector(
                              key: selected ? selectedTiles.putIfAbsent(gName, GlobalKey.new) : null,
                              onTap: selectable && !selected
                                  ? () => selectNode(group, nName)
                                  : null,
                              child: Container(
                                width: tileW,
                                height: 76,
                                padding: const EdgeInsets.all(10),
                                decoration: BoxDecoration(
                                  color: selected
                                      ? c.primarySoft
                                      : (c.dark
                                          ? const Color(0x14FFFFFF)
                                          : const Color(0x38FFFFFF)),
                                  borderRadius: BorderRadius.circular(12),
                                  border: Border.all(
                                    color: selected
                                        ? c.selectionBorder
                                        : c.hairline,
                                    width: selected ? 1 : 0.5,
                                  ),
                                ),
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Expanded(
                                      child: Text(
                                        nName,
                                        maxLines: 2,
                                        overflow: TextOverflow.ellipsis,
                                        style: TextStyle(
                                          fontSize: 14,
                                          height: 1.2,
                                          fontWeight: selected
                                              ? FontWeight.w600
                                              : FontWeight.w400,
                                          color: selected ? c.selectionInk : c.ink,
                                        ),
                                      ),
                                    ),
                                    Row(
                                      children: [
                                        Expanded(
                                          child: Text(
                                            _formatType(node, isV6),
                                            maxLines: 1,
                                            overflow: TextOverflow.ellipsis,
                                            style: TextStyle(
                                              fontSize: 12,
                                              color: selected
                                                  ? c.selectionInk
                                                  : c.muted,
                                            ),
                                          ),
                                        ),
                                        if (showLat)
                                          GestureDetector(
                                            onTap: testing
                                                ? null
                                                : () => testNode(
                                                      group['test_url']?.toString() ?? '',
                                                      nName,
                                                    ),
                                            child: Container(
                                              padding: const EdgeInsets.symmetric(
                                                horizontal: 7,
                                                vertical: 1.5,
                                              ),
                                              decoration: BoxDecoration(
                                                color: latBg,
                                                borderRadius: BorderRadius.circular(999),
                                              ),
                                              child: Text(
                                                testing ? '…' : _formatLatency(delay),
                                                style: TextStyle(
                                                  fontSize: 12,
                                                  fontWeight: FontWeight.w500,
                                                  color: latFg,
                                                  fontFeatures: const [
                                                    FontFeature.tabularFigures(),
                                                  ],
                                                ),
                                              ),
                                            ),
                                          ),
                                      ],
                                    ),
                                  ],
                                ),
                              ),
                            );
                          },
                        ),
                    ],
                  );
                },
              ),
            ),
        ],
      )),
    );
  }

  Widget _buildProxyProviderCard(Item item, SingboardColors c) {
    final name = item['name']?.toString() ?? '';
    final nodes = (item['proxies'] as List? ?? []).cast<Item>();
    final tested = nodes.where((n) => _delayOf(n) != null).length;
    final open = expandedProviders.contains(name);
    final vehicle = item['vehicleType']?.toString() ?? '';
    final updated = (item['updatedAt']?.toString() ?? '').split('T').first;
    final sub = item['subscriptionInfo'] as Map?;
    final total = (sub?['Total'] as num?)?.toInt() ?? 0;
    final used = ((sub?['Download'] as num?)?.toInt() ?? 0) +
        ((sub?['Upload'] as num?)?.toInt() ?? 0);
    final testUrl = widget.preferences['latencyTestUrl']?.toString() ?? '';

    return GlassCard(
      radius: 18,
      padding: EdgeInsets.zero,
      child: AnimatedSize(alignment: Alignment.topCenter,
        duration: Duration(milliseconds: MediaQuery.disableAnimationsOf(context) ? 0 : 260),
        curve: const Cubic(.2,.8,.2,1), child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          InkWell(
            borderRadius: BorderRadius.circular(18),
            onTap: () => setState(() {
              if (!expandedProviders.remove(name)) expandedProviders.add(name);
            }),
            child: Padding(
              padding: const EdgeInsets.fromLTRB(16, 14, 12, 14),
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          children: [
                            Flexible(
                              child: Text(
                                name,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: TextStyle(
                                  fontSize: 16,
                                  fontWeight: FontWeight.w600,
                                  color: c.ink,
                                ),
                              ),
                            ),
                            const SizedBox(width: 6),
                            Text(
                              '${nodes.length} 个节点 ($tested 已测试)',
                              style: TextStyle(fontSize: 12, color: c.muted),
                            ),
                            const SizedBox(width: 4),
                            AnimatedRotation(turns: open ? .25 : 0, duration: const Duration(milliseconds: 200), child: PanelIcon('chevron', size: 14, color: c.muted)),
                          ],
                        ),
                        if (total > 0) ...[
                          const SizedBox(height: 4),
                          Text(
                            '${formatBytes(used)} / ${formatBytes(total)} (${((used / total) * 100).toStringAsFixed(2)}%)',
                            style: TextStyle(
                              fontSize: 12,
                              color: c.muted,
                              fontFeatures: const [FontFeature.tabularFigures()],
                            ),
                          ),
                        ],
                        if (updated.isNotEmpty) ...[
                          const SizedBox(height: 2),
                          Text(
                            '更新于 $updated',
                            style: TextStyle(fontSize: 12, color: c.muted),
                          ),
                        ],
                        if (!open && nodes.isNotEmpty) ...[
                          const SizedBox(height: 10),
                          Wrap(
                            spacing: 6,
                            runSpacing: 6,
                            children: [
                              for (final node in nodes)
                                _buildDot(_delayOf(node), false, c),
                            ],
                          ),
                        ],
                      ],
                    ),
                  ),
                  Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      IconButton(
                        tooltip: '健康检查',
                        icon: PanelIcon('bolt', size: 18, color: c.muted),
                        onPressed: updating ? null : () => providerAction(item, 'health_check'),
                      ),
                      if (vehicle.toLowerCase() != 'inline')
                        IconButton(
                          tooltip: '更新',
                          icon: PanelIcon('refresh', size: 18, color: c.primary),
                          onPressed: updating
                              ? null
                              : () => providerAction(item, 'update_proxy_provider'),
                        ),
                    ],
                  ),
                ],
              ),
            ),
          ),
          if (open)
            Padding(
              padding: const EdgeInsets.fromLTRB(14, 0, 14, 14),
              child: LayoutBuilder(
                builder: (_, constraints) {
                  final cols = ((constraints.maxWidth + 8) / 136).floor().clamp(1, 1000);
                  final tileW = (constraints.maxWidth - 8 * (cols - 1)) / cols;
                  return Wrap(
                    spacing: 8,
                    runSpacing: 8,
                    children: [
                      for (final node in nodes)
                        Builder(
                          builder: (_) {
                            final nName = node['name'].toString();
                            final delay = _delayOf(node);
                            final testing = testingNodes.contains(nName);
                            final (latBg, latFg) = _latencyBadge(delay, c);
                            return Container(
                              width: tileW,
                              height: 76,
                              padding: const EdgeInsets.all(10),
                              decoration: BoxDecoration(
                                color: c.dark
                                    ? const Color(0x14FFFFFF)
                                    : const Color(0x38FFFFFF),
                                borderRadius: BorderRadius.circular(12),
                                border: Border.all(color: c.hairline, width: 0.5),
                              ),
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  Expanded(
                                    child: Text(
                                      nName,
                                      maxLines: 2,
                                      overflow: TextOverflow.ellipsis,
                                      style: TextStyle(fontSize: 14, height: 1.2, color: c.ink),
                                    ),
                                  ),
                                  Row(
                                    children: [
                                      Expanded(
                                        child: Text(
                                          _formatType(node, false),
                                          maxLines: 1,
                                          overflow: TextOverflow.ellipsis,
                                          style: TextStyle(fontSize: 12, color: c.muted),
                                        ),
                                      ),
                                      GestureDetector(
                                        onTap: testing ? null : () => testNode(testUrl, nName),
                                        child: Container(
                                          padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 1.5),
                                          decoration: BoxDecoration(
                                            color: latBg,
                                            borderRadius: BorderRadius.circular(999),
                                          ),
                                          child: Text(
                                            testing ? '…' : _formatLatency(delay),
                                            style: TextStyle(
                                              fontSize: 12,
                                              color: latFg,
                                              fontFeatures: const [FontFeature.tabularFigures()],
                                            ),
                                          ),
                                        ),
                                      ),
                                    ],
                                  ),
                                ],
                              ),
                            );
                          },
                        ),
                    ],
                  );
                },
              ),
            ),
        ],
      )),
    );
  }

  Widget _buildRuleCard(Item rule, int index, SingboardColors c) {
    final tokens = _tokenizeRule(rule);
    final action = _parseAction(rule);
    final rejectFinal = action.finalNode?.toLowerCase().contains('reject') == true;

    Widget destWidget;
    if (action.kind == 'route') {
      destWidget = Wrap(
        spacing: 5,
        crossAxisAlignment: WrapCrossAlignment.center,
        children: [
          Text(
            action.label,
            style: TextStyle(
              fontSize: 13,
              fontWeight: FontWeight.w600,
              color: c.ink,
            ),
          ),
          if (action.finalNode != null)
            Text(
              '› ${action.finalNode}',
              style: TextStyle(
                fontSize: 13,
                color: rejectFinal ? c.dangerInk : c.muted,
              ),
            ),
        ],
      );
    } else {
      final bg = switch (action.kind) {
        'reject' => c.dangerSoft,
        'stop' => c.fill,
        _ => c.fill.withValues(alpha: 0.45),
      };
      final fg = action.kind == 'reject' ? c.dangerInk : c.muted;
      destWidget = Container(
        height: 24,
        padding: const EdgeInsets.symmetric(horizontal: 10),
        decoration: BoxDecoration(
          color: bg,
          borderRadius: BorderRadius.circular(999),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (action.icon != null) ...[
              PanelIcon(action.icon!, size: 13, color: fg),
              const SizedBox(width: 4),
            ],
            Text(
              action.label,
              style: TextStyle(
                fontSize: 12,
                fontWeight: FontWeight.w500,
                color: fg,
              ),
            ),
          ],
        ),
      );
    }

    return GlassCard(
      radius: 16,
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: MediaQuery.textScalerOf(context).scale(6.5) * items.length.toString().length,
            child: Text(
              '${index + 1}',
              textAlign: TextAlign.right,
              style: TextStyle(
                fontSize: 11,
                height: 24 / 11,
                color: c.muted,
                fontFeatures: const [FontFeature.tabularFigures()],
              ),
            ),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                ConstrainedBox(constraints: const BoxConstraints(minHeight: 24), child: Align(
                  alignment: Alignment.centerLeft,
                  child: Wrap(
                  spacing: 5,
                  runSpacing: 5,
                  crossAxisAlignment: WrapCrossAlignment.center,
                  children: [
                    for (final tok in tokens)
                      switch (tok.kind) {
                        'set' => GestureDetector(
                          onTap: readable && providerItems.any((p) => p['name'] == tok.full) ? () => rulesTool(tok.full) : null,
                          child: Container(
                            height: 22,
                            padding: const EdgeInsets.symmetric(horizontal: 9),
                            decoration: BoxDecoration(
                              color: matched.contains(tok.full) ? c.primarySoft : c.fill,
                              borderRadius: BorderRadius.circular(999),
                            ),
                            child: Row(
                              mainAxisSize: MainAxisSize.min,
                              children: [
                                PanelIcon('layers', size: 13, color: c.muted),
                                const SizedBox(width: 4),
                                Text(
                                  tok.text,
                                  style: TextStyle(
                                    fontSize: 12,
                                    fontWeight: FontWeight.w500,
                                    color: c.ink,
                                  ),
                                ),
                              ],
                            ),
                          ),
                        ),
                        'word' => Text(
                          tok.text,
                          style: TextStyle(fontSize: 13, color: c.muted),
                        ),
                        'mono' => Text(
                          tok.text,
                          style: TextStyle(
                            fontSize: 12,
                            fontFamily: 'monospace',
                            color: c.ink,
                          ),
                        ),
                        _ => Text(
                          tok.text,
                          style: TextStyle(fontSize: 13, color: c.ink),
                        ),
                      },
                  ],
                ))),
                const SizedBox(height: 6),
                destWidget,
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildRuleProviderCard(
    Item item,
    Map<String, List<(int, String)>> usageMap,
    SingboardColors c,
  ) {
    final name = item['name']?.toString() ?? '';
    final cut = name.lastIndexOf('/');
    final prefix = cut >= 0 ? name.substring(0, cut + 1) : '';
    final base = cut >= 0 ? name.substring(cut + 1) : name;
    final vehicle = (item['vehicleType']?.toString() ?? '').toLowerCase();
    final behavior = (item['behavior']?.toString() ?? '').toLowerCase();
    final isInline = vehicle == 'inline';
    final vLabel = switch (vehicle) {
      'remote' || 'http' => '远程',
      'local' || 'file' => '本地',
      'inline' => '内联',
      _ => vehicle,
    };
    final fLabel = switch (behavior) {
      'binary' => '二进制',
      'source' => '源文件',
      _ => behavior,
    };
    final updated = (item['updatedAt']?.toString() ?? '').split('T').first;
    final meta = [
      if (vLabel.isNotEmpty) vLabel,
      if (fLabel.isNotEmpty) fLabel,
      if (updated.isNotEmpty) '$updated 更新',
    ].join(' · ');
    final uses = usageMap[name];
    final usageText = uses == null || uses.isEmpty
        ? '未被路由规则引用'
        : uses.length == 1
            ? '第 ${uses.first.$1} 条 → ${uses.first.$2}'
            : '第 ${uses.map((u) => u.$1).join('、')} 条';
    final count = (item['ruleCount'] as num?) ?? 0;
    final tileIcon = switch (vehicle) {
      'remote' || 'http' => 'remote',
      'inline' => 'inline',
      _ => 'file',
    };

    return GestureDetector(
      onTap: readable ? () => rulesTool(name) : null,
      child: GlassCard(
        radius: 16,
        padding: const EdgeInsets.fromLTRB(14, 12, 10, 12),
        child: Row(
          children: [
            Container(
              width: 30,
              height: 30,
              decoration: BoxDecoration(
                color: c.fill,
                borderRadius: BorderRadius.circular(9),
              ),
              child: PanelIcon(tileIcon, size: 16, color: c.muted),
            ),
            const SizedBox(width: 12),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text.rich(
                    TextSpan(
                      children: [
                        if (prefix.isNotEmpty)
                          TextSpan(
                            text: prefix,
                            style: TextStyle(
                              fontWeight: FontWeight.w400,
                              color: c.muted.withValues(alpha: 0.75),
                            ),
                          ),
                        TextSpan(
                          text: base,
                          style: TextStyle(
                            fontWeight: FontWeight.w600,
                            color: c.ink,
                          ),
                        ),
                      ],
                    ),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontSize: 14),
                  ),
                  if (meta.isNotEmpty)
                    Text(
                      meta,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(fontSize: 12, color: c.muted),
                    ),
                  Text(
                    usageText,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 12,
                      color: uses == null
                          ? c.muted.withValues(alpha: 0.7)
                          : c.muted,
                    ),
                  ),
                ],
              ),
            ),
            const SizedBox(width: 8),
            Text.rich(
              TextSpan(
                children: [
                  TextSpan(
                    text: _formatCount(count),
                    style: TextStyle(
                      fontSize: 13,
                      fontWeight: FontWeight.w600,
                      color: c.ink,
                      fontFeatures: const [FontFeature.tabularFigures()],
                    ),
                  ),
                  TextSpan(
                    text: ' 条',
                    style: TextStyle(fontSize: 12, color: c.muted),
                  ),
                ],
              ),
            ),
            if (!isInline) ...[
              const SizedBox(width: 4),
              GestureDetector(
                onTap: updating
                    ? null
                    : () => providerAction(item, 'update_rule_provider'),
                child: Container(
                  width: 30,
                  height: 30,
                  decoration: BoxDecoration(shape: BoxShape.circle, color: c.fill),
                  child: PanelIcon('refresh', size: 16, color: c.ink),
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final source = providers ? providerItems : items;
    final shown = source.where((item) {
      if (query.isEmpty) return true;
      if (!widget.proxies && providers) return matched.contains(item['name']);
      if (!widget.proxies && _tokenizeRule(item).any((t) => t.kind == 'set' && matched.contains(t.full))) return true;
      final values = <dynamic>[
        item['name'],
        item['type'],
        item['condition'],
        item['payload'],
        item['route'],
        item['proxy'],
        item['now'],
      ];
      if (widget.proxies && !providers) {
        values.addAll((item['nodes'] as List? ?? []).map((n) => n['name']));
      }
      return values.whereType<String>().any(
        (v) => v.toLowerCase().contains(query),
      );
    }).toList();
    final usageMap = !widget.proxies && providers ? _ruleProviderUsage() : const <String, List<(int, String)>>{};

    final scope = GlassScope.of(context);
    final header = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Text(
              widget.proxies ? '代理' : '规则',
              style: TextStyle(
                fontSize: 26,
                fontWeight: FontWeight.w700,
                letterSpacing: -0.4,
                color: scope.backdropInk(c),
                shadows: scope.backdropShadows(),
              ),
            ),
            const SizedBox(width: 10),
            Flexible(child: SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              child: GlassSegmented<bool>(
                items: [
                  (
                    false,
                    widget.proxies
                        ? '代理组 (${items.length})'
                        : '规则 (${items.length})',
                  ),
                  if (providersAvailable) (
                    true,
                    widget.proxies
                        ? '代理提供商 (${providerItems.length})'
                        : '规则提供商 (${providerItems.length})',
                  ),
                ],
                value: providers,
                onChanged: (v) {
                  setState(() { providers = v; query = ''; _searchCtrl.clear(); });
                  if (!widget.proxies) scheduleMatch();
                },
              ),
            )),
            if (providers)
              GestureDetector(
                onTap: updating || providerItems.isEmpty ? null : () => providerAction({}, 'update_providers'),
                child: GlassControl(
                  radius: 999,
                  child: SizedBox(
                    width: 32,
                    height: 32,
                    child: PanelIcon('refresh', size: 16, color: providerItems.isEmpty ? c.muted : c.ink),
                  ),
                ),
              ),
          ],
        ),
        const SizedBox(height: 10),
        Row(
          children: [
            Expanded(
              child: GlassSearchField(
                controller: _searchCtrl,
                hint: widget.proxies
                    ? '搜索节点或代理组'
                    : providers
                        ? (readable ? '输入域名或 IP，查找命中的规则提供商' : '开启 root 模式后可按域名或 IP 查找')
                        : (readable ? '搜索规则、出站、域名或 IP' : '搜索规则或出站'),
                value: query,
                onChanged: (v) {
                  setState(() => query = v.trim().toLowerCase());
                  if (!widget.proxies) scheduleMatch();
                },
              ),
            ),
            if (widget.proxies && !providers) ...[
              const SizedBox(width: 8),
              GestureDetector(
                onTap: () {
                  setState(() => sortByLatency = !sortByLatency);
                  widget.preferences['proxySortLatency'] = sortByLatency;
                  _savePreference('set_proxy_sort', {'enabled': sortByLatency});
                },
                child: GlassControl(
                  radius: 999,
                  tint: sortByLatency ? c.primarySoft : c.fill,
                  child: SizedBox(
                    width: 32,
                    height: 32,
                    child: PanelIcon('sort', size: 18, color: sortByLatency ? c.selectionInk : c.ink),
                  ),
                ),
              ),
            ],
          ],
        ),
        if (matchError.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 6),
            child: Text(
              matchError,
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(fontSize: 12, color: c.dangerInk),
            ),
          ),
        if (busy || updating || matching)
          const Padding(
            padding: EdgeInsets.only(top: 8),
            child: LinearProgressIndicator(minHeight: 2),
          ),
        SizedBox(height: widget.proxies ? 12 : 8),
      ],
    );

    final hasPlaceholder = error.isNotEmpty || shown.isEmpty;
    return ListView.builder(
      padding: EdgeInsets.fromLTRB(panelGutter(context), 0, panelGutter(context), 108),
      itemCount: hasPlaceholder ? 2 : shown.length + 1,
      itemBuilder: (_, index) {
        if (index == 0) return header;
        if (error.isNotEmpty) {
          return Padding(
            padding: const EdgeInsets.fromLTRB(12, 56, 12, 24),
            child: GlassCard(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text(
                    widget.proxies ? '无法读取代理列表' : '无法读取规则列表',
                    style: TextStyle(
                      fontSize: 16,
                      fontWeight: FontWeight.w600,
                      color: c.ink,
                    ),
                  ),
                  const SizedBox(height: 6),
                  Text(
                    error,
                    textAlign: TextAlign.center,
                    style: TextStyle(fontSize: 14, color: c.muted),
                  ),
                  const SizedBox(height: 12),
                  FilledButton(onPressed: load, child: const Text('重试')),
                ],
              ),
            ),
          );
        }
        if (shown.isEmpty) {
          return Padding(
            padding: const EdgeInsets.only(top: 72),
            child: Center(
              child: !widget.proxies && providers && !busy && providerItems.isEmpty
                  ? Padding(
                      padding: const EdgeInsets.all(16),
                      child: Column(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Text(
                            '当前配置里没有规则提供商',
                            style: TextStyle(
                              fontSize: 14,
                              color: scope.backdropInk(c),
                              shadows: scope.backdropShadows(),
                            ),
                          ),
                          const SizedBox(height: 4),
                          Text(
                            '在 config.json 的 route.rule_set 中添加规则集后，这里会列出它们。',
                            textAlign: TextAlign.center,
                            style: TextStyle(
                              fontSize: 12,
                              color: scope.backdropMuted(c),
                              shadows: scope.backdropShadows(),
                            ),
                          ),
                        ],
                      ),
                    )
                  : Text(
                      busy
                          ? '正在读取…'
                          : query.isEmpty
                              ? '暂无记录'
                              : '没有找到匹配项',
                      style: TextStyle(
                        fontSize: 14,
                        color: scope.backdropMuted(c),
                        shadows: scope.backdropShadows(),
                      ),
                    ),
            ),
          );
        }
        final i = index - 1;
        final item = shown[i];
        final Widget card;
        if (providers) {
          card = widget.proxies
              ? _buildProxyProviderCard(item, c)
              : _buildRuleProviderCard(item, usageMap, c);
        } else if (widget.proxies) {
          final name = item['name'] as String;
          card = Focus(
            key: ValueKey(name),
            canRequestFocus: false,
            onFocusChange: (focused) {
              if (focused) {
                _focused.add(name);
                _holdOrder(item);
              } else {
                setState(() {
                  _focused.remove(name);
                  _releaseOrder(name);
                });
              }
            },
            child: MouseRegion(
              onEnter: (_) {
                _pointed.add(name);
                _holdOrder(item);
              },
              onExit: (_) => setState(() {
                _pointed.remove(name);
                _releaseOrder(name);
              }),
              child: _buildProxyGroupCard(item, c),
            ),
          );
        } else {
          final tags = _tokenizeRule(item)
              .where((t) => t.kind == 'set')
              .map((t) => t.full)
              .where((tag) => providerItems.any((p) => p['name'] == tag))
              .toSet();
          card = GestureDetector(
            onTap: readable && tags.length == 1 ? () => rulesTool(tags.single) : null,
            child: _buildRuleCard(item, items.indexOf(item), c),
          );
        }
        if (i + 1 >= shown.length) return card;
        return Padding(
          padding: EdgeInsets.only(bottom: widget.proxies ? 12 : 8),
          child: card,
        );
      },
    );
  }
}

class _RuleDetail extends StatefulWidget {
  final Item provider;
  final List<(int, String)> uses;
  const _RuleDetail({required this.provider, required this.uses});

  @override
  State<_RuleDetail> createState() => _RuleDetailState();
}

class _RuleDetailState extends State<_RuleDetail> {
  final search = TextEditingController();
  List<Item> entries = [], shown = [];
  List<(String, int)> typeCounts = [];
  String? activeType;
  String query = '', error = '', match = '';
  Timer? timer;
  bool loading = true;
  String get name => widget.provider['name'] as String;

  static int _typeOrder(String type) => switch (type) {
    'domain_suffix' => 0,
    'domain' => 1,
    'domain_keyword' => 2,
    'domain_regex' => 3,
    'ip_cidr' => 4,
    'source_ip_cidr' => 5,
    _ => 6,
  };

  static (Color, Color) _badgeColors(String type, SingboardColors c) => switch (type) {
    'domain_suffix' => (c.primarySoft, c.selectionInk),
    'domain' => (c.successSoft, c.successInk),
    'domain_keyword' => (c.warningSoft, c.warningInk),
    'domain_regex' => (c.tcpSoft, c.tcp),
    'ip_cidr' || 'source_ip_cidr' => (c.udpSoft, c.udp),
    _ => (c.fill, c.muted),
  };

  static List<Item> _sortEntries(List<Item> rows) {
    final indexed = rows.indexed.toList();
    indexed.sort((a, b) {
      final ta = a.$2['type']?.toString() ?? '';
      final tb = b.$2['type']?.toString() ?? '';
      final ord = _typeOrder(ta).compareTo(_typeOrder(tb));
      if (ord != 0) return ord;
      final typeCmp = ta.compareTo(tb);
      if (typeCmp != 0) return typeCmp;
      if (ta == 'ip_cidr' || ta == 'source_ip_cidr') return a.$1.compareTo(b.$1);
      final va = a.$2['value']?.toString() ?? '';
      final vb = b.$2['value']?.toString() ?? '';
      final lowerCmp = va.toLowerCase().compareTo(vb.toLowerCase());
      return lowerCmp != 0 ? lowerCmp : va.compareTo(vb);
    });
    return indexed.map((e) => e.$2).toList();
  }

  static List<(String, int)> _collectTypeCounts(List<Item> rows) {
    final counts = <(String, int)>[];
    for (final row in rows) {
      final type = row['type']?.toString() ?? '';
      if (counts.isNotEmpty && counts.last.$1 == type) {
        counts[counts.length - 1] = (type, counts.last.$2 + 1);
      } else {
        counts.add((type, 1));
      }
    }
    return counts;
  }

  @override
  void initState() { super.initState(); load(); }

  @override
  void dispose() { timer?.cancel(); search.dispose(); super.dispose(); }

  Future<void> load() async {
    setState(() { loading = true; error = ''; });
    try {
      final rows = await configRequest(await configDirectory(), 'rule_entries', {'tag': name}) as List;
      if (!mounted) return;
      entries = _sortEntries(rows.cast<Item>());
      typeCounts = _collectTypeCounts(entries);
      activeType = null;
      filter(search.text);
    } catch (e) {
      if (mounted) setState(() => error = e.toString());
    } finally {
      if (mounted) setState(() => loading = false);
    }
  }

  List<Item> _computeShown(String q, String? selectedType) {
    return entries.where((r) {
      final rawType = r['type']?.toString() ?? '';
      if (selectedType != null && rawType != selectedType) return false;
      if (q.isEmpty) return true;
      final type = rawType.toLowerCase();
      final value = r['value']?.toString().toLowerCase() ?? '';
      final label = (_CatalogPageState._fieldLabels[rawType] ?? '').toLowerCase();
      return type.contains(q) || value.contains(q) || label.contains(q) ||
          (type == 'domain_suffix' && q.endsWith('.${value.replaceFirst(RegExp(r"^\."), "")}'));
    }).toList();
  }

  void _selectType(String? type) {
    if (activeType == type) return;
    setState(() {
      activeType = type;
      shown = _computeShown(query, activeType);
    });
  }

  void filter(String input) {
    timer?.cancel();
    final q = input.trim().toLowerCase();
    setState(() {
      query = q;
      match = q.isEmpty ? '' : '正在匹配…';
      shown = _computeShown(q, activeType);
    });
    if (q.isEmpty) return;
    timer = Timer(const Duration(milliseconds: 500), () async {
      try {
        final result = await configRequest(await configDirectory(), 'rule_matches', {'tags': [name], 'query': q}) as Item;
        if (mounted && query == q) setState(() => match = (result['errors'] as List).isNotEmpty
            ? (result['errors'] as List).first.toString() : (result['hits'] as List).isEmpty ? '未命中' : '命中');
      } catch (e) {
        if (mounted && query == q) setState(() => match = e.toString());
      }
    });
  }

  Widget _typeChip(String label, int count, bool active, Color activeBg, Color activeFg, SingboardColors c, VoidCallback onTap) =>
      GestureDetector(
        onTap: onTap,
        child: Container(
          height: 26,
          padding: const EdgeInsets.symmetric(horizontal: 10),
          decoration: BoxDecoration(
            color: active ? activeBg : c.fill,
            borderRadius: BorderRadius.circular(999),
          ),
          child: Row(mainAxisSize: MainAxisSize.min, children: [
            Text(label, style: TextStyle(fontSize: 12, fontWeight: active ? FontWeight.w500 : FontWeight.w400, color: active ? activeFg : c.muted)),
            const SizedBox(width: 4),
            Text('$count', style: TextStyle(fontSize: 11, color: (active ? activeFg : c.muted).withValues(alpha: 0.75))),
          ]),
        ),
      );

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final p = widget.provider;
    final vehicle = (p['vehicleType'] ?? '').toString().toLowerCase();
    final icon = switch (vehicle) { 'remote' || 'http' => 'remote', 'inline' => 'inline', _ => 'file' };
    final source = switch (vehicle) { 'remote' || 'http' => '远程', 'inline' => '内联', 'local' || 'file' => '本地', _ => vehicle };
    final date = p['updatedAt']?.toString().split('T').first ?? '';
    return Dialog(backgroundColor: Colors.transparent, insetPadding: const EdgeInsets.symmetric(horizontal: 16),
      child: ConstrainedBox(constraints: BoxConstraints(maxWidth: 672, maxHeight: MediaQuery.sizeOf(context).height * .8),
        child: GlassCard(popover: true, child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Row(children: [
            PanelIcon(icon, size: 20, color: c.muted), const SizedBox(width: 8),
            Expanded(child: Text(name, maxLines: 2, overflow: TextOverflow.ellipsis, style: TextStyle(fontSize: 16, fontWeight: FontWeight.w600, color: c.ink))),
            IconButton(tooltip: '关闭详情', onPressed: () => Navigator.pop(context), icon: PanelIcon('close-detail', size: 14, color: c.muted)),
          ]),
          Text([source, "${p['ruleCount'] ?? 0} 条", if (date.isNotEmpty) '$date 更新'].join(' · '), style: TextStyle(fontSize: 12, color: c.muted)),
          if (widget.uses.isNotEmpty) Text(widget.uses.length == 1 ? '用于第 ${widget.uses.first.$1} 条规则 → ${widget.uses.first.$2}' :
              '用于第 ${widget.uses.map((u) => u.$1).join('、')} 条规则', style: TextStyle(fontSize: 12, color: c.muted)),
          const SizedBox(height: 10),
          GlassSearchField(hint: '筛选条目或检查域名 / IP', value: query, controller: search, onChanged: filter),
          if (match.isNotEmpty) Padding(padding: const EdgeInsets.only(top: 6), child: Text(match, style: TextStyle(fontSize: 12, color: c.selectionInk))),
          if (!loading && error.isEmpty && typeCounts.length > 1) Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Wrap(spacing: 6, runSpacing: 6, children: [
              _typeChip('全部', entries.length, activeType == null, c.primarySoft, c.selectionInk, c, () => _selectType(null)),
              for (final (type, count) in typeCounts) ...[
                () {
                  final (bg, fg) = _badgeColors(type, c);
                  return _typeChip(_CatalogPageState._fieldLabels[type] ?? type, count, activeType == type, bg, fg, c, () => _selectType(type));
                }(),
              ],
            ]),
          ),
          const SizedBox(height: 8),
          Expanded(child: loading ? const Center(child: CircularProgressIndicator()) : error.isNotEmpty
              ? Center(child: Column(mainAxisSize: MainAxisSize.min, children: [Text(error), TextButton(onPressed: load, child: const Text('重试'))]))
              : shown.isEmpty ? Center(child: Text(entries.isEmpty ? '这个规则提供商没有规则' : query.isNotEmpty ? '没有包含“$query”的条目' : '没有符合条件的条目'))
              : Container(
                  decoration: BoxDecoration(color: c.fill, borderRadius: BorderRadius.circular(12)),
                  padding: const EdgeInsets.all(4),
                  child: ListView.builder(
                    itemCount: shown.length,
                    itemBuilder: (_, i) {
                      final type = shown[i]['type']?.toString() ?? '';
                      final value = shown[i]['value']?.toString() ?? '';
                      final label = _CatalogPageState._fieldLabels[type] ?? type;
                      final (bg, fg) = _badgeColors(type, c);
                      final groupBreak = i > 0 && shown[i - 1]['type'] != shown[i]['type'];
                      return Container(
                        decoration: groupBreak ? BoxDecoration(border: Border(top: BorderSide(color: c.hairline, width: 0.75))) : null,
                        child: InkWell(
                          borderRadius: BorderRadius.circular(7),
                          onTap: () {
                            Clipboard.setData(ClipboardData(text: value));
                            showGlassToast(context, '已复制 $value');
                          },
                          child: Padding(
                            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 5),
                            child: Row(children: [
                              Container(
                                width: 72,
                                height: 20,
                                padding: const EdgeInsets.symmetric(horizontal: 6),
                                alignment: Alignment.center,
                                decoration: BoxDecoration(color: bg, borderRadius: BorderRadius.circular(6)),
                                child: Text(label, maxLines: 1, overflow: TextOverflow.ellipsis, style: TextStyle(fontSize: 11, fontWeight: FontWeight.w500, color: fg)),
                              ),
                              const SizedBox(width: 10),
                              Expanded(child: Text(value, maxLines: 1, overflow: TextOverflow.ellipsis, style: TextStyle(fontSize: 12, fontFamily: 'monospace', color: c.ink))),
                            ]),
                          ),
                        ),
                      );
                    },
                  ),
                )),
          if (!loading && error.isEmpty) Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(query.isEmpty && activeType == null ? '共 ${entries.length} 条' : '显示 ${shown.length} / 共 ${entries.length} 条', style: TextStyle(fontSize: 12, color: c.muted)),
          ),
        ])),
      ),
    );
  }
}
