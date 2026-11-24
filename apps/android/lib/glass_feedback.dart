part of 'glass_ui.dart';

CurvedAnimation _feedbackCurve(Animation<double> source, Curve curve) =>
    CurvedAnimation(parent: source, curve: curve, reverseCurve: curve.flipped);

class GlassDialogButtons extends StatelessWidget {
  const GlassDialogButtons({super.key, required this.child});
  final Widget child;

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final label = Theme.of(context).textTheme.labelLarge
        ?.copyWith(fontSize: 14, fontWeight: FontWeight.w600);
    return TextButtonTheme(
      data: TextButtonThemeData(
        style: TextButton.styleFrom(
          foregroundColor: c.ink,
          minimumSize: const Size(0, 32),
          padding: const EdgeInsets.symmetric(horizontal: 12),
          tapTargetSize: MaterialTapTargetSize.shrinkWrap,
          textStyle: label,
        ),
      ),
      child: FilledButtonTheme(
        data: FilledButtonThemeData(
          style: FilledButton.styleFrom(
            backgroundColor: c.primary,
            foregroundColor: Colors.white,
            minimumSize: const Size(0, 32),
            padding: const EdgeInsets.symmetric(horizontal: 12),
            tapTargetSize: MaterialTapTargetSize.shrinkWrap,
            textStyle: label,
          ),
        ),
        child: child,
      ),
    );
  }
}

class GlassConfirmDialog extends StatelessWidget {
  const GlassConfirmDialog({
    super.key,
    required this.title,
    this.content,
    required this.actions,
  });
  final Widget title;
  final Widget? content;
  final List<Widget> actions;

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    return Dialog(
      backgroundColor: Colors.transparent,
      insetPadding: const EdgeInsets.all(16),
      constraints: const BoxConstraints(maxWidth: 384),
      child: SingleChildScrollView(
        child: GlassCard(
          popover: true,
          padding: const EdgeInsets.all(20),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              DefaultTextStyle.merge(
                style: TextStyle(
                  fontSize: 18,
                  fontWeight: FontWeight.w600,
                  color: c.ink,
                ),
                child: title,
              ),
              if (content != null) ...[
                const SizedBox(height: 16),
                DefaultTextStyle.merge(
                  style: TextStyle(fontSize: 14, height: 1.45, color: c.ink),
                  child: content!,
                ),
              ],
              const SizedBox(height: 16),
              Wrap(
                alignment: WrapAlignment.end,
                spacing: 8,
                runSpacing: 8,
                children: actions,
              ),
            ],
          ),
        ),
      ),
    );
  }
}

Future<T?> showGlassDialog<T>({
  required BuildContext context,
  required WidgetBuilder builder,
  bool barrierDismissible = true,
  bool requestFocus = true,
}) {
  final navigator = Navigator.of(context, rootNavigator: true);
  final themes = InheritedTheme.capture(from: context, to: navigator.context);
  final reduceMotion = MediaQuery.disableAnimationsOf(context);
  CurvedAnimation? fade, zoom;
  final route = RawDialogRoute<T>(
    requestFocus: requestFocus,
    barrierDismissible: barrierDismissible,
    traversalEdgeBehavior: TraversalEdgeBehavior.closedLoop,
    barrierColor: Colors.transparent,
    barrierLabel: MaterialLocalizations.of(context).modalBarrierDismissLabel,
    transitionDuration: Duration(milliseconds: reduceMotion ? 0 : 220),
    pageBuilder: (context, _, __) => SafeArea(
      child: themes.wrap(GlassDialogButtons(child: Builder(builder: builder))),
    ),
    transitionBuilder: (_, animation, __, child) {
      final opacity = (fade ??= _feedbackCurve(
        animation,
        const Interval(0, 180 / 220, curve: Curves.ease),
      )).value;
      final scale = (zoom ??= _feedbackCurve(
        animation,
        const Cubic(.32, .72, 0, 1),
      )).value;
      return Stack(
        children: [
          Positioned.fill(
            child: IgnorePointer(
              child: ClipRect(
                child: BackdropFilter(
                  filter: ui.ImageFilter.blur(
                    sigmaX: opacity * 8,
                    sigmaY: opacity * 8,
                  ),
                  child: const SizedBox.expand(),
                ),
              ),
            ),
          ),
          Positioned.fill(
            child: Opacity(
              opacity: opacity,
              child: Transform.scale(scale: .94 + .06 * scale, child: child),
            ),
          ),
        ],
      );
    },
  );
  route.completed.whenComplete(() {
    fade?.dispose();
    zoom?.dispose();
  });
  return navigator.push(route);
}

