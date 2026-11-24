import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import 'config_page.dart';
import 'native_service_api.dart';

enum ServiceChipTone { ok, warn, error, muted }

class TailscaleController extends ChangeNotifier {
  static const _channel = MethodChannel('singboard/config');

  NativeServiceApi? _api;
  int _generation = 0;
  int? _statusHandle;
  Timer? _pollTimer;
  bool _draining = false;
  bool? _lastShareEnabled;

  String chipText = '未配置';
  ServiceChipTone chipTone = ServiceChipTone.muted;
  List<Map<String, dynamic>> endpoints = [];
  String? selectedEndpointTag;
  String? selectedServiceTag;
  List<Map<String, dynamic>> discoveredServices = [];
  bool connecting = false;

  bool get hasEndpoints => endpoints.isNotEmpty;

  int get unreadCount => endpoints.fold<int>(
    0,
    (sum, ep) => sum + ((ep['unreadFileCount'] as num?)?.toInt() ?? 0),
  );

  Map<String, dynamic>? get currentEndpoint {
    if (endpoints.isEmpty) return null;
    if (selectedEndpointTag != null) {
      for (final ep in endpoints) {
        if (ep['endpointTag'] == selectedEndpointTag) return ep;
      }
    }
    return endpoints.first;
  }

  String? get currentEndpointTag =>
      currentEndpoint?['endpointTag']?.toString();

  Future<NativeServiceApi> _ensureApi() async {
    if (_api != null) return _api!;
    final dir = await configDirectory();
    _api = NativeServiceApi(dir);
    return _api!;
  }

  Future<dynamic> request(
    String op, [
    Map<String, dynamic> args = const {},
  ]) async {
    final api = await _ensureApi();
    return api.request(op, args);
  }

  Future<List<Map<String, dynamic>>> discover() async {
    final raw = await request('discover');
    if (raw is! List) return const [];
    return raw
        .whereType<Map>()
        .map((e) => Map<String, dynamic>.from(e))
        .toList();
  }

  void selectEndpoint(String tag) {
    if (selectedEndpointTag == tag) return;
    selectedEndpointTag = tag;
    notifyListeners();
  }

  Future<void> sync({String? serviceTag}) async {
    final epoch = ++_generation;
    _pollTimer?.cancel();
    _pollTimer = null;
    _statusHandle = null;
    selectedServiceTag = serviceTag ?? selectedServiceTag;
    connecting = true;
    endpoints = [];
    chipText = '连接中';
    chipTone = ServiceChipTone.warn;
    notifyListeners();

    try {
      final api = await _ensureApi();
      await api.request('disconnect');
      final candidates = await discover();
      if (_generation != epoch) return;
      discoveredServices = candidates;
      if (!candidates.any((candidate) => candidate['serviceTag'] == selectedServiceTag)) {
        selectedServiceTag = null;
      }
      if (candidates.isEmpty) {
        connecting = false;
        endpoints = [];
        chipText = '未配置';
        chipTone = ServiceChipTone.muted;
        _syncShareTarget(false);
        notifyListeners();
        return;
      }
      if (selectedServiceTag == null && candidates.length > 1) {
        connecting = false;
        endpoints = [];
        chipText = '发现 ${candidates.length} 个服务';
        chipTone = ServiceChipTone.warn;
        _syncShareTarget(true);
        notifyListeners();
        return;
      }
      final chosen = selectedServiceTag != null
          ? candidates.firstWhere(
              (c) => c['serviceTag'] == selectedServiceTag,
              orElse: () => candidates.first,
            )
          : candidates.first;
      final tag = chosen['serviceTag']?.toString() ?? '';
      selectedServiceTag = tag;
      final url = chosen['url']?.toString() ?? '';
      await api.request('connect', {
        if (tag.isNotEmpty) 'serviceTag': tag,
      });
      if (_generation != epoch) return;
      final host = _displayHost(url);
      chipText = tag.isNotEmpty ? '$host (自动)' : host;
      chipTone = ServiceChipTone.ok;
      connecting = false;
      try {
        final snap = await api.request('status');
        if (_generation == epoch && snap is Map) {
          _applyStatus(Map<String, dynamic>.from(snap));
        }
      } catch (_) {}
      final handle = await api.request('open', {'feed': 'status'});
      if (_generation != epoch) return;
      _statusHandle = (handle as num?)?.toInt();
      _startPolling(epoch);
      notifyListeners();
    } on ServiceApiException catch (e) {
      if (_generation != epoch) return;
      connecting = false;
      endpoints = [];
      if (e.code == 'notConfigured') {
        chipText = '未配置';
        chipTone = ServiceChipTone.muted;
      } else {
        chipText = '不可达';
        chipTone = ServiceChipTone.error;
      }
      _syncShareTarget(discoveredServices.length > 1);
      notifyListeners();
    } catch (_) {
      if (_generation != epoch) return;
      connecting = false;
      endpoints = [];
      chipText = '不可达';
      chipTone = ServiceChipTone.error;
      _syncShareTarget(discoveredServices.length > 1);
      notifyListeners();
    }
  }

