import 'dart:async';
import 'dart:io';
import 'dart:math' as math;

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'catalog_page.dart';
import 'config_page.dart';
import 'connections_page.dart';
import 'glass_ui.dart';
import 'native_stream.dart';
import 'maintenance_page.dart';
import 'settings_page.dart';
import 'setup_wizard.dart';
import 'tailscale_page.dart';
import 'tailscale_service.dart';
import 'tools_page.dart';

final _theme = ValueNotifier(ThemeMode.system);
final _fontScale = ValueNotifier<double>(1.0);
final _glassTint = ValueNotifier<double>(0.5);
final _wallpaper = ValueNotifier<String?>(null);
final _wallpaperRev = ValueNotifier<int>(0);
const _platformChannel = MethodChannel('singboard/config');

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  unawaited(SystemChrome.setEnabledSystemUIMode(SystemUiMode.edgeToEdge));
  runApp(const Panel());
}

class _GlassScrollBehavior extends MaterialScrollBehavior {
  const _GlassScrollBehavior();

  @override
  ScrollPhysics getScrollPhysics(BuildContext context) =>
      const BouncingScrollPhysics(decelerationRate: ScrollDecelerationRate.normal);

  @override
  Widget buildOverscrollIndicator(
    BuildContext context,
    Widget child,
    ScrollableDetails details,
  ) => child;
}

class _GentlePageScrollPhysics extends PageScrollPhysics {
  const _GentlePageScrollPhysics({super.parent});

  @override
  _GentlePageScrollPhysics applyTo(ScrollPhysics? ancestor) =>
      _GentlePageScrollPhysics(parent: buildParent(ancestor));

  @override
  SpringDescription get spring => SpringDescription.withDampingRatio(
    mass: 1.0,
    stiffness: 260.0,
    ratio: 1.05,
  );
}

class _MultipliedTextScaler extends TextScaler {
  final TextScaler base;
  final double factor;
  const _MultipliedTextScaler(this.base, this.factor);

  @override
  double scale(double fontSize) => base.scale(fontSize) * factor;

  @override
  double get textScaleFactor => scale(14) / 14;

  @override
  bool operator ==(Object other) =>
      identical(this, other) ||
      other is _MultipliedTextScaler &&
          other.base == base &&
          other.factor == factor;

  @override
  int get hashCode => Object.hash(base, factor);
}

class _FontScaleTransition extends StatefulWidget {
  final double scale;
  final Widget child;
  const _FontScaleTransition({required this.scale, required this.child});

  @override
  State<_FontScaleTransition> createState() => _FontScaleTransitionState();
}

class _FontScaleTransitionState extends State<_FontScaleTransition>
    with SingleTickerProviderStateMixin {
  late final AnimationController _controller = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 220),
    value: 1.0,
  );
  late final Animation<double> _curved = CurvedAnimation(
    parent: _controller,
    curve: Curves.easeOutCubic,
  );
  late final Animation<double> _opacity = Tween<double>(
    begin: 0.62,
    end: 1.0,
  ).animate(_curved);
  late final Animation<double> _scale = Tween<double>(
    begin: 0.992,
    end: 1.0,
  ).animate(_curved);

  @override
  void didUpdateWidget(_FontScaleTransition old) {
    super.didUpdateWidget(old);
    if ((old.scale - widget.scale).abs() > 0.001) {
      if (MediaQuery.disableAnimationsOf(context)) {
        _controller.value = 1.0;
      } else {
        _controller.forward(from: 0.0);
      }
    }
  }

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => FadeTransition(
    opacity: _opacity,
    child: ScaleTransition(scale: _scale, child: widget.child),
  );
}

class Panel extends StatelessWidget {
  const Panel({super.key});

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: Listenable.merge([_theme, _fontScale, _glassTint, _wallpaper, _wallpaperRev]),
    builder: (_, __) => MaterialApp(
      title: 'Singboard',
      debugShowCheckedModeBanner: false,
      scrollBehavior: const _GlassScrollBehavior(),
      themeMode: _theme.value,
      theme: ThemeData(
        useMaterial3: true,
        colorScheme: ColorScheme.fromSeed(
          seedColor: const Color(0xFF007AFF),
          brightness: Brightness.light,
        ),
        scaffoldBackgroundColor: Colors.transparent,
        pageTransitionsTheme: const PageTransitionsTheme(
          builders: {
            TargetPlatform.android: GlassPageTransitionsBuilder(),
            TargetPlatform.iOS: GlassPageTransitionsBuilder(),
            TargetPlatform.linux: GlassPageTransitionsBuilder(),
            TargetPlatform.macOS: GlassPageTransitionsBuilder(),
            TargetPlatform.windows: GlassPageTransitionsBuilder(),
          },
        ),
      ),
      darkTheme: ThemeData(
        useMaterial3: true,
        colorScheme: ColorScheme.fromSeed(
          seedColor: const Color(0xFF0A84FF),
          brightness: Brightness.dark,
        ),
        scaffoldBackgroundColor: Colors.transparent,
        pageTransitionsTheme: const PageTransitionsTheme(
          builders: {
            TargetPlatform.android: GlassPageTransitionsBuilder(),
            TargetPlatform.iOS: GlassPageTransitionsBuilder(),
            TargetPlatform.linux: GlassPageTransitionsBuilder(),
            TargetPlatform.macOS: GlassPageTransitionsBuilder(),
            TargetPlatform.windows: GlassPageTransitionsBuilder(),
          },
        ),
      ),
      builder: (ctx, navigator) {
        final dark = Theme.of(ctx).brightness == Brightness.dark;
        final overlay = SystemUiOverlayStyle(
          statusBarColor: Colors.transparent,
          systemNavigationBarColor: Colors.transparent,
          systemNavigationBarDividerColor: Colors.transparent,
          statusBarIconBrightness: dark ? Brightness.light : Brightness.dark,
          systemNavigationBarIconBrightness: dark ? Brightness.light : Brightness.dark,
          statusBarBrightness: dark ? Brightness.dark : Brightness.light,
        );
        final factor = _fontScale.value;
        final mq = MediaQuery.of(ctx);
        Widget content = AnnotatedRegion<SystemUiOverlayStyle>(
          value: overlay,
          child: GlassEnvironment(
            tint: _glassTint.value,
            wallpaperPath: _wallpaper.value,
            wallpaperRevision: _wallpaperRev.value,
            child: GlassBackdrop(
              child: _FontScaleTransition(
                scale: factor,
                child: GlassToastHost(child: navigator ?? const SizedBox.shrink()),
              ),
            ),
          ),
        );
        if ((factor - 1.0).abs() > 0.001) {
          content = MediaQuery(
            data: mq.copyWith(
              textScaler: _MultipliedTextScaler(mq.textScaler, factor),
            ),
            child: content,
          );
        }
        return content;
      },
      home: const Dashboard(),
    ),
  );
}

class Dashboard extends StatefulWidget {
  const Dashboard({super.key});

  @override
  State<Dashboard> createState() => _DashboardState();
}

class _PagerPanel extends StatefulWidget {
  final Widget child;
  const _PagerPanel({super.key, required this.child});
  @override
  State<_PagerPanel> createState() => _PagerPanelState();
}

class _PagerPanelState extends State<_PagerPanel> with AutomaticKeepAliveClientMixin {
  @override
  bool get wantKeepAlive => true;
  @override
  Widget build(BuildContext context) {
    super.build(context);
    return RepaintBoundary(child: widget.child);
  }
}

class _DashboardState extends State<Dashboard> with WidgetsBindingObserver, SingleTickerProviderStateMixin {
  static const limit = 5000;
  final _logSearchCtrl = TextEditingController();
  final _tailscale = TailscaleController();
  bool _drainingShares = false;
  bool _shareRequested = false;
  NativeStream? logs;
  NativeStream? traffic;
  NativeStream? connections;
  List<Item> active = [], history = [];
  String connectionStatus = '连接中', keyword = '', level = '跟随核心';
  int endpointRevision = 0;
  Timer? timer, profileTimer, summaryTimer, _summaryRetryTimer;
  bool _readingSummary = false, _summaryPending = false;
  int _reconnectGeneration = 0;
  Item summary = {}, metrics = {};
  Item? frozenTopology, hoveredTopology;
  int? coreMemory;
  Item rootService = {};
  final cpuTrend = <int>[];
  Timer? rootTimer;
  int _rootGeneration = 0, _trafficFailures = 0, _connectionFailures = 0;
  bool _samplingRoot = false, _rootComponentSynced = false;
  bool get rootRunning => rootService['state'] == 'running';
  bool get _coreAvailable => preferences['nativeRootEnabled'] != true || rootRunning;
  bool get overviewLive =>
      _coreAvailable && trafficStatus == '已连接' && connectionStatus == '已连接';
  bool get overviewConnecting =>
      setupSaved && !overviewLive && _coreAvailable &&
      !trafficStatus.contains('认证失败') && !connectionStatus.contains('认证失败') &&
      math.max(_trafficFailures, _connectionFailures) < 3;
  final connectionTrend = <int>[],
      memoryTrend = <int>[],
      downTrend = <int>[],
      upTrend = <int>[];
  final expandedLogs = <int>{};

  void appendSample(List<int> list, int n) {
    list.add(n);
    if (list.length > 60) list.removeAt(0);
  }

  bool chart = false;
  Item? domesticInfo;
  Item? abroadInfo;
  List<Item> sitesInfo = [];
  bool domesticBusy = false, abroadBusy = false, sitesBusy = false;
  bool domesticChecked = false, abroadChecked = false, sitesChecked = false;
  bool showIp = false, proxyDirty = false, _networkWasRunning = false;
  int? domesticUpdatedAt, abroadUpdatedAt, sitesUpdatedAt;
  int? _coreStartedAt;
  String abroadFingerprint = '';
  String _networkCacheLoadedFor = '';
  bool _networkCacheLoading = false;
  Timer? exitsScheduled;
  Timer? sitesScheduled;
  Timer? proxyDebounce;
  DateTime? proxyDueAt;
  int domesticGeneration = 0, abroadGeneration = 0, sitesGeneration = 0;
  bool get _networkRunning => preferences['nativeRootEnabled'] == true
      ? (rootRunning && summary.isNotEmpty)
      : (trafficStatus == '已连接' || connectionStatus == '已连接' || summary.isNotEmpty);
  String summaryError = '';

