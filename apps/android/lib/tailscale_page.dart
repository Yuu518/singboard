import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'config_page.dart';
import 'glass_ui.dart';
import 'native_stream.dart';
import 'tailscale_service.dart';

const _platformChannel = MethodChannel('singboard/config');

Future<void> _clearShare(String dir) async {
  await _platformChannel.invokeMethod<void>('clearStagedShare', {'batchDir': dir});
}

class TailscalePage extends StatefulWidget {
  final TailscaleController controller;
  final Map<String, dynamic> preferences;
  const TailscalePage({
    super.key,
    required this.controller,
    this.preferences = const {},
  });

  @override
  State<TailscalePage> createState() => _TailscalePageState();
}

class _TailscalePageState extends State<TailscalePage> {
  int _tab = 0;
  String _filter = 'all';
  String _query = '';
  final _collapsedGroups = <String>{};
  Future<void> _saveQueue = Future<void>.value();
  final _searchCtrl = TextEditingController();
  final _sshUserCtrl = TextEditingController(text: 'root');
  final _sshCmdCtrl = TextEditingController();

  int? _inboxHandle;
  String? _inboxTag;
  Map<String, dynamic>? _inbox;
  Timer? _streamTimer;

  String? _uploadTarget;
  TaildropUpload? _upload;
  bool _pickingUpload = false;
  String _uploadTargetName = '';
  int _uploadTotalFiles = 0;
  int _uploadCompletedFiles = 0;
  int _uploadSentBytes = 0;
  int _uploadTotalBytes = 0;

  int? _downloadHandle;
  String? _downloadTempPath;
  String _downloadName = '';
  int _downloadReceived = 0;
  int _downloadTotal = 0;

  String? _sshPeerId;
  int? _sshHandle;
  bool _sshReady = false;
  String _sshBanner = '';
  String _sshOutput = '';
  String _sshStatus = '';

  @override
  void initState() {
    super.initState();
    _collapsedGroups.addAll(
      (widget.preferences['tailscaleCollapsedGroups'] as List? ?? const [])
          .whereType<String>(),
    );
    widget.controller.addListener(_onControllerChanged);
    _streamTimer = Timer.periodic(
      const Duration(milliseconds: 250),
      (_) => _drainActiveStreams(),
    );
  }

  void _toggleGroupCollapsed(String groupKey) {
    final collapsed = !_collapsedGroups.contains(groupKey);
    setState(() {
      if (collapsed) {
        _collapsedGroups.add(groupKey);
      } else {
        _collapsedGroups.remove(groupKey);
      }
    });
    if (widget.preferences.isNotEmpty) {
      widget.preferences['tailscaleCollapsedGroups'] = _collapsedGroups.toList();
    }
    _saveQueue = _saveQueue.then((_) async {
      await configRequest(await configDirectory(), 'set_tailscale_group_collapsed', {
        'group': groupKey,
        'collapsed': collapsed,
      });
    }).catchError((Object _) {});
  }

  void _onControllerChanged() {
    if (!mounted) return;
    if (!widget.controller.connecting && !widget.controller.hasEndpoints &&
        ModalRoute.of(context)?.isCurrent == true) {
      Navigator.of(context).maybePop();
      return;
    }
    setState(() {});
  }

  @override
  void dispose() {
    widget.controller.removeListener(_onControllerChanged);
    _streamTimer?.cancel();
    if (_inboxHandle != null) unawaited(widget.controller.closeFeed(_inboxHandle!));
    if (_upload != null) unawaited(_upload!.cancel());
    if (_downloadHandle != null) unawaited(widget.controller.closeFeed(_downloadHandle!));
    if (_sshHandle != null) unawaited(widget.controller.closeFeed(_sshHandle!));
    _searchCtrl.dispose();
    _sshUserCtrl.dispose();
    _sshCmdCtrl.dispose();
    super.dispose();
  }

  void _toast(String msg, {bool error = false}) {
    if (!mounted) return;
    showGlassToast(context, msg, error: error);
  }

  Future<void> _ensureInbox() async {
    final tag = widget.controller.currentEndpointTag;
    if (tag == null) return;
    if (_inboxHandle != null && _inboxTag == tag) return;
    if (_inboxHandle != null) {
      await widget.controller.closeFeed(_inboxHandle!);
      _inboxHandle = null;
    }
    _inboxTag = tag;
    try {
      final ep = widget.controller.currentEndpoint;
      if (((ep?['unreadFileCount'] as num?)?.toInt() ?? 0) > 0) {
        await widget.controller.markInboxRead();
      }
      final h = await widget.controller.openFeed({'feed': 'inbox', 'tag': tag});
      if (mounted) setState(() => _inboxHandle = h);
    } catch (_) {}
  }

  Future<void> _drainActiveStreams() async {
    if (!mounted) return;
    if (_inboxHandle != null) {
      try {
        final raw = await widget.controller.drain(_inboxHandle!);
        final events = (raw['events'] as List?) ?? const [];
        for (final ev in events) {
          if (ev is Map && ev['data'] is Map && mounted) {
            setState(() => _inbox = Map<String, dynamic>.from(ev['data'] as Map));
          }
        }
        if (raw['finished'] == true) _inboxHandle = null;
      } catch (_) {}
    }
    if (_downloadHandle != null) {
      try {
        final raw = await widget.controller.drain(_downloadHandle!);
        final events = (raw['events'] as List?) ?? const [];
        String? err;
        var done = false;
        for (final ev in events) {
          if (ev is Map) {
            if (ev['data'] is Map) {
              final d = ev['data'] as Map;
              _downloadReceived = (d['receivedBytes'] as num?)?.toInt() ?? _downloadReceived;
              _downloadTotal = (d['size'] as num?)?.toInt() ?? _downloadTotal;
              if (d['completed'] == true) done = true;
            }
            if (ev['error'] is Map) {
              err = (ev['error'] as Map)['message']?.toString();
            }
          }
        }
        if (err != null) {
          _downloadHandle = null;
          _downloadTempPath = null;
          _toast('下载失败：$err', error: true);
        } else if (done || raw['finished'] == true) {
          final path = _downloadTempPath;
          final name = _downloadName;
          _downloadHandle = null;
          _downloadTempPath = null;
          if (path != null) {
            unawaited(_saveDownloadedFile(path, name));
          }
        }
        if (mounted) setState(() {});
      } catch (_) {}
    }
    if (_sshHandle != null) {
      try {
        final raw = await widget.controller.drain(_sshHandle!);
        final events = (raw['events'] as List?) ?? const [];
        var changed = false;
        for (final ev in events) {
          if (ev is! Map) continue;
          if (ev['output'] is String) {
            final bytes = base64Decode(ev['output'] as String);
            final text = tailscaleStripAnsi(utf8.decode(bytes, allowMalformed: true));
            _sshOutput += text;
            if (_sshOutput.length > 48 * 1024) {
              _sshOutput = _sshOutput.substring(_sshOutput.length - 32 * 1024);
            }
            changed = true;
          }
          if (ev['authBanner'] is String) {
            _sshBanner = ev['authBanner'] as String;
            changed = true;
          }
          if (ev['ready'] == true) {
            _sshReady = true;
            _sshStatus = '已连接 · xterm-256color';
            changed = true;
          }
          if (ev['exit'] is Map) {
            final code = (ev['exit'] as Map)['exitCode'] ?? 0;
            _sshReady = false;
            _sshStatus = '会话已退出 ($code)';
            changed = true;
          }
          if (ev['error'] is Map) {
            _sshReady = false;
            _sshStatus = 'SSH 错误：${(ev['error'] as Map)['message']}';
            changed = true;
          }
        }
        if (raw['finished'] == true) {
          _sshHandle = null;
          _sshReady = false;
          changed = true;
        }
        if (changed && mounted) setState(() {});
      } catch (_) {}
    }
  }

  Future<void> _saveDownloadedFile(String tempPath, String name) async {
    try {
      final saved = await _platformChannel.invokeMethod<bool>(
        'saveTaildropDownload',
        {'path': tempPath, 'name': name},
      );
      if (saved == true) _toast('已另存副本：$name');
    } catch (e) {
      _toast('另存失败：$e', error: true);
    }
  }

  Future<void> _pickAndUpload(Map<String, dynamic> ep) async {
    if (_pickingUpload || _upload != null) return;
    final peers = taildropTargets(ep);
    final peer = peers.where((p) => p['stableId'] == _uploadTarget).firstOrNull ?? peers.firstOrNull;
    if (peer == null) {
      _toast('没有可接收文件的设备', error: true);
      return;
    }
    final controller = widget.controller;
    final service = controller.selectedServiceTag;
    final tag = ep['endpointTag'] as String;
    String? dir;
    setState(() => _pickingUpload = true);
    try {
      final picked = await _platformChannel.invokeMethod<Map>('pickFiles');
      if (picked == null) return;
      dir = picked['batchDir'] as String;
      if (!mounted) return;
      if (controller.connecting || controller.selectedServiceTag != service) {
        throw StateError('Service API 已切换，请重新选择文件');
      }
      final files = (picked['files'] as List).whereType<Map>().toList();
      if (files.isEmpty) return;
      final upload = TaildropUpload(controller);
      setState(() {
        _upload = upload;
        _uploadTargetName = tailscalePeerDisplayName(peer);
        _uploadTotalFiles = files.length;
        _uploadCompletedFiles = 0;
        _uploadSentBytes = 0;
        _uploadTotalBytes = files.fold<int>(0, (sum, f) => sum + (f['size'] as num).toInt());
      });
      final sent = await upload.send(
        tag: tag,
        peerId: peer['stableId'] as String,
        paths: files.map((f) => f['path'] as String).toList(),
        onProgress: (bytes, count) {
          if (mounted) setState(() {
            _uploadSentBytes = bytes;
            _uploadCompletedFiles = count;
          });
        },
      );
      if (sent) _toast('已发送到 $_uploadTargetName');
    } catch (e) {
      _toast('发送文件失败：$e', error: true);
    } finally {
      if (mounted) setState(() {
        _upload = null;
        _pickingUpload = false;
      });
      if (dir != null) await _clearShare(dir);
    }
  }

  Future<void> _startDownload(String name, int size) async {
    try {
      final tempPath = await _platformChannel.invokeMethod<String>(
        'taildropDownloadTemp',
        {'name': name},
      );
      if (tempPath == null || !mounted) return;
      final h = await widget.controller.openFeed({
        'feed': 'download',
        if (widget.controller.currentEndpointTag != null)
          'tag': widget.controller.currentEndpointTag,
        'name': name,
        'destination': tempPath,
      });
      if (!mounted) return;
      setState(() {
        _downloadHandle = h;
        _downloadTempPath = tempPath;
        _downloadName = name;
        _downloadReceived = 0;
        _downloadTotal = size;
      });
    } catch (e) {
      _toast('开始下载失败：$e', error: true);
    }
  }

