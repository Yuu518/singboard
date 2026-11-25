import 'dart:async';
import 'dart:io' show InternetAddress, InternetAddressType;

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'glass_ui.dart';
import 'native_stream.dart';
import 'config_page.dart' show configDirectory;

typedef Item = Map<String, dynamic>;

DateTime? _parseUpdatedAt(dynamic raw) {
  final iso = raw?.toString().trim() ?? '';
  if (iso.isEmpty) return null;
  final dt = DateTime.tryParse(iso);
  if (dt == null || dt.millisecondsSinceEpoch <= 0) return null;
  return dt;
}

String formatUpdatedRelative(dynamic raw, {DateTime? now}) {
  final dt = _parseUpdatedAt(raw);
  if (dt == null) return '';
  final diff = (now ?? DateTime.now()).difference(dt).inSeconds;
  final secs = diff < 0 ? 0 : diff;
  if (secs < 60) return '$secs 秒前';
  if (secs < 3600) return '${secs ~/ 60} 分钟前';
  if (secs < 86400) return '${secs ~/ 3600} 小时前';
  return '${secs ~/ 86400} 天前';
}

class _ClausePart {
  final String kind;
  final bool negated;
  final String? label;
  final List<String> values;
  final List<(String short, String full)> sets;
  final bool mono;
  final String op;
  const _ClausePart.op(this.op)
      : kind = 'op',
        negated = false,
        label = null,
        values = const [],
        sets = const [],
        mono = false;
  const _ClausePart.sets({required this.negated, required this.sets})
      : kind = 'sets',
        label = null,
        values = const [],
        mono = false,
        op = '';
  const _ClausePart.item({
    required this.negated,
    this.label,
    required this.values,
    this.mono = false,
  })  : kind = 'item',
        sets = const [],
        op = '';
  const _ClausePart.finalFallback()
      : kind = 'final',
        negated = false,
        label = null,
        values = const [],
        sets = const [],
        mono = false,
        op = '';
  const _ClausePart.allTraffic()
      : kind = 'all',
        negated = false,
        label = null,
        values = const [],
        sets = const [],
        mono = false,
        op = '';
}

class _RuleAction {
  final String kind;
  final String label;
  final String? finalNode;
  final String? icon;
  final bool direct;
  final bool rejectFinal;
  const _RuleAction(
    this.kind,
    this.label, {
    this.finalNode,
    this.icon,
    this.direct = false,
    this.rejectFinal = false,
  });

  bool get isPreprocessing => kind == 'pass' || kind == 'stop';
}

class _RuleTrace {
  final String query;
  final int? winner;
  final Set<int> passed;
  final Set<int> shadowed;
  final bool pending;
  const _RuleTrace({
    required this.query,
    required this.winner,
    required this.passed,
    required this.shadowed,
    required this.pending,
  });
}

class CatalogPage extends StatefulWidget {
  final String endpoint, secret;
  final bool proxies, enabled, active;
  final int reconnectGeneration;
  final Map<String, dynamic> preferences;
  final Map<String, dynamic> chainSpeeds;
  final VoidCallback? onProxyChanged;