  void _startPolling(int epoch) {
    _pollTimer?.cancel();
    _pollTimer = Timer.periodic(const Duration(milliseconds: 900), (_) {
      _drainStatus(epoch);
    });
  }

  Future<void> _drainStatus(int epoch) async {
    if (_draining || _generation != epoch) return;
    final handle = _statusHandle;
    final api = _api;
    if (handle == null || api == null) return;
    _draining = true;
    try {
      final raw = await api.request('drain', {'handle': handle});
      if (_generation != epoch || raw is! Map) return;
      final events = (raw['events'] as List?) ?? const [];
      for (final ev in events) {
        if (ev is Map && ev['data'] is Map) {
          _applyStatus(Map<String, dynamic>.from(ev['data'] as Map));
        }
      }
      if (raw['finished'] == true) {
        _statusHandle = null;
      }
    } catch (_) {
    } finally {
      _draining = false;
    }
  }

  void _applyStatus(Map<String, dynamic> data) {
    final list = (data['endpoints'] as List? ?? const [])
        .whereType<Map>()
        .map((e) => Map<String, dynamic>.from(e))
        .toList();
    endpoints = list;
    if (list.isEmpty) {
      selectedEndpointTag = null;
    } else if (selectedEndpointTag == null ||
        !list.any((e) => e['endpointTag'] == selectedEndpointTag)) {
      selectedEndpointTag = list.first['endpointTag']?.toString();
    }
    _syncShareTarget(list.any((ep) => ep['canShareFiles'] == true) || discoveredServices.length > 1);
    notifyListeners();
  }

  void _syncShareTarget(bool enabled) {
    if (_lastShareEnabled == enabled) return;
    _lastShareEnabled = enabled;
    unawaited(
      _channel
          .invokeMethod('setTaildropShareEnabled', {'enabled': enabled})
          .catchError((Object _) {}),
    );
  }

  Future<void> setExitNode(String stableId) async {
    await request('setExitNode', {
      if (currentEndpointTag != null) 'tag': currentEndpointTag,
      'stableId': stableId,
    });
  }

  Future<void> logout() async {
    await request('logout', {
      if (currentEndpointTag != null) 'tag': currentEndpointTag,
    });
  }

  Future<Map<String, dynamic>> fetchCertificate(
    String domain,
    int minValiditySeconds,
  ) async {
    final raw = await request('certificate', {
      if (currentEndpointTag != null) 'tag': currentEndpointTag,
      'domain': domain,
      'minValiditySeconds': minValiditySeconds,
    });
    return Map<String, dynamic>.from(raw as Map);
  }

  Future<void> markInboxRead() async {
    await request('markInboxRead', {
      if (currentEndpointTag != null) 'tag': currentEndpointTag,
    });
  }

  Future<void> deleteFile(String name) async {
    await request('deleteFile', {
      if (currentEndpointTag != null) 'tag': currentEndpointTag,
      'name': name,
    });
  }

  Future<void> cancelReceiving(String senderId, String name) async {
    await request('cancelReceiving', {
      if (currentEndpointTag != null) 'tag': currentEndpointTag,
      'senderId': senderId,
      'name': name,
    });
  }

  Future<int> openFeed(Map<String, dynamic> args) async {
    final raw = await request('open', args);
    return (raw as num).toInt();
  }

  Future<Map<String, dynamic>> drain(int handle) async {
    final raw = await request('drain', {'handle': handle});
    return Map<String, dynamic>.from(raw as Map);
  }

  Future<void> closeFeed(int handle) async {
    try {
      await request('close', {'handle': handle});
    } catch (_) {}
  }

  Future<void> sendSshText(int handle, String text) async {
    final b64 = base64Encode(utf8.encode(text));
    await request('sshInput', {'handle': handle, 'data': b64});
  }

  Future<void> sendSshBytes(int handle, List<int> bytes) async {
    final b64 = base64Encode(bytes);
    await request('sshInput', {'handle': handle, 'data': b64});
  }

  @override
  void dispose() {
    _pollTimer?.cancel();
    _api?.close();
    _api = null;
    super.dispose();
  }

  static String _displayHost(String url) {
    final uri = Uri.tryParse(url);
    if (uri == null || uri.host.isEmpty) return url;
    return uri.hasPort ? '${uri.host}:${uri.port}' : uri.host;
  }
}