  String endpoint = 'http://127.0.0.1:9090';
  String secret = '';
  Map<String, dynamic> preferences = {};
  bool ready = false;
  String logStatus = '连接中', trafficStatus = '连接中', rootStatus = '尚未查询';
  int up = 0, down = 0, total = 0, pausedAt = 0, tab = 0;
  int _logSeq = 1, _logFailures = 0;
  String? _logGapSince;
  String _logSubscribedLevel = 'info';
  bool hasTraffic = false, probing = false, showSetupWizard = false, setupSaved = false;
  String? _rootAction;
  bool _logAutoScroll = true;
  late final ScrollController _logScroll = ScrollController(onAttach: (position) {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && _logAutoScroll && _logScroll.positions.contains(position)) {
        position.jumpTo(position.maxScrollExtent);
      }
    });
  });
  final _logSearchFocus = FocusNode();
  bool _logShortcut(KeyEvent event) {
    if (tab != 3 || event is! KeyDownEvent || ModalRoute.of(context)?.isCurrent != true) return false;
    final keyboard = HardwareKeyboard.instance;
    if (event.logicalKey == LogicalKeyboardKey.keyF &&
        (keyboard.isControlPressed || keyboard.isMetaPressed) && !keyboard.isAltPressed && !keyboard.isShiftPressed) {
      _logSearchFocus.requestFocus();
      _logSearchCtrl.selection = TextSelection(baseOffset: 0, extentOffset: _logSearchCtrl.text.length);
      return true;
    }
    if (event.logicalKey == LogicalKeyboardKey.escape && _logSearchFocus.hasFocus) {
      if (_logSearchCtrl.text.isEmpty) {
        _logSearchFocus.unfocus();
      } else {
        _logSearchCtrl.clear();
        setState(() => keyword = '');
      }
      return true;
    }
    return false;
  }
  final _pager = PageController();
  final _pageKeys = List.generate(6, (_) => GlobalKey());
  final _scrollProgress = List<double>.filled(6, 0.0);
  final _headerScroll = ValueNotifier<int>(0);
  final _swipeProgress = ValueNotifier<double?>(null);
  bool _userSwiping = false;
  int? _pendingTab;
  late final _pageFade = AnimationController(vsync: this, duration: const Duration(milliseconds: 260), value: 1);
  late final _glassMotion = Listenable.merge([_pager, _pageFade]);
  double _tabDir = 1.0;
  int? _targetPage;

  void _selectTab(int next) {
    if (next != 0) hoveredTopology = null;
    if (next == tab) {
      void scrollTop(Element element) {
        if (element is StatefulElement && element.state is ScrollableState) {
          final scroll = (element.state as ScrollableState).position;
          if (scroll.axis == Axis.vertical && scroll.hasPixels && scroll.pixels > scroll.minScrollExtent) {
            if (MediaQuery.disableAnimationsOf(context)) {
              scroll.jumpTo(scroll.minScrollExtent);
            } else {
              scroll.animateTo(scroll.minScrollExtent, duration: const Duration(milliseconds: 300), curve: Curves.easeOutQuart);
            }
          }
        }
        element.visitChildElements(scrollTop);
      }
      _pageKeys[next].currentContext?.visitChildElements(scrollTop);
      return;
    }
    final prev = tab;
    _userSwiping = false;
    _pendingTab = null;
    _swipeProgress.value = null;
    setState(() => tab = next);
    if (next == 0) _syncNetwork();
    if (!_pager.hasClients) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted && _pager.hasClients) _pager.jumpToPage(tab);
      });
      return;
    }
    final animated = !MediaQuery.disableAnimationsOf(context);
    if (animated && (next - prev).abs() == 1) {
      _targetPage = next;
      _pageFade.value = 1;
      _pager.animateToPage(next, duration: const Duration(milliseconds: 280), curve: const Cubic(0.22, 1.0, 0.36, 1.0))
          .whenComplete(() { if (_targetPage == next) _targetPage = null; });
    } else {
      _targetPage = null;
      _tabDir = next > prev ? 1.0 : -1.0;
      _pager.jumpToPage(next);
      if (animated) {
        _pageFade.forward(from: 0);
      } else {
        _pageFade.value = 1;
      }
    }
  }
  final entries = <Map<String, dynamic>>[];
  List<Map<String, dynamic>>? frozen;
  List<Map<String, dynamic>>? clearedLogs;
  Timer? clearedTimer;

  String _proxyFingerprint() {
    final config = summary['config'] is Map ? summary['config'] as Map : const {};
    final mode = (config['mode']?.toString() ?? '').trim().toLowerCase();
    final exitObj = summary['exit'] is Map ? summary['exit'] as Map : const {};
    final exitNode = (exitObj['node']?.toString() ?? '').trim();
    final exitDirect = exitObj['direct'] == true ? 'direct' : '';
    final proxies = summary['proxies'] is Map ? summary['proxies'] as Map : const {};
    final picks = <String>[];
    for (final entry in proxies.entries) {
      if (entry.value is Map) {
        final now = (entry.value as Map)['now']?.toString().trim() ?? '';
        if (now.isNotEmpty) picks.add('${entry.key}=$now');
      }
    }
    picks.sort();
    if (mode.isEmpty && exitNode.isEmpty && picks.isEmpty) return '';
    return '$mode|$exitNode|$exitDirect|${picks.join(';')}';
  }

  String _formatRelativeAge(int? updatedAt) {
    if (updatedAt == null || updatedAt <= 0) return '';
    final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final diff = now > updatedAt ? now - updatedAt : 0;
    if (diff < 60) return '刚刚';
    if (diff < 3600) return '${diff ~/ 60} 分钟前';
    if (diff < 86400) return '${diff ~/ 3600} 小时前';
    return '${diff ~/ 86400} 天前';
  }

  bool _invalidateIfCoreRestarted() {
    final started = _coreStartedAt;
    if (started == null) return false;
    final stale = [domesticUpdatedAt, abroadUpdatedAt, sitesUpdatedAt]
        .whereType<int>()
        .any((ts) => ts + 5 < started);
    if (stale) {
      _resetNetwork(clearDisk: true);
      return true;
    }
    return false;
  }

  Future<void> _restoreNetworkCache() async {
    final ep = endpoint;
    if (_networkCacheLoading || _networkCacheLoadedFor == ep) return;
    _networkCacheLoading = true;
    final rev = endpointRevision;
    try {
      final raw = await configRequest(
        await configDirectory(),
        'network_cache_load',
        {
          'endpoint': ep,
          if (_coreStartedAt != null) 'core_started_at': _coreStartedAt,
        },
      );
      if (!mounted || rev != endpointRevision || ep != endpoint) return;
      _networkCacheLoadedFor = ep;
      if (raw is Map) {
        setState(() {
          if (!domesticChecked && raw['domestic'] is Map) {
            final slot = raw['domestic'] as Map;
            if (slot['data'] is Map) {
              domesticInfo = (slot['data'] as Map).cast<String, dynamic>();
              domesticUpdatedAt = (slot['updated_at'] as num?)?.toInt();
              domesticChecked = true;
            }
          }
          if (!abroadChecked && raw['abroad'] is Map) {
            final slot = raw['abroad'] as Map;
            if (slot['data'] is Map) {
              abroadInfo = (slot['data'] as Map).cast<String, dynamic>();
              abroadUpdatedAt = (slot['updated_at'] as num?)?.toInt();
              abroadFingerprint = slot['fingerprint']?.toString() ?? '';
              abroadChecked = true;
            }
          }
          if (!sitesChecked && raw['sites'] is Map) {
            final slot = raw['sites'] as Map;
            if (slot['data'] is List) {
              sitesInfo = (slot['data'] as List).cast<Item>();
              sitesUpdatedAt = (slot['updated_at'] as num?)?.toInt();
              sitesChecked = true;
            }
          }
        });
      }
      _checkCachedProxyFingerprint();
      _syncNetwork();
    } catch (_) {
      _networkCacheLoadedFor = ep;
      if (mounted) _syncNetwork();
    } finally {
      _networkCacheLoading = false;
    }
  }

  void _checkCachedProxyFingerprint() {
    if (!abroadChecked || abroadFingerprint.isEmpty || proxyDirty) return;
    final current = _proxyFingerprint();
    if (current.isNotEmpty && current != abroadFingerprint) {
      _scheduleProxyRecheck();
    }
  }

  void _scheduleProxyRecheck() {
    if (!_networkRunning) return;
    proxyDebounce?.cancel();
    proxyDirty = true;
    proxyDueAt = DateTime.now().add(const Duration(milliseconds: 1500));
    proxyDebounce = Timer(const Duration(milliseconds: 1500), () {
      proxyDebounce = null;
      if (!mounted) return;
      if (_networkRunning && tab == 0 && proxyDirty) {
        refreshProxyExit(resetCooldown: true);
      }
    });
    if (mounted) setState(() {});
  }

  void refreshExits() {
    exitsScheduled?.cancel();
    exitsScheduled = null;
    proxyDebounce?.cancel();
    proxyDebounce = null;
    proxyDirty = false;
    proxyDueAt = null;
    refreshDirectExit();
    refreshProxyExit(resetCooldown: true);
  }

  Future<void> refreshDirectExit() async {
    if (domesticBusy) return;
    final generation = ++domesticGeneration;
    final ep = endpoint;
    setState(() {
      domesticBusy = true;
      domesticChecked = true;
    });
    try {
      final reply = await configRequest(
        await configDirectory(),
        'network_direct',
        {'endpoint': ep},
      );
      if (!mounted || generation != domesticGeneration) return;
      if (reply is Map && reply['data'] is Map) {
        setState(() {
          domesticInfo = (reply['data'] as Map).cast<String, dynamic>();
          final ts = (reply['updatedAt'] as num?)?.toInt();
          if (ts != null) domesticUpdatedAt = ts;
        });
      }
    } catch (_) {
    } finally {
      if (mounted && generation == domesticGeneration) {
        setState(() => domesticBusy = false);
      }
    }
  }

  Future<void> refreshProxyExit({bool resetCooldown = false}) async {
    if (abroadBusy) return;
    proxyDebounce?.cancel();
    proxyDebounce = null;
    proxyDirty = false;
    proxyDueAt = null;
    final generation = ++abroadGeneration;
    final ep = endpoint;
    final fp = _proxyFingerprint();
    setState(() {
      abroadBusy = true;
      abroadChecked = true;
    });
    try {
      final reply = await configRequest(
        await configDirectory(),
        'network_proxy',
        {
          'endpoint': ep,
          'fingerprint': fp,
          'reset_cooldown': resetCooldown,
        },
      );
      if (!mounted || generation != abroadGeneration) return;
      if (reply is Map && reply['data'] is Map) {
        setState(() {
          abroadInfo = (reply['data'] as Map).cast<String, dynamic>();
          final ts = (reply['updatedAt'] as num?)?.toInt();
          if (ts != null) {
            abroadUpdatedAt = ts;
            abroadFingerprint = fp;
          }
        });
      }
    } catch (_) {
    } finally {
      if (mounted && generation == abroadGeneration) {
        setState(() => abroadBusy = false);
      }
    }
  }

  Future<void> refreshSites() async {
    if (sitesBusy) return;
    sitesScheduled?.cancel();
    sitesScheduled = null;
    final generation = ++sitesGeneration;
    final ep = endpoint;
    setState(() {
      sitesBusy = true;
      sitesChecked = true;
    });
    try {
      final reply = await configRequest(
        await configDirectory(),
        'network_sites',
        {'endpoint': ep},
      );
      if (!mounted || generation != sitesGeneration) return;
      if (reply is Map && reply['data'] is List) {
        setState(() {
          sitesInfo = (reply['data'] as List).cast<Item>();
          final ts = (reply['updatedAt'] as num?)?.toInt();
          if (ts != null) sitesUpdatedAt = ts;
        });
      }
    } catch (_) {
    } finally {
      if (mounted && generation == sitesGeneration) {
        setState(() => sitesBusy = false);
      }
    }
  }

  void _syncNetwork() {
    if (!_networkRunning) {
      final hadRunning = _networkWasRunning;
      _networkWasRunning = false;
      if (hadRunning ||
          domesticChecked ||
          abroadChecked ||
          sitesChecked ||
          exitsScheduled != null ||
          sitesScheduled != null ||
          proxyDebounce != null) {
        _resetNetwork(clearDisk: hadRunning);
      }
      return;
    }
    _networkWasRunning = true;
    if (_invalidateIfCoreRestarted()) {
      _networkCacheLoadedFor = endpoint;
    }
    if (_networkCacheLoadedFor != endpoint) {
      _restoreNetworkCache();
      return;
    }
    _checkCachedProxyFingerprint();
    if (tab != 0) return;
    if ((!domesticChecked || !abroadChecked) && exitsScheduled == null) {
      exitsScheduled = Timer(const Duration(seconds: 3), () {
        exitsScheduled = null;
        if (!mounted || !_networkRunning || tab != 0) return;
        if (!domesticChecked) refreshDirectExit();
        if (!abroadChecked) refreshProxyExit();
      });
    }
    if (!sitesChecked && sitesScheduled == null) {
      sitesScheduled = Timer(const Duration(seconds: 3), () {
        sitesScheduled = null;
        if (!mounted || !_networkRunning || tab != 0) return;
        if (!sitesChecked) refreshSites();
      });
    }
    if (proxyDirty &&
        proxyDueAt != null &&
        !DateTime.now().isBefore(proxyDueAt!)) {
      refreshProxyExit(resetCooldown: true);
    }
  }

  void _resetNetwork({bool clearDisk = false}) {
    domesticGeneration++;
    abroadGeneration++;
    sitesGeneration++;
    exitsScheduled?.cancel();
    exitsScheduled = null;
    sitesScheduled?.cancel();
    sitesScheduled = null;
    proxyDebounce?.cancel();
    proxyDebounce = null;
    proxyDirty = false;
    proxyDueAt = null;
    domesticInfo = null;
    abroadInfo = null;
    sitesInfo = [];
    domesticUpdatedAt = null;
    abroadUpdatedAt = null;
    sitesUpdatedAt = null;
    abroadFingerprint = '';
    _networkCacheLoadedFor = '';
    domesticChecked = abroadChecked = sitesChecked = false;
    domesticBusy = abroadBusy = sitesBusy = false;
    if (clearDisk) {
      configDirectory().then((dir) {
        configRequest(dir, 'network_cache_clear').catchError((_) => null);
      });
    }
  }

  bool _isAuthError(String err) =>
      err.contains('401') || err.contains('403') || err.contains('认证失败');

  Future<void> readSummary() async {
    if (_readingSummary) {
      _summaryPending = true;
      return;
    }
    _readingSummary = true;
    _summaryPending = false;
    _summaryRetryTimer?.cancel();
    _summaryRetryTimer = null;
    final revision = endpointRevision;
    try {
      final value = await clashRequest(endpoint, secret, 'summary') as Item;
      if (mounted && revision == endpointRevision) {
        setState(() {
          summary = value;
          summaryError = '';
          _syncNetwork();
          if (level == '跟随核心') _resubscribeLogs();
        });
      }
    } catch (e) {
      if (mounted && revision == endpointRevision) {
        final err = e.toString();
        setState(() {
          summary = {};
          summaryError = err;
          _syncNetwork();
        });
        final shouldRetry =
            timer != null &&
            !_isAuthError(err) &&
            ((preferences['nativeRootEnabled'] == true && rootRunning) ||
                connectionStatus == '已连接' ||
                trafficStatus == '已连接' ||
                logStatus == '已连接');
        if (shouldRetry) {
          _summaryRetryTimer = Timer(const Duration(milliseconds: 1200), () {
            _summaryRetryTimer = null;
            if (mounted && timer != null && summary.isEmpty) {
              unawaited(readSummary());
            }
          });
        }
      }
    } finally {
      _readingSummary = false;
      if (_summaryPending && mounted && timer != null) {
        _summaryPending = false;
        unawaited(readSummary());
      }
    }
  }

  void _syncRootComponentOnce(String dir) {
    if (_rootComponentSynced || preferences['nativeRootEnabled'] != true) return;
    _rootComponentSynced = true;
    unawaited(
      configRequest(dir, 'root_service_sync')
          .then((result) {
            if (mounted && result == 'updated') {
              showGlassToast(context, '服务脚本已自动更新', error: false);
            }
          })
          .catchError((Object _) {}),
    );
  }

  Future<void> _sampleRoot() async {
    if (_samplingRoot || preferences['nativeRootEnabled'] != true) return;
    _samplingRoot = true;
    final generation = _rootGeneration;
    try {
      final status = await configRequest(await configDirectory(), 'root_service_status') as Item;
      if (mounted && generation == _rootGeneration) {
        final hadState = rootService.isNotEmpty;
        final wasRunning = rootRunning;
        final nowRunning = status['state'] == 'running';
        setState(() {
          rootService = status;
          final uptime = status['uptimeSeconds'];
          if (nowRunning && uptime is num) {
            final nowSecs = DateTime.now().millisecondsSinceEpoch ~/ 1000;
            _coreStartedAt = nowSecs - uptime.toInt();
          } else if (!nowRunning) {
            _coreStartedAt = null;
          }
          if (wasRunning && !nowRunning) {
            summary = {};
            summaryError = '';
            _summaryRetryTimer?.cancel();
            _summaryRetryTimer = null;
          }
          _syncNetwork();
          final cpu = status['cpuPercent'];
          if (rootRunning && cpu is num) {
            appendSample(cpuTrend, (cpu * 10).round());
          }
        });
        if (hadState && !wasRunning && nowRunning && _rootAction == null) {
          setState(() {
            summary = {};
            summaryError = '';
            disconnect();
            connect();
          });
        } else if (nowRunning && summary.isEmpty && !_isAuthError(summaryError) && timer != null) {
          unawaited(readSummary());
        }
      }
    } catch (_) {
      if (mounted && generation == _rootGeneration && rootService['state'] != 'unknown') {
        setState(() {
          rootService = {'state': 'unknown'};
          _syncNetwork();
        });
      }
    } finally {
      _samplingRoot = false;
    }
  }

  Future<void> setMode(String mode) async {
    try {
      await clashRequest(endpoint, secret, 'set_mode', {'mode': mode});
      if (mounted) {
        _scheduleProxyRecheck();
        await readSummary();
      }
    } catch (e) {
      if (mounted) {
        showGlassToast(context, e.toString());
      }
    }
  }

  bool refreshingProfiles = false;
  Future<void> refreshDue() async {
    if (refreshingProfiles) return;
    refreshingProfiles = true;
    try {
      final issues =
          await configRequest(await configDirectory(), 'due') as List;
      if (issues.isNotEmpty && mounted) {
        showGlassToast(context, issues.join('\n'));
      }
    } catch (e) {
      if (mounted) {
        showGlassToast(context, '订阅更新失败 · $e');
      }
    } finally {
      refreshingProfiles = false;
    }
  }

  void _onTailscaleChanged() {
    if (mounted) setState(() {});
  }

  Future<void> _checkPendingShare() async {
    _shareRequested = true;
    if (_drainingShares) return;
    _drainingShares = true;
    var finishIfEmpty = false;
    try {
      while (mounted) {
        _shareRequested = false;
        final raw = await _platformChannel.invokeMethod<Map>('consumePendingShare', {
          'finishIfEmpty': finishIfEmpty,
        });
        if (!mounted) {
          if (raw != null) {
            await _platformChannel.invokeMethod<void>('clearStagedShare', {'batchDir': raw['batchDir']});
          }
          return;
        }
        if (raw == null) {
          if (_shareRequested) continue;
          break;
        }
        finishIfEmpty = await _openShareSheet(Map<String, dynamic>.from(raw));
      }
    } on PlatformException catch (e) {
      if (mounted) showGlassToast(context, '无法打开分享：${e.message}', error: true);
    } finally {
      _drainingShares = false;
    }
  }

  Future<bool> _openShareSheet(Map<String, dynamic> payload) async {
    final batchDir = payload['batchDir']?.toString() ?? '';
    final files = (payload['files'] as List? ?? const [])
        .whereType<Map>()
        .map((e) => Map<String, dynamic>.from(e))
        .toList();
    if (batchDir.isEmpty || files.isEmpty) return false;
    final fromExternal = payload['fromExternal'] == true;
    return await showGlassSheet<bool>(
      context: context,
      builder: (_) => TaildropQuickShareSheet(
        controller: _tailscale,
        batchDir: batchDir,
        files: files,
        fromExternal: fromExternal,
      ),
    ) ?? false;
  }

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    HardwareKeyboard.instance.addHandler(_logShortcut);
    _tailscale.addListener(_onTailscaleChanged);
    _platformChannel.setMethodCallHandler((call) async {
      if (call.method == 'taildropShareIncoming') {
        unawaited(_checkPendingShare());
      }
    });
    WidgetsBinding.instance.addPostFrameCallback((_) => _checkPendingShare());
    unawaited(_loadWallpaper());
    loadPreferences(initial: true);
  }

  Future<void> _loadWallpaper() async {
    try {
      final path = await _platformChannel.invokeMethod<String>('wallpaperPath');
      if (mounted) _wallpaper.value = path;
    } catch (_) {}
  }

  Future<void> loadPreferences({bool initial = false}) async {
    try {
      final dir = await configDirectory();
      final prefs =
          await configRequest(dir, 'preferences') as Map<String, dynamic>;
      if (!mounted) return;
      unawaited(_tailscale.sync());
      final previousEndpoint = endpoint, previousSecret = secret;
      if (preferences['nativeRootEnabled'] != prefs['nativeRootEnabled']) {
        _rootGeneration++;
        rootService = {};
        cpuTrend.clear();
      }
      preferences = prefs;
      _syncRootComponentOnce(dir);
      final apis = (prefs['clashApis'] as List).cast<Map<String, dynamic>>();
      if (apis.isNotEmpty) {
        final api = apis.firstWhere(
          (a) => a['id'] == prefs['activeClashApiId'],
          orElse: () => apis.first,
        );
        endpoint = api['url'];
        secret = api['secret'];
      } else {
        endpoint = 'http://127.0.0.1:9090';
        secret = '';
      }
      _theme.value = switch (prefs['theme']) {
        'light' => ThemeMode.light,
        'dark' => ThemeMode.dark,
        _ => ThemeMode.system,
      };
      final tint = prefs['glassTint'];
      _glassTint.value = tint is num ? tint.toDouble().clamp(0.0, 1.0) : 0.5;
      final fontScale = prefs['fontScale'];
      _fontScale.value =
          fontScale is num ? fontScale.toDouble().clamp(0.9, 1.4) : 1.0;
      final startPage = prefs['startPage'] as String? ?? '/home';
      final start = const ['/home', '/proxies', '/connections', '/logs', '/rules', '/settings']
          .indexOf(startPage == '/overview' ? '/home' : startPage);
      if (initial && start >= 0) _selectTab(start);
      if (!initial &&
          (previousEndpoint != endpoint || previousSecret != secret)) {
        disconnect();
        endpointRevision++;
        summary = {};
        metrics = {};
        frozenTopology = hoveredTopology = null;
        coreMemory = null;
        for (final trend in [
          connectionTrend,
          memoryTrend,
          downTrend,
          upTrend,
        ]) {
          trend.clear();
        }
        active = [];
        history = [];
        entries.clear();
        frozen = null;
        total = 0;
        connectionStatus = '连接中';
      }
      setupSaved = prefs['setupCompleted'] == true;
      if (initial && !setupSaved) {
        showSetupWizard = true;
        logStatus = '未连接';
        trafficStatus = '未连接';
        connectionStatus = '未连接';
      }
      if (initial && prefs['panelAutoCheckUpdate'] != false) {
        unawaited(
          checkPanelUpdate(dir)
              .then((info) async {
                if (mounted && !_drainingShares &&
                    (info['available'] == true || info['reinstall'] == true)) {
                  await showPanelUpdate(context, info);
                }
              })
              .catchError((Object _) {}),
        );
      }
    } catch (e) {
      if (mounted) {
        showGlassToast(context, '读取偏好失败 · $e');
      }
    }
    if (!mounted) return;
    setState(() {
      ready = true;
      if (WidgetsBinding.instance.lifecycleState != AppLifecycleState.paused) {
        connect();
      }
    });
  }

  String _normalizeCoreLevel(String? raw) {
    const valid = ['panic', 'fatal', 'error', 'warn', 'info', 'debug', 'trace'];
    final v = (raw ?? 'info').trim().toLowerCase();
    if (v == 'warning') return 'warn';
    return valid.contains(v) ? v : 'info';
  }

  String _effectiveLogLevel() {
    final coreLvl = _normalizeCoreLevel(summary['config']?['log-level'] as String?);
    return level == '跟随核心' ? coreLvl : level;
  }

  void _resubscribeLogs() {
    if (!ready || !setupSaved || timer == null) return;
    final next = _effectiveLogLevel();
    if (_logSubscribedLevel == next && logs != null) return;
    try {
      final replacement = NativeStream.logs(endpoint, secret, next);
      logs?.close();
      logs = replacement;
      _logSubscribedLevel = next;
      _logGapSince = null;
    } catch (_) {}
  }

  void connect() {
    if (!ready || !setupSaved) return;
    if (preferences['nativeRootEnabled'] == true && rootTimer == null) {
      unawaited(_sampleRoot());
      rootTimer = Timer.periodic(const Duration(seconds: 2), (_) => _sampleRoot());
    } else if (preferences['nativeRootEnabled'] != true) {
      rootTimer?.cancel();
      rootTimer = null;
    }
    if (preferences['nativeRootEnabled'] == true) {
      profileTimer ??= Timer.periodic(
        const Duration(minutes: 5),
        (_) => refreshDue(),
      );
      unawaited(refreshDue());
    } else {
      profileTimer?.cancel();
      profileTimer = null;
    }
    if (timer != null) return;
    unawaited(readSummary());
    summaryTimer ??= Timer.periodic(
      const Duration(seconds: 30),
      (_) => readSummary(),
    );
    try {
      final eff = _effectiveLogLevel();
      logs = NativeStream.logs(endpoint, secret, eff);
      _logSubscribedLevel = eff;
      traffic = NativeStream(endpoint, secret, traffic: true);
      connections = NativeStream.connections(endpoint, secret);
      connectionStatus = logStatus = trafficStatus = '连接中';
      timer = Timer.periodic(const Duration(milliseconds: 150), (_) => poll());
    } catch (e) {
      disconnect();
      connectionStatus = logStatus = trafficStatus = e.toString();
    }
  }

  void poll() {
    if (logs == null || traffic == null || connections == null) return;
    final Map<String, dynamic> batch;
    final Map<String, dynamic> sample;
    final Map<String, dynamic> snapshot;
    try {
      batch = logs!.drain();
      sample = traffic!.drain();
      snapshot = connections!.drain();
    } catch (_) {
      return;
    }
    final lines = (batch['logs'] as List? ?? const []).cast<Map<String, dynamic>>();
    final samples = sample['traffic'] as List? ?? const [];
    final nextFailures = (batch['failures'] as num?)?.toInt() ?? 0;
    final trafficFailures = (sample['failures'] as num?)?.toInt() ?? 0;
    final connectionFailures = (snapshot['failures'] as num?)?.toInt() ?? 0;
    final statusChanged =
        (metrics.isNotEmpty && snapshot['has_frame'] != true) ||
        (hasTraffic && sample['has_frame'] != true) ||
        connectionStatus != snapshot['status'] ||
        logStatus != batch['status'] ||
        _logFailures != nextFailures ||
        _trafficFailures != trafficFailures ||
        _connectionFailures != connectionFailures ||
        trafficStatus != sample['status'];
    final hasConnections = snapshot['connections'] != null;
    final hasClosed = snapshot['closed'] != null;
    final hasMemory =
        snapshot['memory'] != null ||
        (coreMemory != null && snapshot['memory_open'] != true);
    final dropped = (batch['dropped'] as num?)?.toInt() ?? 0;
    final hasLogs = lines.isNotEmpty || dropped > 0;
    if (!statusChanged &&
        !hasConnections &&
        !hasClosed &&
        !hasMemory &&
        lines.isEmpty &&
        samples.isEmpty) {
      return;
    }
    final prevLogStatus = logStatus;
    final prevTrafficStatus = trafficStatus;
    final prevConnectionStatus = connectionStatus;
    logStatus = batch['status'] as String? ?? logStatus;
    _logFailures = nextFailures;
    _trafficFailures = trafficFailures;
    _connectionFailures = connectionFailures;
    trafficStatus = sample['status'] as String? ?? trafficStatus;
    connectionStatus = snapshot['status'] as String? ?? connectionStatus;
    final reconnected =
        (prevConnectionStatus != '已连接' && connectionStatus == '已连接') ||
        (prevTrafficStatus != '已连接' && trafficStatus == '已连接') ||
        (prevLogStatus != '已连接' && logStatus == '已连接');
    if (reconnected) {
      _reconnectGeneration++;
      unawaited(readSummary());
    }
    _syncNetwork();
    if (snapshot['connections'] case final Map data) {
      active = (data['connections'] as List? ?? []).cast<Item>();
      metrics = data['metrics'] as Item? ?? {};
      appendSample(connectionTrend, active.length);
    }
    if (snapshot['memory'] case final num bytes) {
      coreMemory = bytes.toInt();
      appendSample(memoryTrend, bytes.toInt());
    }
    if (snapshot['memory_open'] != true) coreMemory = null;
    if (snapshot['has_frame'] != true) {
      active = [];
      metrics = {};
      frozenTopology = hoveredTopology = null;
    }
    if (snapshot['closed'] case final List finished) {
      history = [...finished.cast<Item>(), ...history].take(500).toList();
    }
    if (sample['has_frame'] != true) hasTraffic = false;
    var addedLogMarker = false;
    if (prevLogStatus == '已连接' && logStatus != '已连接') {
      final now = DateTime.now();
      _logGapSince ??=
          '${now.hour.toString().padLeft(2, '0')}:${now.minute.toString().padLeft(2, '0')}:${now.second.toString().padLeft(2, '0')}';
    } else if (logStatus == '已连接') {
      if (_logGapSince != null && entries.isNotEmpty) {
        final now = DateTime.now();
        final end =
            '${now.hour.toString().padLeft(2, '0')}:${now.minute.toString().padLeft(2, '0')}:${now.second.toString().padLeft(2, '0')}';
        entries.add({
          'seq': _logSeq++,
          'type': 'marker',
          'time': now.millisecondsSinceEpoch,
          'payload': '与核心的连接中断，期间的日志未收到（$_logGapSince–$end）',
        });
        addedLogMarker = true;
      }
      _logGapSince = null;
    }
    total += lines.length + dropped;
    for (final raw in lines) {
      entries.add({...raw, 'seq': _logSeq++});
    }
    if (entries.length > limit) {
      entries.removeRange(0, entries.length - limit);
    }
    if (lines.isNotEmpty && tab == 3 && frozen == null && _logAutoScroll) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (_logScroll.hasClients && _logAutoScroll) {
          _logScroll.jumpTo(_logScroll.position.maxScrollExtent);
        }
      });
    }
    final hasTrafficSample = samples.isNotEmpty && trafficStatus == '已连接';
    if (hasTrafficSample) {
      hasTraffic = true;
      up = (samples.last['up'] as num?)?.toInt() ?? 0;
      down = (samples.last['down'] as num?)?.toInt() ?? 0;
      for (final value in samples) {
        appendSample(downTrend, (value['down'] as num?)?.toInt() ?? 0);
        appendSample(upTrend, (value['up'] as num?)?.toInt() ?? 0);
      }
    }
    final shouldRebuild = statusChanged ||
        (!_userSwiping &&
            _targetPage == null &&
            switch (tab) {
              0 => hasConnections || hasMemory || hasTrafficSample,
              1 => false,
              2 => hasConnections || hasClosed,
              3 => hasLogs || addedLogMarker,
              _ => false,
            });
    if (shouldRebuild) setState(() {});
  }

  void disconnect() {
    _resetNetwork();
    rootTimer?.cancel();
    rootTimer = null;
    _rootGeneration++;
    _trafficFailures = _connectionFailures = 0;
    summaryTimer?.cancel();
    summaryTimer = null;
    _summaryRetryTimer?.cancel();
    _summaryRetryTimer = null;
    _summaryPending = false;
    profileTimer?.cancel();
    profileTimer = null;
    timer?.cancel();
    timer = null;
    logs?.close();
    traffic?.close();
    connections?.close();
    connections = null;
    logs = traffic = null;
    hasTraffic = false;
    active = [];
    metrics = {};
    frozenTopology = hoveredTopology = null;
    coreMemory = null;
    connectionStatus = trafficStatus = logStatus = '未连接';
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      setState(connect);
    } else if (state == AppLifecycleState.paused ||
        state == AppLifecycleState.detached) {
      if (timer != null) poll();
      disconnect();
    }
  }

  @override
  void dispose() {
    clearedTimer?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    HardwareKeyboard.instance.removeHandler(_logShortcut);
    _platformChannel.setMethodCallHandler(null);
    _tailscale.removeListener(_onTailscaleChanged);
    _tailscale.dispose();
    _logSearchCtrl.dispose();
    _pager.dispose();
    _headerScroll.dispose();
    _swipeProgress.dispose();
    _pageFade.dispose();
    _logScroll.dispose();
    _logSearchFocus.dispose();
    disconnect();
    super.dispose();
  }

  Future<void> openTools([int initial = 0]) async {
    await Navigator.push(
      context,
      MaterialPageRoute(
        builder: (_) =>
            ToolsPage(initial: initial, onSaved: () => loadPreferences()),
      ),
    );
    if (mounted) await loadPreferences();
  }

  Future<void> probe() async {
    setState(() => probing = true);
    try {
      final reply = await configRequest(await configDirectory(), 'tool', {
        'action': 'root_status',
        'values': <String, String>{},
      });
      final result = reply['text'] as String;
      if (mounted) setState(() => rootStatus = result);
    } catch (error) {
      if (mounted) setState(() => rootStatus = '读取失败：$error');
    } finally {
      if (mounted) setState(() => probing = false);
    }
  }

  Future<void> rootOperation(String action, String label) async {
    if (probing) return;
    if (action != 'root_start') {
      final allowed = await showGlassDialog<bool>(
        context: context,
        builder: (c) => GlassConfirmDialog(
          title: Text('$label？'),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(c, false),
              child: const Text('取消'),
            ),
            FilledButton(
              onPressed: () => Navigator.pop(c, true),
              child: Text(label),
            ),
          ],
        ),
      );
      if (allowed != true || !mounted) return;
    }
    setState(() {
      probing = true;
      _rootAction = action;
      if (action == 'root_stop' || action == 'root_restart') {
        summary = {};
        summaryError = '';
        _summaryRetryTimer?.cancel();
        _summaryRetryTimer = null;
        _coreStartedAt = null;
        _resetNetwork(clearDisk: true);
        _syncNetwork();
      }
    });
    try {
      final reply = await configRequest(await configDirectory(), 'tool', {
        'action': action,
        'values': <String, String>{},
      });
      if (mounted) {
        setState(() => rootStatus = reply['text']);
        await _sampleRoot();
        if (mounted) {
          if (action == 'root_stop') {
            setState(() {
              summary = {};
              summaryError = '';
              _summaryRetryTimer?.cancel();
              _summaryRetryTimer = null;
              _syncNetwork();
            });
          } else {
            setState(() {
              summary = {};
              summaryError = '';
              disconnect();
              connect();
            });
          }
        }
      }
    } catch (e) {
      if (mounted) {
        setState(() => rootStatus = e.toString());
        showGlassToast(context, e.toString(), error: true);
      }
    } finally {
      if (mounted) {
        setState(() {
          probing = false;
          _rootAction = null;
        });
      }
    }
  }

  (String, String) _splitRoute(String route) {
    final parts = route
        .split(RegExp(r'\s*(?:→|›|>)\s*'))
        .where((s) => s.trim().isNotEmpty)
        .toList();
    if (parts.isEmpty) return ('', '');
    if (parts.length == 1) return (parts.first, '');
    return (parts.last, parts.sublist(0, parts.length - 1).join(' › '));
  }

  String _uptime(int seconds) {
    final days = seconds ~/ 86400;
    final hours = seconds ~/ 3600 % 24;
    final minutes = seconds ~/ 60 % 60;
    final sec = (seconds % 60).toString().padLeft(2, '0');
    if (days == 0 && hours == 0) return '$minutes:$sec';
    final clock = '$hours:${minutes.toString().padLeft(2, '0')}:$sec';
    return days > 0 ? '${days}天 $clock' : clock;
  }

  String _modeLabel(String mode) => switch (mode.toLowerCase()) {
    'rule' => '规则模式',
    'global' => '全局模式',
    'direct' => '直连模式',
    '' => '',
    _ => '$mode 模式',
  };

  Widget _ipRow(
    SingboardColors c,
    String label,
    String source,
    Item? data, {
    required bool busy,
    String? pendingText,
  }) {
    final status = data?['status'] as String?;
    String primaryText;
    String ipText = '';
    bool isErr = false;
    if (busy) {
      primaryText = '检测中…';
    } else if (pendingText != null) {
      primaryText = pendingText;
    } else if (status == 'done') {
      final locs = (data?['location'] as List? ?? [])
          .whereType<String>()
          .toList();
      primaryText = showIp
          ? (locs.isEmpty ? '位置未知' : locs.join(' '))
          : (locs.isEmpty ? '位置未知' : locs.first);
      final rawIp = data?['ip'] as String? ?? '';
      ipText = showIp ? rawIp : maskIp(rawIp);
    } else if (status == 'error') {
      isErr = true;
      primaryText = '检测失败：${data?['error'] ?? '未知错误'}';
    } else {
      primaryText = '未检测';
    }
    return Row(
      crossAxisAlignment: CrossAxisAlignment.baseline,
      textBaseline: TextBaseline.alphabetic,
      children: [
        SizedBox(
          width: 76,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(label, style: TextStyle(fontSize: 13, height: 1.5, color: c.muted)),
              Text(
                source,
                style: TextStyle(
                  fontSize: 11,
                  height: 1.5,
                  color: c.muted.withValues(alpha: 0.7),
                ),
              ),
            ],
          ),
        ),
        const SizedBox(width: 12),
        Expanded(
          child: Wrap(
            alignment: WrapAlignment.spaceBetween,
            crossAxisAlignment: WrapCrossAlignment.center,
            spacing: 12,
            runSpacing: 2,
            children: [
              Text(
                primaryText,
                style: TextStyle(
                  fontSize: isErr ? 12 : ipText.isEmpty ? 13 : 14,
                  fontWeight: ipText.isEmpty ? FontWeight.normal : FontWeight.w500,
                  color: isErr ? c.dangerInk : ipText.isEmpty ? c.muted : c.ink,
                ),
              ),
              if (ipText.isNotEmpty)
                Text(
                  ipText,
                  textAlign: TextAlign.end,
                  style: TextStyle(
                    fontSize: 12,
                    fontFamily: 'monospace',
                    color: c.muted,
                  ),
                ),
            ],
          ),
        ),
      ],
    );
  }

  EdgeInsets _pageInsets() {
    final width = MediaQuery.sizeOf(context).width;
    final safe = MediaQuery.viewPaddingOf(context);
    final margin = math.max(width >= 768 ? 24.0 : 12.0,
        (width - safe.left - safe.right - 1024) / 2);
    return EdgeInsets.only(left: margin + safe.left, right: margin + safe.right);
  }

  Widget overview() {
    final c = SingboardColors.of(context);
    final inset = _pageInsets();
    final wide = MediaQuery.sizeOf(context).width - inset.horizontal >= 640;
    final config = summary['config'] as Item? ?? {};
    final modeRaw = config['mode'] as String? ?? '';
    final modeText = _modeLabel(modeRaw);
    final exitHeading =
        (modeRaw.toLowerCase() == 'global' || modeRaw.toLowerCase() == 'direct')
        ? '当前出口'
        : '默认代理';
    final activeMetrics = frozenTopology ?? hoveredTopology ?? metrics;
    final trafficLive = _coreAvailable && trafficStatus == '已连接';
    final connectionsOpen = _coreAvailable && connectionStatus == '已连接';
    final connectionsLive = connectionsOpen && metrics.isNotEmpty;
    final connectionCount =
        (metrics['connectionCount'] as int?) ?? active.length;
    final live = overviewLive;
    final rootEnabled = preferences['nativeRootEnabled'] == true;
    final stopped = rootEnabled && !rootRunning;
    final missingDir = stopped && (preferences['workingDir'] as String? ?? '').trim().isEmpty;
    final unknown = stopped && rootService['state'] == 'unknown';
    final canStart = stopped && !unknown && !missingDir;
    final connecting =
        overviewConnecting || (rootEnabled && rootService.isEmpty);
    final authFailed =
        trafficStatus.contains('认证失败') || connectionStatus.contains('认证失败');
    final showNotice = !live && !connecting;
    final failures = math.max(_trafficFailures, _connectionFailures);
    final uptime = rootRunning ? rootService['uptimeSeconds'] as int? : null;
    final stateLabel = connecting
        ? '正在连接 sing-box…'
        : live
        ? (rootEnabled
              ? (uptime == null ? '运行中' : '运行中 ${_uptime(uptime)}')
              : '已连接')
        : (missingDir
              ? '还没有指定工作目录'
              : unknown
              ? '无法获取 sing-box 状态'
              : stopped
              ? 'sing-box 未运行'
              : authFailed
              ? 'Clash API 密钥不匹配'
              : '连不上 sing-box');
    final tcp = (metrics['tcp'] as num?)?.toInt() ?? 0;
    final udp = (metrics['udp'] as num?)?.toInt() ?? 0;
    final sites = sitesInfo;
    final flowNodes = (activeMetrics['flow']?['nodes'] as List? ?? [])
        .cast<Item>();
    final flowConnCount =
        (activeMetrics['flow']?['connectionCount'] as num?)?.toInt() ??
        (connectionsLive ? active.length : 0);

    String exitNode = '';
    String viaChain = '';
    bool exitDirect = false;
    int? exitLatency;
    if (live) {
      final exitObj = summary['exit'];
      if (exitObj is Map) {
        exitNode = exitObj['node']?.toString() ?? '';
        exitDirect = exitObj['direct'] == true;
        viaChain = (exitObj['via'] as List? ?? []).whereType<String>().join(
          ' › ',
        );
        exitLatency = (exitObj['latency'] as num?)?.toInt();
      } else {
        final (node, via) = _splitRoute(summary['route']?.toString() ?? '');
        exitNode = node;
        viaChain = via;
        exitDirect = node.toUpperCase() == 'DIRECT';
      }
    }
    final hasExit = live && exitNode.isNotEmpty;
    final exitDisplay = !hasExit ? '—' : (exitDirect ? '直连' : exitNode);
    final viaDisplay = !hasExit
        ? ''
        : (viaChain.isNotEmpty ? viaChain : (exitDirect ? '不经过代理' : ''));

    Color latencyDot(int? ms) {
      if (ms == null) return c.muted;
      if (ms == 0) return c.error;
      if (ms < 300) return c.success;
      if (ms < 800) return c.warning;
      return c.error;
    }

    final (dlNum, dlUnit) = trafficLive && hasTraffic
        ? formatSpeedParts(down)
        : ('—', '');
    final (ulNum, ulUnit) = trafficLive && hasTraffic
        ? formatSpeedParts(up)
        : ('—', '');
    final memVal = trafficLive ? coreMemory : null;
    final (memNum, memUnit) = memVal == null
        ? ('—', '')
        : formatBytesParts(memVal);
    final verRaw = summary['version']?.toString().trim() ?? '';
    final activeApiName =
        ((preferences['clashApis'] as List? ?? [])
            .cast<Map>()
            .where((a) => a['id'] == preferences['activeClashApiId'])
            .firstOrNull?['name']
            ?.toString()) ??
        '';
    final metaDetail = rootEnabled
        ? (verRaw.isEmpty
              ? ''
              : (verRaw.toLowerCase().startsWith('sing-box')
                    ? verRaw
                    : 'sing-box $verRaw'))
        : activeApiName;

    List<Item> rankLayer(int layer) {
      final rows = flowNodes
          .where((n) => (n['layer'] as num?)?.toInt() == layer)
          .toList();
      rows.sort((a, b) {
        final am = ((a['merged'] as num?)?.toInt() ?? 0) > 0 ? 1 : 0;
        final bm = ((b['merged'] as num?)?.toInt() ?? 0) > 0 ? 1 : 0;
        if (am != bm) return am.compareTo(bm);
        final ac = (a['count'] as num?)?.toInt() ?? 0;
        final bc = (b['count'] as num?)?.toInt() ?? 0;
        if (ac != bc) return bc.compareTo(ac);
        return (a['name']?.toString() ?? '').compareTo(
          b['name']?.toString() ?? '',
        );
      });
      return rows;
    }

    final exitRanking = rankLayer(3);
    final ruleRanking = rankLayer(1);
    final exitTotal = exitRanking.fold<int>(
      0,
      (s, n) => s + math.max(1, (n['merged'] as num?)?.toInt() ?? 0),
    );
    final topoSummaryBase = flowConnCount == 0
        ? '暂无连接'
        : '$flowConnCount 条连接经 $exitTotal 个出口'
              '${exitRanking.isNotEmpty ? '，最多的是 ${exitRanking.first['name']}（${exitRanking.first['count']} 条）' : ''}';
    final topoSummary = frozenTopology != null
        ? '已暂停更新 · $topoSummaryBase'
        : topoSummaryBase;

    Widget rankSection(String title, List<Item> rows) {
      final denom = math.max(1, flowConnCount).toDouble();
      return Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.only(bottom: 6),
            child: Text(
              title,
              style: TextStyle(
                fontSize: 12,
                fontWeight: FontWeight.w500,
                color: c.muted,
              ),
            ),
          ),
          for (final row in rows)
            Padding(
              padding: const EdgeInsets.only(top: 5, bottom: 9),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Expanded(
                        child: Text(
                          row['name']?.toString() ?? '',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(
                            fontSize: 13,
                            color: ((row['merged'] as num?)?.toInt() ?? 0) > 0
                                ? c.muted
                                : c.ink,
                          ),
                        ),
                      ),
                      const SizedBox(width: 12),
                      Text(
                        '${row['count']} 条',
                        style: TextStyle(
                          fontSize: 12,
                          color: c.muted,
                          fontFeatures: const [FontFeature.tabularFigures()],
                        ),
                      ),
                    ],
                  ),
                  const SizedBox(height: 6),
                  ClipRRect(
                    borderRadius: BorderRadius.circular(999),
                    child: LinearProgressIndicator(
                      value: (((row['count'] as num?)?.toDouble() ?? 0) / denom)
                          .clamp(0.02, 1.0),
                      minHeight: 3,
                      backgroundColor: c.fill,
                      color: c.chartInk.withValues(alpha: 0.40),
                    ),
                  ),
                ],
              ),
            ),
        ],
      );
    }

    final cpu = rootRunning ? rootService['cpuPercent'] as num? : null;
    final resident = rootRunning ? rootService['memoryBytes'] as num? : null;
    final memoryDetail = rootEnabled
        ? (resident == null ? '' : '进程常驻\n${formatBytes(resident)}')
        : '运行时占用';
    final statDivider = VerticalDivider(
      width: 0.5,
      thickness: 0.5,
      indent: 14,
      endIndent: 14,
      color: c.hairline,
    );
    Widget stat(
      String title,
      String value,
      String unit,
      String detail,
      List<int> samples,
      int floor, {
      VoidCallback? onTap,
    }) {
      final body = Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: EdgeInsets.fromLTRB(wide ? 18 : 14, wide ? 14 : 12, wide ? 18 : 14, 8),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(title, style: TextStyle(fontSize: 12, color: c.muted)),
                const SizedBox(height: 2),
                Text.rich(
                  TextSpan(
                    text: value,
                    children: [
                      if (unit.isNotEmpty)
                        TextSpan(
                          text: ' $unit',
                          style: TextStyle(
                            fontSize: 12.5,
                            fontWeight: FontWeight.normal,
                            color: c.muted,
                          ),
                        ),
                    ],
                  ),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: wide ? 19 : 18,
                    fontWeight: FontWeight.w500,
                    color: value == '—' ? c.muted : c.ink,
                    fontFeatures: const [FontFeature.tabularFigures()],
                  ),
                ),
                SizedBox(
                  height: !wide && (connectionsLive || (memVal != null && resident != null)) ? 34 : 17,
                  child: Text(
                    wide ? detail.replaceAll('\n', ' · ') : detail,
                    maxLines: wide ? 1 : 2,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(fontSize: wide ? 12 : 11.5, height: 1.45, color: c.muted),
                  ),
                ),
              ],
            ),
          ),
          SizedBox(
            height: 30,
            width: double.infinity,
            child: CustomPaint(
              painter: MiniSparkPainter(
                samples: List.of(samples),
                floor: floor,
                color: c.chartInk,
              ),
            ),
          ),
        ],
      );
      return Expanded(
        child: onTap == null ? body : InkWell(onTap: onTap, child: body),
      );
    }

    final exitBlock = !showNotice
        ? GestureDetector(
            behavior: HitTestBehavior.opaque,
            onTap: () => _selectTab(1),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  exitHeading,
                  style: TextStyle(fontSize: 12, color: c.muted),
                ),
                const SizedBox(height: 3),
                Row(
                  crossAxisAlignment: CrossAxisAlignment.baseline,
                  textBaseline: TextBaseline.alphabetic,
                  children: [
                    Flexible(
                      child: Text(
                        exitDisplay,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          fontSize: wide ? 28 : 26,
                          fontWeight: FontWeight.w600,
                          letterSpacing: -0.4,
                          color: hasExit ? c.ink : c.muted,
                        ),
                      ),
                    ),
                    if (exitLatency != null) ...[
                      const SizedBox(width: 12),
                      Row(mainAxisSize: MainAxisSize.min, children: [
                      Container(
                        width: 7,
                        height: 7,
                        decoration: BoxDecoration(
                          shape: BoxShape.circle,
                          color: latencyDot(exitLatency),
                        ),
                      ),
                      const SizedBox(width: 6),
                      Text(
                        exitLatency == 0 ? '失败' : '${exitLatency}ms',
                        style: TextStyle(
                          fontSize: 13,
                          color: c.muted,
                          fontFeatures: const [FontFeature.tabularFigures()],
                        ),
                      ),
                      ]),
                    ],
                  ],
                ),
                if (viaDisplay.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(top: 4),
                    child: Text(
                      viaDisplay,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(fontSize: 13, color: c.muted),
                    ),
                  ),
              ],
            ),
          )
        : Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                missingDir
                    ? '在设置里选择 sing-box 的工作目录后才能启动。'
                    : unknown
                    ? '可以在设置中检查工作目录。'
                    : stopped
                    ? '启动后显示出口、实时流量和内存。'
                    : authFailed
                    ? '请在设置里修改 Clash API 密钥。地址 $endpoint'
                    : '已连续 $failures 次连接失败，正在自动重试。地址 $endpoint',
                style: TextStyle(fontSize: 13, height: 1.6, color: c.muted),
              ),
              const SizedBox(height: 14),
              GestureDetector(
                onTap: canStart
                    ? () => rootOperation('root_start', '启动核心')
                    : () => _selectTab(5),
                child: Container(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 20,
                    vertical: 8,
                  ),
                  decoration: BoxDecoration(
                    color: canStart ? c.primary : c.fill,
                    borderRadius: BorderRadius.circular(10),
                  ),
                  child: Text(
                    canStart ? '启动 sing-box' : stopped ? '前往设置' : '检查 API 配置',
                    style: TextStyle(
                      fontSize: 14,
                      fontWeight: FontWeight.w500,
                      color: canStart ? Colors.white : c.ink,
                    ),
                  ),
                ),
              ),
            ],
          );
    final rates = GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: () => _selectTab(2),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    PanelIcon('download', size: 12, color: c.muted),
                    const SizedBox(width: 4),
                    Text('下载', style: TextStyle(fontSize: 12, color: c.muted)),
                  ],
                ),
                Row(
                  crossAxisAlignment: CrossAxisAlignment.baseline,
                  textBaseline: TextBaseline.alphabetic,
                  children: [
                    Text(
                      dlNum,
                      style: TextStyle(
                        fontSize: wide ? 32 : 28,
                        fontWeight: FontWeight.w300,
                        letterSpacing: -0.5,
                        color: trafficLive && hasTraffic ? c.ink : c.muted,
                        fontFeatures: const [FontFeature.tabularFigures()],
                      ),
                    ),
                    if (dlUnit.isNotEmpty) ...[
                      const SizedBox(width: 4),
                      Text(
                        dlUnit,
                        style: TextStyle(fontSize: 14, color: c.muted),
                      ),
                    ],
                  ],
                ),
                Text(
                  '总计 ${connectionsLive && metrics['download'] != null ? formatBytes(metrics['download'] as num) : '—'}',
                  style: TextStyle(fontSize: 12, color: c.muted),
                ),
              ],
            ),
          ),
          SizedBox(width: wide ? 40 : 28),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Row(
                  children: [
                    PanelIcon('upload', size: 12, color: c.muted),
                    const SizedBox(width: 4),
                    Text('上传', style: TextStyle(fontSize: 12, color: c.muted)),
                  ],
                ),
                Row(
                  crossAxisAlignment: CrossAxisAlignment.baseline,
                  textBaseline: TextBaseline.alphabetic,
                  children: [
                    Text(
                      ulNum,
                      style: TextStyle(
                        fontSize: wide ? 32 : 28,
                        fontWeight: FontWeight.w300,
                        letterSpacing: -0.5,
                        color: trafficLive && hasTraffic ? c.ink : c.muted,
                        fontFeatures: const [FontFeature.tabularFigures()],
                      ),
                    ),
                    if (ulUnit.isNotEmpty) ...[
                      const SizedBox(width: 4),
                      Text(
                        ulUnit,
                        style: TextStyle(fontSize: 14, color: c.muted),
                      ),
                    ],
                  ],
                ),
                Text(
                  '总计 ${connectionsLive && metrics['upload'] != null ? formatBytes(metrics['upload'] as num) : '—'}',
                  style: TextStyle(fontSize: 12, color: c.muted),
                ),
              ],
            ),
          ),
        ],
      ),
    );

    final exitsBusy = domesticBusy || abroadBusy;
    final exitTs = [domesticUpdatedAt, abroadUpdatedAt]
        .whereType<int>()
        .fold<int?>(null, (a, b) => a == null || b > a ? b : a);
    final exitAge = _formatRelativeAge(exitTs);
    final sitesAge = _formatRelativeAge(sitesUpdatedAt);
    final padX = wide ? 18.0 : 16.0;

    final domPending = !domesticBusy && !domesticChecked && exitsScheduled != null
        ? '即将自动检测'
        : null;
    final abrPending = !abroadBusy
        ? (proxyDirty
              ? (abroadInfo != null ? '节点已切换，即将重检' : '即将自动检测')
              : (!abroadChecked && exitsScheduled != null ? '即将自动检测' : null))
        : null;

    Widget sectionPill(
      String label, {
      VoidCallback? onTap,
      bool active = false,
      bool muted = false,
    }) {
      return GestureDetector(
        onTap: onTap,
        child: GlassControl(
          radius: 999,
          tint: active ? c.primarySoft : c.fill,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 2),
            child: Text(
              label,
              style: TextStyle(
                fontSize: 12,
                fontWeight: FontWeight.w500,
                color: muted ? c.muted : c.selectionInk,
              ),
            ),
          ),
        ),
      );
    }

    final exitHeader = Padding(
      padding: EdgeInsets.fromLTRB(padX, 10, padX, 6),
      child: Row(
        children: [
          Text(
            '出口 IP',
            style: TextStyle(
              fontSize: 12,
              fontWeight: FontWeight.w500,
              color: c.muted,
            ),
          ),
          if (exitAge.isNotEmpty && !exitsBusy) ...[
            const SizedBox(width: 6),
            Text(
              '· $exitAge',
              style: TextStyle(
                fontSize: 11,
                color: c.muted.withValues(alpha: 0.8),
              ),
            ),
          ],
          const Spacer(),
          if (_networkRunning) ...[
            sectionPill(
              '显示 IP',
              active: showIp,
              onTap: () => setState(() => showIp = !showIp),
            ),
            const SizedBox(width: 6),
            sectionPill(
              exitsBusy ? '检测中…' : '检测',
              muted: exitsBusy,
              onTap: exitsBusy ? null : refreshExits,
            ),
          ],
        ],
      ),
    );

    final siteHeader = Padding(
      padding: EdgeInsets.fromLTRB(padX, 10, padX, 6),
      child: Row(
        children: [
          Text(
            '连通性测试',
            style: TextStyle(
              fontSize: 12,
              fontWeight: FontWeight.w500,
              color: c.muted,
            ),
          ),
          if (sitesAge.isNotEmpty && !sitesBusy) ...[
            const SizedBox(width: 6),
            Text(
              '· $sitesAge',
              style: TextStyle(
                fontSize: 11,
                color: c.muted.withValues(alpha: 0.8),
              ),
            ),
          ],
          const Spacer(),
          if (_networkRunning)
            sectionPill(
              sitesBusy ? '测速中…' : '测速',
              muted: sitesBusy,
              onTap: sitesBusy ? null : refreshSites,
            ),
        ],
      ),
    );

    final ips = Column(
      mainAxisSize: MainAxisSize.min,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        exitHeader,
        Divider(
          height: 0.5,
          thickness: 0.5,
          indent: padX,
          endIndent: padX,
          color: c.hairline,
        ),
        Padding(
          padding: EdgeInsets.symmetric(horizontal: padX, vertical: wide ? 11 : 10),
          child: _ipRow(
            c,
            '直连出口',
            'ipip.net',
            domesticInfo,
            busy: domesticBusy,
            pendingText: domPending,
          ),
        ),
        Divider(height: 0.5, thickness: 0.5, indent: 16, color: c.hairline),
        Padding(
          padding: EdgeInsets.symmetric(horizontal: padX, vertical: wide ? 11 : 10),
          child: _ipRow(
            c,
            '代理出口',
            'ip.sb',
            abroadInfo,
            busy: abroadBusy,
            pendingText: abrPending,
          ),
        ),
      ],
    );
    final siteRows = <Widget>[
      for (final (key, label) in const [
        ('wechat', '微信'),
        ('bilibili', 'Bilibili'),
        ('github', 'GitHub'),
        ('cloudflare', 'Cloudflare'),
        ('youtube', 'YouTube'),
      ])
        Builder(
          builder: (_) {
            final item = sites.where((s) => s['key'] == key).firstOrNull;
            final status = sitesBusy ? 'pending' : (item?['status'] as String?);
            final reason = status == 'error'
                ? (item?['error'] as String?)
                : null;
            final ms = (item?['ms'] as num?)?.toInt();
            final dotColor = switch (status) {
              'done' => latencyDot(ms),
              'error' => c.error,
              _ => c.muted,
            };
            final msText = switch (status) {
              'done' => ms == 0 ? '失败' : '${ms ?? 0}ms',
              'error' => '失败',
              _ => sitesBusy || sitesScheduled != null ? '测速中' : '未测',
            };
            return Padding(
              padding: EdgeInsets.symmetric(horizontal: padX, vertical: wide ? 7 : 8),
              child: Row(
                mainAxisAlignment: MainAxisAlignment.spaceBetween,
                children: [
                  Text(
                      label,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(fontSize: 13, height: 1.5, color: c.ink),
                  ),
                  if (reason != null && reason.isNotEmpty)
                    Expanded(
                      child: Tooltip(
                        message: reason,
                        child: Padding(
                          padding: const EdgeInsets.symmetric(horizontal: 10),
                          child: Text(
                            reason,
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                            textAlign: TextAlign.end,
                            style: TextStyle(fontSize: 12, height: 1.5, color: c.muted),
                          ),
                        ),
                      ),
                    ),
                  Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Container(
                        width: 7,
                        height: 7,
                        decoration: BoxDecoration(
                          shape: BoxShape.circle,
                          color: dotColor,
                        ),
                      ),
                      const SizedBox(width: 6),
                      ConstrainedBox(
                        constraints: const BoxConstraints(minWidth: 48),
                        child: Text(
                          msText,
                          maxLines: 1,
                          softWrap: false,
                          style: TextStyle(
                            fontSize: 13,
                            height: 1.5,
                            color: c.ink.withValues(alpha: 0.85),
                            fontFeatures: const [FontFeature.tabularFigures()],
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
    ];
    return ListView(
      padding: EdgeInsets.fromLTRB(inset.left, 4, inset.right, 96 + MediaQuery.viewPaddingOf(context).bottom),
      children: [
        Padding(
          padding: const EdgeInsets.only(bottom: 12),
          child: Align(
            alignment: Alignment.centerLeft,
            child: Transform.translate(
              offset: const Offset(0, -3),
              child: const GlassBackdropHeading(
                '概览',
                style: TextStyle(
                  fontSize: 26,
                  height: 1.25,
                  fontWeight: FontWeight.w700,
                  letterSpacing: -0.4,
                ),
              ),
            ),
          ),
        ),
        GlassCard(
          radius: 22,
          floating: true,
          padding: EdgeInsets.zero,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Padding(
                padding: EdgeInsets.fromLTRB(
                  wide ? 24 : 20,
                  wide ? 20 : 18,
                  wide ? 24 : 20,
                  0,
                ),
                child: Row(
                  crossAxisAlignment: CrossAxisAlignment.center,
                  children: [
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Row(
                            children: [
                              Container(
                                width: 14,
                                height: 14,
                                decoration: BoxDecoration(
                                  shape: BoxShape.circle,
                                  color: live
                                      ? c.successSoft
                                      : (connecting
                                            ? Colors.transparent
                                            : c.dangerSoft),
                                ),
                                alignment: Alignment.center,
                                child: Container(
                                  width: 8,
                                  height: 8,
                                  decoration: BoxDecoration(
                                    shape: BoxShape.circle,
                                    color: live
                                        ? c.success
                                        : (connecting
                                              ? Colors.transparent
                                              : c.error),
                                    border: connecting
                                        ? Border.all(color: c.muted)
                                        : null,
                                  ),
                                ),
                              ),
                              const SizedBox(width: 8),
                              Text(
                                stateLabel,
                                style: TextStyle(
                                  fontSize: 13,
                                  fontWeight: FontWeight.w600,
                                  color: c.ink,
                                ),
                              ),
                              if (live && modeText.isNotEmpty) ...[
                                Text(
                                  ' · ',
                                  style: TextStyle(
                                    fontSize: 13,
                                    color: c.muted,
                                  ),
                                ),
                                GestureDetector(
                                  onTap: () => _selectTab(5),
                                  child: Text(
                                    modeText,
                                    style: TextStyle(
                                      fontSize: 13,
                                      color: c.muted,
                                    ),
                                  ),
                                ),
                              ],
                            ],
                          ),
                          if (live && metaDetail.isNotEmpty)
                            Padding(
                              padding: const EdgeInsets.only(left: 22, top: 2),
                              child: Text(
                                metaDetail,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                                style: TextStyle(fontSize: 12, color: c.muted),
                              ),
                            ),
                        ],
                      ),
                    ),
                    if (rootEnabled && rootRunning)
                      Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          GestureDetector(
                            onTap: probing
                                ? null
                                : () => rootOperation('root_restart', '重启核心'),
                            child: Container(
                              width: 36,
                              height: 36,
                              decoration: BoxDecoration(
                                shape: BoxShape.circle,
                                color: c.fill,
                              ),
                              child: Center(
                                child: _rootAction == 'root_restart'
                                    ? SizedBox(
                                        width: 16,
                                        height: 16,
                                        child: CircularProgressIndicator(
                                          strokeWidth: 2,
                                          color: c.ink.withValues(alpha: 0.72),
                                        ),
                                      )
                                    : PanelIcon(
                                        'hero-restart',
                                        size: 18,
                                        color: c.ink.withValues(alpha: 0.72),
                                      ),
                              ),
                            ),
                          ),
                          const SizedBox(width: 8),
                          GestureDetector(
                            onTap: probing
                                ? null
                                : () => rootOperation('root_stop', '停止核心'),
                            child: Container(
                              width: 36,
                              height: 36,
                              decoration: BoxDecoration(
                                shape: BoxShape.circle,
                                color: c.success.withValues(alpha: 0.16),
                                border: Border.all(
                                  color: c.success.withValues(alpha: 0.35),
                                  width: 1,
                                ),
                              ),
                              child: Center(
                                child: _rootAction == 'root_stop'
                                    ? SizedBox(
                                        width: 16,
                                        height: 16,
                                        child: CircularProgressIndicator(
                                          strokeWidth: 2,
                                          color: c.success,
                                        ),
                                      )
                                    : PanelIcon(
                                        'hero-power',
                                        size: 18,
                                        color: c.success,
                                      ),
                              ),
                            ),
                          ),
                        ],
                      ),
                  ],
                ),
              ),
              Padding(
                padding: EdgeInsets.fromLTRB(
                  wide ? 24 : 20,
                  wide ? 24 : 20,
                  wide ? 24 : 20,
                  0,
                ),
                child: wide
                    ? Row(
                        crossAxisAlignment: CrossAxisAlignment.end,
                        children: [
                          Expanded(child: exitBlock),
                          const SizedBox(width: 32),
                          SizedBox(width: 306, child: rates),
                        ],
                      )
                    : Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          exitBlock,
                          const SizedBox(height: 20),
                          Divider(
                            height: 0.5,
                            thickness: 0.5,
                            color: c.hairline,
                          ),
                          const SizedBox(height: 16),
                          rates,
                        ],
                      ),
              ),
              SizedBox(
                height: wide ? 104 : 92,
                width: double.infinity,
                child: CustomPaint(
                  painter: TrafficWavePainter(
                    downSamples: List.of(downTrend),
                    upSamples: List.of(upTrend),
                    live: trafficLive && hasTraffic,
                    ink: c.chartInk,
                  ),
                ),
              ),
            ],
          ),
        ),
        if (_tailscale.hasEndpoints) ...[
          const SizedBox(height: 12),
          Builder(
            builder: (_) {
              final ep = _tailscale.currentEndpoint ?? const <String, dynamic>{};
              final selfPeer =
                  (ep['self'] as Map?)?.cast<String, dynamic>() ?? const {};
              final exitNode =
                  (ep['exitNode'] as Map?)?.cast<String, dynamic>() ?? const {};
              final peers = tailscaleAllPeers(ep);
              final online = peers.where((p) => p['online'] == true).length;
              final network = ep['networkName']?.toString() ?? 'Tailnet';
              final selfIp = tailscalePrimaryIp(selfPeer);
              final exitId = exitNode['stableId']?.toString() ?? '';
              final exitName = exitId.isEmpty &&
                      (exitNode['dnsName']?.toString() ?? '').isEmpty &&
                      (exitNode['hostName']?.toString() ?? '').isEmpty
                  ? ''
                  : tailscalePeerDisplayName(exitNode);
              return GlassCard(
                padding: EdgeInsets.zero,
                child: InkWell(
                  borderRadius: BorderRadius.circular(20),
                  onTap: () => Navigator.of(context).push(
                    MaterialPageRoute(
                      builder: (_) => TailscalePage(
                        controller: _tailscale,
                        preferences: preferences,
                      ),
                    ),
                  ),
                  child: Padding(
                    padding: EdgeInsets.symmetric(
                      horizontal: padX,
                      vertical: 14,
                    ),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Row(
                          children: [
                            Container(
                              width: 30,
                              height: 30,
                              decoration: BoxDecoration(
                                color: c.primarySoft,
                                borderRadius: BorderRadius.circular(10),
                              ),
                              child: Center(
                                child: PanelIcon(
                                  'tailscale',
                                  size: 16,
                                  color: c.selectionInk,
                                ),
                              ),
                            ),
                            const SizedBox(width: 10),
                            Expanded(
                              child: Column(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  Row(
                                    children: [
                                      Text(
                                        'Tailscale',
                                        style: TextStyle(
                                          fontSize: 14,
                                          fontWeight: FontWeight.w600,
                                          color: c.ink,
                                        ),
                                      ),
                                      const SizedBox(width: 6),
                                      Container(
                                        width: 7,
                                        height: 7,
                                        decoration: BoxDecoration(
                                          shape: BoxShape.circle,
                                          color: ep['backendState'] == 'Running'
                                              ? c.success
                                              : c.warning,
                                        ),
                                      ),
                                    ],
                                  ),
                                  Text(
                                    '$network · $online/${peers.length} 设备在线',
                                    maxLines: 1,
                                    overflow: TextOverflow.ellipsis,
                                    style: TextStyle(fontSize: 12, color: c.muted),
                                  ),
                                ],
                              ),
                            ),
                            PanelIcon('chevron', size: 15, color: c.muted),
                          ],
                        ),
                        if (selfIp.isNotEmpty || exitName.isNotEmpty) ...[
                          const SizedBox(height: 10),
                          Divider(
                            height: 0.5,
                            thickness: 0.5,
                            color: c.hairline,
                          ),
                          const SizedBox(height: 8),
                          Row(
                            children: [
                              if (selfIp.isNotEmpty)
                                Text(
                                  selfIp,
                                  style: TextStyle(
                                    fontSize: 12,
                                    fontFamily: 'monospace',
                                    color: c.muted,
                                  ),
                                ),
                              const Spacer(),
                              Text(
                                exitName.isEmpty ? '未启用出口节点' : '出口 · $exitName',
                                style: TextStyle(
                                  fontSize: 12,
                                  fontWeight: FontWeight.w500,
                                  color: c.ink,
                                ),
                              ),
                            ],
                          ),
                        ],
                      ],
                    ),
                  ),
                ),
              );
            },
          ),
        ],
        const SizedBox(height: 12),
        GlassCard(
          padding: EdgeInsets.zero,
          child: IntrinsicHeight(
            child: Row(
              children: [
                stat(
                  '连接',
                  connectionsLive ? '$connectionCount' : '—',
                  '',
                  connectionsLive ? 'TCP $tcp\nUDP $udp' : '',
                  connectionsLive ? connectionTrend : const [],
                  10,
                  onTap: () => _selectTab(2),
                ),
                statDivider,
                stat(
                  '内存',
                  memNum,
                  memUnit,
                  memVal == null ? '' : memoryDetail,
                  memVal == null ? const [] : memoryTrend,
                  16 * 1024 * 1024,
                ),
                if (rootEnabled) ...[
                  statDivider,
                  stat(
                    'CPU',
                    cpu?.toStringAsFixed(1) ?? '—',
                    cpu == null ? '' : '%',
                    cpu == null ? '' : '占全部核心',
                    cpu == null ? const [] : cpuTrend,
                    100,
                  ),
                ],
              ],
            ),
          ),
        ),
        const SizedBox(height: 26),
        Padding(
          padding: const EdgeInsets.fromLTRB(6, 0, 6, 9),
          child: Row(
            children: [
              const GlassBackdropPill(
                '网络',
                style: TextStyle(
                  fontSize: 15,
                  fontWeight: FontWeight.w600,
                ),
              ),
              if (!_networkRunning) ...[
                const SizedBox(width: 10),
                Flexible(
                  child: const GlassBackdropPill(
                    'sing-box 未运行，启动后自动检测',
                    muted: true,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(fontSize: 12),
                  ),
                ),
              ],
            ],
          ),
        ),
        GlassCard(
          padding: const EdgeInsets.only(bottom: 4),
          child: wide
              ? IntrinsicHeight(
                  child: Row(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      Expanded(flex: 11, child: ips),
                      statDivider,
                      Expanded(
                        flex: 10,
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.stretch,
                          children: [
                            siteHeader,
                            Divider(
                              height: 0.5,
                              thickness: 0.5,
                              indent: padX,
                              endIndent: padX,
                              color: c.hairline,
                            ),
                            Expanded(
                              child: Column(
                                mainAxisAlignment: MainAxisAlignment.center,
                                children: [
                                  for (var i = 0; i < siteRows.length; i += 2)
                                    Row(
                                      crossAxisAlignment: CrossAxisAlignment.center,
                                      children: [
                                        Expanded(child: siteRows[i]),
                                        Expanded(
                                          child: i + 1 < siteRows.length
                                              ? siteRows[i + 1]
                                              : const SizedBox.shrink(),
                                        ),
                                      ],
                                    ),
                                ],
                              ),
                            ),
                          ],
                        ),
                      ),
                    ],
                  ),
                )
              : Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    ips,
                    const SizedBox(height: 4),
                    Divider(
                      height: 0.5,
                      thickness: 0.5,
                      indent: 16,
                      color: c.hairline,
                    ),
                    siteHeader,
                    Divider(
                      height: 0.5,
                      thickness: 0.5,
                      indent: padX,
                      endIndent: padX,
                      color: c.hairline,
                    ),
                    ...siteRows,
                  ],
                ),
        ),
        const SizedBox(height: 26),
        Padding(
          padding: const EdgeInsets.fromLTRB(6, 0, 6, 9),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  const GlassBackdropPill(
                    '连接拓扑',
                    style: TextStyle(
                      fontSize: 15,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                  const Spacer(),
                  if (connectionsLive) ...[
                    GlassSegmented<bool>(
                      items: const [(true, '图'), (false, '列表')],
                      value: chart,
                      onChanged: (v) => setState(() { chart = v; hoveredTopology = null; }),
                    ),
                    const SizedBox(width: 6),
                    GestureDetector(
                      onTap: () => setState(
                        () => frozenTopology = frozenTopology == null
                            ? Map.of(metrics)
                            : null,
                      ),
                      child: GlassControl(
                        radius: 999,
                        tint: frozenTopology != null
                            ? c.selectionInk.withValues(alpha: 0.13)
                            : c.fill,
                        child: SizedBox(
                          width: 28,
                          height: 28,
                          child: PlaybackIcon(
                            paused: frozenTopology != null,
                            topology: true,
                            color: frozenTopology != null
                                ? c.selectionInk
                                : c.muted,
                          ),
                        ),
                      ),
                    ),
                  ],
                ],
              ),
              if (connectionsLive)
                Padding(
                  padding: const EdgeInsets.only(top: 4),
                  child: GlassBackdropPill(
                    topoSummary,
                    muted: true,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontSize: 12),
                  ),
                ),
            ],
          ),
        ),
        GlassCard(
          padding: const EdgeInsets.fromLTRB(18, 12, 14, 12),
          child: !connectionsLive
              ? SizedBox(
                  height: 140,
                  child: Center(
                    child: Text(
                      connecting || (connectionsOpen && !connectionsLive)
                          ? '正在连接 sing-box…'
                          : stopped
                          ? 'sing-box 运行后显示连接拓扑'
                          : '与 sing-box 的连接已中断，恢复后显示连接拓扑',
                      textAlign: TextAlign.center,
                      style: TextStyle(fontSize: 13, color: c.muted),
                    ),
                  ),
                )
              : (flowNodes.isEmpty
                    ? SizedBox(
                        height: 120,
                        child: Center(
                          child: Text(
                            '暂无连接数据',
                            style: TextStyle(fontSize: 13, color: c.muted),
                          ),
                        ),
                      )
                    : (chart
                          ? SizedBox(
                              width: double.infinity,
                              height: math.max(
                                160.0,
                                math.min(
                                  320.0,
                                  [0, 1, 2, 3]
                                              .map(
                                                (l) => flowNodes
                                                    .where(
                                                      (n) =>
                                                          (n['layer'] as num?)
                                                              ?.toInt() ==
                                                          l,
                                                    )
                                                    .length,
                                              )
                                              .reduce(math.max) *
                                          28.0 +
                                      24.0,
                                ),
                              ),
                              child: _FlowChart(
                                flow: activeMetrics['flow'] as Item,
                                colors: c,
                                onHoverChanged: (hovered) => setState(() =>
                                    hoveredTopology = hovered ? metrics : null),
                              ),
                            )
                          : wide
                          ? Row(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Expanded(
                                  child: rankSection('按出口', exitRanking),
                                ),
                                const SizedBox(width: 24),
                                Expanded(
                                  child: rankSection('按规则', ruleRanking),
                                ),
                              ],
                            )
                          : Column(
                              crossAxisAlignment: CrossAxisAlignment.stretch,
                              children: [
                                rankSection('按出口', exitRanking),
                                const SizedBox(height: 10),
                                rankSection('按规则', ruleRanking),
                              ],
                            ))),
        ),
      ],
    );
  }

  Widget logPage() {
    final c = SingboardColors.of(context);
    final source = frozen ?? entries;
    const levels = [
      'panic',
      'fatal',
      'error',
      'warn',
      'info',
      'debug',
      'trace',
    ];
    String normalizeLvl(String? raw) {
      final v = (raw ?? 'info').trim().toLowerCase();
      if (v == 'warning') return 'warn';
      return levels.contains(v) ? v : 'info';
    }

    int rank(String value) {
      final index = levels.indexOf(value == 'warning' ? 'warn' : value);
      return index < 0 ? 4 : index;
    }

    int countLines(List<Map<String, dynamic>> list) =>
        list.where((r) => r['type'] != 'marker').length;

    final coreLvl = normalizeLvl(summary['config']?['log-level'] as String?);
    final current = level == '跟随核心' ? coreLvl : level;
    final levelFiltered = current != coreLvl;
    var shown = source.where((r) {
      if (r['type'] == 'marker') return keyword.isEmpty;
      return rank(r['type'].toString().toLowerCase()) <= rank(current) &&
          (keyword.isEmpty ||
              r['payload'].toString().toLowerCase().contains(keyword));
    }).toList();
    if (!shown.any((r) => r['type'] != 'marker')) {
      shown = [];
    }
    final totalCount = countLines(source);
    final shownCount = countLines(shown);
    final countText = shownCount == totalCount
        ? '$totalCount'
        : '$shownCount / $totalCount';
    final clearedCount = clearedLogs == null ? 0 : countLines(clearedLogs!);
    final apiUnreachable = logStatus != '已连接' && _logFailures >= 3;
    final showReconnecting =
        frozen == null &&
        clearedCount == 0 &&
        logStatus != '已连接' &&
        totalCount > 0;

    void selectLevel(String next) {
      final prevEff = level == '跟随核心' ? coreLvl : level;
      final nextEff = next == '跟随核心' ? coreLvl : next;
      setState(() {
        level = next;
        if (prevEff != nextEff) {
          if (entries.isNotEmpty) {
            entries.add({
              'seq': _logSeq++,
              'type': 'marker',
              'time': DateTime.now().millisecondsSinceEpoch,
              'payload': '日志级别切换为 $nextEff，之后的日志按新级别接收',
            });
          }
          _resubscribeLogs();
        }
      });
    }

    final scope = GlassScope.of(context);
    final inset = _pageInsets();
    final wide = MediaQuery.sizeOf(context).width >= 640;
    final clearance = MediaQuery.viewInsetsOf(context).bottom > 0
        ? 0.0
        : 80.0 + math.max(16.0, MediaQuery.viewPaddingOf(context).bottom + 8);
    final picker = PopupMenuButton<String>(
      initialValue: level,
      onSelected: selectLevel,
      itemBuilder: (_) => [
        CheckedPopupMenuItem(
          value: '跟随核心',
          checked: level == '跟随核心',
          child: Text('跟随核心（$coreLvl）'),
        ),
        for (final v in levels)
          CheckedPopupMenuItem(value: v, checked: level == v, child: Text(v)),
      ],
      child: GlassControl(
        radius: 10,
        child: Container(
          height: 32,
          constraints: const BoxConstraints(minWidth: 156),
          padding: const EdgeInsets.only(left: 12, right: 12),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            mainAxisAlignment: MainAxisAlignment.spaceBetween,
            children: [
              Text(
                level == '跟随核心' ? '跟随核心（$coreLvl）' : level,
                style: TextStyle(fontSize: 14, color: c.ink),
              ),
              const SizedBox(width: 4),
              PanelIcon('select-caret', size: 16, color: c.ink),
            ],
          ),
        ),
      ),
    );

    return Column(
      children: [
        Padding(
          padding: EdgeInsets.fromLTRB(inset.left, 4, inset.right, 12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Row(
                children: [
                  Transform.translate(
                    offset: const Offset(0, -3),
                    child: const GlassBackdropHeading(
                      '日志',
                      style: TextStyle(
                        fontSize: 26,
                        height: 1.25,
                        fontWeight: FontWeight.w700,
                        letterSpacing: -0.4,
                      ),
                    ),
                  ),
                  const SizedBox(width: 8),
                  GlassBackdropPill(
                    countText,
                    muted: true,
                    style: const TextStyle(
                      fontSize: 14,
                      fontFeatures: [FontFeature.tabularFigures()],
                    ),
                  ),
                  const Spacer(),
                  if (wide) ...[picker, const SizedBox(width: 8)],
                  GestureDetector(
                    onTap: () => setState(() {
                      if (frozen == null) {
                        frozen = List.of(entries);
                        pausedAt = total;
                      } else {
                        frozen = null;
                      }
                    }),
                    child: Container(
                      width: 32,
                      height: 32,
                      decoration: BoxDecoration(
                        shape: BoxShape.circle,
                        color: frozen != null
                            ? c.primarySoft
                            : Colors.transparent,
                      ),
                      child: PlaybackIcon(
                        paused: frozen != null,
                        color: frozen != null
                            ? c.selectionInk
                            : scope.backdropInk(c),
                      ),
                    ),
                  ),
                  const SizedBox(width: 8),
                  GestureDetector(
                    onTap: entries.isEmpty && frozen == null
                        ? null
                        : () {
                            final removed = List.of(entries);
                            clearedTimer?.cancel();
                            setState(() {
                              entries.clear();
                              frozen = null;
                              expandedLogs.clear();
                              clearedLogs = countLines(removed) > 0
                                  ? removed
                                  : null;
                            });
                            if (clearedLogs != null) {
                              clearedTimer = Timer(
                                const Duration(seconds: 8),
                                () {
                                  if (mounted)
                                    setState(() => clearedLogs = null);
                                },
                              );
                            }
                          },
                    child: GlassBackdropPill(
                      '清空',
                      muted: entries.isEmpty && frozen == null,
                      padding: const EdgeInsets.symmetric(
                        horizontal: 14,
                        vertical: 6,
                      ),
                      style: const TextStyle(
                        fontSize: 14,
                        fontWeight: FontWeight.w500,
                      ),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: 12),
              Row(
                children: [
                  Expanded(
                    child: GlassSearchField(
                      controller: _logSearchCtrl,
                      focusNode: _logSearchFocus,
                      hint: '搜索日志内容',
                      value: keyword,
                      onChanged: (text) =>
                          setState(() => keyword = text.trim().toLowerCase()),
                    ),
                  ),
                  if (!wide) ...[const SizedBox(width: 8), picker],
                ],
              ),
            ],
          ),
        ),
        if (frozen != null || clearedCount > 0 || showReconnecting)
          Padding(
            padding: EdgeInsets.fromLTRB(inset.left, 0, inset.right, 12),
            child: GlassCard(
              radius: 14,
              padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 7),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      frozen != null
                          ? (total - pausedAt > 0
                                ? '已暂停刷新，暂停后收到 ${total - pausedAt} 条新日志。'
                                : '已暂停刷新。')
                          : clearedCount > 0
                          ? '已清空 $clearedCount 条日志。'
                          : (apiUnreachable
                                ? '无法连接 Clash API，正在重试；下面是中断前的日志。'
                                : '与核心的连接中断，正在重连；下面是中断前的日志。'),
                      style: TextStyle(fontSize: 12, color: c.ink),
                    ),
                  ),
                  if (frozen != null || clearedCount > 0 || apiUnreachable)
                    GestureDetector(
                      onTap: () {
                        if (frozen != null) {
                          setState(() => frozen = null);
                        } else if (clearedCount > 0) {
                          final restore = clearedLogs!;
                          clearedTimer?.cancel();
                          setState(() {
                            entries.insertAll(0, restore);
                            if (entries.length > limit) {
                              entries.removeRange(0, entries.length - limit);
                            }
                            clearedLogs = null;
                          });
                        } else {
                          _selectTab(5);
                        }
                      },
                      child: Padding(
                        padding: const EdgeInsets.symmetric(
                          horizontal: 8,
                          vertical: 4,
                        ),
                        child: Text(
                          frozen != null
                              ? '继续刷新'
                              : (clearedCount > 0 ? '撤销' : '检查 API 配置'),
                          style: TextStyle(
                            fontSize: 14,
                            fontWeight: FontWeight.w600,
                            color: c.selectionInk,
                          ),
                        ),
                      ),
                    ),
                ],
              ),
            ),
          ),
        Expanded(
          child: shown.isEmpty
              ? (!((keyword.isNotEmpty || levelFiltered) && totalCount > 0) &&
                        logStatus != '已连接' &&
                        apiUnreachable)
                    ? SingleChildScrollView(
                        padding: const EdgeInsets.fromLTRB(24, 24, 24, 112),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              '无法连接 Clash API',
                              style: TextStyle(
                                fontSize: 16,
                                fontWeight: FontWeight.w600,
                                color: c.ink,
                              ),
                            ),
                            const SizedBox(height: 6),
                            Text(
                              '请确认 sing-box 配置里开启了 clash_api，并且地址和密钥与当前的 Clash API 配置一致。',
                              style: TextStyle(fontSize: 14, color: c.muted),
                            ),
                            const SizedBox(height: 12),
                            Row(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text(
                                  '地址',
                                  style: TextStyle(
                                    fontSize: 12,
                                    color: c.muted,
                                  ),
                                ),
                                const SizedBox(width: 12),
                                Expanded(
                                  child: Text(
                                    endpoint.isEmpty ? '未配置' : endpoint,
                                    style: TextStyle(
                                      fontSize: 12,
                                      fontFamily: 'monospace',
                                      color: c.ink,
                                    ),
                                  ),
                                ),
                              ],
                            ),
                            const SizedBox(height: 4),
                            Row(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text(
                                  '状态',
                                  style: TextStyle(
                                    fontSize: 12,
                                    color: c.muted,
                                  ),
                                ),
                                const SizedBox(width: 12),
                                Expanded(
                                  child: Text(
                                    '已连续 $_logFailures 次连接失败，仍在自动重试',
                                    style: TextStyle(
                                      fontSize: 12,
                                      color: c.ink,
                                    ),
                                  ),
                                ),
                              ],
                            ),
                            const SizedBox(height: 16),
                            GestureDetector(
                              onTap: () => _selectTab(5),
                              child: Container(
                                padding: const EdgeInsets.symmetric(
                                  horizontal: 18,
                                  vertical: 8,
                                ),
                                decoration: BoxDecoration(
                                  color: c.fill,
                                  borderRadius: BorderRadius.circular(10),
                                ),
                                child: Text(
                                  '检查 API 配置',
                                  style: TextStyle(
                                    fontSize: 14,
                                    fontWeight: FontWeight.w500,
                                    color: c.selectionInk,
                                  ),
                                ),
                              ),
                            ),
                          ],
                        ),
                      )
                    : Center(
                        child: Padding(
                          padding: const EdgeInsets.symmetric(horizontal: 24),
                          child: Column(
                            mainAxisSize: MainAxisSize.min,
                            children: [
                              Text(
                                (keyword.isNotEmpty || levelFiltered) &&
                                        totalCount > 0
                                    ? (keyword.isNotEmpty
                                          ? '没有匹配“$keyword”的日志'
                                          : '当前级别 $current 下没有日志')
                                    : frozen != null
                                    ? '暂停前没有日志'
                                    : logStatus != '已连接'
                                    ? '正在连接核心…'
                                    : '已连接，暂时没有日志',
                                style: TextStyle(fontSize: 14, color: c.ink),
                              ),
                              if ((keyword.isNotEmpty || levelFiltered) &&
                                  totalCount > 0) ...[
                                const SizedBox(height: 12),
                                Wrap(
                                  spacing: 8,
                                  children: [
                                    if (keyword.isNotEmpty)
                                      GestureDetector(
                                        onTap: () {
                                          _logSearchCtrl.clear();
                                          setState(() => keyword = '');
                                        },
                                        child: Container(
                                          padding: const EdgeInsets.symmetric(
                                            horizontal: 14,
                                            vertical: 7,
                                          ),
                                          decoration: BoxDecoration(
                                            color: c.fill,
                                            borderRadius: BorderRadius.circular(
                                              10,
                                            ),
                                          ),
                                          child: Text(
                                            '清除搜索',
                                            style: TextStyle(
                                              fontSize: 14,
                                              color: c.selectionInk,
                                            ),
                                          ),
                                        ),
                                      ),
                                    if (levelFiltered)
                                      GestureDetector(
                                        onTap: () => selectLevel('跟随核心'),
                                        child: Container(
                                          padding: const EdgeInsets.symmetric(
                                            horizontal: 14,
                                            vertical: 7,
                                          ),
                                          decoration: BoxDecoration(
                                            color: c.fill,
                                            borderRadius: BorderRadius.circular(
                                              10,
                                            ),
                                          ),
                                          child: Text(
                                            '跟随核心级别',
                                            style: TextStyle(
                                              fontSize: 14,
                                              color: c.selectionInk,
                                            ),
                                          ),
                                        ),
                                      ),
                                  ],
                                ),
                              ] else if (keyword.isEmpty &&
                                  logStatus == '已连接') ...[
                                const SizedBox(height: 4),
                                Text(
                                  '当前级别 $current${level == '跟随核心' ? '（跟随核心）' : ''}，有新日志时会出现在这里。',
                                  textAlign: TextAlign.center,
                                  style: TextStyle(
                                    fontSize: 12,
                                    color: c.muted,
                                  ),
                                ),
                              ],
                            ],
                          ),
                        ),
                      )
              : Stack(
                  children: [
                    NotificationListener<ScrollNotification>(
                      onNotification: (n) {
                        if (n is ScrollUpdateNotification ||
                            n is ScrollEndNotification) {
                          final atBottom =
                              n.metrics.maxScrollExtent - n.metrics.pixels < 50;
                          if (atBottom != _logAutoScroll) {
                            setState(() => _logAutoScroll = atBottom);
                          }
                        }
                        return false;
                      },
                      child: ListView.separated(
                        controller: _logScroll,
                        padding: EdgeInsets.fromLTRB(inset.left, 0, inset.right, clearance + 8),
                        itemCount: shown.length,
                        separatorBuilder: (_, __) => const SizedBox(height: 6),
                        itemBuilder: (context, index) {
                          final line = shown[index];
                          if (line['type'] == 'marker') {
                            return SizedBox(
                              height: 32,
                              child: Row(
                                children: [
                                  Expanded(
                                    child: Divider(
                                      height: 0.5,
                                      thickness: 0.5,
                                      color: c.hairline,
                                    ),
                                  ),
                                  Flexible(
                                    flex: 8,
                                    child: Padding(
                                      padding: const EdgeInsets.symmetric(
                                        horizontal: 8,
                                      ),
                                      child: Text(
                                        line['payload'].toString(),
                                        textAlign: TextAlign.center,
                                        maxLines: 2,
                                        overflow: TextOverflow.ellipsis,
                                        style: TextStyle(
                                          fontSize: 11.5,
                                          color: c.muted,
                                        ),
                                      ),
                                    ),
                                  ),
                                  Expanded(
                                    child: Divider(
                                      height: 0.5,
                                      thickness: 0.5,
                                      color: c.hairline,
                                    ),
                                  ),
                                ],
                              ),
                            );
                          }
                          final ts = line['time'] as int;
                          final seq = (line['seq'] as int?) ?? ts;
                          final time = DateTime.fromMillisecondsSinceEpoch(ts);
                          final hh = time.hour.toString().padLeft(2, '0');
                          final mm = time.minute.toString().padLeft(2, '0');
                          final ss = time.second.toString().padLeft(2, '0');
                          final lvl = (line['type'] as String? ?? 'info')
                              .toLowerCase();
                          final (pillBg, pillFg, tileTint) = switch (lvl) {
                            'error' ||
                            'fatal' ||
                            'panic' => (c.dangerSoft, c.dangerInk, c.rowError),
                            'warn' || 'warning' => (
                              c.warningSoft,
                              c.warningInk,
                              c.rowWarn,
                            ),
                            'info' => (
                              c.fill,
                              c.ink.withValues(alpha: 0.78),
                              null,
                            ),
                            _ => (c.fill.withValues(alpha: 0.6), c.muted, null),
                          };
                          final isExpanded = expandedLogs.contains(seq);
                          return GestureDetector(
                            onTap: () => setState(() {
                              if (!expandedLogs.remove(seq)) {
                                expandedLogs.add(seq);
                              }
                            }),
                            child: Container(
                              padding: const EdgeInsets.symmetric(
                                horizontal: 12,
                                vertical: 8,
                              ),
                              decoration: BoxDecoration(
                                color: tileTint ?? c.fill,
                                borderRadius: BorderRadius.circular(12),
                              ),
                              child: Row(
                                crossAxisAlignment: CrossAxisAlignment.start,
                                children: [
                                  SizedBox(
                                    width: 56,
                                    child: Column(
                                      children: [
                                        Container(
                                          width: 56,
                                          height: 18,
                                          alignment: Alignment.center,
                                          decoration: BoxDecoration(
                                            color: pillBg,
                                            borderRadius: BorderRadius.circular(
                                              999,
                                            ),
                                          ),
                                          child: Text(
                                            lvl.toUpperCase(),
                                            style: TextStyle(
                                              fontSize: 10.5,
                                              height: 18 / 10.5,
                                              fontWeight: FontWeight.w600,
                                              letterSpacing: 0.42,
                                              color: pillFg,
                                            ),
                                          ),
                                        ),
                                        const SizedBox(height: 4),
                                        Text(
                                          '$hh:$mm:$ss',
                                          textAlign: TextAlign.center,
                                          style: TextStyle(
                                            fontSize: 11,
                                            height: 18 / 11,
                                            fontFamily: 'monospace',
                                            color: c.muted,
                                          ),
                                        ),
                                      ],
                                    ),
                                  ),
                                  const SizedBox(width: 10),
                                  Expanded(
                                    child: ConstrainedBox(
                                      constraints: const BoxConstraints(
                                        minHeight: 40,
                                      ),
                                      child: Text(
                                        line['payload'] as String,
                                        maxLines: isExpanded ? null : 2,
                                        overflow: isExpanded
                                            ? TextOverflow.visible
                                            : TextOverflow.ellipsis,
                                        style: TextStyle(
                                          fontSize: 13,
                                          height: 20 / 13,
                                          color: c.ink.withValues(alpha: 0.9),
                                        ),
                                      ),
                                    ),
                                  ),
                                ],
                              ),
                            ),
                          );
                        },
                      ),
                    ),
                    if (!_logAutoScroll)
                      Positioned(
                        right: 12,
                        bottom: clearance + 12,
                        child: GestureDetector(
                          onTap: () {
                            setState(() => _logAutoScroll = true);
                            if (_logScroll.hasClients) {
                              _logScroll.jumpTo(
                                _logScroll.position.maxScrollExtent,
                              );
                            }
                          },
                          child: GlassControl(
                            radius: 999,
                            child: Padding(
                              padding: const EdgeInsets.symmetric(
                                horizontal: 12,
                                vertical: 7,
                              ),
                              child: Row(
                                mainAxisSize: MainAxisSize.min,
                                children: [
                                  PanelIcon(
                                    'arrow-down',
                                    size: 14,
                                    color: c.selectionInk,
                                  ),
                                  const SizedBox(width: 6),
                                  Text(
                                    '回到最新',
                                    style: TextStyle(
                                      fontSize: 14,
                                      fontWeight: FontWeight.w500,
                                      color: c.selectionInk,
                                    ),
                                  ),
                                ],
                              ),
                            ),
                          ),
                        ),
                      ),
                  ],
                ),
        ),
      ],
    );
  }

  static const _titles = ['概览', '代理', '连接', '日志', '规则', '设置'];

  (String, String, bool)? _gate(int page) {
    if (page < 1 || page > 4) return null;
    final root = preferences['nativeRootEnabled'] == true;
    if (root && _rootAction == 'root_restart') {
      return ('正在重启 sing-box…', '核心重启后会自动重新连接…', true);
    }
    if (root && rootService.isNotEmpty && !rootRunning) {
      return ('服务未运行', '请先启动 sing-box 服务以查看${_titles[page]}', false);
    }
    if (_isAuthError(summaryError)) {
      return ('Clash API 密钥不匹配', '请在设置中检查 Clash API 密钥', false);
    }
    if (summary.isNotEmpty) return null;
    if (!root && summaryError.isNotEmpty) {
      return ('未连接到 Clash API', '请检查设置中的 Clash API 地址和端口', false);
    }
    return ('服务启动中', '正在等待 Clash API 就绪…', true);
  }

  Widget _page(int index) {
    final gate = _gate(index);
    final content = switch (index) {
    0 => overview(),
    1 || 4 => CatalogPage(
      key: ValueKey((endpointRevision, index)),
      endpoint: endpoint,
      secret: secret,
      proxies: index == 1,
      enabled: gate == null,
      active: tab == index,
      reconnectGeneration: _reconnectGeneration,
      preferences: preferences,
      chainSpeeds:
          (metrics['chainSpeeds'] as Map<String, dynamic>?) ??
              const {},
      onProxyChanged: index == 1 ? _scheduleProxyRecheck : null,
    ),
    2 => ConnectionsPage(
      key: ValueKey(endpointRevision),
      endpoint: endpoint,
      secret: secret,
      status: connectionStatus,
      sort: preferences['connectionSort'] as String? ?? '',
      onSort: (key) {
        preferences['connectionSort'] = key;
      },
      active: active,
      history: history,
      download: metrics['download'] as num? ?? 0,
      upload: metrics['upload'] as num? ?? 0,
    ),
    3 => logPage(),
    _ => SettingsPage(
      preferences: preferences,
      summary: summary,
      endpoint: endpoint,
      tailscale: _tailscale,
      clashConnected: overviewLive,
      rootStatus: switch (rootService['state']) {
        'running' => '运行中',
        'stopped' => '已停止',
        'not_installed' => '未安装服务',
        'unknown' => '状态未知',
        _ => rootStatus,
      },
      rootRunning: rootRunning,
      probing: probing,
      rootAction: _rootAction,
      themeMode: _theme.value,
      fontScale: _fontScale.value,
      glassTint: _glassTint.value,
      wallpaperPath: _wallpaper.value,
      onReload: () => loadPreferences(),
      onSetMode: setMode,
      onProbeRoot: probe,
      onRootAction: rootOperation,
      onOpenTools: openTools,
      onThemeChanged: (m) => _theme.value = m,
      onFontScaleChanged: (s) => _fontScale.value = s,
      onTintChanged: (t) => _glassTint.value = t,
      onWallpaperChanged: (p) {
        final prev = _wallpaper.value;
        if (prev != null && prev.isNotEmpty) {
          unawaited(FileImage(File(prev)).evict());
        }
        if (p != null && p.isNotEmpty) {
          unawaited(FileImage(File(p)).evict());
        }
        _wallpaper.value = p;
        _wallpaperRev.value++;
      },
    ),
    };
    return _PagerPanel(key: _pageKeys[index], child: NotificationListener<ScrollNotification>(
      onNotification: (event) {
        if (index != 3 && event.metrics.axis == Axis.vertical && event.depth == 0) {
          final raw = ((event.metrics.pixels - event.metrics.minScrollExtent) / 32.0).clamp(0.0, 1.0);
          final next = raw * raw * (3.0 - 2.0 * raw);
          final prev = _scrollProgress[index];
          if ((prev - next).abs() > 0.015 || (next == 0 && prev != 0) || (next == 1 && prev != 1)) {
            _scrollProgress[index] = next;
            _headerScroll.value++;
          }
        }
        return false;
      },
      child: GlassApiGate(mask: gate, child: content),
    ));
  }

  @override
  Widget build(BuildContext context) {
    final rootEnabled = preferences['nativeRootEnabled'] == true;
    final running = rootEnabled ? rootRunning : summary.isNotEmpty;
    final state = rootService['state'];
    final busy = rootEnabled && (probing || state == 'starting' || state == 'stopping');
    final uptime = rootRunning ? rootService['uptimeSeconds'] as int? : null;
    final statusText = uptime != null ? _uptime(uptime) : state == 'starting' ? '启动中'
        : state == 'stopping' ? '停止中' : rootEnabled ? (running ? '运行中' : '已停止')
        : running ? '已连接' : overviewConnecting ? '连接中' : '未连接';
    final onStatusTap = rootEnabled && !busy && (running || state == 'stopped' || state == 'not_installed')
        ? () => rootOperation(rootRunning ? 'root_stop' : 'root_start', rootRunning ? '停止核心' : '启动核心')
        : null;
    return PopScope(
      canPop: !showSetupWizard && tab == 0,
      onPopInvokedWithResult: (popped, _) {
        if (!popped && !showSetupWizard && tab != 0) _selectTab(0);
      },
      child: GlassBackdrop(
        child: Stack(
          children: [
            ExcludeFocus(
              excluding: showSetupWizard,
              child: IgnorePointer(
                ignoring: showSetupWizard,
                child: Stack(
                  children: [
                    Scaffold(
                      backgroundColor: Colors.transparent,
                      extendBody: true,
                      body: Column(
                        children: [
                          AnimatedBuilder(
                            animation: Listenable.merge([_pager, _headerScroll]),
                            builder: (_, __) {
                              final pos = _pager.positions.length == 1 && _pager.position.haveDimensions
                                  ? (_pager.page ?? tab.toDouble())
                                  : tab.toDouble();
                              final lo = pos.floor().clamp(0, _scrollProgress.length - 1);
                              final hi = pos.ceil().clamp(0, _scrollProgress.length - 1);
                              final frac = (pos - lo).clamp(0.0, 1.0);
                              final headerProgress =
                                  _scrollProgress[lo] + (_scrollProgress[hi] - _scrollProgress[lo]) * frac;
                              final titleIdx = _scrollProgress[lo] >= _scrollProgress[hi] ? lo : hi;
                              return GlassAppHeader(
                                title: 'singboard',
                                scrolledTitle: titleIdx != 3 && headerProgress > 0.01
                                    ? _titles[titleIdx]
                                    : null,
                                scrollProgress: headerProgress,
                                showStatusPill: true,
                                statusPillOpacity: pos.clamp(0.0, 1.0),
                                connected: running,
                                busy: busy,
                                statusText: statusText,
                                onStatusTap: onStatusTap,
                              );
                            },
                          ),
                          Expanded(
                            child: AnimatedBuilder(
                              animation: _pageFade,
                              builder: (_, child) {
                                final t = _pageFade.value;
                                final curved = const Cubic(0.16, 1.0, 0.3, 1.0).transform(t);
                                final opacity = Curves.easeOut.transform(
                                  (t / 0.65).clamp(0.0, 1.0).toDouble(),
                                );
                                final dx = _tabDir * 28.0 * (1.0 - curved);
                                final scale = 0.985 + 0.015 * curved;
                                return Opacity(
                                  opacity: opacity,
                                  child: Transform.translate(
                                    offset: Offset(dx, 0),
                                    child: Transform.scale(scale: scale, child: child),
                                  ),
                                );
                              },
                              child: GlassScrollScope(
                                listenable: _glassMotion,
                                child: NotificationListener<ScrollNotification>(
                                  onNotification: (event) {
                                    if (event.depth != 0) return false;
                                    if (event is ScrollStartNotification &&
                                        event.dragDetails != null) {
                                      _userSwiping = true;
                                      _pendingTab = null;
                                    } else if (event is ScrollUpdateNotification &&
                                        _userSwiping &&
                                        _pager.positions.length == 1) {
                                      _swipeProgress.value = _pager.page;
                                    } else if (event is ScrollEndNotification &&
                                        _userSwiping) {
                                      _userSwiping = false;
                                      _swipeProgress.value = null;
                                      final settled =
                                          _pendingTab ??
                                          (_pager.positions.length == 1 &&
                                                  _pager.position.haveDimensions
                                              ? (_pager.page?.round() ?? tab)
                                              : tab);
                                      _pendingTab = null;
                                      if (settled != tab) {
                                        if (settled != 0) hoveredTopology = null;
                                        setState(() => tab = settled);
                                        if (settled == 0) _syncNetwork();
                                      }
                                    }
                                    return false;
                                  },
                                  child: PageView.builder(
                                    controller: _pager,
                                    allowImplicitScrolling: true,
                                    itemCount: _titles.length,
                                    physics: preferences['pageSwipe'] == true
                                        ? const _GentlePageScrollPhysics(
                                            parent: BouncingScrollPhysics(),
                                          )
                                        : const NeverScrollableScrollPhysics(),
                                    onPageChanged: (i) {
                                      if (_targetPage != null) return;
                                      if (_userSwiping) {
                                        _pendingTab = i;
                                      } else if (tab != i) {
                                        if (i != 0) hoveredTopology = null;
                                        setState(() => tab = i);
                                        if (i == 0) _syncNetwork();
                                      }
                                    },
                                    itemBuilder: (_, i) => _page(i),
                                  ),
                                ),
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),
                    Positioned(
                      left: 0,
                      right: 0,
                      bottom: 0,
                      child: _KeyboardAwareBottomNav(
                        child: GlassBottomNav(
                          index: tab,
                          onChanged: _selectTab,
                          pageProgress: _swipeProgress,
                          items: const [
                            GlassBottomNavItem('home', '概览'),
                            GlassBottomNavItem('proxies', '代理'),
                            GlassBottomNavItem('connections', '连接'),
                            GlassBottomNavItem('logs', '日志'),
                            GlassBottomNavItem('rules', '规则'),
                            GlassBottomNavItem('settings', '设置'),
                          ],
                        ),
                      ),
                    ),
                  ],
                ),
              ),
            ),
            if (showSetupWizard)
              SetupWizardOverlay(
                preferences: preferences,
                initialUrl: endpoint,
                initialSecret: secret,
                onCompleted: () {
                  setState(() => showSetupWizard = false);
                  unawaited(loadPreferences());
                },
                onGoToSettings: () {
                  setState(() {
                    showSetupWizard = false;
                  });
                  _selectTab(5);
                  unawaited(loadPreferences());
                },
              ),
          ],
        ),
      ),
    );
  }
}

class _KeyboardAwareBottomNav extends StatefulWidget {
  final Widget child;
  const _KeyboardAwareBottomNav({required this.child});

  @override
  State<_KeyboardAwareBottomNav> createState() =>
      _KeyboardAwareBottomNavState();
}

class _KeyboardAwareBottomNavState extends State<_KeyboardAwareBottomNav> {
  bool _offstage = false;

  @override
  Widget build(BuildContext context) {
    final hidden = MediaQuery.viewInsetsOf(context).bottom > 0;
    if (!hidden && _offstage) _offstage = false;
    final reduceMotion = MediaQuery.disableAnimationsOf(context);
    final duration = reduceMotion
        ? Duration.zero
        : Duration(milliseconds: hidden ? 160 : 240);
    final curve = hidden ? Curves.easeInCubic : Curves.easeOutCubic;
    return Material(
      type: MaterialType.transparency,
      child: Offstage(
        offstage: hidden && _offstage,
        child: IgnorePointer(
          ignoring: hidden,
          child: ExcludeSemantics(
            excluding: hidden,
            child: AnimatedSlide(
              offset: hidden ? const Offset(0, 1.4) : Offset.zero,
              duration: duration,
              curve: curve,
              onEnd: () {
                if (mounted &&
                    MediaQuery.viewInsetsOf(context).bottom > 0 &&
                    !_offstage) {
                  setState(() => _offstage = true);
                }
              },
              child: widget.child,
            ),
          ),
        ),
      ),
    );
  }
}

class _FlowChart extends StatefulWidget {
  const _FlowChart({required this.flow, required this.colors, required this.onHoverChanged});
  final Item flow;
  final SingboardColors colors;
  final ValueChanged<bool> onHoverChanged;

  @override
  State<_FlowChart> createState() => _FlowChartState();
}

class _FlowChartState extends State<_FlowChart>
    with SingleTickerProviderStateMixin {
  late Item current = widget.flow;
  Item previous = {};
  late final motion = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 240),
  );
  Offset? pointer;
  bool hovering = false;

  @override
  void didUpdateWidget(_FlowChart old) {
    super.didUpdateWidget(old);
    if (!hovering) updateFlow();
  }

  void updateFlow() {
    if (identical(current, widget.flow)) return;
    previous = current;
    current = widget.flow;
    if (MediaQuery.disableAnimationsOf(context)) {
      motion.value = 1;
    } else {
      motion.forward(from: 0);
    }
  }

  @override
  void dispose() {
    motion.dispose();
    super.dispose();
  }

  void inspect(Offset? position) {
    final active = position != null;
    if (active != hovering) widget.onHoverChanged(active);
    setState(() {
      hovering = active;
      pointer = position;
      if (!active) updateFlow();
    });
  }

  @override
  Widget build(BuildContext context) {
    return MouseRegion(
      onEnter: (event) => inspect(event.localPosition),
      onHover: (event) => setState(() => pointer = event.localPosition),
      onExit: (_) => inspect(null),
      child: GestureDetector(
        onLongPressStart: (event) => inspect(event.localPosition),
        onLongPressEnd: (_) => inspect(null),
        child: AnimatedBuilder(
          animation: motion,
          builder: (context, _) => LayoutBuilder(
            builder: (context, constraints) {
              final size = constraints.biggest;
              final painter = _FlowPainter(
                current,
                previous,
                Curves.easeOutCubic.transform(motion.value),
                widget.colors,
                size,
                pointer,
                Theme.of(context).textTheme.bodyMedium?.fontFamily,
                MediaQuery.textScalerOf(context),
              );
              return Stack(
                children: [
                  Positioned.fill(child: CustomPaint(painter: painter)),
                  if (painter.message.isNotEmpty && pointer != null)
                    Positioned(
                      left: pointer!.dx.clamp(
                        0.0,
                        math.max(0.0, size.width - 184),
                      ),
                      top: (pointer!.dy + 10).clamp(
                        0.0,
                        math.max(0.0, size.height - 65),
                      ),
                      child: IgnorePointer(
                        child: DecoratedBox(
                          decoration: BoxDecoration(
                            color: widget.colors.surface,
                            borderRadius: BorderRadius.circular(8),
                          ),
                          child: Container(
                            constraints: const BoxConstraints(maxWidth: 184),
                            padding: const EdgeInsets.symmetric(
                              horizontal: 12,
                              vertical: 8,
                            ),
                            child: Text(
                              painter.message,
                              maxLines: 3,
                              overflow: TextOverflow.ellipsis,
                              style: TextStyle(
                                fontSize: 12,
                                color: widget.colors.ink,
                              ),
                            ),
                          ),
                        ),
                      ),
                    ),
                ],
              );
            },
          ),
        ),
      ),
    );
  }
}

class _FlowPainter extends CustomPainter {
  _FlowPainter(
    Item flow,
    Item previous,
    this.t,
    this.colors,
    this.chartSize,
    Offset? pointer,
    this.fontFamily,
    this.textScaler,
  ) : nodes = (flow['nodes'] as List).cast<Item>(),
      edges = (flow['edges'] as List).cast<List>() {
    final oldNodes = (previous['nodes'] as List? ?? const []).cast<Item>();
    for (final node in oldNodes) {
      oldPositions[nodeId(node)] = node;
    }
    for (final edge in (previous['edges'] as List? ?? const []).cast<List>()) {
      oldLinks[edgeId(edge, oldNodes)] = edge;
    }
    if (pointer != null) select(pointer);
  }
  final List<List> edges;
  final List<Item> nodes;
  final double t;
  final SingboardColors colors;
  final Size chartSize;
  final String? fontFamily;
  final TextScaler textScaler;
  final oldPositions = <(int, String), Item>{};
  final oldLinks = <((int, String), (int, String)), List>{};
  final selected = <int>{};
  String message = '';

  (int, String) nodeId(Item n) => (n['layer'] as int, n['name'] as String);
  ((int, String), (int, String)) edgeId(List edge, List<Item> rows) =>
      (nodeId(rows[edge[5] as int]), nodeId(rows[edge[6] as int]));
  double interpolate(num a, num b) => a.toDouble() + (b - a) * t;
  double get labelWidth =>
      ((chartSize.width - 32) / 4 - 8).floorToDouble().clamp(56.0, 180.0);
  double xOf(int layer) =>
      layer * math.max(0.0, chartSize.width - labelWidth - 14) / 3;

  Rect nodeRect(Item node) {
    final old = oldPositions[nodeId(node)] ?? node;
    return Rect.fromLTWH(
      xOf(node['layer']),
      chartSize.height * interpolate(old['y'], node['y']),
      8,
      chartSize.height * interpolate(old['h'], node['h']),
    );
  }

  (Offset, Offset, double) link(List edge) {
    final old = oldLinks[edgeId(edge, nodes)] ?? edge;
    return (
      Offset(xOf(edge[0]) + 8, chartSize.height * interpolate(old[1], edge[1])),
      Offset(xOf(edge[2]), chartSize.height * interpolate(old[3], edge[3])),
      chartSize.height * interpolate(old[4], edge[4]),
    );
  }

  void select(Offset p) {
    for (var i = 0; i < nodes.length; i++) {
      final node = nodes[i];
      final rect = nodeRect(node);
      if (!Rect.fromLTRB(
        rect.left,
        rect.top,
        rect.right + labelWidth,
        rect.bottom,
      ).contains(p))
        continue;
      message =
          "${node['name']}\n类型: ${const ['来源 IP', '规则', '策略组', '出口'][node['layer']]}\n连接数: ${node['count']}";
      for (final forward in [true, false]) {
        final found = <int>{i};
        for (var depth = 0; depth < 3; depth++) {
          for (final edge in edges) {
            if (found.contains(edge[forward ? 5 : 6]))
              found.add(edge[forward ? 6 : 5]);
          }
        }
        selected.addAll(found);
      }
      return;
    }
    for (final edge in edges) {
      final (a, b, width) = link(edge);
      if (p.dx < a.dx || p.dx > b.dx || b.dx <= a.dx) continue;
      final path = linkPath(a, b);
      final metric = path.computeMetrics().firstOrNull;
      if (metric == null) continue;
      var lo = 0.0, hi = metric.length;
      for (var step = 0; step < 8; step++) {
        final mid = (lo + hi) / 2;
        if (metric.getTangentForOffset(mid)!.position.dx < p.dx) {
          lo = mid;
        } else {
          hi = mid;
        }
      }
      final pos = metric.getTangentForOffset((lo + hi) / 2)!.position;
      if ((p.dy - pos.dy).abs() > math.max(2, width / 2)) continue;
      final aId = edge[5] as int, bId = edge[6] as int;
      selected.addAll([aId, bId]);
      message =
          "${nodes[aId]['name']} → ${nodes[bId]['name']}\n连接数: ${edge[7]}";
      return;
    }
  }

  Path linkPath(Offset a, Offset b) {
    final mid = (a.dx + b.dx) / 2;
    return Path()
      ..moveTo(a.dx, a.dy)
      ..cubicTo(mid, a.dy, mid, b.dy, b.dx, b.dy);
  }

  @override
  void paint(Canvas canvas, Size size) {
    final palette = colors.topologyColors;
    for (final edge in edges) {
      final (a, b, width) = link(edge);
      final opacity = selected.isEmpty
          ? 0.16
          : (selected.contains(edge[5]) && selected.contains(edge[6])
                ? 0.5
                : 0.05);
      final gradient = LinearGradient(
        colors: [
          palette[edge[0]].withValues(alpha: opacity),
          palette[edge[2]].withValues(alpha: opacity),
        ],
      );
      canvas.drawPath(
        linkPath(a, b),
        Paint()
          ..style = PaintingStyle.stroke
          ..shader = gradient.createShader(
            Rect.fromLTRB(a.dx, 0, b.dx, size.height),
          )
          ..strokeWidth = width,
      );
    }
    for (var i = 0; i < nodes.length; i++) {
      final node = nodes[i];
      final rect = nodeRect(node);
      final merged = (node['merged'] as num) > 0;
      final dim = selected.isNotEmpty && !selected.contains(i);
      canvas.drawRect(
        rect,
        Paint()
          ..color = palette[node['layer']].withValues(
            alpha: dim
                ? 0.16
                : merged
                ? 0.35
                : 0.75,
          ),
      );
      final label = TextPainter(
        text: TextSpan(
          text: node['name'],
          style: TextStyle(
            fontSize: 12,
            fontFamily: fontFamily,
            color: colors.ink.withValues(
              alpha: dim
                  ? 0.2
                  : merged
                  ? 0.5
                  : 0.72,
            ),
          ),
        ),
        maxLines: 1,
        ellipsis: '…',
        textDirection: TextDirection.ltr,
        textScaler: textScaler,
      )..layout(maxWidth: labelWidth);
      label.paint(
        canvas,
        Offset(rect.right + 5, rect.center.dy - label.height / 2),
      );
      label.dispose();
    }
  }

  @override
  bool shouldRepaint(_FlowPainter old) => true;
}
