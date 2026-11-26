part of 'glass_ui.dart';

class _NavSpring {
  double value, target, velocity = 0;
  final double damping, stiffness, threshold;
  _NavSpring(this.value, this.damping, this.stiffness, [this.threshold = .001])
    : target = value;

  bool get settled =>
      (value - target).abs() < threshold && velocity.abs() < threshold * 10;

  void snap() {
    value = target;
    velocity = 0;
  }

  void step(double dt) {
    if (settled) {
      snap();
      return;
    }
    final resistance = 2 * damping * math.sqrt(stiffness);
    while (dt > 0) {
      final h = math.min(1 / 240, dt);
      velocity += (-stiffness * (value - target) - resistance * velocity) * h;
      value += velocity * h;
      dt -= h;
    }
  }
}

class GlassBottomNavItem {
  final String icon, label;
  const GlassBottomNavItem(this.icon, this.label);
}

class GlassBottomNav extends StatefulWidget {
  final List<GlassBottomNavItem> items;
  final int index;
  final ValueChanged<int> onChanged;
  final ValueListenable<double?>? pageProgress;
  const GlassBottomNav({
    super.key,
    required this.items,
    required this.index,
    required this.onChanged,
    this.pageProgress,
  });

  @override
  State<GlassBottomNav> createState() => _GlassBottomNavState();
}