  Future<void> _toggleSsh(Map<String, dynamic> ep) async {
    if (_sshHandle != null) {
      await widget.controller.closeFeed(_sshHandle!);
      if (mounted) {
        setState(() {
          _sshHandle = null;
          _sshReady = false;
          _sshStatus = '已断开';
        });
      }
      return;
    }
    final peers = tailscaleAllPeers(ep).where(tailscaleHasSsh).toList();
    final peerId = _sshPeerId ?? peers.firstOrNull?['stableId']?.toString();
    if (peerId == null || peerId.isEmpty) {
      _toast('请选择支持 SSH 的设备', error: true);
      return;
    }
    final user = _sshUserCtrl.text.trim().isEmpty ? 'root' : _sshUserCtrl.text.trim();
    setState(() {
      _sshBanner = '';
      _sshOutput = '';
      _sshReady = false;
      _sshStatus = '正在连接 SSH…';
    });
    try {
      final h = await widget.controller.openFeed({
        'feed': 'ssh',
        if (widget.controller.currentEndpointTag != null)
          'tag': widget.controller.currentEndpointTag,
        'peerStableId': peerId,
        'username': user,
        'columns': 64,
        'rows': 22,
      });
      if (mounted) setState(() => _sshHandle = h);
    } catch (e) {
      if (mounted) setState(() => _sshStatus = '连接失败：$e');
      _toast('SSH 连接失败：$e', error: true);
    }
  }

