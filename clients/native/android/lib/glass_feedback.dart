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
  final dark = SingboardColors.of(context).dark;
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
                    sigmaX: opacity * 3,
                    sigmaY: opacity * 3,
                  ),
                  child: ColoredBox(
                    color: (dark ? Colors.black : Colors.white).withValues(
                      alpha: opacity * (dark ? .35 : .22),
                    ),
                  ),
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