class _GlassBottomNavState extends State<GlassBottomNav>
    with SingleTickerProviderStateMixin {
  late final _position = _NavSpring(widget.index.toDouble(), 1, 1000);
  final _press = _NavSpring(0, 1, 1000),
      _scaleX = _NavSpring(1, .6, 250),
      _scaleY = _NavSpring(1, .7, 250),
      _speed = _NavSpring(0, .5, 300, .01),
      _offset = _NavSpring(0, 1, 300, .5),
      _glow = _NavSpring(0, .5, 300);
  late final _springs = [
    _position,
    _press,
    _scaleX,
    _scaleY,
    _speed,
    _offset,
    _glow,
  ];
  late final Ticker _ticker = createTicker(_tick);
  Duration _last = Duration.zero;
  int? _pointer, _focused;
  double _travel = 0;
  bool _releasing = false;
  double? _externalPage;

  @override
  void initState() {
    super.initState();
    widget.pageProgress?.addListener(_onPageProgress);
  }

  void _onPageProgress() {
    if (!mounted || _pointer != null) return;
    final p = widget.pageProgress?.value;
    if (p != null) {
      final clamped = p.clamp(0.0, (widget.items.length - 1).toDouble());
      final still = MediaQuery.disableAnimationsOf(context);
      _externalPage = clamped;
      _releasing = false;
      _position.target = clamped;
      _glow.target = still ? 0 : .55;
      _scaleX.target = still ? 1 : 1.05;
      _scaleY.target = still ? 1 : 1.05;
      _animate();
    } else if (_externalPage != null) {
      _externalPage = null;
      _position.target = widget.index.toDouble();
      _glow.target = 0;
      _scaleX.target = _scaleY.target = 1;
      _animate();
    }
  }

  void _animate() {
    if (!_ticker.isActive) {
      _last = Duration.zero;
      _ticker.start();
    }
  }

  void _tick(Duration elapsed) {
    final dt = ((elapsed - _last).inMicroseconds / 1e6).clamp(.001, .05);
    _last = elapsed;
    final prior = _position.value;
    final still = MediaQuery.disableAnimationsOf(context);
    _position.step(dt);
    _speed.target = !still && (_pointer != null || _releasing || _externalPage != null)
        ? (_position.value - prior) / dt / (widget.items.length - 1)
        : 0;
    if (_releasing &&
        (_position.value - _position.target).abs() <
            (widget.items.length - 1) * .025) {
      _releasing = false;
      _press.target = 0;
      _scaleX.target = _scaleY.target = 1;
    }
    for (final spring in _springs) {
      if (still) {
        spring.snap();
      } else if (spring != _position) {
        spring.step(dt);
      }
    }
    setState(() {});
    if (!_releasing &&
        _externalPage == null &&
        _springs.every((spring) => spring.settled)) {
      _ticker.stop();
    }
  }

  @override
  void didUpdateWidget(GlassBottomNav old) {
    super.didUpdateWidget(old);
    if (old.pageProgress != widget.pageProgress) {
      old.pageProgress?.removeListener(_onPageProgress);
      widget.pageProgress?.addListener(_onPageProgress);
    }
    if (old.index != widget.index && _pointer == null && _externalPage == null) {
      _position.target = widget.index.toDouble();
      _animate();
    }
  }

  void _finish(int pointer, bool commit) {
    if (pointer != _pointer) return;
    _pointer = null;
    final target = commit ? _position.target.round() : widget.index;
    _position.target = target.toDouble();
    _offset.target = _glow.target = 0;
    _releasing = true;
    if (commit && (widget.index != target || _travel < 8))
      widget.onChanged(target);
    _animate();
  }

  @override
  void dispose() {
    widget.pageProgress?.removeListener(_onPageProgress);
    _ticker.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context), scope = GlassScope.of(context);
    final inset = MediaQuery.viewPaddingOf(context);
    final margin = MediaQuery.sizeOf(context).width < 360
        ? 12.0
        : (widget.items.length <= 5 ? 16.0 : 24.0);
    final barAlpha = scope.alpha(
      0.08 + 0.22 * scope.tint + 0.40 * scope.tint * scope.tint,
      .58,
    );
    final navSel = scope.navSelectionStyle(c, barAlpha: barAlpha);
    Widget row(bool accent) => Padding(
      padding: const EdgeInsets.all(4),
      child: Row(
        children: [
          for (var i = 0; i < widget.items.length; i++)
            Expanded(
              child: Transform.scale(
                scale: accent ? 1 + .2 * _press.value.clamp(0, 1) : 1,
                child: Column(
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: [
                    PanelIcon(
                      widget.items[i].icon,
                      size: 22,
                      color: accent
                          ? navSel.ink
                          : c.ink.withValues(alpha: .72),
                    ),
                    const SizedBox(height: 1),
                    Text(
                      widget.items[i].label,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        fontSize: 12,
                        height: 1.25,
                        color: accent
                            ? navSel.ink
                            : c.ink.withValues(alpha: .72),
                        fontWeight: accent || scope.tint < .5
                            ? FontWeight.w600
                            : FontWeight.w500,
                      ),
                    ),
                  ],
                ),
              ),
            ),
        ],
      ),
    );
    return Padding(
      padding: EdgeInsets.fromLTRB(
        margin + inset.left,
        0,
        margin + inset.right,
        math.max(16, inset.bottom + 8),
      ),
      child: Align(
        heightFactor: 1,
        alignment: Alignment.bottomCenter,
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 560),
          child: LayoutBuilder(
            builder: (_, constraints) {
              final width = constraints.maxWidth,
                  tabWidth = (width - 8) / widget.items.length;
              final press = _press.value.clamp(0.0, 1.0);
              final v = _speed.value / 10;
              final sx = _scaleX.value / (1 - (v * .75).clamp(-.2, .2));
              final sy = _scaleY.value * (1 - (v * .25).clamp(-.2, .2));
              final box = Rect.fromCenter(
                center: Offset(4 + (_position.value + .5) * tabWidth, 32),
                width: tabWidth * sx,
                height: 56 * sy,
              );
              final pill = RRect.fromRectAndRadius(
                box,
                const Radius.circular(99),
              );
              final drift = (_offset.value / width).clamp(-1.0, 1.0);
              return Listener(
                behavior: HitTestBehavior.opaque,
                onPointerDown: (e) {
                  if (_pointer != null) return;
                  _pointer = e.pointer;
                  _externalPage = null;
                  _travel = 0;
                  _releasing = false;
                  _position.target = ((e.localPosition.dx - 4) / tabWidth)
                      .floor()
                      .clamp(0, widget.items.length - 1)
                      .toDouble();
                  _scaleX.target = _scaleY.target =
                      MediaQuery.disableAnimationsOf(context) ? 1 : 78 / 56;
                  _glow.target = _press.target = 1;
                  _animate();
                },
                onPointerMove: (e) {
                  if (e.pointer != _pointer) return;
                  final dx = e.delta.dx;
                  _travel += dx.abs();
                  _position.target = (_position.target + dx / tabWidth).clamp(
                    0.0,
                    (widget.items.length - 1).toDouble(),
                  );
                  _offset.target = _offset.value + dx;
                  _offset.snap();
                  _animate();
                },
                onPointerUp: (e) => _finish(e.pointer, true),
                onPointerCancel: (e) => _finish(e.pointer, false),
                child: Transform.translate(
                  offset: Offset(
                    4 * drift.sign * (1 - math.pow(1 - drift.abs(), 2)),
                    0,
                  ),
                  child: Transform.scale(
                    scale: 1 + 16 / width * press,
                    child: Container(
                      height: 64,
                      decoration: BoxDecoration(
                        borderRadius: BorderRadius.circular(32),
                        boxShadow: [
                          BoxShadow(
                            color: Colors.black.withValues(
                              alpha: c.dark ? .32 : .12,
                            ),
                            blurRadius: 20,
                            offset: const Offset(0, 4),
                          ),
                        ],
                      ),
                      child: ClipRRect(
                        borderRadius: BorderRadius.circular(32),
                        child: _GlassFilter(
                          blur: _glassBlurSigma(scope.tint),
                          rim: 20,
                          amount: 10,
                          spread: 0.035,
                          child: Stack(
                            fit: StackFit.expand,
                            children: [
                              DecoratedBox(
                                decoration: BoxDecoration(
                                  color: c.glassRgb.withValues(
                                    alpha: barAlpha,
                                  ),
                                  image: scope.grain,
                                  borderRadius: BorderRadius.circular(32),
                                  border: Border.all(
                                    width: 1.0,
                                    color: Colors.white.withValues(
                                      alpha: c.dark
                                          ? (0.20 +
                                                0.18 *
                                                    _glassLensStrength(
                                                      scope.tint,
                                                    ))
                                          : (0.64 +
                                                0.30 *
                                                    _glassLensStrength(
                                                      scope.tint,
                                                    )),
                                    ),
                                  ),
                                ),
                              ),
                              if (_glow.value > .001)
                                DecoratedBox(
                                  decoration: BoxDecoration(
                                    gradient: RadialGradient(
                                      center: Alignment(
                                        2 * box.center.dx / width - 1,
                                        0,
                                      ),
                                      radius: 1.2,
                                      colors: [
                                        Colors.white.withValues(
                                          alpha: (.12 * _glow.value).clamp(
                                            0,
                                            1,
                                          ),
                                        ),
                                        Colors.transparent,
                                      ],
                                    ),
                                  ),
                                ),
                              Positioned.fromRect(
                                rect: box,
                                child: ClipRRect(
                                  borderRadius: BorderRadius.circular(99),
                                  child: _GlassFilter(
                                    blur: 0,
                                    radius: 99,
                                    rim: 10,
                                    amount: 14 * press,
                                    spread: .08,
                                    depth: 1,
                                    child: DecoratedBox(
                                      decoration: BoxDecoration(
                                        color: Color.lerp(
                                          navSel.bg,
                                          Colors.black.withValues(alpha: .04),
                                          press,
                                        ),
                                        borderRadius: BorderRadius.circular(99),
                                        border: Border.all(
                                          width: 1,
                                          color: Color.lerp(
                                            navSel.border,
                                            Colors.white.withValues(alpha: .45),
                                            press,
                                          )!,
                                        ),
                                      ),
                                    ),
                                  ),
                                ),
                              ),
                              ExcludeSemantics(
                                child: ClipPath(
                                  clipper: _NavPillClip(pill, outside: true),
                                  child: row(false),
                                ),
                              ),
                              ExcludeSemantics(
                                child: ClipPath(
                                  clipper: _NavPillClip(pill),
                                  child: row(true),
                                ),
                              ),
                              Padding(
                                padding: const EdgeInsets.all(4),
                                child: Row(
                                  children: [
                                    for (
                                      var i = 0;
                                      i < widget.items.length;
                                      i++
                                    )
                                      Expanded(
                                        child: Semantics(
                                          button: true,
                                          selected: i == widget.index,
                                          label: widget.items[i].label,
                                          onTap: () => widget.onChanged(i),
                                          child: FocusableActionDetector(
                                            onShowFocusHighlight: (focus) =>
                                                setState(
                                                  () => _focused = focus
                                                      ? i
                                                      : _focused == i
                                                      ? null
                                                      : _focused,
                                                ),
                                            actions: {
                                              ActivateIntent:
                                                  CallbackAction<
                                                    ActivateIntent
                                                  >(
                                                    onInvoke: (_) {
                                                      widget.onChanged(i);
                                                      return null;
                                                    },
                                                  ),
                                            },
                                            child: Container(
                                              decoration: BoxDecoration(
                                                borderRadius:
                                                    BorderRadius.circular(99),
                                                border: _focused == i
                                                    ? Border.all(
                                                        color: c.primary,
                                                        width: 2,
                                                      )
                                                    : null,
                                              ),
                                            ),
                                          ),
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
                ),
              );
            },
          ),
        ),
      ),
    );
  }
}

class _NavPillClip extends CustomClipper<Path> {
  final RRect pill;
  final bool outside;
  const _NavPillClip(this.pill, {this.outside = false});
  @override
  Path getClip(Size size) {
    final clip = Path()..fillType = PathFillType.evenOdd;
    if (outside) clip.addRect(Offset.zero & size);
    clip.addRRect(pill);
    return clip;
  }

  @override
  bool shouldReclip(_NavPillClip old) =>
      old.pill != pill || old.outside != outside;
}