  Future<void> _showCertDialog(List<String> domains) async {
    final c = SingboardColors.of(context);
    var domain = domains.first;
    var days = 0;
    var loading = false;
    var certPem = '';
    var keyPem = '';
    var revealKey = false;
    var error = '';

    await showGlassDialog<void>(
      context: context,
      builder: (ctx) => StatefulBuilder(
        builder: (ctx, setLocal) => Dialog(
          backgroundColor: Colors.transparent,
          child: GlassCard(
            popover: true,
            padding: const EdgeInsets.all(18),
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Row(
                    children: [
                      Expanded(
                        child: Text(
                          'Tailscale TLS 证书',
                          style: TextStyle(
                            fontSize: 16,
                            fontWeight: FontWeight.w600,
                            color: c.ink,
                          ),
                        ),
                      ),
                      TextButton(
                        onPressed: () => Navigator.pop(ctx),
                        child: const Text('关闭'),
                      ),
                    ],
                  ),
                  const SizedBox(height: 8),
                  Text('证书域名', style: TextStyle(fontSize: 12, color: c.muted)),
                  const SizedBox(height: 6),
                  Wrap(
                    spacing: 6,
                    runSpacing: 6,
                    children: [
                      for (final d in domains)
                        GestureDetector(
                          onTap: () => setLocal(() => domain = d),
                          child: Container(
                            padding: const EdgeInsets.symmetric(
                              horizontal: 10,
                              vertical: 5,
                            ),
                            decoration: BoxDecoration(
                              color: d == domain ? c.primarySoft : c.fill,
                              borderRadius: BorderRadius.circular(999),
                            ),
                            child: Text(
                              d,
                              style: TextStyle(
                                fontSize: 12,
                                fontFamily: 'monospace',
                                color: d == domain ? c.selectionInk : c.ink,
                              ),
                            ),
                          ),
                        ),
                    ],
                  ),
                  const SizedBox(height: 12),
                  Row(
                    children: [
                      Text('最短剩余有效期', style: TextStyle(fontSize: 12, color: c.muted)),
                      const Spacer(),
                      for (final (d, label) in const [(0, '默认'), (7, '7 天'), (30, '30 天')])
                        Padding(
                          padding: const EdgeInsets.only(left: 6),
                          child: GestureDetector(
                            onTap: () => setLocal(() => days = d),
                            child: Container(
                              padding: const EdgeInsets.symmetric(
                                horizontal: 10,
                                vertical: 4,
                              ),
                              decoration: BoxDecoration(
                                color: d == days ? c.primarySoft : c.fill,
                                borderRadius: BorderRadius.circular(999),
                              ),
                              child: Text(
                                label,
                                style: TextStyle(
                                  fontSize: 12,
                                  color: d == days ? c.selectionInk : c.muted,
                                ),
                              ),
                            ),
                          ),
                        ),
                    ],
                  ),
                  if (error.isNotEmpty) ...[
                    const SizedBox(height: 10),
                    Text(error, style: TextStyle(fontSize: 12, color: c.dangerInk)),
                  ],
                  if (certPem.isNotEmpty) ...[
                    const SizedBox(height: 12),
                    _pemPreviewBox(c, 'Certificate PEM', certPem, false, null),
                    const SizedBox(height: 8),
                    _pemPreviewBox(
                      c,
                      'Private Key PEM',
                      keyPem,
                      !revealKey,
                      () => setLocal(() => revealKey = !revealKey),
                    ),
                  ],
                  const SizedBox(height: 14),
                  FilledButton(
                    onPressed: loading
                        ? null
                        : () async {
                            setLocal(() {
                              loading = true;
                              error = '';
                            });
                            try {
                              final res = await widget.controller
                                  .fetchCertificate(domain, days * 86400);
                              setLocal(() {
                                certPem = res['certificatePem']?.toString() ?? '';
                                keyPem = res['privateKeyPem']?.toString() ?? '';
                              });
                            } catch (e) {
                              setLocal(() => error = e.toString());
                            } finally {
                              setLocal(() => loading = false);
                            }
                          },
                    style: FilledButton.styleFrom(
                      backgroundColor: c.primary,
                      foregroundColor: Colors.white,
                      shape: const StadiumBorder(),
                    ),
                    child: Text(loading ? '正在获取证书…' : '获取证书'),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }

  Widget _pemPreviewBox(
    SingboardColors c,
    String title,
    String pem,
    bool masked,
    VoidCallback? onToggle,
  ) => Container(
    padding: const EdgeInsets.all(10),
    decoration: BoxDecoration(
      color: c.fill,
      borderRadius: BorderRadius.circular(12),
    ),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Expanded(
              child: Text(
                title,
                style: TextStyle(fontSize: 12, fontWeight: FontWeight.w500, color: c.ink),
              ),
            ),
            if (onToggle != null)
              GestureDetector(
                onTap: onToggle,
                child: Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 6),
                  child: Text(
                    masked ? '显示' : '隐藏',
                    style: TextStyle(fontSize: 11, color: c.muted),
                  ),
                ),
              ),
            GestureDetector(
              onTap: () {
                Clipboard.setData(ClipboardData(text: pem));
                _toast('已复制 $title');
              },
              child: Text(
                '复制',
                style: TextStyle(fontSize: 11, fontWeight: FontWeight.w600, color: c.selectionInk),
              ),
            ),
          ],
        ),
        const SizedBox(height: 4),
        Text(
          masked
              ? '••••••••••••••••••••••••••••••••••••'
              : pem.split('\n').take(2).join(' '),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: TextStyle(fontSize: 11, fontFamily: 'monospace', color: c.muted),
        ),
      ],
    ),
  );

  Future<void> _openPeerSheet(
    Map<String, dynamic> peer,
    String activeExitId,
  ) async {
    await showGlassSheet<void>(
      context: context,
      builder: (_) => _PeerDetailSheet(
        controller: widget.controller,
        peer: peer,
        isExit: peer['stableId'] == activeExitId && activeExitId.isNotEmpty,
        onExitToggle: (id) async {
          try {
            await widget.controller.setExitNode(id);
            _toast(id.isEmpty ? '已停用出口节点' : '已切换出口节点');
          } catch (e) {
            _toast('设置出口失败：$e', error: true);
          }
        },
        onOpenSsh: (id) {
          setState(() {
            _sshPeerId = id;
            _tab = 2;
          });
        },
        onOpenSend: (id) {
          setState(() {
            _uploadTarget = id;
            _tab = 1;
          });
          unawaited(_ensureInbox());
        },
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final scope = GlassScope.of(context);
    final ep = widget.controller.currentEndpoint;
    final eps = widget.controller.endpoints;

    return GlassBackdrop(
      child: Scaffold(
        backgroundColor: Colors.transparent,
        body: Column(
          children: [
            GlassSubpageHeader(
              title: 'Tailscale',
              trailing: ep != null && (ep['certDomains'] as List? ?? const []).isNotEmpty
                  ? Material(
                      color: Colors.transparent,
                      child: InkWell(
                        borderRadius: BorderRadius.circular(999),
                        onTap: () => _showCertDialog(
                          (ep['certDomains'] as List)
                              .map((e) => e.toString())
                              .toList(),
                        ),
                        child: Container(
                          padding: const EdgeInsets.symmetric(
                            horizontal: 12,
                            vertical: 5,
                          ),
                          decoration: GlassSubpageHeader.pillDecoration(c, scope),
                          child: Text(
                            'TLS 证书',
                            style: TextStyle(
                              fontSize: 12,
                              height: 16 / 12,
                              fontWeight: FontWeight.w500,
                              color: c.ink,
                            ),
                          ),
                        ),
                      ),
                    )
                  : null,
            ),
            Expanded(
              child: ep == null
                  ? Center(
                      child: Text(
                        '未检测到已配置的 Tailscale 端点',
                        style: TextStyle(color: c.muted),
                      ),
                    )
                  : _buildBody(c, ep, eps),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildBody(
    SingboardColors c,
    Map<String, dynamic> ep,
    List<Map<String, dynamic>> eps,
  ) {
    final tag = ep['endpointTag']?.toString() ?? '';
    final backendState = ep['backendState']?.toString() ?? '';
    final stateText = ep['stateText']?.toString() ?? '';
    final authUrl = ep['authUrl']?.toString() ?? '';
    final networkName = ep['networkName']?.toString() ?? '';
    final selfPeer = (ep['self'] as Map?)?.cast<String, dynamic>() ?? const {};
    final exitNode = (ep['exitNode'] as Map?)?.cast<String, dynamic>() ?? const {};
    final canShare = ep['canShareFiles'] == true;
    final peers = tailscaleAllPeers(ep);
    final onlineCount = peers.where((p) => p['online'] == true).length;
    final activeExitId = exitNode['stableId']?.toString() ?? '';
    final activeExitName = activeExitId.isEmpty ? '' : tailscalePeerDisplayName(exitNode);

    final needsLogin = backendState == 'NeedsLogin' || authUrl.isNotEmpty;

    return ListView(
      padding: EdgeInsets.fromLTRB(
        panelGutter(context),
        10,
        panelGutter(context),
        MediaQuery.paddingOf(context).bottom + 24,
      ),
      children: [
        if (eps.length > 1) ...[
          Builder(
            builder: (context) {
              final scope = GlassScope.of(context);
              final selChip = scope.adaptiveSelectionChip(c);
              return SingleChildScrollView(
                scrollDirection: Axis.horizontal,
                child: Row(
                  children: [
                    for (final item in eps)
                      Padding(
                        padding: const EdgeInsets.only(right: 8),
                        child: GestureDetector(
                          onTap: () {
                            widget.controller.selectEndpoint(
                              item['endpointTag'].toString(),
                            );
                            _inboxTag = null;
                            _inbox = null;
                            if (_tab == 1) unawaited(_ensureInbox());
                          },
                          child: GlassControl(
                            radius: 999,
                            bordered: false,
                            tint: item['endpointTag'] == tag
                                ? selChip.bg
                                : scope.adaptivePillBg(c),
                            child: Container(
                              padding: const EdgeInsets.symmetric(
                                horizontal: 12,
                                vertical: 6,
                              ),
                              decoration: BoxDecoration(
                                borderRadius: BorderRadius.circular(999),
                                border: Border.all(
                                  color: item['endpointTag'] == tag
                                      ? selChip.border
                                      : scope.adaptivePillBorder(c),
                                ),
                              ),
                              child: Text(
                                item['endpointTag'].toString(),
                                style: TextStyle(
                                  fontSize: 12,
                                  fontWeight: FontWeight.w600,
                                  color: item['endpointTag'] == tag
                                      ? selChip.ink
                                      : c.ink,
                                ),
                              ),
                            ),
                          ),
                        ),
                      ),
                  ],
                ),
              );
            },
          ),
          const SizedBox(height: 12),
        ],
        if (needsLogin)
          GlassCard(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    Container(
                      width: 8,
                      height: 8,
                      decoration: BoxDecoration(
                        shape: BoxShape.circle,
                        color: c.warning,
                      ),
                    ),
                    const SizedBox(width: 8),
                    Text(
                      '等待节点授权${stateText.isEmpty ? '' : ' · $stateText'}',
                      style: TextStyle(
                        fontSize: 14,
                        fontWeight: FontWeight.w600,
                        color: c.ink,
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 8),
                Text(
                  '端点 $tag 需要完成 Tailscale 登录授权。授权完成后设备列表将自动刷新。',
                  style: TextStyle(fontSize: 12, color: c.muted),
                ),
                if (authUrl.isNotEmpty) ...[
                  const SizedBox(height: 12),
                  Container(
                    width: double.infinity,
                    padding: const EdgeInsets.all(10),
                    decoration: BoxDecoration(
                      color: c.fill,
                      borderRadius: BorderRadius.circular(12),
                    ),
                    child: Text(
                      authUrl,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        fontSize: 12,
                        fontFamily: 'monospace',
                        color: c.ink,
                      ),
                    ),
                  ),
                  const SizedBox(height: 12),
                  Wrap(
                    spacing: 8,
                    children: [
                      FilledButton(
                        onPressed: () => _platformChannel
                            .invokeMethod('openUrl', {'url': authUrl}),
                        style: FilledButton.styleFrom(
                          backgroundColor: c.primary,
                          foregroundColor: Colors.white,
                          shape: const StadiumBorder(),
                        ),
                        child: const Text('在浏览器中打开'),
                      ),
                      TextButton(
                        onPressed: () {
                          Clipboard.setData(ClipboardData(text: authUrl));
                          _toast('已复制授权链接');
                        },
                        child: const Text('复制链接'),
                      ),
                    ],
                  ),
                ],
              ],
            ),
          )
        else ...[
          _buildHeroCard(
            c,
            selfPeer,
            networkName,
            backendState,
            stateText,
            activeExitId,
            activeExitName,
            peers,
          ),
          const SizedBox(height: 12),
          GlassCard(
            radius: 999,
            padding: const EdgeInsets.all(4),
            child: Row(
              children: [
                _segTab(c, 0, '设备 ($onlineCount/${peers.length})', null),
                _segTab(c, 1, 'Taildrop', null),
                _segTab(c, 2, 'SSH 终端', _sshReady ? '1' : null),
              ],
            ),
          ),
          const SizedBox(height: 12),
          if (_tab == 0) _buildDevicesTab(c, ep, activeExitId),
          if (_tab == 1) _buildTaildropTab(c, ep, canShare),
          if (_tab == 2) _buildSshTab(c, ep),
        ],
      ],
    );
  }

  Widget _segTab(SingboardColors c, int index, String label, String? badge) {
    final active = _tab == index;
    final sel = GlassScope.of(context).adaptiveSelectionChip(c);
    return Expanded(
      child: GestureDetector(
        onTap: () {
          setState(() => _tab = index);
          if (index == 1) unawaited(_ensureInbox());
        },
        child: Container(
          padding: const EdgeInsets.symmetric(vertical: 8),
          decoration: BoxDecoration(
            color: active ? sel.bg : Colors.transparent,
            borderRadius: BorderRadius.circular(999),
            border: active ? Border.all(color: sel.border) : null,
          ),
          child: Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Text(
                label,
                style: TextStyle(
                  fontSize: 12,
                  fontWeight: active ? FontWeight.w600 : FontWeight.w500,
                  color: active ? sel.ink : c.ink.withValues(alpha: 0.78),
                ),
              ),
              if (badge != null) ...[
                const SizedBox(width: 4),
                Container(
                  padding: const EdgeInsets.symmetric(horizontal: 5, vertical: 1),
                  decoration: BoxDecoration(
                    color: c.primary,
                    borderRadius: BorderRadius.circular(999),
                  ),
                  child: Text(
                    badge,
                    style: const TextStyle(fontSize: 10, color: Colors.white),
                  ),
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildHeroCard(
    SingboardColors c,
    Map<String, dynamic> selfPeer,
    String networkName,
    String backendState,
    String stateText,
    String activeExitId,
    String activeExitName,
    List<Map<String, dynamic>> peers,
  ) {
    final selfName = tailscalePeerDisplayName(selfPeer);
    final selfHost = selfName == '未命名' ? '本机' : selfName;
    final selfIp = tailscalePrimaryIp(selfPeer);
    final selfDns =
        (selfPeer['dnsName']?.toString() ?? '').replaceAll(RegExp(r'\.$'), '');
    final exitChoices = peers.where((p) => p['exitNodeOption'] == true).toList();
    final scope = GlassScope.of(context);
    final networkInk = c.dark ? const Color(0xFF6EE787) : const Color(0xFF116329);

    return GlassCard(
      padding: const EdgeInsets.all(14),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Container(
                width: 9,
                height: 9,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: backendState == 'Running' ? c.success : c.warning,
                ),
              ),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  selfHost,
                  style: TextStyle(
                    fontSize: 15,
                    fontWeight: FontWeight.w600,
                    color: c.ink,
                  ),
                ),
              ),
              Container(
                padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 3),
                decoration: BoxDecoration(
                  color: scope.adaptivePillBg(c, tintOverlay: c.successSoft),
                  borderRadius: BorderRadius.circular(999),
                  border: Border.all(
                    color: scope.adaptivePillBorder(c, accent: c.success),
                  ),
                ),
                child: Text(
                  networkName.isNotEmpty
                      ? networkName
                      : (stateText.isNotEmpty ? stateText : backendState),
                  style: TextStyle(
                    fontSize: 11,
                    fontWeight: FontWeight.w600,
                    color: networkInk,
                  ),
                ),
              ),
            ],
          ),
          const SizedBox(height: 8),
          Wrap(
            spacing: 6,
            runSpacing: 6,
            children: [
              if (selfIp.isNotEmpty)
                GestureDetector(
                  onTap: () {
                    Clipboard.setData(ClipboardData(text: selfIp));
                    _toast('已复制 $selfIp');
                  },
                  child: Container(
                    padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
                    decoration: BoxDecoration(
                      color: scope.adaptivePillBg(c),
                      borderRadius: BorderRadius.circular(999),
                      border: Border.all(color: scope.adaptivePillBorder(c)),
                    ),
                    child: Text(
                      selfIp,
                      style: TextStyle(
                        fontSize: 12,
                        fontWeight: FontWeight.w500,
                        fontFamily: 'monospace',
                        color: c.ink,
                      ),
                    ),
                  ),
                ),
              if (selfDns.isNotEmpty)
                GestureDetector(
                  onTap: () {
                    Clipboard.setData(ClipboardData(text: selfDns));
                    _toast('已复制 MagicDNS');
                  },
                  child: Container(
                    padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
                    decoration: BoxDecoration(
                      color: scope.adaptivePillBg(c),
                      borderRadius: BorderRadius.circular(999),
                      border: Border.all(color: scope.adaptivePillBorder(c)),
                    ),
                    child: Text(
                      selfDns,
                      style: TextStyle(
                        fontSize: 12,
                        fontFamily: 'monospace',
                        color: c.ink,
                      ),
                    ),
                  ),
                ),
            ],
          ),
          const SizedBox(height: 12),
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 10),
            decoration: BoxDecoration(
              color: c.fill,
              borderRadius: BorderRadius.circular(14),
              border: Border.all(color: c.hairline),
            ),
            child: Row(
              children: [
                if (activeExitId.isNotEmpty) ...[
                  Container(
                    width: 7,
                    height: 7,
                    decoration: BoxDecoration(
                      shape: BoxShape.circle,
                      color: c.success,
                    ),
                  ),
                  const SizedBox(width: 6),
                ],
                Text('出口节点', style: TextStyle(fontSize: 12, color: c.muted)),
                const Spacer(),
                if (exitChoices.isEmpty && activeExitId.isEmpty)
                  Text('无可用出口节点', style: TextStyle(fontSize: 12, color: c.muted))
                else
                  PopupMenuButton<String>(
                    initialValue: activeExitId,
                    onSelected: (id) async {
                      if (id == activeExitId) return;
                      try {
                        await widget.controller.setExitNode(id);
                        _toast(id.isEmpty ? '已停用出口节点' : '已切换出口节点');
                      } catch (e) {
                        _toast('操作失败：$e', error: true);
                      }
                    },
                    itemBuilder: (_) {
                      final items = <(String, String)>[
                        ('', '不使用出口节点'),
                        for (final p in exitChoices)
                          (
                            p['stableId'].toString(),
                            p['online'] == true
                                ? tailscalePeerDisplayName(p)
                                : '${tailscalePeerDisplayName(p)} (离线)',
                          ),
                      ];
                      if (activeExitId.isNotEmpty &&
                          !items.any((it) => it.$1 == activeExitId)) {
                        items.add((
                          activeExitId,
                          activeExitName.isEmpty ? activeExitId : activeExitName,
                        ));
                      }
                      return [
                        for (final (id, label) in items)
                          PopupMenuItem<String>(
                            value: id,
                            child: Row(
                              children: [
                                Expanded(
                                  child: Text(
                                    label,
                                    style: TextStyle(
                                      fontWeight: id == activeExitId
                                          ? FontWeight.w600
                                          : FontWeight.w400,
                                      color: id == activeExitId ? c.selectionInk : c.ink,
                                    ),
                                  ),
                                ),
                                if (id == activeExitId) ...[
                                  const SizedBox(width: 8),
                                  Text(
                                    '✓',
                                    style: TextStyle(
                                      fontWeight: FontWeight.w700,
                                      color: c.selectionInk,
                                    ),
                                  ),
                                ],
                              ],
                            ),
                          ),
                      ];
                    },
                    child: Container(
                      padding: const EdgeInsets.symmetric(
                        horizontal: 11,
                        vertical: 5,
                      ),
                      decoration: BoxDecoration(
                        color: activeExitId.isNotEmpty
                            ? scope.adaptiveSelectionChip(c).bg
                            : scope.adaptivePillBg(c),
                        borderRadius: BorderRadius.circular(999),
                        border: Border.all(
                          color: activeExitId.isNotEmpty
                              ? scope.adaptiveSelectionChip(c).border
                              : scope.adaptivePillBorder(c),
                        ),
                      ),
                      child: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          ConstrainedBox(
                            constraints: const BoxConstraints(maxWidth: 160),
                            child: Text(
                              activeExitId.isEmpty ? '不使用出口节点' : activeExitName,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: TextStyle(
                                fontSize: 12,
                                fontWeight: FontWeight.w600,
                                color: activeExitId.isNotEmpty
                                    ? scope.adaptiveSelectionChip(c).ink
                                    : c.ink,
                              ),
                            ),
                          ),
                          const SizedBox(width: 4),
                          Transform.rotate(
                            angle: math.pi / 2,
                            child: PanelIcon(
                              'chevron',
                              size: 11,
                              color: activeExitId.isNotEmpty
                                  ? scope.adaptiveSelectionChip(c).ink
                                  : c.muted,
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildDevicesTab(
    SingboardColors c,
    Map<String, dynamic> ep,
    String activeExitId,
  ) {
    final groups = tailscaleSortedGroups(ep);
    final q = _query.trim().toLowerCase();

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        GlassCard(
          padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 2),
          child: TextField(
            controller: _searchCtrl,
            onChanged: (v) => setState(() => _query = v),
            style: TextStyle(fontSize: 13, color: c.ink),
            decoration: InputDecoration(
              border: InputBorder.none,
              isDense: true,
              hintText: '搜索设备、IP、系统…',
              hintStyle: TextStyle(fontSize: 13, color: c.muted),
            ),
          ),
        ),
        const SizedBox(height: 8),
        Builder(
          builder: (context) {
            final scope = GlassScope.of(context);
            final selChip = scope.adaptiveSelectionChip(c);
            return Wrap(
              spacing: 6,
              children: [
                for (final (id, label) in const [
                  ('all', '全部'),
                  ('online', '在线'),
                  ('exit', '出口'),
                  ('ssh', 'SSH'),
                ])
                  GestureDetector(
                    onTap: () => setState(() => _filter = id),
                    child: GlassControl(
                      radius: 999,
                      bordered: false,
                      tint: _filter == id ? selChip.bg : scope.adaptivePillBg(c),
                      child: Container(
                        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
                        decoration: BoxDecoration(
                          borderRadius: BorderRadius.circular(999),
                          border: Border.all(
                            color: _filter == id
                                ? selChip.border
                                : scope.adaptivePillBorder(c),
                          ),
                        ),
                        child: Text(
                          label,
                          style: TextStyle(
                            fontSize: 12,
                            fontWeight: _filter == id ? FontWeight.w600 : FontWeight.w500,
                            color: _filter == id ? selChip.ink : c.ink,
                          ),
                        ),
                      ),
                    ),
                  ),
              ],
            );
          },
        ),
        const SizedBox(height: 10),
        GlassCard(
          padding: const EdgeInsets.all(10),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              for (final group in groups) ..._buildGroupSection(c, group, q, activeExitId),
            ],
          ),
        ),
      ],
    );
  }

  List<Widget> _buildGroupSection(
    SingboardColors c,
    Map<String, dynamic> group,
    String q,
    String activeExitId,
  ) {
    final peers = (group['peers'] as List? ?? const [])
        .whereType<Map>()
        .map((e) => Map<String, dynamic>.from(e))
        .where((p) {
          if (_filter == 'online' && p['online'] != true) return false;
          if (_filter == 'exit' && p['exitNodeOption'] != true) return false;
          if (_filter == 'ssh' && !tailscaleHasSsh(p)) return false;
          if (q.isNotEmpty) {
            final host = tailscalePeerDisplayName(p).toLowerCase();
            final os = (p['os']?.toString() ?? '').toLowerCase();
            final ip = tailscalePrimaryIp(p).toLowerCase();
            if (!host.contains(q) && !os.contains(q) && !ip.contains(q)) return false;
          }
          return true;
        })
        .toList();
    if (peers.isEmpty) return const [];
    final displayName = group['displayName']?.toString() ?? '';
    final loginName = group['loginName']?.toString() ?? '';
    final title = displayName.isNotEmpty
        ? displayName
        : (loginName.isNotEmpty ? loginName : '设备组');
    final groupKey = loginName.isNotEmpty ? loginName : title;
    final collapsed = q.isEmpty && _collapsedGroups.contains(groupKey);
    final onlineCount = peers.where((p) => p['online'] == true).length;

    return [
      InkWell(
        borderRadius: BorderRadius.circular(10),
        onTap: () => _toggleGroupCollapsed(groupKey),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(8, 7, 8, 7),
          child: Row(
            children: [
              Transform.rotate(
                angle: collapsed ? 0 : math.pi / 2,
                child: PanelIcon('chevron', size: 12, color: c.muted),
              ),
              const SizedBox(width: 6),
              Expanded(
                child: Row(
                  children: [
                    Flexible(
                      child: Text(
                        title,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          fontSize: 12,
                          fontWeight: FontWeight.w600,
                          color: c.ink,
                        ),
                      ),
                    ),
                    if (loginName.isNotEmpty && loginName != title) ...[
                      const SizedBox(width: 6),
                      Flexible(
                        child: Text(
                          loginName,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(fontSize: 11, color: c.muted),
                        ),
                      ),
                    ],
                  ],
                ),
              ),
              const SizedBox(width: 6),
              Text(
                '$onlineCount/${peers.length} 在线',
                style: TextStyle(fontSize: 11, color: c.muted),
              ),
            ],
          ),
        ),
      ),
      if (!collapsed)
        for (final peer in peers)
        InkWell(
          borderRadius: BorderRadius.circular(14),
          onTap: () => _openPeerSheet(peer, activeExitId),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 9),
            child: Row(
              children: [
                Container(
                  width: 8,
                  height: 8,
                  decoration: BoxDecoration(
                    shape: BoxShape.circle,
                    color: peer['online'] == true ? c.success : c.muted.withValues(alpha: 0.4),
                  ),
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          Flexible(
                            child: Text(
                              tailscalePeerDisplayName(peer),
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: TextStyle(
                                fontSize: 13,
                                fontWeight: FontWeight.w600,
                                color: c.ink,
                              ),
                            ),
                          ),
                          if ((peer['os']?.toString() ?? '').isNotEmpty) ...[
                            const SizedBox(width: 6),
                            Container(
                              padding: const EdgeInsets.symmetric(
                                horizontal: 6,
                                vertical: 1,
                              ),
                              decoration: BoxDecoration(
                                color: c.fill,
                                borderRadius: BorderRadius.circular(6),
                              ),
                              child: Text(
                                peer['os'].toString(),
                                style: TextStyle(fontSize: 10, color: c.muted),
                              ),
                            ),
                          ],
                          if (peer['stableId'] == activeExitId &&
                              activeExitId.isNotEmpty) ...[
                            const SizedBox(width: 6),
                            Container(
                              padding: const EdgeInsets.symmetric(
                                horizontal: 6,
                                vertical: 1,
                              ),
                              decoration: BoxDecoration(
                                color: c.primarySoft,
                                borderRadius: BorderRadius.circular(999),
                              ),
                              child: Text(
                                '出口中',
                                style: TextStyle(
                                  fontSize: 10,
                                  fontWeight: FontWeight.w600,
                                  color: c.selectionInk,
                                ),
                              ),
                            ),
                          ],
                          if (peer['shareeNode'] == true) ...[
                            const SizedBox(width: 6),
                            Container(
                              padding: const EdgeInsets.symmetric(
                                horizontal: 6,
                                vertical: 1,
                              ),
                              decoration: BoxDecoration(
                                color: c.udpSoft,
                                borderRadius: BorderRadius.circular(999),
                              ),
                              child: Text(
                                '共享',
                                style: TextStyle(
                                  fontSize: 10,
                                  fontWeight: FontWeight.w500,
                                  color: c.udp,
                                ),
                              ),
                            ),
                          ],
                        ],
                      ),
                      const SizedBox(height: 2),
                      Text(
                        _peerMetaLine(peer),
                        style: TextStyle(
                          fontSize: 11,
                          fontFamily: 'monospace',
                          color: c.muted,
                        ),
                      ),
                    ],
                  ),
                ),
                PanelIcon('chevron', size: 14, color: c.muted),
              ],
            ),
          ),
        ),
    ];
  }

  String _peerMetaLine(Map<String, dynamic> peer) {
    final ip = tailscalePrimaryIp(peer);
    if (peer['online'] != true) {
      final seen = tailscaleRelativeTime((peer['lastSeen'] as num?)?.toInt() ?? 0);
      return ip.isEmpty ? seen : '$ip · $seen';
    }
    final rx = (peer['rxBytes'] as num?)?.toInt() ?? 0;
    final tx = (peer['txBytes'] as num?)?.toInt() ?? 0;
    if (rx > 0 || tx > 0) {
      final traffic = '↓ ${tailscaleFormatBytes(rx)} · ↑ ${tailscaleFormatBytes(tx)}';
      return ip.isEmpty ? traffic : '$ip · $traffic';
    }
    return ip;
  }

  Widget _buildTaildropTab(
    SingboardColors c,
    Map<String, dynamic> ep,
    bool canShare,
  ) {
    final targets = taildropTargets(ep);
    final selectedTarget = (targets.where((p) => p['stableId'] == _uploadTarget).firstOrNull ?? targets.firstOrNull)?['stableId'];
    final receiving = (_inbox?['receiving'] as List? ?? const [])
        .whereType<Map>()
        .map((e) => Map<String, dynamic>.from(e))
        .toList();
    final files = (_inbox?['files'] as List? ?? const [])
        .whereType<Map>()
        .map((e) => Map<String, dynamic>.from(e))
        .toList();

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        GlassCard(
          padding: const EdgeInsets.all(14),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Text(
                '发送文件 (Taildrop)',
                style: TextStyle(
                  fontSize: 14,
                  fontWeight: FontWeight.w600,
                  color: c.ink,
                ),
              ),
              const SizedBox(height: 8),
              if (!canShare)
                Text('当前 Tailnet 未启用 Taildrop 文件传输。',
                    style: TextStyle(fontSize: 12, color: c.muted))
              else ...[
                if (targets.isEmpty)
                  Text(
                    '当前 Tailnet 中没有可接收文件的设备。',
                    style: TextStyle(fontSize: 12, color: c.muted),
                  )
                else
                  Builder(
                    builder: (context) {
                      final scope = GlassScope.of(context);
                      final selChip = scope.adaptiveSelectionChip(c);
                      return Wrap(
                        spacing: 6,
                        runSpacing: 6,
                        children: [
                          for (final t in targets)
                            Builder(
                              builder: (_) {
                                final selected = t['stableId'] == selectedTarget;
                                final online = t['online'] == true;
                                return GestureDetector(
                                  onTap: () => setState(
                                    () => _uploadTarget = t['stableId']?.toString(),
                                  ),
                                  child: Container(
                                    padding: const EdgeInsets.symmetric(
                                      horizontal: 11,
                                      vertical: 6,
                                    ),
                                    decoration: BoxDecoration(
                                      color: selected ? selChip.bg : scope.adaptivePillBg(c),
                                      borderRadius: BorderRadius.circular(999),
                                      border: Border.all(
                                        color: selected
                                            ? selChip.border
                                            : scope.adaptivePillBorder(c),
                                      ),
                                    ),
                                    child: Row(
                                      mainAxisSize: MainAxisSize.min,
                                      children: [
                                        Container(
                                          width: 7,
                                          height: 7,
                                          decoration: BoxDecoration(
                                            shape: BoxShape.circle,
                                            color: online ? c.success : c.muted,
                                          ),
                                        ),
                                        const SizedBox(width: 6),
                                        Text(
                                          tailscalePeerDisplayName(t),
                                          style: TextStyle(
                                            fontSize: 12,
                                            fontWeight: selected
                                                ? FontWeight.w600
                                                : FontWeight.w500,
                                            color: selected ? selChip.ink : c.ink,
                                          ),
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
                if (_upload != null) ...[
                  const SizedBox(height: 10),
                  Text(
                    '正在发送到 $_uploadTargetName · $_uploadCompletedFiles/$_uploadTotalFiles (${tailscaleFormatBytes(_uploadSentBytes)} / ${tailscaleFormatBytes(_uploadTotalBytes)})',
                    style: TextStyle(fontSize: 12, color: c.muted),
                  ),
                  const SizedBox(height: 6),
                  ClipRRect(
                    borderRadius: BorderRadius.circular(999),
                    child: LinearProgressIndicator(
                      value: _uploadTotalBytes > 0
                          ? (_uploadSentBytes / _uploadTotalBytes).clamp(0.0, 1.0)
                          : null,
                      minHeight: 5,
                    ),
                  ),
                ],
                const SizedBox(height: 12),
                Builder(
                  builder: (context) {
                    final scope = GlassScope.of(context);
                    final busy = _pickingUpload || _upload != null;
                    final disabled = targets.isEmpty || busy;
                    final targetPeer = targets
                        .where((p) => p['stableId'] == selectedTarget)
                        .firstOrNull;
                    final targetLabel = targetPeer == null
                        ? null
                        : tailscalePeerDisplayName(targetPeer);
                    return Opacity(
                      opacity: disabled ? 0.48 : 1.0,
                      child: Material(
                        color: Colors.transparent,
                        child: InkWell(
                          borderRadius: BorderRadius.circular(16),
                          onTap: disabled ? null : () => _pickAndUpload(ep),
                          child: Container(
                            padding: const EdgeInsets.symmetric(
                              horizontal: 16,
                              vertical: 18,
                            ),
                            decoration: BoxDecoration(
                              color: c.fill,
                              borderRadius: BorderRadius.circular(16),
                              border: Border.all(
                                color: scope.adaptivePillBorder(c),
                              ),
                            ),
                            child: Column(
                              children: [
                                Container(
                                  width: 34,
                                  height: 34,
                                  decoration: BoxDecoration(
                                    color: scope.adaptivePillBg(c),
                                    shape: BoxShape.circle,
                                    border: Border.all(
                                      color: scope.adaptivePillBorder(c),
                                    ),
                                  ),
                                  child: Center(
                                    child: PanelIcon('upload', size: 16, color: c.ink),
                                  ),
                                ),
                                const SizedBox(height: 8),
                                Text(
                                  busy
                                      ? '正在发送文件…'
                                      : (targets.isEmpty
                                          ? '暂无可接收设备'
                                          : '拖拽文件到此处，或点击选择'),
                                  textAlign: TextAlign.center,
                                  style: TextStyle(
                                    fontSize: 13,
                                    fontWeight: FontWeight.w600,
                                    color: c.ink,
                                  ),
                                ),
                                const SizedBox(height: 3),
                                Text(
                                  targetLabel != null && !busy
                                      ? '支持多选、分屏拖入或系统分享 · 将发送到 $targetLabel'
                                      : '支持多选、分屏拖入或从其他应用分享',
                                  textAlign: TextAlign.center,
                                  style: TextStyle(fontSize: 11, color: c.muted),
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
        ),
        const SizedBox(height: 12),
        GlassCard(
          padding: const EdgeInsets.all(14),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Row(
                children: [
                  Expanded(
                    child: Text(
                      '收件箱',
                      style: TextStyle(
                        fontSize: 14,
                        fontWeight: FontWeight.w600,
                        color: c.ink,
                      ),
                    ),
                  ),
                  Text(
                    _inbox == null ? '等待读取收件箱' : '已接收 ${files.length} 个文件',
                    style: TextStyle(fontSize: 12, color: c.muted),
                  ),
                ],
              ),
              if (files.isNotEmpty) ...[
                const SizedBox(height: 6),
                Text(
                  '文件已保存在此端点的 Taildrop 接收目录。',
                  style: TextStyle(fontSize: 12, color: c.muted),
                ),
              ],
              for (final item in receiving) ...[
                const SizedBox(height: 10),
                Container(
                  padding: const EdgeInsets.all(10),
                  decoration: BoxDecoration(
                    color: c.fill,
                    borderRadius: BorderRadius.circular(12),
                  ),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Row(
                        children: [
                          Expanded(
                            child: Text(
                              item['name']?.toString() ?? '',
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: TextStyle(
                                fontSize: 12,
                                fontWeight: FontWeight.w600,
                                color: c.ink,
                              ),
                            ),
                          ),
                          TextButton(
                            onPressed: () => widget.controller.cancelReceiving(
                              item['senderId']?.toString() ?? '',
                              item['name']?.toString() ?? '',
                            ),
                            child: Text('取消', style: TextStyle(color: c.dangerInk, fontSize: 12)),
                          ),
                        ],
                      ),
                      Text(
                        '正在接收 · ${tailscaleFormatBytes((item['receivedBytes'] as num?) ?? 0)} / ${tailscaleFormatBytes((item['size'] as num?) ?? 0)}',
                        style: TextStyle(fontSize: 12, color: c.muted),
                      ),
                      ClipRRect(
                        borderRadius: BorderRadius.circular(999),
                        child: LinearProgressIndicator(
                          value: ((item['size'] as num?) ?? 0) > 0
                              ? (((item['receivedBytes'] as num?) ?? 0) /
                                      ((item['size'] as num?) ?? 1))
                                  .clamp(0.0, 1.0)
                              : null,
                          minHeight: 5,
                          color: c.success,
                        ),
                      ),
                    ],
                  ),
                ),
              ],
              if (_inbox != null && files.isEmpty && receiving.isEmpty)
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 20),
                  child: Center(
                    child: Text(
                      '收件箱为空',
                      style: TextStyle(fontSize: 12, color: c.muted),
                    ),
                  ),
                )
              else
                for (final f in files) ...[
                  const SizedBox(height: 10),
                  Divider(height: 0.5, thickness: 0.5, color: c.hairline),
                  const SizedBox(height: 10),
                  Row(
                    children: [
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              f['name']?.toString() ?? '',
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: TextStyle(
                                fontSize: 13,
                                fontWeight: FontWeight.w500,
                                color: c.ink,
                              ),
                            ),
                            const SizedBox(height: 2),
                            Text(
                              '${f['senderName'] ?? 'Taildrop'} · ${tailscaleFormatBytes((f['size'] as num?) ?? 0)} · ${tailscaleRelativeTime((f['modifiedAt'] as num?)?.toInt() ?? 0)}',
                              style: TextStyle(fontSize: 11, color: c.muted),
                            ),
                          ],
                        ),
                      ),
                      TextButton(
                        onPressed: _downloadHandle != null
                            ? null
                            : () => _startDownload(
                                  f['name']?.toString() ?? '',
                                  (f['size'] as num?)?.toInt() ?? 0,
                                ),
                        child: Text(
                          _downloadHandle != null &&
                                  _downloadName == (f['name']?.toString() ?? '')
                              ? (_downloadTotal > 0 && _downloadReceived > 0
                                  ? '另存中 ${((_downloadReceived / _downloadTotal).clamp(0.0, 1.0) * 100).round()}%'
                                  : '另存中…')
                              : '另存为',
                        ),
                      ),
                      TextButton(
                        onPressed: () async {
                          try {
                            await widget.controller.deleteFile(f['name']?.toString() ?? '');
                            _toast('已从 Taildrop 接收目录删除文件');
                          } catch (e) {
                            _toast('删除失败：$e', error: true);
                          }
                        },
                        child: Text('删除原文件', style: TextStyle(color: c.dangerInk)),
                      ),
                    ],
                  ),
                ],
            ],
          ),
        ),
      ],
    );
  }

  Widget _buildSshTab(SingboardColors c, Map<String, dynamic> ep) {
    final peers = tailscaleAllPeers(ep).where(tailscaleHasSsh).toList();
    final selectedPeer = _sshPeerId ?? peers.firstOrNull?['stableId']?.toString();
    final hasSshPeers = peers.isNotEmpty;

    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        GlassCard(
          padding: const EdgeInsets.all(12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (!hasSshPeers)
                Container(
                  padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
                  decoration: BoxDecoration(
                    color: c.fill,
                    borderRadius: BorderRadius.circular(999),
                  ),
                  child: Text(
                    '暂无支持 Tailscale SSH 的设备',
                    style: TextStyle(fontSize: 12, color: c.muted),
                  ),
                )
              else
                Builder(
                  builder: (context) {
                    final scope = GlassScope.of(context);
                    final selChip = scope.adaptiveSelectionChip(c);
                    return Wrap(
                      spacing: 6,
                      runSpacing: 6,
                      children: [
                        for (final p in peers)
                          Builder(
                            builder: (_) {
                              final selected = p['stableId'] == selectedPeer;
                              final online = p['online'] == true;
                              return GestureDetector(
                                onTap: () => setState(
                                  () => _sshPeerId = p['stableId']?.toString(),
                                ),
                                child: Container(
                                  padding: const EdgeInsets.symmetric(
                                    horizontal: 11,
                                    vertical: 6,
                                  ),
                                  decoration: BoxDecoration(
                                    color: selected ? selChip.bg : scope.adaptivePillBg(c),
                                    borderRadius: BorderRadius.circular(999),
                                    border: Border.all(
                                      color: selected
                                          ? selChip.border
                                          : scope.adaptivePillBorder(c),
                                    ),
                                  ),
                                  child: Row(
                                    mainAxisSize: MainAxisSize.min,
                                    children: [
                                      Container(
                                        width: 7,
                                        height: 7,
                                        decoration: BoxDecoration(
                                          shape: BoxShape.circle,
                                          color: online ? c.success : c.muted,
                                        ),
                                      ),
                                      const SizedBox(width: 6),
                                      Text(
                                        tailscalePeerDisplayName(p),
                                        style: TextStyle(
                                          fontSize: 12,
                                          fontWeight: selected
                                              ? FontWeight.w600
                                              : FontWeight.w500,
                                          color: selected ? selChip.ink : c.ink,
                                        ),
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
              const SizedBox(height: 10),
              Row(
                children: [
                  Expanded(
                    child: Container(
                      height: 34,
                      padding: const EdgeInsets.symmetric(horizontal: 10),
                      decoration: BoxDecoration(
                        color: c.fill,
                        borderRadius: BorderRadius.circular(10),
                      ),
                      child: TextField(
                        controller: _sshUserCtrl,
                        style: TextStyle(
                          fontSize: 13,
                          fontFamily: 'monospace',
                          color: c.ink,
                        ),
                        decoration: InputDecoration(
                          border: InputBorder.none,
                          isDense: true,
                          hintText: '用户名 (root)',
                          hintStyle: TextStyle(fontSize: 12, color: c.muted),
                        ),
                      ),
                    ),
                  ),
                  const SizedBox(width: 8),
                  FilledButton(
                    onPressed: _sshHandle == null && !hasSshPeers
                        ? null
                        : () => _toggleSsh(ep),
                    style: FilledButton.styleFrom(
                      backgroundColor: _sshHandle != null ? c.error : c.primary,
                      foregroundColor: Colors.white,
                      shape: const StadiumBorder(),
                    ),
                    child: Text(_sshHandle != null ? '断开' : '连接 SSH'),
                  ),
                ],
              ),
              if (_sshStatus.isNotEmpty) ...[
                const SizedBox(height: 6),
                Text(_sshStatus, style: TextStyle(fontSize: 11, color: c.muted)),
              ],
            ],
          ),
        ),
        const SizedBox(height: 10),
        ClipRRect(
          borderRadius: BorderRadius.circular(18),
          child: Container(
            color: const Color(0xE80F121A),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                if (_sshBanner.isNotEmpty)
                  Container(
                    padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
                    color: const Color(0x280A84FF),
                    child: Text(
                      _sshBanner,
                      style: const TextStyle(fontSize: 11, color: Color(0xFF8CC4FF)),
                    ),
                  ),
                SizedBox(
                  height: 240,
                  child: SingleChildScrollView(
                    reverse: true,
                    padding: const EdgeInsets.all(12),
                    child: SelectableText(
                      _sshOutput.isEmpty
                          ? (_sshHandle != null
                              ? '等待终端输出…'
                              : (hasSshPeers
                                  ? '选择支持 Tailscale SSH 的设备后点击「连接 SSH」。'
                                  : '当前 Tailnet 中没有启用 Tailscale SSH 的设备。'))
                          : _sshOutput,
                      style: const TextStyle(
                        fontFamily: 'monospace',
                        fontSize: 12,
                        color: Color(0xFFE6EDF3),
                      ),
                    ),
                  ),
                ),
                Container(
                  padding: const EdgeInsets.fromLTRB(10, 6, 10, 8),
                  decoration: const BoxDecoration(
                    border: Border(top: BorderSide(color: Color(0x22FFFFFF))),
                  ),
                  child: Column(
                    children: [
                      Row(
                        children: [
                          Expanded(
                            child: TextField(
                              controller: _sshCmdCtrl,
                              style: const TextStyle(
                                fontFamily: 'monospace',
                                fontSize: 13,
                                color: Colors.white,
                              ),
                              decoration: const InputDecoration(
                                border: InputBorder.none,
                                isDense: true,
                                hintText: '输入命令…',
                                hintStyle: TextStyle(color: Color(0x88FFFFFF)),
                              ),
                              onSubmitted: (text) {
                                if (_sshHandle != null) {
                                  unawaited(widget.controller.sendSshText(_sshHandle!, '$text\n'));
                                  _sshCmdCtrl.clear();
                                }
                              },
                            ),
                          ),
                          TextButton(
                            onPressed: _sshHandle == null
                                ? null
                                : () {
                                    final text = _sshCmdCtrl.text;
                                    unawaited(widget.controller.sendSshText(_sshHandle!, '$text\n'));
                                    _sshCmdCtrl.clear();
                                  },
                            child: const Text('发送'),
                          ),
                        ],
                      ),
                      const SizedBox(height: 6),
                      Row(
                        mainAxisAlignment: MainAxisAlignment.spaceBetween,
                        children: [
                          _termKey('Esc', () {
                            if (_sshHandle != null) {
                              unawaited(widget.controller.sendSshBytes(_sshHandle!, [0x1b]));
                            }
                          }),
                          _termKey('Tab', () {
                            if (_sshHandle != null) {
                              unawaited(widget.controller.sendSshBytes(_sshHandle!, [0x09]));
                            }
                          }),
                          _termKey('Ctrl+C', () {
                            if (_sshHandle != null) {
                              unawaited(widget.controller.sendSshBytes(_sshHandle!, [0x03]));
                            }
                          }),
                          _termKey('↑', () {
                            if (_sshHandle != null) {
                              unawaited(widget.controller.sendSshText(_sshHandle!, '\x1b[A'));
                            }
                          }),
                          _termKey('↓', () {
                            if (_sshHandle != null) {
                              unawaited(widget.controller.sendSshText(_sshHandle!, '\x1b[B'));
                            }
                          }),
                          _termKey('清屏', () => setState(() => _sshOutput = '')),
                        ],
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),
      ],
    );
  }

  Widget _termKey(String label, VoidCallback onTap) => GestureDetector(
    onTap: onTap,
    child: Container(
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 5),
      decoration: BoxDecoration(
        color: const Color(0x1FFFFFFF),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Text(
        label,
        style: const TextStyle(
          fontSize: 11,
          fontFamily: 'monospace',
          color: Color(0xFFE6EDF3),
        ),
      ),
    ),
  );
}

class _PeerDetailSheet extends StatefulWidget {
  final TailscaleController controller;
  final Map<String, dynamic> peer;
  final bool isExit;
  final Future<void> Function(String stableId) onExitToggle;
  final ValueChanged<String> onOpenSsh;
  final ValueChanged<String> onOpenSend;

  const _PeerDetailSheet({
    required this.controller,
    required this.peer,
    required this.isExit,
    required this.onExitToggle,
    required this.onOpenSsh,
    required this.onOpenSend,
  });

  @override
  State<_PeerDetailSheet> createState() => _PeerDetailSheetState();
}

class _PeerDetailSheetState extends State<_PeerDetailSheet> {
  int? _pingHandle;
  Timer? _timer;
  final _samples = <Map<String, dynamic>>[];

  @override
  void dispose() {
    _timer?.cancel();
    if (_pingHandle != null) {
      unawaited(widget.controller.closeFeed(_pingHandle!));
    }
    super.dispose();
  }

  Future<void> _togglePing() async {
    if (_pingHandle != null) {
      _timer?.cancel();
      await widget.controller.closeFeed(_pingHandle!);
      if (mounted) setState(() => _pingHandle = null);
      return;
    }
    final ip = tailscalePrimaryIp(widget.peer);
    if (ip.isEmpty) return;
    _samples.clear();
    try {
      final h = await widget.controller.openFeed({
        'feed': 'ping',
        if (widget.controller.currentEndpointTag != null)
          'tag': widget.controller.currentEndpointTag,
        'peerIp': ip,
      });
      if (!mounted) return;
      setState(() => _pingHandle = h);
      _timer = Timer.periodic(const Duration(milliseconds: 250), (_) async {
        if (_pingHandle == null || !mounted) return;
        try {
          final raw = await widget.controller.drain(_pingHandle!);
          final events = (raw['events'] as List?) ?? const [];
          for (final ev in events) {
            if (ev is Map && ev['data'] is Map) {
              _samples.add(Map<String, dynamic>.from(ev['data'] as Map));
              if (_samples.length > 24) _samples.removeAt(0);
            }
          }
          if (raw['finished'] == true) {
            _timer?.cancel();
            _pingHandle = null;
          }
          if (mounted) setState(() {});
        } catch (_) {}
      });
    } catch (_) {}
  }

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final p = widget.peer;
    final stableId = p['stableId']?.toString() ?? '';
    final host = tailscalePeerDisplayName(p);
    final dns = (p['dnsName']?.toString() ?? '').replaceAll(RegExp(r'\.$'), '');
    final ips = (p['tailscaleIps'] as List? ?? const []).map((e) => e.toString()).toList();
    final online = p['online'] == true;
    final last = _samples.lastOrNull;

    final scope = GlassScope.of(context);
    final selChip = scope.adaptiveSelectionChip(c);
    final maxH = MediaQuery.sizeOf(context).height * 0.78;

    return ConstrainedBox(
      constraints: BoxConstraints(maxHeight: maxH),
      child: GlassCard(
        popover: true,
        radius: 26,
        padding: const EdgeInsets.fromLTRB(18, 12, 18, 18),
        child: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Center(
                child: Container(
                  width: 36,
                  height: 4,
                  margin: const EdgeInsets.only(bottom: 10),
                  decoration: BoxDecoration(
                    color: c.muted.withValues(alpha: 0.32),
                    borderRadius: BorderRadius.circular(999),
                  ),
                ),
              ),
              Row(
                children: [
                  Container(
                    width: 9,
                    height: 9,
                    decoration: BoxDecoration(
                      shape: BoxShape.circle,
                      color: online ? c.success : c.muted,
                    ),
                  ),
                  const SizedBox(width: 8),
                  Expanded(
                    child: Text(
                      host,
                      style: TextStyle(
                        fontSize: 16,
                        fontWeight: FontWeight.w600,
                        color: c.ink,
                      ),
                    ),
                  ),
                  TextButton(
                    onPressed: () => Navigator.pop(context),
                    child: const Text('关闭'),
                  ),
                ],
              ),
              if (dns.isNotEmpty)
                Text(
                  dns,
                  style: TextStyle(fontSize: 12, fontFamily: 'monospace', color: c.muted),
                ),
              const SizedBox(height: 12),
              Wrap(
                spacing: 8,
                runSpacing: 8,
                children: [
                  if (p['exitNodeOption'] == true)
                    GestureDetector(
                      onTap: () async {
                        await widget.onExitToggle(widget.isExit ? '' : stableId);
                        if (context.mounted) Navigator.pop(context);
                      },
                      child: Container(
                        padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 8),
                        decoration: BoxDecoration(
                          color: widget.isExit ? c.dangerSoft : selChip.bg,
                          borderRadius: BorderRadius.circular(999),
                          border: Border.all(
                            color: widget.isExit
                                ? c.error.withValues(alpha: 0.45)
                                : selChip.border,
                          ),
                        ),
                        child: Text(
                          widget.isExit ? '停用出口' : '设为出口',
                          style: TextStyle(
                            fontSize: 13,
                            fontWeight: FontWeight.w600,
                            color: widget.isExit ? c.dangerInk : selChip.ink,
                          ),
                        ),
                      ),
                    ),
                  if (tailscaleHasSsh(p) && online)
                    TextButton(
                      onPressed: () {
                        Navigator.pop(context);
                        widget.onOpenSsh(stableId);
                      },
                      child: const Text('SSH 终端'),
                    ),
                  if (p['canReceiveFiles'] == true)
                    TextButton(
                      onPressed: () {
                        Navigator.pop(context);
                        widget.onOpenSend(stableId);
                      },
                      child: const Text('发送文件'),
                    ),
                ],
              ),
              const SizedBox(height: 12),
              Container(
                padding: const EdgeInsets.all(12),
                decoration: BoxDecoration(
                  color: c.fill,
                  borderRadius: BorderRadius.circular(14),
                ),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Row(
                      children: [
                        Expanded(
                          child: Text(
                            '实时链路 Ping',
                            style: TextStyle(
                              fontSize: 13,
                              fontWeight: FontWeight.w600,
                              color: c.ink,
                            ),
                          ),
                        ),
                        TextButton(
                          onPressed: _togglePing,
                          child: Text(_pingHandle != null ? '停止探测' : '开始 Ping'),
                        ),
                      ],
                    ),
                    if (last != null) ...[
                      Row(
                        children: [
                          Container(
                            padding: const EdgeInsets.symmetric(
                              horizontal: 8,
                              vertical: 3,
                            ),
                            decoration: BoxDecoration(
                              color: last['isDirect'] == true
                                  ? c.successSoft
                                  : ((last['peerRelay']?.toString() ?? '').isNotEmpty
                                      ? c.udpSoft
                                      : c.warningSoft),
                              borderRadius: BorderRadius.circular(999),
                            ),
                            child: Text(
                              last['isDirect'] == true
                                  ? '直连 Direct'
                                  : ((last['peerRelay']?.toString() ?? '').isNotEmpty
                                      ? 'Peer Relay'
                                      : 'DERP · ${last['derpRegionCode'] ?? ''} #${last['derpRegionId'] ?? ''}'),
                              style: TextStyle(
                                fontSize: 11,
                                fontWeight: FontWeight.w600,
                                color: last['isDirect'] == true
                                    ? c.successInk
                                    : ((last['peerRelay']?.toString() ?? '').isNotEmpty
                                        ? c.udp
                                        : c.warningInk),
                              ),
                            ),
                          ),
                          const Spacer(),
                          Text(
                            '${((last['latencyMs'] as num?) ?? 0).toStringAsFixed(1)} ms',
                            style: TextStyle(
                              fontSize: 15,
                              fontWeight: FontWeight.w600,
                              fontFamily: 'monospace',
                              color: c.ink,
                            ),
                          ),
                        ],
                      ),
                      const SizedBox(height: 8),
                      SizedBox(
                        height: 44,
                        child: CustomPaint(
                          painter: _PingSparkPainter(
                            _samples
                                .map((s) => ((s['latencyMs'] as num?) ?? 0).toDouble())
                                .where((v) => v > 0)
                                .toList(),
                            last['isDirect'] == true ? c.success : c.warning,
                          ),
                        ),
                      ),
                    ] else
                      Text(
                        _pingHandle != null
                            ? '正在等待探测响应…'
                            : '点击「开始 Ping」实时检测直连或 DERP 链路。',
                        style: TextStyle(fontSize: 12, color: c.muted),
                      ),
                  ],
                ),
              ),
              const SizedBox(height: 12),
              for (final ip in ips)
                Padding(
                  padding: const EdgeInsets.only(bottom: 4),
                  child: Row(
                    children: [
                      Text('IP', style: TextStyle(fontSize: 12, color: c.muted)),
                      const Spacer(),
                      GestureDetector(
                        onTap: () {
                          Clipboard.setData(ClipboardData(text: ip));
                          showGlassToast(context, '已复制 $ip', error: false);
                        },
                        child: Text(
                          ip,
                          style: TextStyle(
                            fontSize: 12,
                            fontFamily: 'monospace',
                            color: c.ink,
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
              Row(
                children: [
                  Text('累计流量', style: TextStyle(fontSize: 12, color: c.muted)),
                  const Spacer(),
                  Text(
                    '↓ ${tailscaleFormatBytes((p['rxBytes'] as num?) ?? 0)} · ↑ ${tailscaleFormatBytes((p['txBytes'] as num?) ?? 0)}',
                    style: TextStyle(
                      fontSize: 12,
                      fontFamily: 'monospace',
                      color: c.ink,
                    ),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _PingSparkPainter extends CustomPainter {
  final List<double> values;
  final Color color;
  _PingSparkPainter(this.values, this.color);

  @override
  void paint(Canvas canvas, Size size) {
    if (values.length < 2) return;
    final maxV = values.fold<double>(25, math.max) * 1.15;
    final path = Path();
    for (var i = 0; i < values.length; i++) {
      final x = (i / (values.length - 1)) * size.width;
      final y = size.height - 4 - (values[i] / maxV).clamp(0.0, 1.0) * (size.height - 8);
      if (i == 0) {
        path.moveTo(x, y);
      } else {
        path.lineTo(x, y);
      }
    }
    canvas.drawPath(
      path,
      Paint()
        ..color = color
        ..style = PaintingStyle.stroke
        ..strokeWidth = 2
        ..strokeCap = StrokeCap.round
        ..strokeJoin = StrokeJoin.round,
    );
  }

  @override
  bool shouldRepaint(_PingSparkPainter old) =>
      old.values.length != values.length || old.color != color;
}

class TaildropQuickShareSheet extends StatefulWidget {
  final TailscaleController controller;
  final String batchDir;
  final List<Map<String, dynamic>> files;
  final bool fromExternal;

  const TaildropQuickShareSheet({
    super.key,
    required this.controller,
    required this.batchDir,
    required this.files,
    required this.fromExternal,
  });

  @override
  State<TaildropQuickShareSheet> createState() => _TaildropQuickShareSheetState();
}

class _TaildropQuickShareSheetState extends State<TaildropQuickShareSheet> {
  late final List<Map<String, dynamic>> _files = List.of(widget.files);
  String? _selectedPeerId;
  String? _endpointTag;
  String _targetName = '';
  TaildropUpload? _upload;
  int _completedFiles = 0;
  int _sentBytes = 0;
  bool _done = false;
  String _error = '';

  int get _totalBytes => _files.fold<int>(
    0,
    (sum, f) => sum + ((f['size'] as num?)?.toInt() ?? 0),
  );

  @override
  void initState() {
    super.initState();
    widget.controller.addListener(_refresh);
  }

  void _refresh() {
    if (mounted) setState(() {});
  }

  @override
  void dispose() {
    widget.controller.removeListener(_refresh);
    final dir = widget.batchDir;
    final upload = _upload;
    unawaited((upload?.cancel() ?? Future<void>.value()).then((_) => _clearShare(dir)));
    super.dispose();
  }

  Future<void> _send(String tag, Map<String, dynamic> peer) async {
    if (_upload != null || _done || _files.isEmpty || widget.controller.connecting) return;
    final upload = TaildropUpload(widget.controller);
    setState(() {
      _upload = upload;
      _targetName = tailscalePeerDisplayName(peer);
      _error = '';
      _completedFiles = 0;
      _sentBytes = 0;
    });
    try {
      final sent = await upload.send(
        tag: tag,
        peerId: peer['stableId'] as String,
        paths: _files.map((f) => f['path'] as String).toList(),
        onProgress: (bytes, count) {
          if (mounted) setState(() {
            _sentBytes = bytes;
            _completedFiles = count;
          });
        },
      );
      if (mounted) setState(() => _done = sent);
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    } finally {
      if (mounted) setState(() => _upload = null);
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final controller = widget.controller;
    final endpoints = controller.connecting
        ? <Map<String, dynamic>>[]
        : controller.endpoints.where((ep) => ep['canShareFiles'] == true).toList();
    final tag = _endpointTag ?? controller.currentEndpointTag;
    final ep = endpoints.where((item) => item['endpointTag'] == tag).firstOrNull ?? endpoints.firstOrNull;
    final targets = ep == null ? <Map<String, dynamic>>[] : (taildropTargets(ep)
        ..sort((a, b) => (b['online'] == true ? 1 : 0) - (a['online'] == true ? 1 : 0)));
    final peer = targets.where((p) => p['stableId'] == _selectedPeerId).firstOrNull ?? targets.firstOrNull;
    final pickedId = peer?['stableId'];
    final pickedName = _upload != null || _done
        ? _targetName
        : (peer == null ? '设备' : tailscalePeerDisplayName(peer));
    final emptyMessage = switch (controller) {
      _ when controller.connecting => '正在获取端点…',
      _ when ep != null => '当前 Tailnet 中没有可接收 Taildrop 文件的设备。',
      _ when controller.hasEndpoints => '当前服务没有可发送文件的端点。',
      _ when controller.discoveredServices.length > 1 && controller.selectedServiceTag == null => '请选择 Service API 服务。',
      _ => '尚未连接到 Tailscale Service API，请检查核心和 API 设置。',
    };

    final scope = GlassScope.of(context);
    final selChip = scope.adaptiveSelectionChip(c);
    final maxH = MediaQuery.sizeOf(context).height * 0.78;

    Widget glassActionButton({
      required String label,
      required VoidCallback? onTap,
      bool danger = false,
    }) {
      final enabled = onTap != null;
      return Opacity(
        opacity: enabled ? 1.0 : 0.45,
        child: Material(
          color: Colors.transparent,
          child: InkWell(
            borderRadius: BorderRadius.circular(999),
            onTap: onTap,
            child: Container(
              height: 44,
              alignment: Alignment.center,
              decoration: BoxDecoration(
                color: danger ? c.dangerSoft : selChip.bg,
                borderRadius: BorderRadius.circular(999),
                border: Border.all(
                  color: danger
                      ? c.error.withValues(alpha: 0.45)
                      : selChip.border,
                ),
              ),
              child: Text(
                label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  fontSize: 14,
                  fontWeight: FontWeight.w600,
                  color: danger ? c.dangerInk : selChip.ink,
                ),
              ),
            ),
          ),
        ),
      );
    }

    Widget glassChoicePill({
      required String label,
      required bool selected,
      required VoidCallback? onTap,
    }) => GestureDetector(
      onTap: onTap,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 11, vertical: 6),
        decoration: BoxDecoration(
          color: selected ? selChip.bg : c.fill,
          borderRadius: BorderRadius.circular(999),
          border: Border.all(
            color: selected ? selChip.border : scope.adaptivePillBorder(c),
          ),
        ),
        child: Text(
          label,
          style: TextStyle(
            fontSize: 12,
            fontWeight: selected ? FontWeight.w600 : FontWeight.w500,
            color: selected ? selChip.ink : c.ink,
          ),
        ),
      ),
    );

    return ConstrainedBox(
      constraints: BoxConstraints(maxHeight: maxH),
      child: GlassCard(
        popover: true,
        radius: 26,
        padding: const EdgeInsets.fromLTRB(18, 12, 18, 18),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Center(
              child: Container(
                width: 36,
                height: 4,
                margin: const EdgeInsets.only(bottom: 10),
                decoration: BoxDecoration(
                  color: c.muted.withValues(alpha: 0.32),
                  borderRadius: BorderRadius.circular(999),
                ),
              ),
            ),
            Row(
              children: [
                PanelIcon('tailscale', size: 20, color: selChip.ink),
                const SizedBox(width: 8),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        '通过 Taildrop 发送',
                        style: TextStyle(
                          fontSize: 15,
                          fontWeight: FontWeight.w600,
                          color: c.ink,
                        ),
                      ),
                      Text(
                        ep == null
                            ? (controller.connecting ? '正在连接 Tailscale…' : controller.chipText)
                            : '${ep['endpointTag'] ?? ''} · ${ep['networkName'] ?? ''}',
                        style: TextStyle(fontSize: 11, color: c.muted),
                      ),
                    ],
                  ),
                ),
                TextButton(
                  onPressed: () => Navigator.pop(context),
                  child: Text('关闭', style: TextStyle(color: c.muted)),
                ),
              ],
            ),
            const SizedBox(height: 10),
            Flexible(
              fit: FlexFit.loose,
              child: SingleChildScrollView(
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Container(
                      padding: const EdgeInsets.all(10),
                      decoration: BoxDecoration(
                        color: c.fill,
                        borderRadius: BorderRadius.circular(14),
                        border: Border.all(color: scope.adaptivePillBorder(c)),
                      ),
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Row(
                            children: [
                              Text(
                                '已接收 ${_files.length} 个文件',
                                style: TextStyle(
                                  fontSize: 12,
                                  fontWeight: FontWeight.w600,
                                  color: c.ink,
                                ),
                              ),
                              const Spacer(),
                              Text(
                                tailscaleFormatBytes(_totalBytes),
                                style: TextStyle(
                                  fontSize: 12,
                                  fontFamily: 'monospace',
                                  color: c.muted,
                                ),
                              ),
                            ],
                          ),
                          const SizedBox(height: 6),
                          for (var i = 0; i < _files.length; i++)
                            Padding(
                              padding: const EdgeInsets.only(top: 4),
                              child: Row(
                                children: [
                                  Expanded(
                                    child: Text(
                                      _files[i]['name']?.toString() ?? '',
                                      maxLines: 1,
                                      overflow: TextOverflow.ellipsis,
                                      style: TextStyle(fontSize: 12, color: c.ink),
                                    ),
                                  ),
                                  const SizedBox(width: 8),
                                  Text(
                                    tailscaleFormatBytes((_files[i]['size'] as num?) ?? 0),
                                    style: TextStyle(
                                      fontSize: 11,
                                      fontFamily: 'monospace',
                                      color: c.muted,
                                    ),
                                  ),
                                  if (_upload == null && !_done && _files.length > 1)
                                    GestureDetector(
                                      onTap: () => setState(() => _files.removeAt(i)),
                                      child: Padding(
                                        padding: const EdgeInsets.only(left: 8),
                                        child: Text('×', style: TextStyle(fontSize: 15, color: c.muted)),
                                      ),
                                    ),
                                ],
                              ),
                            ),
                        ],
                      ),
                    ),
                    if (_upload == null && !_done && controller.discoveredServices.length > 1) ...[
                      const SizedBox(height: 10),
                      Wrap(
                        spacing: 6,
                        runSpacing: 6,
                        children: [
                          for (final service in controller.discoveredServices)
                            glassChoicePill(
                              label: service['serviceTag']?.toString() ?? '',
                              selected: service['serviceTag'] == controller.selectedServiceTag,
                              onTap: controller.connecting ? null : () {
                                setState(() {
                                  _endpointTag = null;
                                  _selectedPeerId = null;
                                });
                                unawaited(controller.sync(serviceTag: service['serviceTag'] as String));
                              },
                            ),
                        ],
                      ),
                    ],
                    if (_upload == null && !_done && endpoints.length > 1) ...[
                      const SizedBox(height: 10),
                      Wrap(
                        spacing: 6,
                        runSpacing: 6,
                        children: [
                          for (final endpoint in endpoints)
                            glassChoicePill(
                              label: endpoint['endpointTag'] as String,
                              selected: endpoint['endpointTag'] == ep?['endpointTag'],
                              onTap: () => setState(() {
                                _endpointTag = endpoint['endpointTag'] as String;
                                _selectedPeerId = null;
                              }),
                            ),
                        ],
                      ),
                    ],
                    if (_error.isNotEmpty) ...[
                      const SizedBox(height: 10),
                      Text(_error, style: TextStyle(fontSize: 12, color: c.dangerInk)),
                    ],
                    const SizedBox(height: 12),
                    if (_done)
                      Container(
                        padding: const EdgeInsets.all(14),
                        decoration: BoxDecoration(
                          color: c.successSoft,
                          borderRadius: BorderRadius.circular(14),
                        ),
                        child: Text(
                          '已成功发送 ${_files.length} 个文件到 $pickedName',
                          textAlign: TextAlign.center,
                          style: TextStyle(
                            fontSize: 13,
                            fontWeight: FontWeight.w600,
                            color: c.successInk,
                          ),
                        ),
                      )
                    else if (_upload != null) ...[
                      Text(
                        '正在发送到 $pickedName · $_completedFiles/${_files.length} (${tailscaleFormatBytes(_sentBytes)} / ${tailscaleFormatBytes(_totalBytes)})',
                        style: TextStyle(fontSize: 12, color: c.muted),
                      ),
                      const SizedBox(height: 8),
                      ClipRRect(
                        borderRadius: BorderRadius.circular(999),
                        child: LinearProgressIndicator(
                          value: _totalBytes > 0
                              ? (_sentBytes / _totalBytes).clamp(0.0, 1.0)
                              : null,
                          minHeight: 6,
                        ),
                      ),
                    ] else if (targets.isEmpty)
                      Text(
                        emptyMessage,
                        style: TextStyle(fontSize: 12, color: c.muted),
                      )
                    else ...[
                      Text('选择接收设备', style: TextStyle(fontSize: 12, color: c.muted)),
                      const SizedBox(height: 8),
                      for (final t in targets)
                        Builder(
                          builder: (_) {
                            final selected = t['stableId'] == pickedId;
                            final online = t['online'] == true;
                            return GestureDetector(
                              onTap: () => setState(
                                () => _selectedPeerId = t['stableId']?.toString(),
                              ),
                              child: Container(
                                margin: const EdgeInsets.only(bottom: 6),
                                padding: const EdgeInsets.symmetric(
                                  horizontal: 12,
                                  vertical: 10,
                                ),
                                decoration: BoxDecoration(
                                  color: selected ? selChip.bg : c.fill,
                                  borderRadius: BorderRadius.circular(14),
                                  border: Border.all(
                                    color: selected
                                        ? selChip.border
                                        : scope.adaptivePillBorder(c),
                                  ),
                                ),
                                child: Row(
                                  children: [
                                    Container(
                                      width: 8,
                                      height: 8,
                                      decoration: BoxDecoration(
                                        shape: BoxShape.circle,
                                        color: online ? c.success : c.muted,
                                      ),
                                    ),
                                    const SizedBox(width: 10),
                                    Expanded(
                                      child: Column(
                                        crossAxisAlignment: CrossAxisAlignment.start,
                                        children: [
                                          Text(
                                            tailscalePeerDisplayName(t),
                                            style: TextStyle(
                                              fontSize: 13,
                                              fontWeight: FontWeight.w600,
                                              color: selected ? selChip.ink : c.ink,
                                            ),
                                          ),
                                          Text(
                                            '${t['os'] ?? ''} · ${tailscalePrimaryIp(t)}',
                                            style: TextStyle(
                                              fontSize: 11,
                                              fontFamily: 'monospace',
                                              color: selected
                                                  ? selChip.ink.withValues(alpha: 0.78)
                                                  : c.muted,
                                            ),
                                          ),
                                        ],
                                      ),
                                    ),
                                    if (selected)
                                      Container(
                                        width: 20,
                                        height: 20,
                                        alignment: Alignment.center,
                                        decoration: BoxDecoration(
                                          shape: BoxShape.circle,
                                          color: selChip.ink.withValues(alpha: 0.14),
                                        ),
                                        child: Text(
                                          '✓',
                                          style: TextStyle(
                                            fontSize: 12,
                                            fontWeight: FontWeight.w700,
                                            color: selChip.ink,
                                          ),
                                        ),
                                      ),
                                  ],
                                ),
                              ),
                            );
                          },
                        ),
                    ],
                  ],
                ),
              ),
            ),
            if (_done) ...[
              const SizedBox(height: 12),
              glassActionButton(
                label: widget.fromExternal ? '完成并返回原应用' : '完成',
                onTap: () => Navigator.pop(context, widget.fromExternal),
              ),
            ] else if (_upload != null) ...[
              const SizedBox(height: 12),
              glassActionButton(
                label: '取消发送',
                onTap: () => _upload?.cancel(),
                danger: true,
              ),
            ] else if (targets.isNotEmpty) ...[
              const SizedBox(height: 10),
              glassActionButton(
                label: '发送到 $pickedName',
                onTap: peer == null || ep == null
                    ? null
                    : () => _send(ep['endpointTag'] as String, peer),
              ),
            ],
          ],
        ),
      ),
    );
  }
}