class GlassPageTransitionsBuilder extends PageTransitionsBuilder {
  const GlassPageTransitionsBuilder();

  @override
  Widget buildTransitions<T>(
    PageRoute<T> route,
    BuildContext context,
    Animation<double> animation,
    Animation<double> secondaryAnimation,
    Widget child,
  ) {
    if (MediaQuery.disableAnimationsOf(context)) return child;
    return _GlassPageTransition(
      animation: animation,
      secondaryAnimation: secondaryAnimation,
      child: child,
    );
  }
}

class _GlassPageTransition extends StatefulWidget {
  final Animation<double> animation;
  final Animation<double> secondaryAnimation;
  final Widget child;

  const _GlassPageTransition({
    required this.animation,
    required this.secondaryAnimation,
    required this.child,
  });

  @override
  State<_GlassPageTransition> createState() => _GlassPageTransitionState();
}

class _GlassPageTransitionState extends State<_GlassPageTransition> {
  late CurvedAnimation _enterCurve;
  late CurvedAnimation _exitCurve;
  late Animation<double> _exitOpacity;

  void _initAnimations() {
    _enterCurve = CurvedAnimation(
      parent: widget.animation,
      curve: const Interval(0.15, 1.0, curve: Curves.easeOut),
      reverseCurve: const Interval(0.0, 0.75, curve: Curves.easeIn),
    );
    _exitCurve = CurvedAnimation(
      parent: widget.secondaryAnimation,
      curve: const Interval(0.0, 0.55, curve: Curves.easeOut),
      reverseCurve: const Interval(0.25, 1.0, curve: Curves.easeIn),
    );
    _exitOpacity = Tween<double>(begin: 1.0, end: 0.0).animate(_exitCurve);
  }

  @override
  void initState() {
    super.initState();
    _initAnimations();
  }

  @override
  void didUpdateWidget(_GlassPageTransition old) {
    super.didUpdateWidget(old);
    if (old.animation != widget.animation ||
        old.secondaryAnimation != widget.secondaryAnimation) {
      _enterCurve.dispose();
      _exitCurve.dispose();
      _initAnimations();
    }
  }

  @override
  void dispose() {
    _enterCurve.dispose();
    _exitCurve.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => FadeTransition(
    opacity: _enterCurve,
    child: FadeTransition(
      opacity: _exitOpacity,
      child: widget.child,
    ),
  );
}

Future<T?> showGlassSheet<T>({
  required BuildContext context,
  required WidgetBuilder builder,
  bool barrierDismissible = true,
}) {
  final navigator = Navigator.of(context, rootNavigator: true);
  final themes = InheritedTheme.capture(from: context, to: navigator.context);
  final reduceMotion = MediaQuery.disableAnimationsOf(context);
  final dark = SingboardColors.of(context).dark;
  CurvedAnimation? fade, slide;
  final route = RawDialogRoute<T>(
    barrierDismissible: barrierDismissible,
    traversalEdgeBehavior: TraversalEdgeBehavior.closedLoop,
    barrierColor: Colors.transparent,
    barrierLabel: MaterialLocalizations.of(context).modalBarrierDismissLabel,
    transitionDuration: Duration(milliseconds: reduceMotion ? 0 : 240),
    pageBuilder: (sheetContext, _, __) => themes.wrap(
      Align(
        alignment: Alignment.bottomCenter,
        child: Padding(
          padding: EdgeInsets.fromLTRB(
            10,
            MediaQuery.paddingOf(sheetContext).top + 16,
            10,
            MediaQuery.viewInsetsOf(sheetContext).bottom +
                MediaQuery.paddingOf(sheetContext).bottom +
                10,
          ),
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 520),
            child: Material(
              type: MaterialType.transparency,
              child: _GlassSheetDragDismiss(
                enabled: barrierDismissible,
                child: Builder(builder: builder),
              ),
            ),
          ),
        ),
      ),
    ),
    transitionBuilder: (_, animation, __, child) {
      final opacity = (fade ??= _feedbackCurve(
        animation,
        const Interval(0, 180 / 240, curve: Curves.easeOut),
      )).value;
      final offset = (slide ??= _feedbackCurve(
        animation,
        const Cubic(.32, .72, 0, 1),
      )).value;
      return Stack(
        children: [
          Positioned.fill(
            child: IgnorePointer(
              child: ClipRect(
                child: BackdropFilter(
                  filter: ui.ImageFilter.blur(
                    sigmaX: opacity * 14,
                    sigmaY: opacity * 14,
                  ),
                  child: ColoredBox(
                    color: Colors.black.withValues(
                      alpha: opacity * (dark ? 0.36 : 0.18),
                    ),
                  ),
                ),
              ),
            ),
          ),
          Positioned.fill(
            child: Opacity(
              opacity: opacity,
              child: FractionalTranslation(
                translation: Offset(0, 0.16 * (1 - offset)),
                child: child,
              ),
            ),
          ),
        ],
      );
    },
  );
  route.completed.whenComplete(() {
    fade?.dispose();
    slide?.dispose();
  });
  return navigator.push(route);
}