  const CatalogPage({
    super.key,
    required this.endpoint,
    required this.secret,
    required this.proxies,
    this.enabled = true,
    this.active = true,
    this.reconnectGeneration = 0,
    this.preferences = const {},
    this.chainSpeeds = const {},
    this.onProxyChanged,
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

  bool _groupHasTestingNode(String groupName) {
    for (final g in items) {
      if (g['name'] == groupName) {
        final list = g['nodes'] as List? ?? const [];
        return list.any((n) => n is Map && _isNodeTesting(n['name']?.toString() ?? ''));
      }
    }
    return false;
  }

  void _releaseOrder(String name) {
    if (!_pointed.contains(name) &&
        !_focused.contains(name) &&
        !testingGroups.containsKey(name) &&
        !_groupHasTestingNode(name)) {
      _frozenOrder.remove(name);
    }
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
  final testingGroups = <String, (int, int)>{};
  final testingProviders = <String, (int, int)>{};
  final testingNodes = <String, int>{};
  final delays = <String, int?>{};
  final v6Nodes = <String>{};
  int testGeneration = 0;

  bool _isNodeTesting(String name) => (testingNodes[name] ?? 0) > 0;

  void _markNodeTesting(String name) {
    testingNodes[name] = (testingNodes[name] ?? 0) + 1;
  }

  void _unmarkNodeTesting(String name) {
    final count = testingNodes[name] ?? 0;
    if (count <= 1) {
      testingNodes.remove(name);
    } else {
      testingNodes[name] = count - 1;
    }
  }

  void _resetTesting() {
    testGeneration++;
    testingGroups.clear();
    testingProviders.clear();
    testingNodes.clear();
  }
  (String group, String prev)? undoSwitch;
  Timer? undoTimer;
  Timer? ruleTimer;
  Timer? ageTimer;
  int matchRevision = 0;
  Set<String> matched = {};
  final togglingRules = <String>{};
  String matchError = '';
  bool matching = false;
  bool get readable => widget.preferences['nativeRootEnabled'] == true;

  bool providers = false, busy = false, sortByLatency = false;
  final updatingProviders = <String>{};
  (int, int)? updatingAllProviders;
  int providerUpdateGeneration = 0;
  bool preExpanded = false;
  int generation = 0;
  String query = '', error = '';

  @override
  void initState() {
    super.initState();
    sortByLatency = widget.preferences['proxySortLatency'] == true;
    expanded.addAll((widget.preferences['expandedProxyGroups'] as List? ?? []).whereType<String>());
    if (widget.enabled) load();
  }

  void _syncAgeTimer() {
    ageTimer?.cancel();
    ageTimer = null;
    if (!mounted || !widget.active || !providers || providerItems.isEmpty) {
      return;
    }
    final now = DateTime.now();
    var fast = false;
    var hasAny = false;
    for (final item in providerItems) {
      final dt = _parseUpdatedAt(item['updatedAt']);
      if (dt == null) continue;
      hasAny = true;
      final secs = now.difference(dt).inSeconds;
      if (secs < 60) {
        fast = true;
        break;
      }
    }
    if (!hasAny) return;
    ageTimer = Timer(Duration(seconds: fast ? 1 : 30), () {
      if (!mounted) return;
      setState(() {});
      _syncAgeTimer();
    });
  }

  @override
  void didUpdateWidget(CatalogPage old) {
    super.didUpdateWidget(old);
    final endpointChanged =
        widget.endpoint != old.endpoint || widget.secret != old.secret;
    if (endpointChanged || widget.reconnectGeneration != old.reconnectGeneration) {
      _resetTesting();
      togglingRules.clear();
      providerUpdateGeneration++;
      updatingProviders.clear();
      updatingAllProviders = null;
      if (endpointChanged) {
        delays.clear();
        v6Nodes.clear();
      }
    }
    if (widget.enabled && !old.enabled) {
      load();
    } else if (widget.enabled &&
        widget.reconnectGeneration != old.reconnectGeneration) {
      load(silent: items.isNotEmpty);
    } else if (widget.enabled && widget.active && !old.active) {
      load(silent: items.isNotEmpty);
    }
    if (widget.active != old.active) {
      _syncAgeTimer();
    }
  }

  @override
  void dispose() {
    undoTimer?.cancel();
    ruleTimer?.cancel();
    ageTimer?.cancel();
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
        if (!widget.proxies && togglingRules.isNotEmpty) {
          final pending = <String, bool>{
            for (final item in items)
              if (togglingRules.contains(_ruleUuid(item)))
                _ruleUuid(item): _isRuleDisabled(item),
          };
          for (final item in next) {
            final u = _ruleUuid(item);
            if (pending.containsKey(u)) item['disabled'] = pending[u];
          }
        }
        items = next;
        if (widget.proxies) {
          delays.removeWhere((k, _) => !_isNodeTesting(k));
        }
        providerItems = nextProviders;
        providersAvailable = providerApiReady;
        if (!providersAvailable) providers = false;
        error = '';
      });
      _syncAgeTimer();
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

  String _ruleUuid(Item rule) => (rule['uuid']?.toString() ?? '').trim();

  bool _isRuleDisabled(Item rule) => rule['disabled'] == true;

  void _setRuleDisabled(String uuid, bool disabled, [Item? extra]) {
    for (final item in items) {
      if (_ruleUuid(item) == uuid) {
        item['disabled'] = disabled;
      }
    }
    if (extra != null && _ruleUuid(extra) == uuid) {
      extra['disabled'] = disabled;
    }
  }

  Future<void> toggleRule(Item rule) async {
    final uuid = _ruleUuid(rule);
    if (uuid.isEmpty || togglingRules.contains(uuid)) return;
    final prevDisabled = _isRuleDisabled(rule);
    final endpoint = widget.endpoint;
    final secret = widget.secret;
    setState(() {
      togglingRules.add(uuid);
      _setRuleDisabled(uuid, !prevDisabled, rule);
    });
    try {
      await clashRequest(endpoint, secret, 'toggle_rule', {'uuid': uuid});
      if (!mounted || endpoint != widget.endpoint || secret != widget.secret) {
        return;
      }
      setState(() => togglingRules.remove(uuid));
      if (togglingRules.isEmpty) {
        unawaited(load(silent: true));
      }
    } catch (e) {
      if (!mounted) return;
      if (endpoint == widget.endpoint && secret == widget.secret) {
        setState(() {
          togglingRules.remove(uuid);
          _setRuleDisabled(uuid, prevDisabled, rule);
        });
      }
      showGlassToast(context, '切换规则状态失败：$e');
    }
  }

  void inspectRule(int index, Item rule) {
    final uuid = _ruleUuid(rule);
    final providerCounts = <String, int>{
      for (final p in providerItems)
        if (p['name'] is String)
          p['name'] as String: (p['ruleCount'] as num?)?.toInt() ?? 0,
    };
    showGlassDialog<void>(
      context: context,
      builder: (_) => _RuleInspectDialog(
        index: index,
        rule: rule,
        clauses: _parseClauses(rule),
        action: _parseAction(rule),
        providerCounts: providerCounts,
        readable: readable,
        onOpenProvider: rulesTool,
        initialToggling: uuid.isNotEmpty && togglingRules.contains(uuid),
        onToggleRule: uuid.isEmpty ? null : () => toggleRule(rule),
      ),
    );
  }

  List<String> _updatableProviderNames() => [
    for (final item in providerItems)
      if ((item['vehicleType']?.toString() ?? '').toLowerCase() != 'inline')
        if ((item['name']?.toString() ?? '').isNotEmpty)
          item['name'].toString(),
  ];

  Future<void> updateProvider(Item item) async {
    final name = item['name']?.toString() ?? '';
    if (name.isEmpty ||
        updatingAllProviders != null ||
        updatingProviders.contains(name)) {
      return;
    }
    final runGen = providerUpdateGeneration;
    final endpoint = widget.endpoint;
    final secret = widget.secret;
    final op = widget.proxies ? 'update_proxy_provider' : 'update_rule_provider';
    setState(() => updatingProviders.add(name));
    try {
      await clashRequest(endpoint, secret, op, {'name': name});
      if (!mounted || runGen != providerUpdateGeneration) return;
      await load(silent: true);
    } catch (e) {
      if (mounted && runGen == providerUpdateGeneration) {
        showGlassToast(context, e.toString());
      }
    } finally {
      if (mounted && runGen == providerUpdateGeneration) {
        setState(() => updatingProviders.remove(name));
      }
    }
  }

  Future<void> updateAllProviders() async {
    if (updatingAllProviders != null || updatingProviders.isNotEmpty) return;
    final names = _updatableProviderNames();
    if (names.isEmpty) return;
    final runGen = providerUpdateGeneration;
    final endpoint = widget.endpoint;
    final secret = widget.secret;
    final op = widget.proxies ? 'update_proxy_provider' : 'update_rule_provider';
    final total = names.length;
    var done = 0;
    setState(() {
      updatingAllProviders = (0, total);
      updatingProviders.addAll(names);
    });
    final failures = <String>[];
    try {
      for (final name in names) {
        if (!mounted || runGen != providerUpdateGeneration) return;
        try {
          await clashRequest(endpoint, secret, op, {'name': name});
        } catch (e) {
          failures.add('$name · $e');
        }
        if (!mounted || runGen != providerUpdateGeneration) return;
        done++;
        setState(() {
          updatingProviders.remove(name);
          updatingAllProviders = (done, total);
        });
      }
      if (mounted && runGen == providerUpdateGeneration) {
        await load(silent: true);
        if (failures.isNotEmpty &&
            mounted &&
            runGen == providerUpdateGeneration) {
          showGlassToast(context, failures.join('\n'));
        }
      }
    } finally {
      if (mounted && runGen == providerUpdateGeneration) {
        setState(() {
          updatingAllProviders = null;
          updatingProviders.removeAll(names);
        });
      }
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
        _syncGroupSelectionDelay(group, nodeName);
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
      widget.onProxyChanged?.call();
    } catch (e) {
      if (mounted) {
        showGlassToast(context, '切换失败：$e');
      }
    }
  }

  Future<void> testGroup(Item group) async {
    final gName = group['name'].toString();
    if (testingGroups.containsKey(gName)) return;
    final rawNodes = (group['nodes'] as List? ?? []).cast<Item>();
    final seen = <String>{};
    final nodes = <String>[];
    for (final n in rawNodes) {
      if (n['kind']?.toString().toLowerCase() == 'direct') continue;
      final name = n['name']?.toString() ?? '';
      if (name.isNotEmpty && seen.add(name)) nodes.add(name);
    }
    if (nodes.isEmpty) return;
    _holdOrder(group);
    await _testNodesBatch(
      isProvider: false,
      target: gName,
      url: group['test_url']?.toString() ?? '',
      nodes: nodes,
    );
  }

  Future<void> testProvider(Item item) async {
    final name = item['name'].toString();
    if (testingProviders.containsKey(name)) return;
    final rawNodes = (item['proxies'] as List? ?? []).cast<Item>();
    final seen = <String>{};
    final nodes = <String>[];
    for (final n in rawNodes) {
      if (n['type']?.toString().toLowerCase() == 'direct') continue;
      final nName = n['name']?.toString() ?? '';
      if (nName.isNotEmpty && seen.add(nName)) nodes.add(nName);
    }
    if (nodes.isEmpty) return;
    final rawUrl = item['testUrl']?.toString().trim() ?? '';
    final url = rawUrl.isNotEmpty
        ? rawUrl
        : (widget.preferences['latencyTestUrl']?.toString() ?? '');
    await _testNodesBatch(
      isProvider: true,
      target: name,
      url: url,
      nodes: nodes,
    );
  }

  Future<void> _testNodesBatch({
    required bool isProvider,
    required String target,
    required String url,
    required List<String> nodes,
  }) async {
    const concurrency = 8;
    final runGen = testGeneration;
    final ipv6 = widget.preferences['ipv6TestEnabled'] == true;
    setState(() {
      if (isProvider) {
        testingProviders[target] = (0, nodes.length);
      } else {
        testingGroups[target] = (0, nodes.length);
      }
      for (final node in nodes) {
        delays[node] = null;
        v6Nodes.remove(node);
        _markNodeTesting(node);
      }
    });
    var cursor = 0;
    final workers = nodes.length < concurrency ? nodes.length : concurrency;
    Future<void> worker() async {
      while (mounted && runGen == testGeneration) {
        if (cursor >= nodes.length) break;
        final nodeName = nodes[cursor++];
        var delay = 0;
        bool? v6 = ipv6 ? false : null;
        try {
          final value = await clashRequest(widget.endpoint, widget.secret, 'delay', {
            'name': nodeName,
            'url': url,
            'ipv6': ipv6,
          });
          if (value is Map) {
            if (value['delay'] case final num d) delay = d.toInt();
            if (value['ipv6'] case final bool b) v6 = b;
          }
        } catch (_) {}
        if (!mounted || runGen != testGeneration) break;
        setState(() {
          _recordDelay(nodeName, delay, ipv6: v6);
          _unmarkNodeTesting(nodeName);
          final map = isProvider ? testingProviders : testingGroups;
          final current = map[target];
          if (current != null) map[target] = (current.$1 + 1, current.$2);
        });
      }
    }

    await Future.wait(List.generate(workers, (_) => worker()));
    if (!mounted || runGen != testGeneration) return;
    setState(() {
      if (isProvider) {
        testingProviders.remove(target);
      } else {
        testingGroups.remove(target);
        _releaseOrder(target);
      }
    });
  }

  Future<void> testNode(String testUrl, String nodeName, {Item? group}) async {
    if (_isNodeTesting(nodeName)) return;
    final runGen = testGeneration;
    final ipv6 = widget.preferences['ipv6TestEnabled'] == true;
    if (group != null && sortByLatency) _holdOrder(group);
    setState(() {
      delays[nodeName] = null;
      v6Nodes.remove(nodeName);
      _markNodeTesting(nodeName);
    });
    try {
      final value = await clashRequest(widget.endpoint, widget.secret, 'delay', {
        'name': nodeName,
        'url': testUrl,
        'ipv6': ipv6,
      });
      if (!mounted || runGen != testGeneration) return;
      setState(() {
        final d = value is Map && value['delay'] is num
            ? (value['delay'] as num).toInt()
            : 0;
        final v6 = value is Map && value['ipv6'] == true
            ? true
            : (ipv6 ? false : null);
        _recordDelay(nodeName, d, ipv6: v6);
      });
    } catch (_) {
      if (mounted && runGen == testGeneration) {
        setState(() {
          _recordDelay(nodeName, 0, ipv6: ipv6 ? false : null);
        });
      }
    } finally {
      if (mounted && runGen == testGeneration) {
        setState(() {
          _unmarkNodeTesting(nodeName);
          if (group != null) _releaseOrder(group['name'].toString());
        });
      }
    }
  }

  List<String> _linkedProxyNames(String name) {
    final visited = <String>{name};
    final queue = <String>[name];
    final result = <String>[name];
    if (!widget.proxies) return result;
    var cur = name;
    while (true) {
      Item? found;
      for (final g in items) {
        if (g['name']?.toString() == cur) {
          found = g;
          break;
        }
      }
      if (found == null) break;
      final now = found['now']?.toString() ?? '';
      if (now.isEmpty || !visited.add(now)) break;
      queue.add(now);
      result.add(now);
      cur = now;
    }
    var idx = 0;
    while (idx < queue.length) {
      final target = queue[idx++];
      for (final g in items) {
        if (g['now']?.toString() == target) {
          final gName = g['name']?.toString() ?? '';
          if (gName.isNotEmpty && visited.add(gName)) {
            queue.add(gName);
            result.add(gName);
          }
        }
      }
    }
    return result;
  }

  List<String> _ancestorGroupNames(String groupName) {
    final visited = <String>{groupName};
    final queue = <String>[groupName];
    final result = <String>[groupName];
    if (!widget.proxies) return result;
    var idx = 0;
    while (idx < queue.length) {
      final target = queue[idx++];
      for (final g in items) {
        if (g['now']?.toString() == target) {
          final gName = g['name']?.toString() ?? '';
          if (gName.isNotEmpty && visited.add(gName)) {
            queue.add(gName);
            result.add(gName);
          }
        }
      }
    }
    return result;
  }

  void _recordDelay(String name, int delay, {bool? ipv6}) {
    for (final target in _linkedProxyNames(name)) {
      delays[target] = delay;
      if (ipv6 == true) {
        v6Nodes.add(target);
      } else if (ipv6 == false) {
        v6Nodes.remove(target);
      }
    }
  }

  void _syncGroupSelectionDelay(Item group, String nodeName) {
    final gName = group['name']?.toString() ?? '';
    if (gName.isEmpty) return;
    final rawNodes = (group['nodes'] as List? ?? []).cast<Item>();
    Item? selected;
    for (final n in rawNodes) {
      if (n['name']?.toString() == nodeName) {
        selected = n;
        break;
      }
    }
    final delay = selected == null ? null : _delayOf(selected);
    final ipv6Enabled = widget.preferences['ipv6TestEnabled'] == true;
    final isV6 = v6Nodes.contains(nodeName);
    for (final target in _ancestorGroupNames(gName)) {
      delays[target] = delay;
      if (ipv6Enabled) {
        if (isV6) {
          v6Nodes.add(target);
        } else {
          v6Nodes.remove(target);
        }
      }
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

  Widget _buildLatencyBadge(
    int? delay,
    bool testing,
    Color bg,
    Color fg, {
    double verticalPadding = 1.5,
    FontWeight? fontWeight,
  }) {
    return Container(
      padding: EdgeInsets.symmetric(horizontal: 7, vertical: verticalPadding),
      decoration: BoxDecoration(
        color: bg,
        borderRadius: BorderRadius.circular(999),
      ),
      child: Stack(
        alignment: Alignment.center,
        children: [
          Opacity(
            opacity: testing ? 0 : 1,
            child: Text(
              _formatLatency(delay),
              style: TextStyle(
                fontSize: 12,
                fontWeight: fontWeight,
                color: fg,
                fontFeatures: const [FontFeature.tabularFigures()],
              ),
            ),
          ),
          if (testing)
            SizedBox.square(
              dimension: 11,
              child: CircularProgressIndicator(
                strokeWidth: 1.6,
                color: fg,
              ),
            ),
        ],
      ),
    );
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

  (Color bg, Color fg) _latencyBadge(
    int? delay,
    SingboardColors c, {
    bool testing = false,
  }) {
    if (testing) return (c.primarySoft, c.selectionInk);
    if (delay == null) return (c.fill, c.muted);
    if (delay == 0) return (c.dangerSoft, c.dangerInk);
    if (delay < 300) return (c.successSoft, c.successInk);
    if (delay < 800) return (c.warningSoft, c.warningInk);
    return (c.dangerSoft, c.dangerInk);
  }

  List<String> _splitRawValues(String raw) {
    if (raw.startsWith('[') && raw.endsWith(']')) {
      return raw
          .substring(1, raw.length - 1)
          .split(RegExp(r'\s+'))
          .where((s) => s.isNotEmpty)
          .toList();
    }
    return [raw];
  }

  _ClausePart _itemToClause(String key, String raw, bool negated) {
    final list = _splitRawValues(raw);
    if (key == 'rule_set') {
      return _ClausePart.sets(
        negated: negated,
        sets: [for (final full in list) (full.split('/').last, full)],
      );
    }
    if (key == 'ip_is_private') {
      return _ClausePart.item(
        negated: negated,
        values: [raw == 'true' ? '私有 IP' : '非私有 IP'],
      );
    }
    if (key == 'ip_version') {
      return _ClausePart.item(
        negated: negated,
        values: ['IPv$raw'],
      );
    }
    final label = _fieldLabels[key];
    if (label == null) {
      return _ClausePart.item(
        negated: negated,
        values: ['$key=$raw'],
        mono: true,
      );
    }
    final values = [
      for (final v in list) _upperFields.contains(key) ? v.toUpperCase() : v,
    ];
    return _ClausePart.item(
      negated: negated,
      label: label,
      values: values,
      mono: _monoFields.contains(key),
    );
  }

  List<_ClausePart> _termToClauses(String term) {
    var body = term.trim();
    var negated = false;
    if (body.startsWith('!(') && body.endsWith(')')) {
      negated = true;
      body = body.substring(2, body.length - 1);
    } else if (body.startsWith('(') && body.endsWith(')')) {
      body = body.substring(1, body.length - 1);
    }
    final matches =
        RegExp(r'([a-z_]+)=(\[[^\]]*\]|\S+)').allMatches(body).toList();
    if (matches.isEmpty) {
      return [_ClausePart.item(negated: negated, values: [body], mono: true)];
    }
    final out = <_ClausePart>[];
    for (var i = 0; i < matches.length; i++) {
      if (i > 0) out.add(const _ClausePart.op('且'));
      out.add(
        _itemToClause(
          matches[i].group(1)!,
          matches[i].group(2)!,
          negated && i == 0,
        ),
      );
    }
    return out;
  }

  List<(String?, String)> _splitPayloadTerms(String payload) {
    final trimmed = payload.trim();
    if (trimmed.isEmpty) return const [];
    final parts = trimmed.split(RegExp(r'\s(&&|\|\|)\s'));
    final ops = RegExp(r'\s(&&|\|\|)\s')
        .allMatches(trimmed)
        .map((m) => m.group(1) == '&&' ? '且' : '或')
        .toList();
    final out = <(String?, String)>[];
    for (var i = 0; i < parts.length; i++) {
      out.add((i == 0 ? null : ops[i - 1], parts[i]));
    }
    return out;
  }

  List<_ClausePart> _parseClauses(Item rule) {
    if (rule['condition']?.toString() == '最终回退') {
      return const [_ClausePart.finalFallback()];
    }
    final payload = (rule['payload']?.toString() ?? '').trim();
    if (payload.isEmpty) return const [_ClausePart.allTraffic()];
    final out = <_ClausePart>[];
    for (final (op, term) in _splitPayloadTerms(payload)) {
      if (op != null) out.add(_ClausePart.op(op));
      out.addAll(_termToClauses(term));
    }
    return out;
  }

  bool _isLookupQuery(String q) {
    if (q.isEmpty) return false;
    final parts = q.split('/');
    final addr = InternetAddress.tryParse(parts.first);
    final prefix = parts.length == 2 ? int.tryParse(parts.last) : null;
    final isIp = addr != null &&
        (parts.length == 1 ||
            (parts.length == 2 &&
                prefix != null &&
                prefix >= 0 &&
                prefix <= addr.rawAddress.length * 8));
    if (isIp) return true;
    return RegExp(
      r'^(?:[a-z0-9_-]+\.)+[a-z0-9-]{2,}\.?$',
      caseSensitive: false,
    ).hasMatch(q);
  }

  bool _isPrivateIp(InternetAddress addr) {
    final b = addr.rawAddress;
    if (addr.type == InternetAddressType.IPv4 && b.length == 4) {
      if (b[0] == 10 || b[0] == 127 || b[0] == 0) return true;
      if (b[0] == 172 && b[1] >= 16 && b[1] <= 31) return true;
      if (b[0] == 192 && b[1] == 168) return true;
      if (b[0] == 169 && b[1] == 254) return true;
      if (b[0] == 100 && (b[1] & 0xC0) == 64) return true;
      if (b[0] == 255 && b[1] == 255 && b[2] == 255 && b[3] == 255) return true;
      return false;
    }
    if (addr.type == InternetAddressType.IPv6 && b.length == 16) {
      final allZero = b.every((x) => x == 0);
      final loopback = b.sublist(0, 15).every((x) => x == 0) && b[15] == 1;
      final ula = (b[0] & 0xFE) == 0xFC;
      final linkLocal = b[0] == 0xFE && (b[1] & 0xC0) == 0x80;
      return allZero || loopback || ula || linkLocal;
    }
    return false;
  }

  bool _ipInCidr(InternetAddress addr, String cidr) {
    final cut = cidr.indexOf('/');
    if (cut < 0) {
      final net = InternetAddress.tryParse(cidr);
      if (net == null || net.type != addr.type) return false;
      final a = addr.rawAddress;
      final n = net.rawAddress;
      for (var i = 0; i < a.length; i++) {
        if (a[i] != n[i]) return false;
      }
      return true;
    }
    final net = InternetAddress.tryParse(cidr.substring(0, cut));
    final prefix = int.tryParse(cidr.substring(cut + 1));
    if (net == null || prefix == null || net.type != addr.type) return false;
    final a = addr.rawAddress;
    final n = net.rawAddress;
    final maxBits = a.length * 8;
    if (prefix < 0 || prefix > maxBits) return false;
    final fullBytes = prefix ~/ 8;
    for (var i = 0; i < fullBytes; i++) {
      if (a[i] != n[i]) return false;
    }
    final rem = prefix % 8;
    if (rem > 0) {
      final mask = (0xFF << (8 - rem)) & 0xFF;
      if ((a[fullBytes] & mask) != (n[fullBytes] & mask)) return false;
    }
    return true;
  }

  bool? _evalItemForLookup(String key, String raw, String q, InternetAddress? ip) {
    final list = _splitRawValues(raw);
    switch (key) {
      case 'rule_set':
        return list.any(matched.contains);
      case 'domain':
        if (ip != null) return false;
        final host = q.replaceFirst(RegExp(r'\.$'), '').toLowerCase();
        return list.any(
          (v) => host == v.replaceFirst(RegExp(r'\.$'), '').toLowerCase(),
        );
      case 'domain_suffix':
        if (ip != null) return false;
        final host = q.replaceFirst(RegExp(r'\.$'), '').toLowerCase();
        return list.any((v) {
          final suffix = v
              .replaceFirst(RegExp(r'^\.'), '')
              .replaceFirst(RegExp(r'\.$'), '')
              .toLowerCase();
          return suffix.isNotEmpty &&
              (host == suffix || host.endsWith('.$suffix'));
        });
      case 'domain_keyword':
        if (ip != null) return false;
        final host = q.toLowerCase();
        return list.any((v) => v.isNotEmpty && host.contains(v.toLowerCase()));
      case 'ip_cidr':
        if (ip == null) return null;
        return list.any((cidr) => _ipInCidr(ip, cidr));
      case 'ip_is_private':
        if (ip == null) return null;
        return _isPrivateIp(ip) == (raw == 'true');
      case 'ip_version':
        if (ip == null) return null;
        return (ip.type == InternetAddressType.IPv4 && raw == '4') ||
            (ip.type == InternetAddressType.IPv6 && raw == '6');
      default:
        return null;
    }
  }

  bool? _evalTermForLookup(String term, String q, InternetAddress? ip) {
    var body = term.trim();
    var negated = false;
    if (body.startsWith('!(') && body.endsWith(')')) {
      negated = true;
      body = body.substring(2, body.length - 1);
    } else if (body.startsWith('(') && body.endsWith(')')) {
      body = body.substring(1, body.length - 1);
    }
    final matches =
        RegExp(r'([a-z_]+)=(\[[^\]]*\]|\S+)').allMatches(body).toList();
    if (matches.isEmpty) return null;
    var allKnown = true;
    for (final m in matches) {
      final res = _evalItemForLookup(m.group(1)!, m.group(2)!, q, ip);
      if (res == false) return negated;
      if (res == null) allKnown = false;
    }
    return allKnown ? !negated : null;
  }

  bool _evalRuleForLookup(Item rule, String q) {
    if (rule['condition']?.toString() == '最终回退') return true;
    final payload = (rule['payload']?.toString() ?? '').trim();
    if (payload.isEmpty) return true;
    final host = q.contains('/') ? q.split('/').first : q;
    final ip = InternetAddress.tryParse(host);
    bool? acc;
    for (final (op, term) in _splitPayloadTerms(payload)) {
      final val = _evalTermForLookup(term, q, ip);
      if (op == null) {
        acc = val;
      } else if (op == '且') {
        if (acc == false || val == false) {
          acc = false;
        } else if (acc == true && val == true) {
          acc = true;
        } else {
          acc = null;
        }
      } else {
        if (acc == true || val == true) {
          acc = true;
        } else if (acc == false && val == false) {
          acc = false;
        } else {
          acc = null;
        }
      }
    }
    return acc == true;
  }

  _RuleTrace? _computeTrace() {
    if (widget.proxies || providers || !_isLookupQuery(query)) return null;
    int? winner;
    final passed = <int>{};
    final shadowed = <int>{};
    for (var i = 0; i < items.length; i++) {
      final r = items[i];
      if (_isRuleDisabled(r)) continue;
      if (!_evalRuleForLookup(r, query)) continue;
      final act = _parseAction(r);
      if (act.kind == 'pass') {
        if (winner == null) passed.add(i);
      } else if (winner == null) {
        winner = i;
      } else {
        shadowed.add(i);
      }
    }
    return _RuleTrace(
      query: query,
      winner: winner,
      passed: passed,
      shadowed: shadowed,
      pending: matching,
    );
  }

  bool _isDirectLabel(String label) {
    final lower = label.toLowerCase();
    return lower == 'direct' || lower.contains('direct') || label.contains('直连');
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
    final rejectFinal = finalNode?.toLowerCase().contains('reject') == true;
    final direct = !rejectFinal &&
        ((finalNode != null && _isDirectLabel(finalNode)) ||
            (finalNode == null && _isDirectLabel(outbound)));
    return _RuleAction(
      'route',
      outbound,
      finalNode: finalNode,
      direct: direct,
      rejectFinal: rejectFinal,
    );
  }

  String _formatCompactCount(num n) {
    final v = n.toInt();
    if (v >= 10000) {
      final s = (v / 1000).toStringAsFixed(1);
      return '${s.endsWith('.0') ? s.substring(0, s.length - 2) : s}k';
    }
    if (v >= 1000) {
      return '${(v / 1000).toStringAsFixed(1)}k';
    }
    return '$v';
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

  Widget _buildDot(
    int? delay,
    bool selected,
    SingboardColors c, {
    bool testing = false,
    VoidCallback? onTap,
  }) {
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
              ? Border.all(
                  color: testing ? c.primary : c.muted.withValues(alpha: 0.55),
                  width: testing ? 1.5 : 1.2,
                )
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
    final groupProgress = testingGroups[gName];
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
    final nowTesting = _isNodeTesting(now);
    final showNowBadge = now.isNotEmpty &&
        ((nowDelay != null && nowDelay > 0) ||
            (nowNode?['kind']?.toString().toLowerCase() != 'direct'));
    final (nowBg, nowFg) = _latencyBadge(nowDelay, c, testing: nowTesting);
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
                              _buildLatencyBadge(
                                nowDelay,
                                nowTesting,
                                nowBg,
                                nowFg,
                                verticalPadding: 2,
                                fontWeight: FontWeight.w500,
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
                                  testing: _isNodeTesting(node['name'].toString()),
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
                      if (groupProgress != null)
                        _buildActionPill(
                          icon: SizedBox.square(
                            dimension: 11,
                            child: CircularProgressIndicator(
                              strokeWidth: 1.6,
                              color: c.selectionInk,
                            ),
                          ),
                          label: '${groupProgress.$1}/${groupProgress.$2}',
                          c: c,
                          active: true,
                          disabled: true,
                        )
                      else
                        _buildActionPill(
                          icon: PanelIcon(
                            'bolt',
                            size: 13,
                            color: c.ink.withValues(alpha: 0.78),
                          ),
                          label: '测速',
                          c: c,
                          tooltip: '延迟测试',
                          onTap: () => testGroup(group),
                        ),
                      const SizedBox(height: 4),
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
                builder: (context, constraints) {
                  final ts = MediaQuery.textScalerOf(context);
                  final scale = ts.scale(14) / 14;
                  final minColW = 128 * (1 + (scale - 1).clamp(0.0, 1.25) * 0.35);
                  final cols = ((constraints.maxWidth + 8) / (minColW + 8)).floor().clamp(1, 1000);
                  final tileHeight = (20.0 + 4.0 + ts.scale(14) * 1.25 * 2 + ts.scale(12) * 1.4 + 3.0).ceilToDouble();
                  var selectedKeyAssigned = false;
                  Widget buildTile(Item node) {
                    final nName = node['name'].toString();
                    final selected = nName == now;
                    final assignKey = selected && !selectedKeyAssigned;
                    if (assignKey) selectedKeyAssigned = true;
                    final delay = _delayOf(node);
                    final testing = _isNodeTesting(nName);
                    final showLat = (delay != null && delay > 0) ||
                        (node['kind']?.toString().toLowerCase() != 'direct');
                    final (latBg, latFg) = _latencyBadge(delay, c, testing: testing);
                    final isV6 = ipv6Enabled && v6Nodes.contains(nName);
                    return GestureDetector(
                      key: assignKey ? selectedTiles.putIfAbsent(gName, GlobalKey.new) : null,
                      onTap: selectable && !selected
                          ? () => selectNode(group, nName)
                          : null,
                      child: Container(
                        height: tileHeight,
                        padding: const EdgeInsets.all(10),
                        decoration: BoxDecoration(
                          color: selected
                              ? c.primarySoft
                              : (c.dark
                                  ? const Color(0x14FFFFFF)
                                  : const Color(0x38FFFFFF)),
                          borderRadius: BorderRadius.circular(12),
                          border: Border.all(
                            color: selected || testing
                                ? c.selectionBorder
                                : c.hairline,
                            width: selected || testing ? 1 : 0.5,
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
                            const SizedBox(height: 4),
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
                                              group: group,
                                            ),
                                    child: _buildLatencyBadge(
                                      delay,
                                      testing,
                                      latBg,
                                      latFg,
                                      fontWeight: FontWeight.w500,
                                    ),
                                  ),
                              ],
                            ),
                          ],
                        ),
                      ),
                    );
                  }
                  return Column(
                    children: [
                      for (var row = 0; row < nodes.length; row += cols) ...[
                        if (row > 0) const SizedBox(height: 8),
                        Row(
                          children: [
                            for (var col = 0; col < cols; col++) ...[
                              if (col > 0) const SizedBox(width: 8),
                              Expanded(
                                child: row + col < nodes.length
                                    ? buildTile(nodes[row + col])
                                    : const SizedBox.shrink(),
                              ),
                            ],
                          ],
                        ),
                      ],
                    ],
                  );
                },
              ),
            ),
        ],
      )),
    );
  }

  Widget _buildActionPill({
    required Widget icon,
    required String label,
    required SingboardColors c,
    bool active = false,
    bool disabled = false,
    VoidCallback? onTap,
    String? tooltip,
  }) {
    final fg = active ? c.selectionInk : c.ink;
    Widget pill = Opacity(
      opacity: !active && disabled ? 0.45 : 1.0,
      child: Material(
        color: active ? c.primarySoft : c.fill,
        shape: StadiumBorder(
          side: BorderSide(
            color: active ? c.selectionBorder : c.hairline,
            width: active ? 0.85 : 0.65,
          ),
        ),
        clipBehavior: Clip.antiAlias,
        child: InkWell(
          onTap: disabled ? null : onTap,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 4.5),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                icon,
                const SizedBox(width: 4.5),
                Text(
                  label,
                  style: TextStyle(
                    fontSize: 12,
                    fontWeight: FontWeight.w500,
                    color: fg,
                    fontFeatures: const [FontFeature.tabularFigures()],
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
    if (tooltip != null) {
      pill = Tooltip(message: tooltip, child: pill);
    }
    return pill;
  }

  Widget _buildProxyProviderCard(Item item, SingboardColors c) {
    final name = item['name']?.toString() ?? '';
    final nodes = (item['proxies'] as List? ?? []).cast<Item>();
    final tested = nodes.where((n) => _delayOf(n) != null).length;
    final open = expandedProviders.contains(name);
    final vehicle = item['vehicleType']?.toString() ?? '';
    final updated = formatUpdatedRelative(item['updatedAt']);
    final sub = item['subscriptionInfo'] as Map?;
    final total = (sub?['Total'] as num?)?.toInt() ?? 0;
    final used = ((sub?['Download'] as num?)?.toInt() ?? 0) +
        ((sub?['Upload'] as num?)?.toInt() ?? 0);
    final rawTestUrl = item['testUrl']?.toString().trim() ?? '';
    final testUrl = rawTestUrl.isNotEmpty
        ? rawTestUrl
        : (widget.preferences['latencyTestUrl']?.toString() ?? '');
    final providerProgress = testingProviders[name];
    final ipv6Enabled = widget.preferences['ipv6TestEnabled'] == true;

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
                            const SizedBox(width: 4),
                            AnimatedRotation(turns: open ? .25 : 0, duration: const Duration(milliseconds: 200), child: PanelIcon('chevron', size: 14, color: c.muted)),
                          ],
                        ),
                        const SizedBox(height: 3),
                        Text(
                          '${nodes.length} 个节点 ($tested 已测试)',
                          style: TextStyle(fontSize: 12, color: c.muted),
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
                                _buildDot(
                                  _delayOf(node),
                                  false,
                                  c,
                                  testing: _isNodeTesting(node['name'].toString()),
                                ),
                            ],
                          ),
                        ],
                      ],
                    ),
                  ),
                  const SizedBox(width: 8),
                  Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      if (providerProgress != null)
                        _buildActionPill(
                          icon: SizedBox.square(
                            dimension: 11,
                            child: CircularProgressIndicator(
                              strokeWidth: 1.6,
                              color: c.selectionInk,
                            ),
                          ),
                          label: '${providerProgress.$1}/${providerProgress.$2}',
                          c: c,
                          active: true,
                          disabled: true,
                        )
                      else
                        _buildActionPill(
                          icon: PanelIcon(
                            'bolt',
                            size: 13,
                            color: c.ink.withValues(alpha: 0.78),
                          ),
                          label: '测速',
                          c: c,
                          tooltip: '健康检查',
                          onTap: () => testProvider(item),
                        ),
                      if (vehicle.toLowerCase() != 'inline') ...[
                        const SizedBox(width: 6),
                        Builder(
                          builder: (_) {
                            final itemUpdating = updatingProviders.contains(name);
                            final disabled =
                                itemUpdating || updatingAllProviders != null;
                            return _buildActionPill(
                              icon: SpinningPanelIcon(
                                'refresh',
                                size: 13,
                                color: itemUpdating
                                    ? c.selectionInk
                                    : c.ink.withValues(alpha: 0.78),
                                spinning: itemUpdating,
                              ),
                              label: itemUpdating ? '更新中' : '更新',
                              c: c,
                              active: itemUpdating,
                              disabled: disabled,
                              tooltip: itemUpdating ? '正在更新' : '更新',
                              onTap: () => updateProvider(item),
                            );
                          },
                        ),
                      ],
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
                builder: (context, constraints) {
                  final ts = MediaQuery.textScalerOf(context);
                  final scale = ts.scale(14) / 14;
                  final minColW = 128 * (1 + (scale - 1).clamp(0.0, 1.25) * 0.35);
                  final cols = ((constraints.maxWidth + 8) / (minColW + 8)).floor().clamp(1, 1000);
                  final tileHeight = (20.0 + 4.0 + ts.scale(14) * 1.25 * 2 + ts.scale(12) * 1.4 + 3.0).ceilToDouble();
                  Widget buildTile(Item node) {
                    final nName = node['name'].toString();
                    final delay = _delayOf(node);
                    final testing = _isNodeTesting(nName);
                    final (latBg, latFg) = _latencyBadge(delay, c, testing: testing);
                    final isV6 = ipv6Enabled && v6Nodes.contains(nName);
                    return Material(
                      color: Colors.transparent,
                      borderRadius: BorderRadius.circular(12),
                      clipBehavior: Clip.antiAlias,
                      child: InkWell(
                        borderRadius: BorderRadius.circular(12),
                        onTap: testing ? null : () => testNode(testUrl, nName),
                        child: Container(
                          height: tileHeight,
                          padding: const EdgeInsets.all(10),
                          decoration: BoxDecoration(
                            color: c.dark
                                ? const Color(0x14FFFFFF)
                                : const Color(0x38FFFFFF),
                            borderRadius: BorderRadius.circular(12),
                            border: Border.all(
                              color: testing ? c.selectionBorder : c.hairline,
                              width: testing ? 1 : 0.5,
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
                                  style: TextStyle(fontSize: 14, height: 1.2, color: c.ink),
                                ),
                              ),
                              const SizedBox(height: 4),
                              Row(
                                children: [
                                  Expanded(
                                    child: Text(
                                      _formatType(node, isV6),
                                      maxLines: 1,
                                      overflow: TextOverflow.ellipsis,
                                      style: TextStyle(fontSize: 12, color: c.muted),
                                    ),
                                  ),
                                  _buildLatencyBadge(
                                    delay,
                                    testing,
                                    latBg,
                                    latFg,
                                    fontWeight: FontWeight.w500,
                                  ),
                                ],
                              ),
                            ],
                          ),
                        ),
                      ),
                    );
                  }
                  return Column(
                    children: [
                      for (var row = 0; row < nodes.length; row += cols) ...[
                        if (row > 0) const SizedBox(height: 8),
                        Row(
                          children: [
                            for (var col = 0; col < cols; col++) ...[
                              if (col > 0) const SizedBox(width: 8),
                              Expanded(
                                child: row + col < nodes.length
                                    ? buildTile(nodes[row + col])
                                    : const SizedBox.shrink(),
                              ),
                            ],
                          ],
                        ),
                      ],
                    ],
                  );
                },
              ),
            ),
        ],
      )),
    );
  }

  Widget _buildActionCapsule(_RuleAction action, SingboardColors c, {bool compact = false}) {
    if (action.kind == 'route') {
      final Color bg;
      final Color fg;
      if (action.rejectFinal) {
        bg = c.dangerSoft;
        fg = c.dangerInk;
      } else if (action.direct) {
        bg = c.successSoft;
        fg = c.successInk;
      } else {
        bg = c.primarySoft;
        fg = c.selectionInk;
      }
      return Container(
        constraints: const BoxConstraints(minHeight: 24),
        padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 3),
        decoration: BoxDecoration(
          color: bg,
          borderRadius: BorderRadius.circular(999),
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            if (action.direct) ...[
              Container(
                width: 6,
                height: 6,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: c.success,
                ),
              ),
              const SizedBox(width: 5),
            ] else if (action.rejectFinal) ...[
              PanelIcon('reject', size: 12, color: c.dangerInk),
              const SizedBox(width: 4),
            ],
            Flexible(
              child: Text(
                action.label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  fontSize: 12,
                  fontWeight: FontWeight.w600,
                  color: fg,
                ),
              ),
            ),
            if (action.finalNode != null && !(compact && action.direct)) ...[
              const SizedBox(width: 4),
              Flexible(
                child: Text(
                  '› ${action.finalNode}',
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 12,
                    color: action.rejectFinal
                        ? c.dangerInk
                        : (action.direct ? c.successInk.withValues(alpha: 0.75) : c.muted),
                  ),
                ),
              ),
            ],
          ],
        ),
      );
    }
    final bg = switch (action.kind) {
      'reject' => c.dangerSoft,
      'stop' => c.fill,
      _ => c.fill.withValues(alpha: 0.45),
    };
    final fg = action.kind == 'reject' ? c.dangerInk : c.muted;
    return Container(
      constraints: const BoxConstraints(minHeight: 24),
      padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 3),
      decoration: BoxDecoration(
        color: bg,
        borderRadius: BorderRadius.circular(999),
        border: action.kind == 'pass'
            ? Border.all(color: c.hairline, width: 0.75)
            : null,
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

  Widget _buildRailDot(
    _RuleAction action,
    bool isFinal,
    SingboardColors c, {
    bool disabled = false,
  }) {
    if (disabled) {
      return Container(
        width: 9,
        height: 9,
        decoration: BoxDecoration(
          shape: BoxShape.circle,
          color: Colors.transparent,
          border: Border.all(color: c.muted.withValues(alpha: 0.45), width: 1.8),
        ),
      );
    }
    if (isFinal) {
      return Container(
        width: 10,
        height: 10,
        decoration: BoxDecoration(
          color: c.ink,
          borderRadius: BorderRadius.circular(3),
        ),
      );
    }
    if (action.kind == 'reject' || action.rejectFinal) {
      return Container(
        width: 14,
        height: 14,
        decoration: BoxDecoration(shape: BoxShape.circle, color: c.dangerSoft),
        alignment: Alignment.center,
        child: Container(
          width: 8,
          height: 8,
          decoration: BoxDecoration(shape: BoxShape.circle, color: c.error),
        ),
      );
    }
    if (action.direct) {
      return Container(
        width: 14,
        height: 14,
        decoration: BoxDecoration(shape: BoxShape.circle, color: c.successSoft),
        alignment: Alignment.center,
        child: Container(
          width: 8,
          height: 8,
          decoration: BoxDecoration(shape: BoxShape.circle, color: c.success),
        ),
      );
    }
    if (action.kind == 'route') {
      return Container(
        width: 14,
        height: 14,
        decoration: BoxDecoration(shape: BoxShape.circle, color: c.primarySoft),
        alignment: Alignment.center,
        child: Container(
          width: 8,
          height: 8,
          decoration: BoxDecoration(shape: BoxShape.circle, color: c.selectionInk),
        ),
      );
    }
    if (action.kind == 'pass') {
      return Container(
        width: 9,
        height: 9,
        decoration: BoxDecoration(
          shape: BoxShape.circle,
          color: Colors.transparent,
          border: Border.all(color: c.muted, width: 1.8),
        ),
      );
    }
    return Container(
      width: 9,
      height: 9,
      decoration: BoxDecoration(shape: BoxShape.circle, color: c.muted),
    );
  }

  Widget _buildTraceBanner(_RuleTrace trace, SingboardColors c) {
    final winnerIdx = trace.winner;
    if (winnerIdx == null || winnerIdx >= items.length) {
      return const SizedBox.shrink();
    }
    final winnerRule = items[winnerIdx];
    final isFinal = winnerRule['condition']?.toString() == '最终回退';
    final winnerAction = _parseAction(winnerRule);
    return Container(
      margin: const EdgeInsets.only(bottom: 10),
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 9),
      decoration: BoxDecoration(
        color: c.primarySoft,
        borderRadius: BorderRadius.circular(14),
        border: Border.all(color: c.selectionBorder, width: 1),
      ),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Text(
                '路由试算',
                style: TextStyle(
                  fontSize: 12,
                  fontWeight: FontWeight.w600,
                  color: c.selectionInk,
                ),
              ),
              const SizedBox(width: 6),
              Expanded(
                child: Text(
                  trace.query,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 12,
                    fontFamily: 'monospace',
                    fontWeight: FontWeight.w500,
                    color: c.ink,
                  ),
                ),
              ),
            ],
          ),
          const SizedBox(height: 6),
          Wrap(
            spacing: 6,
            runSpacing: 6,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              Text(
                isFinal ? '命中 最终回退' : '命中 第 #${winnerIdx + 1} 条',
                style: TextStyle(
                  fontSize: 12,
                  fontWeight: FontWeight.w600,
                  color: c.ink,
                ),
              ),
              _buildActionCapsule(winnerAction, c),
              if (trace.passed.isNotEmpty)
                Text(
                  '途经 ${trace.passed.length} 条预处理',
                  style: TextStyle(fontSize: 11, color: c.muted),
                ),
              if (trace.pending)
                Text(
                  '正在核验规则提供商…',
                  style: TextStyle(fontSize: 11, color: c.muted),
                ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _buildRuleRow(
    Item rule,
    int origIndex,
    int pos,
    int total,
    Map<String, int> providerCounts,
    _RuleTrace? trace,
    SingboardColors c,
  ) {
    final clauses = _parseClauses(rule);
    final action = _parseAction(rule);
    final isFinal = rule['condition']?.toString() == '最终回退';
    final uuid = _ruleUuid(rule);
    final isDisabled = _isRuleDisabled(rule);
    final isToggling = uuid.isNotEmpty && togglingRules.contains(uuid);
    final isFirst = pos == 0;
    final isLast = pos + 1 == total;
    final isWinner = trace?.winner == origIndex;
    final isPassed = trace?.passed.contains(origIndex) == true;
    final isShadowed = trace?.shadowed.contains(origIndex) == true;

    final chips = <Widget>[];
    for (final part in clauses) {
      switch (part.kind) {
        case 'op':
          chips.add(
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 2),
              child: Text(
                part.op,
                style: TextStyle(
                  fontSize: 11,
                  fontWeight: FontWeight.w500,
                  color: c.muted,
                ),
              ),
            ),
          );
        case 'final':
          chips.add(
            Text(
              '以上条件均未命中',
              style: TextStyle(fontSize: 12, color: c.muted),
            ),
          );
        case 'all':
          chips.add(
            Text(
              '所有流量',
              style: TextStyle(fontSize: 12, color: c.muted),
            ),
          );
        case 'sets':
          if (part.negated) {
            chips.add(
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                decoration: BoxDecoration(
                  color: c.dangerSoft,
                  borderRadius: BorderRadius.circular(6),
                ),
                child: Text(
                  '非',
                  style: TextStyle(
                    fontSize: 11,
                    fontWeight: FontWeight.w600,
                    color: c.dangerInk,
                  ),
                ),
              ),
            );
          }
          for (final (short, full) in part.sets) {
            final isHit = matched.contains(full);
            final provCount = providerCounts[full];
            final canOpen = readable && provCount != null;
            chips.add(
              GestureDetector(
                onTap: canOpen ? () => rulesTool(full) : null,
                child: Container(
                  constraints: const BoxConstraints(minHeight: 22),
                  padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 2),
                  decoration: BoxDecoration(
                    color: isHit ? c.primarySoft : c.fill,
                    borderRadius: BorderRadius.circular(999),
                    border: isHit
                        ? Border.all(color: c.selectionBorder, width: 0.75)
                        : null,
                  ),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      PanelIcon(
                        'layers',
                        size: 12,
                        color: isHit ? c.selectionInk : c.muted,
                      ),
                      const SizedBox(width: 4),
                      Text(
                        short,
                        style: TextStyle(
                          fontSize: 12,
                          fontWeight: FontWeight.w500,
                          color: isHit ? c.selectionInk : c.ink,
                        ),
                      ),
                      if (provCount != null && provCount > 0) ...[
                        const SizedBox(width: 4),
                        Text(
                          _formatCompactCount(provCount),
                          style: TextStyle(
                            fontSize: 11,
                            color: isHit
                                ? c.selectionInk.withValues(alpha: 0.8)
                                : c.muted,
                            fontFeatures: const [FontFeature.tabularFigures()],
                          ),
                        ),
                      ],
                    ],
                  ),
                ),
              ),
            );
          }
        case 'item':
          if (part.negated) {
            chips.add(
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
                decoration: BoxDecoration(
                  color: c.dangerSoft,
                  borderRadius: BorderRadius.circular(6),
                ),
                child: Text(
                  '非',
                  style: TextStyle(
                    fontSize: 11,
                    fontWeight: FontWeight.w600,
                    color: c.dangerInk,
                  ),
                ),
              ),
            );
          }
          final shownVals = part.values.take(2).toList();
          final hidden = part.values.length > 2 ? part.values.length - 2 : 0;
          chips.add(
            Container(
              constraints: const BoxConstraints(minHeight: 22),
              padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
              decoration: BoxDecoration(
                color: c.fill.withValues(alpha: 0.55),
                borderRadius: BorderRadius.circular(7),
                border: Border.all(color: c.hairline, width: 0.75),
              ),
              child: Row(
                mainAxisSize: MainAxisSize.min,
                children: [
                  if (part.label != null) ...[
                    Text(
                      part.label!,
                      style: TextStyle(fontSize: 12, color: c.muted),
                    ),
                    const SizedBox(width: 4),
                  ],
                  Flexible(
                    child: Text(
                      shownVals.join(', '),
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        fontSize: 12,
                        fontFamily: part.mono ? 'monospace' : null,
                        color: c.ink,
                      ),
                    ),
                  ),
                  if (hidden > 0) ...[
                    const SizedBox(width: 4),
                    Container(
                      padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 1),
                      decoration: BoxDecoration(
                        color: c.fillStrong,
                        borderRadius: BorderRadius.circular(4),
                      ),
                      child: Text(
                        '+$hidden',
                        style: TextStyle(
                          fontSize: 10,
                          color: c.muted,
                          fontFeatures: const [FontFeature.tabularFigures()],
                        ),
                      ),
                    ),
                  ],
                ],
              ),
            ),
          );
      }
    }

    Widget? traceTag;
    if (isWinner) {
      traceTag = Container(
        padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 2),
        decoration: BoxDecoration(
          color: c.successSoft,
          borderRadius: BorderRadius.circular(999),
        ),
        child: Text(
          '生效命中',
          style: TextStyle(
            fontSize: 11,
            fontWeight: FontWeight.w600,
            color: c.successInk,
          ),
        ),
      );
    } else if (isPassed) {
      traceTag = Container(
        padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 2),
        decoration: BoxDecoration(
          color: c.fill,
          borderRadius: BorderRadius.circular(999),
        ),
        child: Text(
          '路过',
          style: TextStyle(fontSize: 11, color: c.muted),
        ),
      );
    } else if (isShadowed && trace?.winner != null) {
      traceTag = Container(
        padding: const EdgeInsets.symmetric(horizontal: 7, vertical: 2),
        decoration: BoxDecoration(
          color: c.fill,
          borderRadius: BorderRadius.circular(999),
        ),
        child: Text(
          '已被 #${trace!.winner! + 1} 截断',
          style: TextStyle(fontSize: 11, color: c.muted),
        ),
      );
    }

    return Opacity(
      opacity: !isDisabled && isShadowed ? 0.58 : 1.0,
      child: Material(
        color: isWinner
            ? c.primarySoft
            : (isFinal ? c.fill.withValues(alpha: 0.7) : Colors.transparent),
        child: InkWell(
          onTap: () => inspectRule(origIndex, rule),
          child: Container(
            decoration: BoxDecoration(
              border: Border(
                top: isFirst
                    ? BorderSide.none
                    : BorderSide(
                        color: isWinner ? c.selectionBorder : c.hairline,
                        width: 0.75,
                      ),
              ),
            ),
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
            child: IntrinsicHeight(
              child: Row(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  SizedBox(
                    width: 16,
                    child: Column(
                      children: [
                        Container(
                          width: 1.5,
                          height: 4,
                          color: isFirst
                              ? Colors.transparent
                              : c.ink.withValues(alpha: 0.18),
                        ),
                        _buildRailDot(action, isFinal, c, disabled: isDisabled),
                        Expanded(
                          child: Container(
                            width: 1.5,
                            color: isLast
                                ? Colors.transparent
                                : c.ink.withValues(alpha: 0.18),
                          ),
                        ),
                      ],
                    ),
                  ),
                  const SizedBox(width: 10),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          children: [
                            Opacity(
                              opacity: isDisabled ? 0.45 : 1.0,
                              child: Text(
                                isFinal
                                    ? '回退'
                                    : '#${(origIndex + 1).toString().padLeft(2, '0')}',
                                style: TextStyle(
                                  fontSize: 11,
                                  fontWeight: FontWeight.w500,
                                  color: c.muted,
                                  fontFeatures: const [FontFeature.tabularFigures()],
                                ),
                              ),
                            ),
                            const Spacer(),
                            if (traceTag != null) ...[
                              Opacity(
                                opacity: isDisabled ? 0.45 : 1.0,
                                child: traceTag,
                              ),
                              const SizedBox(width: 6),
                            ],
                            Flexible(
                              flex: 3,
                              child: Align(
                                alignment: Alignment.centerRight,
                                child: Opacity(
                                  opacity: isDisabled ? 0.45 : 1.0,
                                  child: _buildActionCapsule(action, c, compact: true),
                                ),
                              ),
                            ),
                            if (uuid.isNotEmpty) ...[
                              const SizedBox(width: 8),
                              Tooltip(
                                message: isDisabled ? '重新启用此规则' : '临时禁用此规则',
                                child: GestureDetector(
                                  behavior: HitTestBehavior.opaque,
                                  onTap: () {},
                                  child: GlassToggle(
                                    value: !isDisabled,
                                    onChanged: isToggling ? null : (_) => toggleRule(rule),
                                  ),
                                ),
                              ),
                            ],
                          ],
                        ),
                        const SizedBox(height: 6),
                        Opacity(
                          opacity: isDisabled ? 0.45 : 1.0,
                          child: Wrap(
                            spacing: 5,
                            runSpacing: 5,
                            crossAxisAlignment: WrapCrossAlignment.center,
                            children: chips,
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
      ),
    );
  }

  Widget _buildRulesPanel(List<Item> shown, _RuleTrace? trace, SingboardColors c) {
    final providerCounts = <String, int>{
      for (final p in providerItems)
        if (p['name'] is String)
          p['name'] as String: (p['ruleCount'] as num?)?.toInt() ?? 0,
    };
    var leadingPre = 0;
    if (query.isEmpty) {
      while (leadingPre < shown.length && _parseAction(shown[leadingPre]).isPreprocessing) {
        leadingPre++;
      }
    }
    final hasPreCollapse = leadingPre > 0 && leadingPre < shown.length;
    final preSummary = <String>[];
    if (hasPreCollapse) {
      for (var i = 0; i < leadingPre; i++) {
        final lbl = _parseAction(shown[i]).label;
        if (!preSummary.contains(lbl)) preSummary.add(lbl);
      }
    }
    final visible = hasPreCollapse && !preExpanded
        ? shown.sublist(leadingPre)
        : shown;

    return GlassCard(
      radius: 18,
      padding: EdgeInsets.zero,
      child: ClipRRect(
        borderRadius: BorderRadius.circular(18),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (hasPreCollapse)
              Material(
                color: c.fill.withValues(alpha: 0.45),
                child: InkWell(
                  onTap: () => setState(() => preExpanded = !preExpanded),
                  child: Container(
                    padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
                    decoration: BoxDecoration(
                      border: Border(
                        bottom: BorderSide(color: c.hairline, width: 0.75),
                      ),
                    ),
                    child: Row(
                      children: [
                        AnimatedRotation(
                          turns: preExpanded ? 0.25 : 0,
                          duration: const Duration(milliseconds: 180),
                          child: PanelIcon('chevron', size: 13, color: c.muted),
                        ),
                        const SizedBox(width: 8),
                        Text(
                          '预处理规则 ($leadingPre)',
                          style: TextStyle(
                            fontSize: 12,
                            fontWeight: FontWeight.w600,
                            color: c.ink,
                          ),
                        ),
                        const SizedBox(width: 8),
                        Expanded(
                          child: Text(
                            preSummary.join(' · '),
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            style: TextStyle(fontSize: 12, color: c.muted),
                          ),
                        ),
                        const SizedBox(width: 8),
                        Text(
                          preExpanded ? '收起' : '展开',
                          style: TextStyle(
                            fontSize: 11,
                            fontWeight: FontWeight.w500,
                            color: c.selectionInk,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ),
            for (var pos = 0; pos < visible.length; pos++)
              _buildRuleRow(
                visible[pos],
                items.indexOf(visible[pos]),
                pos,
                visible.length,
                providerCounts,
                trace,
                c,
              ),
          ],
        ),
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
    final updated = formatUpdatedRelative(item['updatedAt']);
    final meta = [
      if (vLabel.isNotEmpty) vLabel,
      if (fLabel.isNotEmpty) fLabel,
      if (updated.isNotEmpty) '更新于 $updated',
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
              const SizedBox(width: 8),
              Builder(
                builder: (_) {
                  final itemUpdating = updatingProviders.contains(name);
                  final disabled = itemUpdating || updatingAllProviders != null;
                  return Tooltip(
                    message: itemUpdating ? '正在更新' : '更新',
                    child: Opacity(
                      opacity: !itemUpdating && disabled ? 0.45 : 1.0,
                      child: Material(
                        color: itemUpdating ? c.primarySoft : c.fill,
                        shape: CircleBorder(
                          side: BorderSide(
                            color: itemUpdating ? c.selectionBorder : c.hairline,
                            width: itemUpdating ? 0.85 : 0.65,
                          ),
                        ),
                        clipBehavior: Clip.antiAlias,
                        child: InkWell(
                          onTap: disabled ? null : () => updateProvider(item),
                          child: SizedBox(
                            width: 28,
                            height: 28,
                            child: SpinningPanelIcon(
                              'refresh',
                              size: 14,
                              color: itemUpdating
                                  ? c.selectionInk
                                  : c.ink.withValues(alpha: 0.78),
                              spinning: itemUpdating,
                            ),
                          ),
                        ),
                      ),
                    ),
                  );
                },
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
      if (!widget.proxies) {
        final clauses = _parseClauses(item);
        if (clauses.any((p) => p.kind == 'sets' && p.sets.any((s) => matched.contains(s.$2)))) {
          return true;
        }
        if (_isLookupQuery(query) && _evalRuleForLookup(item, query)) {
          return true;
        }
      }
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
    final trace = _computeTrace();
    final groupedRulesMode = !widget.proxies && !providers;

    final header = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            GlassBackdropHeading(
              widget.proxies ? '代理' : '规则',
              style: const TextStyle(
                fontSize: 26,
                fontWeight: FontWeight.w700,
                letterSpacing: -0.4,
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
                  _syncAgeTimer();
                  if (!widget.proxies) scheduleMatch();
                },
              ),
            )),
            if (providers) ...[
              const SizedBox(width: 8),
              Builder(
                builder: (_) {
                  final hasUpdatable = _updatableProviderNames().isNotEmpty;
                  final progress = updatingAllProviders;
                  final spinning = progress != null;
                  final disabled = spinning ||
                      updatingProviders.isNotEmpty ||
                      !hasUpdatable;
                  final fg = spinning ? c.selectionInk : c.ink;
                  return Opacity(
                    opacity: !spinning && disabled ? 0.45 : 1.0,
                    child: GestureDetector(
                      onTap: disabled ? null : updateAllProviders,
                      child: GlassControl(
                        radius: 999,
                        tint: spinning ? c.primarySoft : null,
                        borderColor: spinning ? c.selectionBorder : null,
                        child: Container(
                          constraints: const BoxConstraints(minHeight: 30),
                          padding: const EdgeInsets.symmetric(
                            horizontal: 10,
                            vertical: 5,
                          ),
                          child: Row(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              SpinningPanelIcon(
                                'refresh',
                                size: 13.5,
                                color: spinning
                                    ? c.selectionInk
                                    : c.ink.withValues(alpha: 0.78),
                                spinning: spinning,
                              ),
                              const SizedBox(width: 5),
                              Text(
                                progress != null
                                    ? '更新中 ${progress.$1}/${progress.$2}'
                                    : '全部更新',
                                style: TextStyle(
                                  fontSize: 12,
                                  fontWeight: FontWeight.w500,
                                  color: fg,
                                  fontFeatures: const [
                                    FontFeature.tabularFigures(),
                                  ],
                                ),
                              ),
                            ],
                          ),
                        ),
                      ),
                    ),
                  );
                },
              ),
            ],
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
        if (busy || matching)
          const Padding(
            padding: EdgeInsets.only(top: 8),
            child: LinearProgressIndicator(minHeight: 2),
          ),
        SizedBox(height: widget.proxies ? 12 : 8),
      ],
    );

    final hasPlaceholder = error.isNotEmpty || shown.isEmpty;
    final viewPad = MediaQuery.viewPaddingOf(context);
    final bottomPad = MediaQuery.paddingOf(context).bottom;
    final hasTraceBanner = groupedRulesMode && trace != null && trace.winner != null;
    final listCount = hasPlaceholder
        ? 2
        : groupedRulesMode
            ? (hasTraceBanner ? 3 : 2)
            : shown.length + 1;
    return ListView.builder(
      padding: EdgeInsets.fromLTRB(
        panelGutter(context) + viewPad.left,
        0,
        panelGutter(context) + viewPad.right,
        120 + bottomPad,
      ),
      itemCount: listCount,
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
                          const GlassBackdropPill(
                            '当前配置里没有规则提供商',
                            style: TextStyle(fontSize: 14),
                          ),
                          const SizedBox(height: 6),
                          const GlassBackdropPill(
                            '在 config.json 的 route.rule_set 中添加规则集后，这里会列出它们。',
                            muted: true,
                            textAlign: TextAlign.center,
                            style: TextStyle(fontSize: 12),
                          ),
                        ],
                      ),
                    )
                  : GlassBackdropPill(
                      busy
                          ? '正在读取…'
                          : query.isEmpty
                              ? '暂无记录'
                              : '没有找到匹配项',
                      muted: true,
                      style: const TextStyle(fontSize: 14),
                    ),
            ),
          );
        }
        if (groupedRulesMode) {
          if (hasTraceBanner && index == 1) {
            return _buildTraceBanner(trace, c);
          }
          return _buildRulesPanel(shown, trace, c);
        }
        final i = index - 1;
        final item = shown[i];
        final Widget card;
        if (providers) {
          card = widget.proxies
              ? _buildProxyProviderCard(item, c)
              : _buildRuleProviderCard(item, usageMap, c);
        } else {
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
  Timer? ageTimer;
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

  void _syncAgeTimer() {
    ageTimer?.cancel();
    ageTimer = null;
    if (!mounted) return;
    final dt = _parseUpdatedAt(widget.provider['updatedAt']);
    if (dt == null) return;
    final secs = DateTime.now().difference(dt).inSeconds;
    ageTimer = Timer(Duration(seconds: secs < 60 ? 1 : 30), () {
      if (!mounted) return;
      setState(() {});
      _syncAgeTimer();
    });
  }

  @override
  void initState() { super.initState(); load(); _syncAgeTimer(); }

  @override
  void dispose() { timer?.cancel(); ageTimer?.cancel(); search.dispose(); super.dispose(); }

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
          constraints: const BoxConstraints(minHeight: 26),
          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 3),
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
    final updated = formatUpdatedRelative(p['updatedAt']);
    return Dialog(backgroundColor: Colors.transparent, insetPadding: const EdgeInsets.symmetric(horizontal: 16),
      child: ConstrainedBox(constraints: BoxConstraints(maxWidth: 672, maxHeight: MediaQuery.sizeOf(context).height * .8),
        child: GlassCard(popover: true, child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Row(children: [
            PanelIcon(icon, size: 20, color: c.muted), const SizedBox(width: 8),
            Expanded(child: Text(name, maxLines: 2, overflow: TextOverflow.ellipsis, style: TextStyle(fontSize: 16, fontWeight: FontWeight.w600, color: c.ink))),
            IconButton(tooltip: '关闭详情', onPressed: () => Navigator.pop(context), icon: PanelIcon('close-detail', size: 14, color: c.muted)),
          ]),
          Text([source, "${p['ruleCount'] ?? 0} 条", if (updated.isNotEmpty) '更新于 $updated'].join(' · '), style: TextStyle(fontSize: 12, color: c.muted)),
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
                            showGlassToast(context, '已复制 $value', error: false);
                          },
                          child: Padding(
                            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 5),
                            child: Row(children: [
                              Container(
                                width: MediaQuery.textScalerOf(context).scale(72).clamp(72.0, 110.0),
                                constraints: const BoxConstraints(minHeight: 20),
                                padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 2),
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

class _RuleInspectDialog extends StatefulWidget {
  final int index;
  final Item rule;
  final List<_ClausePart> clauses;
  final _RuleAction action;
  final Map<String, int> providerCounts;
  final bool readable;
  final ValueChanged<String> onOpenProvider;
  final bool initialToggling;
  final Future<void> Function()? onToggleRule;

  const _RuleInspectDialog({
    required this.index,
    required this.rule,
    required this.clauses,
    required this.action,
    required this.providerCounts,
    required this.readable,
    required this.onOpenProvider,
    this.initialToggling = false,
    this.onToggleRule,
  });

  @override
  State<_RuleInspectDialog> createState() => _RuleInspectDialogState();
}

class _RuleInspectDialogState extends State<_RuleInspectDialog> {
  late bool _disabled;
  late bool _toggling;

  @override
  void initState() {
    super.initState();
    _disabled = widget.rule['disabled'] == true;
    _toggling = widget.initialToggling;
  }

  Future<void> _handleToggle() async {
    final toggle = widget.onToggleRule;
    if (toggle == null || _toggling) return;
    setState(() {
      _toggling = true;
      _disabled = !_disabled;
    });
    try {
      await toggle();
    } finally {
      if (mounted) {
        setState(() {
          _disabled = widget.rule['disabled'] == true;
          _toggling = false;
        });
      }
    }
  }

  String _formatCount(int n) {
    final s = n.toString();
    final buf = StringBuffer();
    for (var i = 0; i < s.length; i++) {
      if (i > 0 && (s.length - i) % 3 == 0) buf.write(',');
      buf.write(s[i]);
    }
    return buf.toString();
  }

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final index = widget.index;
    final rule = widget.rule;
    final clauses = widget.clauses;
    final action = widget.action;
    final providerCounts = widget.providerCounts;
    final readable = widget.readable;
    final onOpenProvider = widget.onOpenProvider;
    final isFinal = rule['condition']?.toString() == '最终回退';
    final rawPayload = (rule['payload']?.toString() ?? '').trim();
    final copyPayload = rawPayload.isEmpty
        ? (rule['condition']?.toString() ?? 'final')
        : rawPayload;
    final chain = (rule['chain'] as List? ?? []).whereType<String>().toList();
    final relatedSets = <String>[];
    for (final part in clauses) {
      if (part.kind == 'sets') {
        for (final s in part.sets) {
          if (!relatedSets.contains(s.$2)) relatedSets.add(s.$2);
        }
      }
    }

    Widget buildActionBadge() {
      if (action.kind == 'route') {
        final bg = action.rejectFinal
            ? c.dangerSoft
            : (action.direct ? c.successSoft : c.primarySoft);
        final fg = action.rejectFinal
            ? c.dangerInk
            : (action.direct ? c.successInk : c.selectionInk);
        return Container(
          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
          decoration: BoxDecoration(
            color: bg,
            borderRadius: BorderRadius.circular(999),
          ),
          child: Text(
            action.finalNode != null
                ? '${action.label} › ${action.finalNode}'
                : action.label,
            style: TextStyle(
              fontSize: 12,
              fontWeight: FontWeight.w600,
              color: fg,
            ),
          ),
        );
      }
      final bg = action.kind == 'reject' ? c.dangerSoft : c.fill;
      final fg = action.kind == 'reject' ? c.dangerInk : c.muted;
      return Container(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
        decoration: BoxDecoration(
          color: bg,
          borderRadius: BorderRadius.circular(999),
        ),
        child: Text(
          action.label,
          style: TextStyle(fontSize: 12, fontWeight: FontWeight.w500, color: fg),
        ),
      );
    }

    return Dialog(
      backgroundColor: Colors.transparent,
      insetPadding: const EdgeInsets.symmetric(horizontal: 16),
      child: ConstrainedBox(
        constraints: BoxConstraints(
          maxWidth: 580,
          maxHeight: MediaQuery.sizeOf(context).height * 0.8,
        ),
        child: GlassCard(
          popover: true,
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Container(
                    padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 3),
                    decoration: BoxDecoration(
                      color: c.fill,
                      borderRadius: BorderRadius.circular(6),
                    ),
                    child: Text(
                      isFinal
                          ? '最终回退'
                          : '#${(index + 1).toString().padLeft(2, '0')}',
                      style: TextStyle(
                        fontSize: 12,
                        fontWeight: FontWeight.w600,
                        color: c.muted,
                        fontFeatures: const [FontFeature.tabularFigures()],
                      ),
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Row(
                      children: [
                        Text(
                          '规则详情',
                          style: TextStyle(
                            fontSize: 16,
                            fontWeight: FontWeight.w700,
                            color: c.ink,
                          ),
                        ),
                        if (_disabled) ...[
                          const SizedBox(width: 8),
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
                              '已禁用',
                              style: TextStyle(
                                fontSize: 11,
                                fontWeight: FontWeight.w500,
                                color: c.muted,
                              ),
                            ),
                          ),
                        ],
                      ],
                    ),
                  ),
                  IconButton(
                    tooltip: '关闭详情',
                    onPressed: () => Navigator.pop(context),
                    icon: PanelIcon('close-detail', size: 14, color: c.muted),
                  ),
                ],
              ),
              const SizedBox(height: 8),
              Flexible(
                child: SingleChildScrollView(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      if (widget.onToggleRule != null) ...[
                        Container(
                          width: double.infinity,
                          padding: const EdgeInsets.symmetric(
                            horizontal: 12,
                            vertical: 10,
                          ),
                          decoration: BoxDecoration(
                            color: c.fill,
                            borderRadius: BorderRadius.circular(12),
                          ),
                          child: Row(
                            children: [
                              Expanded(
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Text(
                                      '启用此规则',
                                      style: TextStyle(
                                        fontSize: 12,
                                        fontWeight: FontWeight.w600,
                                        color: c.ink,
                                      ),
                                    ),
                                    const SizedBox(height: 2),
                                    Text(
                                      _disabled
                                          ? '已临时禁用，重启核心或重载配置后恢复'
                                          : '关闭后将临时跳过此规则，不修改配置文件',
                                      style: TextStyle(
                                        fontSize: 11,
                                        color: c.muted,
                                      ),
                                    ),
                                  ],
                                ),
                              ),
                              const SizedBox(width: 12),
                              GlassToggle(
                                value: !_disabled,
                                onChanged: _toggling ? null : (_) => _handleToggle(),
                              ),
                            ],
                          ),
                        ),
                        const SizedBox(height: 12),
                      ],
                      Text(
                        '匹配条件',
                        style: TextStyle(
                          fontSize: 12,
                          fontWeight: FontWeight.w500,
                          color: c.muted,
                        ),
                      ),
                      const SizedBox(height: 6),
                      Container(
                        width: double.infinity,
                        padding: const EdgeInsets.all(12),
                        decoration: BoxDecoration(
                          color: c.fill,
                          borderRadius: BorderRadius.circular(12),
                        ),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            for (var i = 0; i < clauses.length; i++) ...[
                              if (i > 0) const SizedBox(height: 6),
                              switch (clauses[i].kind) {
                                'op' => Padding(
                                    padding: const EdgeInsets.only(left: 2),
                                    child: Text(
                                      clauses[i].op == '且' ? 'AND · 且' : 'OR · 或',
                                      style: TextStyle(
                                        fontSize: 11,
                                        fontWeight: FontWeight.w600,
                                        color: c.selectionInk,
                                      ),
                                    ),
                                  ),
                                'final' => Text(
                                    '以上所有规则均未命中时进入此回退规则',
                                    style: TextStyle(fontSize: 12, color: c.ink),
                                  ),
                                'all' => Text(
                                    '匹配所有流量',
                                    style: TextStyle(fontSize: 12, color: c.ink),
                                  ),
                                'sets' => Wrap(
                                    spacing: 6,
                                    runSpacing: 6,
                                    crossAxisAlignment: WrapCrossAlignment.center,
                                    children: [
                                      if (clauses[i].negated)
                                        Container(
                                          padding: const EdgeInsets.symmetric(
                                            horizontal: 6,
                                            vertical: 2,
                                          ),
                                          decoration: BoxDecoration(
                                            color: c.dangerSoft,
                                            borderRadius: BorderRadius.circular(6),
                                          ),
                                          child: Text(
                                            '不满足',
                                            style: TextStyle(
                                              fontSize: 11,
                                              fontWeight: FontWeight.w600,
                                              color: c.dangerInk,
                                            ),
                                          ),
                                        ),
                                      Text(
                                        '规则集',
                                        style: TextStyle(
                                          fontSize: 12,
                                          fontWeight: FontWeight.w500,
                                          color: c.muted,
                                        ),
                                      ),
                                      for (final s in clauses[i].sets)
                                        Container(
                                          padding: const EdgeInsets.symmetric(
                                            horizontal: 8,
                                            vertical: 3,
                                          ),
                                          decoration: BoxDecoration(
                                            color: c.surface,
                                            borderRadius: BorderRadius.circular(6),
                                          ),
                                          child: Text(
                                            s.$2,
                                            style: TextStyle(
                                              fontSize: 12,
                                              fontFamily: 'monospace',
                                              color: c.ink,
                                            ),
                                          ),
                                        ),
                                    ],
                                  ),
                                _ => Wrap(
                                    spacing: 6,
                                    runSpacing: 6,
                                    crossAxisAlignment: WrapCrossAlignment.center,
                                    children: [
                                      if (clauses[i].negated)
                                        Container(
                                          padding: const EdgeInsets.symmetric(
                                            horizontal: 6,
                                            vertical: 2,
                                          ),
                                          decoration: BoxDecoration(
                                            color: c.dangerSoft,
                                            borderRadius: BorderRadius.circular(6),
                                          ),
                                          child: Text(
                                            '不满足',
                                            style: TextStyle(
                                              fontSize: 11,
                                              fontWeight: FontWeight.w600,
                                              color: c.dangerInk,
                                            ),
                                          ),
                                        ),
                                      if (clauses[i].label != null)
                                        Text(
                                          clauses[i].label!,
                                          style: TextStyle(
                                            fontSize: 12,
                                            fontWeight: FontWeight.w500,
                                            color: c.muted,
                                          ),
                                        ),
                                      for (final v in clauses[i].values)
                                        Container(
                                          padding: const EdgeInsets.symmetric(
                                            horizontal: 8,
                                            vertical: 3,
                                          ),
                                          decoration: BoxDecoration(
                                            color: c.surface,
                                            borderRadius: BorderRadius.circular(6),
                                          ),
                                          child: Text(
                                            v,
                                            style: TextStyle(
                                              fontSize: 12,
                                              fontFamily: clauses[i].mono ? 'monospace' : null,
                                              color: c.ink,
                                            ),
                                          ),
                                        ),
                                    ],
                                  ),
                              },
                            ],
                          ],
                        ),
                      ),
                      if (relatedSets.isNotEmpty) ...[
                        const SizedBox(height: 12),
                        Text(
                          '关联规则提供商',
                          style: TextStyle(
                            fontSize: 12,
                            fontWeight: FontWeight.w500,
                            color: c.muted,
                          ),
                        ),
                        const SizedBox(height: 6),
                        Wrap(
                          spacing: 6,
                          runSpacing: 6,
                          children: [
                            for (final tag in relatedSets)
                              GestureDetector(
                                onTap: readable && providerCounts.containsKey(tag)
                                    ? () {
                                        Navigator.pop(context);
                                        onOpenProvider(tag);
                                      }
                                    : null,
                                child: Container(
                                  padding: const EdgeInsets.symmetric(
                                    horizontal: 10,
                                    vertical: 6,
                                  ),
                                  decoration: BoxDecoration(
                                    color: c.fill,
                                    borderRadius: BorderRadius.circular(10),
                                  ),
                                  child: Row(
                                    mainAxisSize: MainAxisSize.min,
                                    children: [
                                      PanelIcon('layers', size: 13, color: c.muted),
                                      const SizedBox(width: 6),
                                      Flexible(
                                        child: Text(
                                          tag,
                                          style: TextStyle(
                                            fontSize: 12,
                                            fontWeight: FontWeight.w500,
                                            color: c.ink,
                                          ),
                                        ),
                                      ),
                                      const SizedBox(width: 6),
                                      Text(
                                        providerCounts.containsKey(tag)
                                            ? '${_formatCount(providerCounts[tag]!)} 条'
                                            : '未加载',
                                        style: TextStyle(
                                          fontSize: 11,
                                          color: c.muted,
                                          fontFeatures: const [
                                            FontFeature.tabularFigures(),
                                          ],
                                        ),
                                      ),
                                      if (readable && providerCounts.containsKey(tag)) ...[
                                        const SizedBox(width: 4),
                                        PanelIcon('chevron', size: 12, color: c.muted),
                                      ],
                                    ],
                                  ),
                                ),
                              ),
                          ],
                        ),
                      ],
                      const SizedBox(height: 12),
                      Text(
                        '执行动作与代理链',
                        style: TextStyle(
                          fontSize: 12,
                          fontWeight: FontWeight.w500,
                          color: c.muted,
                        ),
                      ),
                      const SizedBox(height: 6),
                      if (chain.length > 1)
                        Wrap(
                          spacing: 6,
                          runSpacing: 6,
                          crossAxisAlignment: WrapCrossAlignment.center,
                          children: [
                            for (var i = 0; i < chain.length; i++) ...[
                              if (i > 0)
                                PanelIcon('arrow-right', size: 12, color: c.muted),
                              Container(
                                padding: const EdgeInsets.symmetric(
                                  horizontal: 10,
                                  vertical: 4,
                                ),
                                decoration: BoxDecoration(
                                  color: i == 0 ? c.primarySoft : c.fill,
                                  borderRadius: BorderRadius.circular(999),
                                ),
                                child: Text(
                                  chain[i],
                                  style: TextStyle(
                                    fontSize: 12,
                                    fontWeight:
                                        i == 0 ? FontWeight.w600 : FontWeight.w500,
                                    color: i == 0 ? c.selectionInk : c.ink,
                                  ),
                                ),
                              ),
                            ],
                          ],
                        )
                      else
                        buildActionBadge(),
                      const SizedBox(height: 12),
                      Row(
                        children: [
                          Text(
                            '原始规则',
                            style: TextStyle(
                              fontSize: 12,
                              fontWeight: FontWeight.w500,
                              color: c.muted,
                            ),
                          ),
                          const Spacer(),
                          GestureDetector(
                            onTap: () {
                              Clipboard.setData(ClipboardData(text: copyPayload));
                              showGlassToast(context, '已复制原始规则', error: false);
                            },
                            child: Padding(
                              padding: const EdgeInsets.symmetric(
                                horizontal: 6,
                                vertical: 2,
                              ),
                              child: Text(
                                '复制',
                                style: TextStyle(
                                  fontSize: 12,
                                  fontWeight: FontWeight.w500,
                                  color: c.selectionInk,
                                ),
                              ),
                            ),
                          ),
                        ],
                      ),
                      const SizedBox(height: 6),
                      Container(
                        width: double.infinity,
                        padding: const EdgeInsets.symmetric(
                          horizontal: 10,
                          vertical: 8,
                        ),
                        decoration: BoxDecoration(
                          color: c.fill,
                          borderRadius: BorderRadius.circular(10),
                        ),
                        child: SelectableText(
                          copyPayload,
                          style: TextStyle(
                            fontSize: 12,
                            fontFamily: 'monospace',
                            color: c.muted,
                          ),
                        ),
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