List<Map<String, dynamic>> tailscaleSortedGroups(Map<String, dynamic> endpoint) {
  final network = (endpoint['networkName']?.toString() ?? '').toLowerCase();
  final groups = (endpoint['userGroups'] as List? ?? const [])
      .whereType<Map>()
      .map((e) => Map<String, dynamic>.from(e))
      .toList();
  int priority(Map<String, dynamic> g) {
    final login = (g['loginName']?.toString() ?? '').toLowerCase();
    if (network.isNotEmpty && login == network) return 0;
    final peers = (g['peers'] as List?) ?? const [];
    final hasOwnPeer = peers.whereType<Map>().any((p) => p['shareeNode'] != true);
    return hasOwnPeer ? 1 : 2;
  }
  groups.sort((a, b) => priority(a).compareTo(priority(b)));
  return groups;
}

List<Map<String, dynamic>> tailscaleAllPeers(Map<String, dynamic> endpoint) {
  final result = <Map<String, dynamic>>[];
  for (final g in tailscaleSortedGroups(endpoint)) {
    final peers = (g['peers'] as List?) ?? const [];
    for (final p in peers) {
      if (p is Map) result.add(Map<String, dynamic>.from(p));
    }
  }
  return result;
}

String tailscalePeerDisplayName(Map<String, dynamic> peer) {
  final host = (peer['hostName']?.toString() ?? '').trim();
  if (host.isNotEmpty && host != 'device-of-shared-to-user') return host;
  final dns = (peer['dnsName']?.toString() ?? '').replaceAll(RegExp(r'\.+$'), '');
  if (dns.isNotEmpty) {
    final label = dns.split('.').first.trim();
    if (label.isNotEmpty && label != 'device-of-shared-to-user') return label;
  }
  if (host == 'device-of-shared-to-user') return '共享设备';
  return host.isNotEmpty ? host : '未命名';
}

List<Map<String, dynamic>> taildropTargets(Map<String, dynamic> endpoint) {
  if (endpoint['canShareFiles'] != true) return [];
  return tailscaleAllPeers(endpoint)
      .where((p) => p['canReceiveFiles'] == true && p['stableId'] is String && (p['stableId'] as String).isNotEmpty)
      .toList();
}

class TaildropUpload {
  final TailscaleController controller;
  final _closed = Completer<void>();
  bool _cancelled = false;

  TaildropUpload(this.controller);

  Future<void> cancel() {
    _cancelled = true;
    return _closed.future;
  }

  Future<bool> send({
    required String tag,
    required String peerId,
    required List<String> paths,
    required void Function(int bytes, int files) onProgress,
  }) async {
    int? handle;
    try {
      handle = await controller.openFeed({
        'feed': 'upload', 'tag': tag, 'peerStableId': peerId, 'paths': paths,
      });
      while (!_cancelled) {
        final batch = await controller.drain(handle);
        if (_cancelled) break;
        for (final event in (batch['events'] as List? ?? const []).whereType<Map>()) {
          if (event['error'] case final Map error) {
            throw StateError(error['message']?.toString() ?? '发送失败');
          }
          if (event['totalSentBytes'] case final num bytes) {
            onProgress(bytes.toInt(), (event['completedFiles'] as num).toInt());
          }
        }
        if (batch['finished'] == true) return true;
        await Future<void>.delayed(const Duration(milliseconds: 220));
      }
      return false;
    } finally {
      try {
        if (handle != null) await controller.closeFeed(handle);
      } finally {
        _closed.complete();
      }
    }
  }
}

String tailscalePrimaryIp(Map<String, dynamic> peer) {
  final ips = (peer['tailscaleIps'] as List?) ?? const [];
  return ips.isEmpty ? '' : ips.first.toString();
}

bool tailscaleHasSsh(Map<String, dynamic> peer) {
  final keys = (peer['sshHostKeys'] as List?) ?? const [];
  return keys.isNotEmpty;
}

String tailscaleFormatBytes(num bytes) {
  if (bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  var n = bytes.toDouble();
  var idx = 0;
  while (n >= 1024 && idx + 1 < units.length) {
    n /= 1024;
    idx++;
  }
  return idx == 0 ? '${bytes.toInt()} B' : '${n.toStringAsFixed(1)} ${units[idx]}';
}

String tailscaleRelativeTime(int unixSecs) {
  if (unixSecs <= 0) return '—';
  final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
  final diff = now - unixSecs;
  if (diff < 0) {
    final days = (-diff) ~/ 86400;
    if (days > 0) return '$days 天后过期';
    final hours = ((-diff) ~/ 3600).clamp(1, 24);
    return '$hours 小时后过期';
  }
  if (diff < 60) return '刚刚';
  if (diff < 3600) return '${diff ~/ 60} 分钟前';
  if (diff < 86400) return '${diff ~/ 3600} 小时前';
  return '${diff ~/ 86400} 天前';
}

String tailscaleStripAnsi(String raw) {
  return raw
      .replaceAll(RegExp(r'\x1B\[[0-9;?]*[ -/]*[@-~]'), '')
      .replaceAll(RegExp(r'\x1B\][^\x07\x1B]*(?:\x07|\x1B\\)'), '')
      .replaceAll('\r', '');
}