class _GlassSheetDragDismiss extends StatefulWidget {
  final bool enabled;
  final Widget child;
  const _GlassSheetDragDismiss({required this.enabled, required this.child});

  @override
  State<_GlassSheetDragDismiss> createState() => _GlassSheetDragDismissState();
}

class _GlassSheetDragDismissState extends State<_GlassSheetDragDismiss>
    with SingleTickerProviderStateMixin {
  double _offset = 0.0;
  late final AnimationController _reset = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 200),
  );
  double _resetStart = 0.0;

  @override
  void initState() {
    super.initState();
    _reset.addListener(() {
      final t = Curves.easeOutCubic.transform(_reset.value);
      setState(() => _offset = _resetStart * (1.0 - t));
    });
  }

  @override
  void dispose() {
    _reset.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (!widget.enabled) return widget.child;
    return GestureDetector(
      behavior: HitTestBehavior.translucent,
      onVerticalDragStart: (_) => _reset.stop(),
      onVerticalDragUpdate: (d) {
        setState(() => _offset = (_offset + d.delta.dy).clamp(0.0, 480.0));
      },
      onVerticalDragEnd: (d) {
        final v = d.primaryVelocity ?? 0.0;
        if (_offset > 96.0 || v > 650.0) {
          Navigator.of(context).maybePop();
        } else if (_offset > 0.0) {
          _resetStart = _offset;
          _reset.forward(from: 0.0);
        }
      },
      onVerticalDragCancel: () {
        if (_offset > 0.0) {
          _resetStart = _offset;
          _reset.forward(from: 0.0);
        }
      },
      child: Transform.translate(
        offset: Offset(0, _offset),
        child: widget.child,
      ),
    );
  }
}

class GlassToastHost extends StatefulWidget {
  const GlassToastHost({super.key, required this.child});
  final Widget child;

  @override
  State<GlassToastHost> createState() => _GlassToastHostState();
}

void showGlassToast(BuildContext context, String message, {bool error = true}) {
  final host = context.findAncestorStateOfType<_GlassToastHostState>();
  if (host != null) {
    host.add(message, error);
  } else {
    ScaffoldMessenger.maybeOf(context)
        ?.showSnackBar(SnackBar(content: Text(message)));
  }
}

class _GlassToastHostState extends State<GlassToastHost> {
  final items = <(Key, String, bool)>[];

  void add(String message, bool error) =>
      setState(() => items.add((UniqueKey(), message, error)));

  @override
  Widget build(BuildContext context) => Stack(
    children: [
      widget.child,
      if (items.isNotEmpty)
        Positioned(
          top: MediaQuery.paddingOf(context).top + 56,
          right: 16,
          left: 16,
          child: Align(
            alignment: Alignment.topRight,
            child: ConstrainedBox(
              constraints: BoxConstraints(
                maxWidth: 420,
                maxHeight: math.max(
                  0,
                  MediaQuery.sizeOf(context).height -
                      MediaQuery.paddingOf(context).vertical -
                      80,
                ),
              ),
              child: SingleChildScrollView(
                child: Column(
                  mainAxisSize: MainAxisSize.min,
                  crossAxisAlignment: CrossAxisAlignment.end,
                  children: [
                    for (final item in items)
                      _GlassToast(
                        key: item.$1,
                        message: item.$2,
                        error: item.$3,
                        close: () => setState(() => items.remove(item)),
                      ),
                  ],
                ),
              ),
            ),
          ),
        ),
    ],
  );
}

class _GlassToast extends StatefulWidget {
  const _GlassToast({
    super.key,
    required this.message,
    required this.error,
    required this.close,
  });
  final String message;
  final bool error;
  final VoidCallback close;
  @override
  State<_GlassToast> createState() => _GlassToastState();
}

class _GlassToastState extends State<_GlassToast>
    with TickerProviderStateMixin {
  late final motion = AnimationController(
    vsync: this,
    duration: const Duration(milliseconds: 260),
  );
  late final time = AnimationController(
    vsync: this,
    duration: const Duration(seconds: 5),
  );
  late final fade = _feedbackCurve(
    motion,
    const Interval(0, 220 / 260, curve: Curves.ease),
  );
  late final slide = _feedbackCurve(motion, const Cubic(.32, .72, 0, 1));
  bool closing = false;

  @override
  void initState() {
    super.initState();
    motion.forward();
    time.addStatusListener((s) {
      if (s == AnimationStatus.completed) dismiss();
    });
    time.forward();
  }

  void dismiss() {
    if (closing) return;
    closing = true;
    time.stop();
    motion.reverse().then((_) {
      if (mounted) widget.close();
    });
  }

  @override
  void dispose() {
    fade.dispose();
    slide.dispose();
    motion.dispose();
    time.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final color = widget.error ? c.dangerInk : c.primary;
    final content = Material(
      type: MaterialType.transparency,
      child: Semantics(
        liveRegion: true,
        child: MouseRegion(
          onEnter: (_) => time.stop(),
          onExit: (_) {
            if (!closing) time.forward();
          },
          child: GlassCard(
            popover: true,
            padding: const EdgeInsets.fromLTRB(14, 12, 14, 10),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Row(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Container(
                      margin: const EdgeInsets.only(top: 2),
                      width: 20,
                      height: 20,
                      decoration: BoxDecoration(
                        color: color,
                        shape: BoxShape.circle,
                      ),
                      child: PanelIcon(
                        widget.error ? 'toast-error' : 'toast-info',
                        size: 12,
                        color: Colors.white,
                      ),
                    ),
                    const SizedBox(width: 10),
                    Expanded(
                      child: Text(
                        widget.message,
                        style: TextStyle(
                          fontSize: 14,
                          height: 1.375,
                          color: c.ink,
                        ),
                      ),
                    ),
                    const SizedBox(width: 10),
                    IconButton(
                      padding: EdgeInsets.zero,
                      constraints: const BoxConstraints.tightFor(
                        width: 20,
                        height: 20,
                      ),
                      onPressed: dismiss,
                      icon: Semantics(
                        label: '关闭',
                        child: PanelIcon('x-small', size: 10, color: c.muted),
                      ),
                    ),
                  ],
                ),
                const SizedBox(height: 10),
                AnimatedBuilder(
                  animation: time,
                  builder: (_, __) => ClipRRect(
                    borderRadius: BorderRadius.circular(99),
                    child: LinearProgressIndicator(
                      value: 1 - time.value,
                      minHeight: 3,
                      color: color,
                      backgroundColor: c.fill,
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
    return AnimatedBuilder(
      animation: motion,
      child: content,
      builder: (_, child) {
        final k = slide.value;
        return Padding(
          padding: const EdgeInsets.only(bottom: 8),
          child: Opacity(
            opacity: fade.value,
            child: Transform.translate(
              offset: Offset(0, -8 * (1 - k)),
              child: Transform.scale(scale: .96 + .04 * k, child: child),
            ),
          ),
        );
      },
    );
  }
}
