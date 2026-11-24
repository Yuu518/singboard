import 'dart:async';
import 'dart:io';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:material_ui/material_ui.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/rendering.dart' show PipelineOwner, RenderProxyBox;
import 'package:flutter/scheduler.dart' show Ticker;
import 'package:flutter/services.dart' show HapticFeedback;

part 'glass_navigation.dart';
part 'glass_filter.dart';
part 'glass_feedback.dart';

class PanelIcon extends StatelessWidget {
  final String name;
  final double size;
  final Color color;

  const PanelIcon(this.name, {super.key, required this.size, required this.color});

  @override
  Widget build(BuildContext context) => Center(widthFactor: 1, heightFactor: 1, child: SizedBox.square(
    dimension: size,
    child: SvgPicture.asset('assets/icons/$name.svg',
      width: size, height: size, excludeFromSemantics: true,
      colorFilter: ColorFilter.mode(color, BlendMode.srcIn)),
  ));
}

double panelGutter(BuildContext context) => MediaQuery.sizeOf(context).width >= 768 ? 24 : 12;

class SingboardColors {
  final bool dark;
  final bool wallpaper;
  const SingboardColors(this.dark, {this.wallpaper = false});

  static SingboardColors of(BuildContext context) {
    final hasWallpaper =
        context.dependOnInheritedWidgetOfExactType<GlassScope>()?.hasWallpaper ??
        false;
    return SingboardColors(
      Theme.of(context).brightness == Brightness.dark,
      wallpaper: hasWallpaper,
    );
  }

  Color get page => dark ? const Color(0xFF0F0F11) : const Color(0xFFF2F2F7);
  Color get surface => dark ? const Color(0xFF20232D) : const Color(0xFFFFFFFF);
  Color get primary => dark ? const Color(0xFF0A84FF) : const Color(0xFF007AFF);
  Color get selectionInk =>
      dark ? const Color(0xFF62AFFF) : const Color(0xFF0057BD);
  Color get primarySoft =>
      dark ? const Color(0x330A84FF) : const Color(0x24007AFF);
  Color get selectionBorder =>
      dark ? const Color(0x660A84FF) : const Color(0x66007AFF);
  Color get secondary =>
      dark ? const Color(0xFF5E5CE6) : const Color(0xFF5856D6);
  Color get accent => dark ? const Color(0xFFBF5AF2) : const Color(0xFFAF52DE);
  Color get ink => dark ? const Color(0xFFF2F2F7) : const Color(0xFF1C1C1E);
  Color get muted => wallpaper
      ? (dark ? const Color(0xFFD4D4DC) : const Color(0xFF3E3E44))
      : (dark ? const Color(0xFFB5B5BC) : const Color(0xFF64646A));
  Color get success => dark ? const Color(0xFF30D158) : const Color(0xFF34C759);
  Color get successInk =>
      dark ? const Color(0xFF30D158) : const Color(0xFF1B7A34);
  Color get successSoft =>
      dark ? const Color(0x2E30D158) : const Color(0x2434C759);
  Color get warning => dark ? const Color(0xFFFF9F0A) : const Color(0xFFFF9500);
  Color get warningInk =>
      dark ? const Color(0xFFFF9F0A) : const Color(0xFFA65C00);
  Color get warningSoft =>
      dark ? const Color(0x2EFF9F0A) : const Color(0x24FF9500);
  Color get error => dark ? const Color(0xFFFF453A) : const Color(0xFFFF3B30);
  Color get dangerInk =>
      dark ? const Color(0xFFFF8F87) : const Color(0xFFB5150B);
  Color get dangerSoft =>
      dark ? const Color(0x2EFF453A) : const Color(0x1FFF3B30);
  Color get tcp => dark ? const Color(0xFF64D2FF) : const Color(0xFF006A8E);
  Color get tcpSoft => dark ? const Color(0x2664D2FF) : const Color(0x1F007AFF);
  Color get udp => dark ? const Color(0xFFD38DF5) : const Color(0xFF8E2FBF);
  Color get udpSoft => dark ? const Color(0x29BF5AF2) : const Color(0x1FAF52DE);
  Color get rowError =>
      dark ? const Color(0x1CFF453A) : const Color(0x14FF3B30);
  Color get rowWarn => dark ? const Color(0x1CFF9F0A) : const Color(0x14FF9500);
  Color get fill => dark ? const Color(0x3D767680) : const Color(0x1F767680);
  Color get fillStrong =>
      dark ? const Color(0x57767680) : const Color(0x33767680);
  Color get hairline =>
      dark ? const Color(0x80545458) : const Color(0x1F3C3C43);
  Color get segmentThumb =>
      dark ? const Color(0xFF636366) : const Color(0xFFFFFFFF);
  Color get glassRgb =>
      dark ? const Color(0xFF2C2C30) : const Color(0xFFFFFFFF);
  Color get backdropVeil =>
      dark ? const Color(0x61000000) : const Color(0x47FFFFFF);
  Color get chartInk =>
      dark ? const Color(0xFFF2F2F7) : const Color(0xFF1C1C1E);
  List<Color> get topologyColors => dark
      ? const [
          Color(0xFF5E5CE6),
          Color(0xFF64D2FF),
          Color(0xFFBF5AF2),
          Color(0xFFFF375F),
        ]
      : const [
          Color(0xFF5856D6),
          Color(0xFF32ADE6),
          Color(0xFFAF52DE),
          Color(0xFFFF2D55),
        ];
}

class GlassScope extends InheritedWidget {
  final double tint;
  final String? wallpaperPath;
  final int wallpaperRevision;
  final bool contrastingBackdrop;
  final bool busyBackdrop;
  final bool? backdropToneLight;
  final bool? headerToneLight;
  final Color? navBackdropRgb;
  final ui.Image? sourceWallpaper;
  final ui.Image? blurredWallpaper;
  final bool inPopover;

  const GlassScope({
    super.key,
    required this.tint,
    required this.wallpaperPath,
    required this.wallpaperRevision,
    this.contrastingBackdrop = false,
    this.busyBackdrop = false,
    this.backdropToneLight,
    this.headerToneLight,
    this.navBackdropRgb,
    this.sourceWallpaper,
    this.blurredWallpaper,
    this.inPopover = false,
    required super.child,
  });

  bool get hasWallpaper => wallpaperPath != null && wallpaperPath!.isNotEmpty;
  DecorationImage? get grain => tint < .5 ? null : const DecorationImage(
    image: AssetImage('assets/glass_grain.png'),
    fit: BoxFit.none,
    alignment: Alignment.topLeft,
    repeat: ImageRepeat.repeat,
    filterQuality: FilterQuality.low,
  );
  double alpha(double raw, double floor) => contrastingBackdrop ? math.max(raw, floor) : raw;
  double get controlAlpha => alpha(.36 + .32 * tint.clamp(0.0, 1.0), .68);

  Color backdropInk(SingboardColors c) {
    final light = backdropToneLight;
    if (!hasWallpaper || light == null) return c.ink;
    return light ? const Color(0xFF16171D) : Colors.white;
  }

  Color backdropHeaderInk(SingboardColors c) {
    final light = headerToneLight ?? backdropToneLight;
    if (!hasWallpaper || light == null) return c.ink;
    return light ? const Color(0xFF16171D) : Colors.white;
  }

  Color backdropMuted(SingboardColors c) {
    final light = backdropToneLight;
    if (!hasWallpaper || light == null) return c.muted;
    return light ? const Color(0xE616171D) : const Color(0xEBFFFFFF);
  }

  List<Shadow>? backdropShadows() {
    final light = backdropToneLight;
    if (!hasWallpaper || light == null || light) return null;
    return const [
      Shadow(color: Color(0x52000000), offset: Offset(0, 1), blurRadius: 8),
    ];
  }

  ({Color bg, Color border, Color ink}) navSelectionStyle(
    SingboardColors c, {
    required double barAlpha,
  }) {
    final fallback = c.dark
        ? const Color(0xFF0E1626)
        : const Color(0xFFE7F0FA);
    final backdrop = hasWallpaper ? (navBackdropRgb ?? fallback) : fallback;
    final surface = c.glassRgb.withValues(alpha: barAlpha.clamp(0.0, 1.0));
    final surfaceRgb = Color.alphaBlend(surface, backdrop);
    final luma = surfaceRgb.computeLuminance();
    final hsl = HSLColor.fromColor(surfaceRgb);
    final h = hsl.hue;
    final s = hsl.saturation;
    final tintS = s < 0.08 ? 0.0 : math.min(s * 0.30, 0.18);

    Color bg;
    Color border;
    if (!c.dark) {
      if (luma >= 0.68) {
        final base = HSLColor.fromAHSL(
          1.0,
          h,
          math.min(s * 0.42, 0.22),
          0.24,
        ).toColor();
        final a = 0.11 + 0.05 * ((luma - 0.68) / 0.32).clamp(0.0, 1.0);
        bg = base.withValues(alpha: a);
        border = base.withValues(alpha: (a * 1.45).clamp(0.0, 1.0));
      } else {
        final base = HSLColor.fromAHSL(1.0, h, tintS, 0.96).toColor();
        final darkness = ((0.68 - luma) / 0.68).clamp(0.0, 1.0);
        final a = 0.56 + 0.26 * darkness;
        bg = base.withValues(alpha: a);
        border = Colors.white.withValues(alpha: 0.42 + 0.30 * darkness);
      }
    } else if (luma < 0.42) {
      final base = HSLColor.fromAHSL(
        1.0,
        h,
        math.min(s * 0.35, 0.20),
        0.88,
      ).toColor();
      final a = 0.18 + 0.10 * (1.0 - (luma / 0.42).clamp(0.0, 1.0));
      bg = base.withValues(alpha: a);
      border = base.withValues(alpha: math.min(a + 0.12, 0.42));
    } else {
      final base = HSLColor.fromAHSL(1.0, h, tintS, 0.12).toColor();
      final lightness = ((luma - 0.42) / 0.58).clamp(0.0, 1.0);
      final a = 0.66 + 0.20 * lightness;
      bg = base.withValues(alpha: a);
      border = Colors.white.withValues(alpha: 0.26 + 0.12 * lightness);
    }

    final pillRgb = Color.alphaBlend(bg, surfaceRgb);
    final pillLuma = pillRgb.computeLuminance();
    final darkInk = tintS > 0.01
        ? HSLColor.fromAHSL(1.0, h, math.min(s * 0.35, 0.24), 0.11).toColor()
        : const Color(0xFF171A1F);
    final lightInk = tintS > 0.01
        ? HSLColor.fromAHSL(1.0, h, math.min(s * 0.22, 0.14), 0.97).toColor()
        : const Color(0xFFFAFAFC);
    double ratio(Color fg) {
      final l = fg.computeLuminance();
      final hi = math.max(l, pillLuma);
      final lo = math.min(l, pillLuma);
      return (hi + 0.05) / (lo + 0.05);
    }

    final darkCr = ratio(darkInk);
    final lightCr = ratio(lightInk);
    final Color ink;
    if (darkCr >= lightCr) {
      ink = darkCr >= 7.0 ? darkInk : const Color(0xFF0F1217);
    } else {
      ink = lightCr >= 7.0 ? lightInk : Colors.white;
    }
    return (bg: bg, border: border, ink: ink);
  }

  Color adaptivePillBg(SingboardColors c, {Color? tintOverlay}) {
    final t = tint.clamp(0.0, 1.0);
    final base = Color.lerp(
      c.fill,
      (c.dark ? const Color(0xFF2C2C30) : Colors.white).withValues(
        alpha: c.dark ? 0.14 : 0.24,
      ),
      1.0 - t,
    )!;
    return tintOverlay == null ? base : Color.alphaBlend(tintOverlay, base);
  }

  Color adaptivePillBorder(SingboardColors c, {Color? accent}) {
    if (accent != null) {
      return accent.withValues(alpha: c.dark ? 0.45 : 0.36);
    }
    final t = tint.clamp(0.0, 1.0);
    if (hasWallpaper) {
      return Color.lerp(
        c.hairline,
        Colors.white.withValues(alpha: c.dark ? 0.20 : 0.55),
        1.0 - t,
      )!;
    }
    return c.dark ? const Color(0x28FFFFFF) : const Color(0x18000000);
  }

  ({Color bg, Color border, Color ink}) adaptiveSelectionChip(
    SingboardColors c,
  ) {
    final fallback = c.dark ? const Color(0xFF0E1626) : const Color(0xFFE7F0FA);
    final backdrop = hasWallpaper ? (navBackdropRgb ?? fallback) : fallback;
    final t = tint.clamp(0.0, 1.0);
    final cardAlpha = (0.12 + 0.24 * t + 0.38 * t * t).clamp(0.0, 1.0);
    final surface = c.glassRgb.withValues(alpha: cardAlpha);
    final surfaceRgb = Color.alphaBlend(surface, backdrop);
    final luma = surfaceRgb.computeLuminance();
    final hsl = HSLColor.fromColor(surfaceRgb);
    final h = hsl.hue;
    final s = hsl.saturation;
    final tintS = s < 0.08 ? 0.0 : math.min(s * 0.36, 0.24);

    Color bg;
    Color border;
    if (!c.dark) {
      if (luma >= 0.22) {
        final base = HSLColor.fromAHSL(1.0, h, math.min(s * 0.50, 0.30), 0.22).toColor();
        final a = 0.16 + 0.08 * ((luma - 0.22) / 0.78).clamp(0.0, 1.0);
        bg = base.withValues(alpha: a);
        border = base.withValues(alpha: math.min(a * 1.7, 0.44));
      } else {
        final base = HSLColor.fromAHSL(1.0, h, tintS, 0.92).toColor();
        final darkness = ((0.22 - luma) / 0.22).clamp(0.0, 1.0);
        final a = 0.34 + 0.18 * darkness;
        bg = base.withValues(alpha: a);
        border = Colors.white.withValues(alpha: 0.42 + 0.22 * darkness);
      }
    } else if (luma < 0.42) {
      final base = HSLColor.fromAHSL(1.0, h, math.min(s * 0.40, 0.24), 0.88).toColor();
      final a = 0.17 + 0.08 * (1.0 - (luma / 0.42).clamp(0.0, 1.0));
      bg = base.withValues(alpha: a);
      border = base.withValues(alpha: math.min(a + 0.14, 0.44));
    } else {
      final base = HSLColor.fromAHSL(1.0, h, tintS, 0.12).toColor();
      final lightness = ((luma - 0.42) / 0.58).clamp(0.0, 1.0);
      final a = 0.56 + 0.20 * lightness;
      bg = base.withValues(alpha: a);
      border = Colors.white.withValues(alpha: 0.28 + 0.14 * lightness);
    }

    final pillRgb = Color.alphaBlend(bg, surfaceRgb);
    final pillLuma = pillRgb.computeLuminance();
    final darkInk = tintS > 0.01
        ? HSLColor.fromAHSL(1.0, h, math.min(s * 0.35, 0.24), 0.11).toColor()
        : const Color(0xFF171A1F);
    final lightInk = tintS > 0.01
        ? HSLColor.fromAHSL(1.0, h, math.min(s * 0.22, 0.14), 0.97).toColor()
        : const Color(0xFFFAFAFC);
    double ratio(Color fg) {
      final l = fg.computeLuminance();
      final hi = math.max(l, pillLuma);
      final lo = math.min(l, pillLuma);
      return (hi + 0.05) / (lo + 0.05);
    }

    final darkCr = ratio(darkInk);
    final lightCr = ratio(lightInk);
    final Color ink;
    if (darkCr >= lightCr) {
      ink = darkCr >= 7.0 ? darkInk : const Color(0xFF0F1217);
    } else {
      ink = lightCr >= 7.0 ? lightInk : Colors.white;
    }
    return (bg: bg, border: border, ink: ink);
  }

  static GlassScope of(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<GlassScope>() ??
      const GlassScope(
        tint: 0.5,
        wallpaperPath: null,
        wallpaperRevision: 0,
        child: SizedBox.shrink(),
      );

  @override
  bool updateShouldNotify(GlassScope old) =>
      tint != old.tint ||
      contrastingBackdrop != old.contrastingBackdrop ||
      busyBackdrop != old.busyBackdrop ||
      backdropToneLight != old.backdropToneLight ||
      headerToneLight != old.headerToneLight ||
      navBackdropRgb != old.navBackdropRgb ||
      wallpaperPath != old.wallpaperPath ||
      wallpaperRevision != old.wallpaperRevision ||
      !identical(sourceWallpaper, old.sourceWallpaper) ||
      !identical(blurredWallpaper, old.blurredWallpaper) ||
      inPopover != old.inPopover;
}

class GlassScrollScope extends InheritedWidget {
  final Listenable? listenable;

  const GlassScrollScope({
    super.key,
    required this.listenable,
    required super.child,
  });

  static Listenable? maybeOf(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<GlassScrollScope>()?.listenable;

  @override
  bool updateShouldNotify(GlassScrollScope old) => listenable != old.listenable;
}

class _DefaultAuroraPainter extends CustomPainter {
  final bool dark;
  const _DefaultAuroraPainter(this.dark);

  @override
  void paint(Canvas canvas, Size size) {
    final rect = Offset.zero & size;
    final baseColors = dark
        ? const [Color(0xFF080D1A), Color(0xFF0E1020), Color(0xFF09121E), Color(0xFF110D1D)]
        : const [Color(0xFFE7F0FD), Color(0xFFEEEDFC), Color(0xFFE6F5FA), Color(0xFFF3EDF9)];
    canvas.drawRect(
      rect,
      Paint()
        ..shader = ui.Gradient.linear(
          Offset.zero,
          Offset(size.width, size.height),
          [baseColors[0], baseColors[1], baseColors[3]],
          const [0.0, 0.5, 1.0],
        ),
    );
    canvas.drawRect(
      rect,
      Paint()
        ..shader = ui.Gradient.linear(
          Offset(size.width, 0),
          Offset(0, size.height),
          [baseColors[1].withValues(alpha: .45), baseColors[2].withValues(alpha: .55)],
        ),
    );
    final blooms = dark
        ? const [
            (Offset(.12, .16), .64, Color(0x5C1276FF)),
            (Offset(.86, .18), .58, Color(0x4D6858F6)),
            (Offset(.20, .88), .66, Color(0x3D10B2D6)),
            (Offset(.82, .80), .56, Color(0x33AC4CDA)),
          ]
        : const [
            (Offset(.13, .16), .62, Color(0x6156ACFF)),
            (Offset(.86, .18), .58, Color(0x4D8E82FF)),
            (Offset(.22, .88), .66, Color(0x4548D6EE)),
            (Offset(.80, .78), .56, Color(0x38EE9CE0)),
          ];
    final maxDim = math.max(size.width, size.height);
    for (final (center, radius, color) in blooms) {
      final pos = Offset(size.width * center.dx, size.height * center.dy);
      final r = maxDim * radius;
      canvas.drawCircle(
        pos,
        r,
        Paint()
          ..shader = ui.Gradient.radial(
            pos,
            r,
            [color, color.withValues(alpha: 0)],
          ),
      );
    }
    final ribbon = Path()
      ..moveTo(0, size.height * .60)
      ..cubicTo(
        size.width * .30,
        size.height * .46,
        size.width * .68,
        size.height * .62,
        size.width,
        size.height * .40,
      );
    canvas.drawPath(
      ribbon,
      Paint()
        ..style = PaintingStyle.stroke
        ..strokeWidth = size.height * .18
        ..maskFilter = const MaskFilter.blur(BlurStyle.normal, 48)
        ..color = dark
            ? const Color(0x1F60B8FF)
            : const Color(0x52FFFFFF),
    );
  }

  @override
  bool shouldRepaint(_DefaultAuroraPainter old) => old.dark != dark;
}

class _GlassBackdropHost extends InheritedWidget {
  const _GlassBackdropHost({required super.child});

  @override
  bool updateShouldNotify(_GlassBackdropHost old) => false;
}

class GlassBackdrop extends StatelessWidget {
  final Widget child;
  const GlassBackdrop({super.key, required this.child});

  @override
  Widget build(BuildContext context) {
    if (context.dependOnInheritedWidgetOfExactType<_GlassBackdropHost>() != null) {
      return child;
    }
    final c = SingboardColors.of(context);
    final scope = GlassScope.of(context);
    final source = scope.sourceWallpaper;
    return _GlassBackdropHost(
      child: Stack(
        fit: StackFit.expand,
        children: [
          RepaintBoundary(
            child: scope.hasWallpaper
                ? Stack(
                    fit: StackFit.expand,
                    children: [
                      ColoredBox(color: c.page),
                      Image.file(
                        File(scope.wallpaperPath!),
                        key: ValueKey(
                          '${scope.wallpaperPath}:${scope.wallpaperRevision}',
                        ),
                        fit: BoxFit.cover,
                        gaplessPlayback: true,
                        frameBuilder: (_, img, frame, sync) {
                          if (sync || frame != null) return img;
                          if (source != null) {
                            return RawImage(
                              image: source,
                              fit: BoxFit.cover,
                              filterQuality: FilterQuality.medium,
                            );
                          }
                          return const SizedBox.shrink();
                        },
                        errorBuilder: (_, __, ___) => CustomPaint(
                          painter: _DefaultAuroraPainter(c.dark),
                          isComplex: true,
                          willChange: false,
                        ),
                      ),
                      ColoredBox(color: c.backdropVeil),
                    ],
                  )
                : CustomPaint(
                    painter: _DefaultAuroraPainter(c.dark),
                    isComplex: true,
                    willChange: false,
                  ),
          ),
          child,
        ],
      ),
    );
  }
}

class GlassControl extends StatelessWidget {
  final Widget child;
  final double radius;
  final Color? tint;
  final bool bordered;

  const GlassControl({
    super.key,
    required this.child,
    this.radius = 10,
    this.tint,
    this.bordered = true,
  });

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final scope = GlassScope.of(context);
    final fill = tint ?? c.fill;
    final edge = bordered
        ? Border.all(
            color: c.dark
                ? Colors.white.withValues(alpha: .12)
                : Colors.black.withValues(alpha: .08),
            width: 0.5,
          )
        : null;
    if (!scope.hasWallpaper || scope.inPopover) {
      return DecoratedBox(
        decoration: BoxDecoration(
          color: fill,
          borderRadius: BorderRadius.circular(radius),
          border: edge,
        ),
        child: child,
      );
    }
    final box = DecoratedBox(
      decoration: BoxDecoration(
        color: Color.alphaBlend(
          fill,
          c.glassRgb.withValues(alpha: scope.controlAlpha),
        ),
        borderRadius: BorderRadius.circular(radius),
        border: edge,
      ),
      child: child,
    );
    final wallpaper = scope.blurredWallpaper;
    if (wallpaper != null) {
      return _WallpaperGlass(
        image: wallpaper,
        radius: radius,
        tint: scope.tint,
        refract: false,
        child: box,
      );
    }
    return box;
  }
}

class GlassCard extends StatelessWidget {
  final Widget child;
  final EdgeInsetsGeometry padding;
  final double radius;
  final bool floating, popover;
  final Color? tintColor;
  final Color? borderColor;

  const GlassCard({
    super.key,
    required this.child,
    this.padding = const EdgeInsets.all(16),
    this.radius = 18,
    this.floating = false,
    this.popover = false,
    this.tintColor,
    this.borderColor,
  });

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final scope = GlassScope.of(context);
    final t = scope.tint.clamp(0.0, 1.0);
    final lens = _glassLensStrength(t);
    final useBackdrop = scope.hasWallpaper || popover || floating;
    final alpha = popover
        ? scope.alpha(
            math.max(0.26 + 0.22 * t + 0.32 * t * t, c.dark ? 0.68 : 0.72),
            0.82,
          )
        : floating
        ? scope.alpha(0.08 + 0.22 * t + 0.40 * t * t, 0.58)
        : scope.alpha(0.12 + 0.24 * t + 0.38 * t * t, 0.64);
    final blur = useBackdrop
        ? (popover ? math.max(_glassBlurSigma(t), 22.0) : _glassBlurSigma(t))
        : 0.0;
    final bg = tintColor ?? c.glassRgb.withValues(alpha: alpha);
    final border =
        borderColor ??
        Colors.white.withValues(
          alpha: c.dark ? (0.20 + 0.18 * lens) : (0.64 + 0.30 * lens),
        );
    final shape = BorderRadius.circular(radius);
    if (!popover && scope.hasWallpaper) {
      final wallpaper = scope.blurredWallpaper;
      final cardBox = Container(
        decoration: BoxDecoration(
          color: wallpaper == null
              ? (tintColor ?? c.glassRgb.withValues(alpha: scope.alpha(alpha, 0.72)))
              : bg,
          image: scope.grain,
          borderRadius: shape,
          border: Border.all(color: border, width: 1.0),
        ),
        child: Padding(padding: padding, child: child),
      );
      return DecoratedBox(
        decoration: BoxDecoration(
          borderRadius: shape,
          boxShadow: [
            BoxShadow(
              color: Colors.black.withValues(alpha: c.dark ? .34 : .10),
              blurRadius: 18,
              offset: const Offset(0, 4),
            ),
          ],
        ),
        child: wallpaper != null
            ? _WallpaperGlass(
                image: wallpaper,
                radius: radius,
                tint: t,
                child: cardBox,
              )
            : cardBox,
      );
    }
    if (!useBackdrop) {
      return RepaintBoundary(
        child: Container(
          decoration: BoxDecoration(
            color: bg,
            image: scope.grain,
            borderRadius: shape,
            border: Border.all(color: border, width: 0.5),
            boxShadow: [
              BoxShadow(
                color: Colors.black.withValues(alpha: c.dark ? .30 : .08),
                blurRadius: 16,
                offset: const Offset(0, 4),
              ),
            ],
          ),
          child: Padding(padding: padding, child: child),
        ),
      );
    }
    final innerChild = popover
        ? GlassScope(
            tint: scope.tint,
            wallpaperPath: scope.wallpaperPath,
            wallpaperRevision: scope.wallpaperRevision,
            contrastingBackdrop: scope.contrastingBackdrop,
            busyBackdrop: scope.busyBackdrop,
            backdropToneLight: scope.backdropToneLight,
            headerToneLight: scope.headerToneLight,
            navBackdropRgb: scope.navBackdropRgb,
            sourceWallpaper: scope.sourceWallpaper,
            blurredWallpaper: scope.blurredWallpaper,
            inPopover: true,
            child: child,
          )
        : child;
    final box = Container(
      decoration: BoxDecoration(
        color: bg,
        image: scope.grain,
        borderRadius: shape,
        border: Border.all(color: border, width: 1.0),
      ),
      child: Padding(padding: padding, child: innerChild),
    );
    final clipped = ClipRRect(
      borderRadius: shape,
      child: blur > 0
          ? _GlassFilter(
              blur: blur, radius: radius, rim: popover ? 18 : 20, amount: popover ? 9 : 10,
              child: box,
            )
          : box,
    );
    if (t >= 0.7) {
      return RepaintBoundary(
        child: DecoratedBox(
          decoration: BoxDecoration(
            borderRadius: shape,
            boxShadow: popover
                ? [
                    BoxShadow(
                      color: (c.dark ? Colors.black : const Color(0xFF0F172A))
                          .withValues(alpha: c.dark ? .52 : .20),
                      blurRadius: 32,
                      offset: const Offset(0, 12),
                    ),
                    BoxShadow(
                      color: (c.dark ? Colors.black : const Color(0xFF0F172A))
                          .withValues(alpha: c.dark ? .32 : .10),
                      blurRadius: 10,
                      offset: const Offset(0, 3),
                    ),
                  ]
                : [
                    BoxShadow(
                      color: Colors.black.withValues(alpha: c.dark ? .34 : .10),
                      blurRadius: 18,
                      offset: const Offset(0, 4),
                    ),
                  ],
          ),
          child: clipped,
        ),
      );
    }
    return RepaintBoundary(
      child: CustomPaint(
        painter: _GlassCardShadow(radius, c.dark, elevated: popover),
        child: clipped,
      ),
    );
  }
}

class _GlassCardShadow extends CustomPainter {
  const _GlassCardShadow(this.radius, this.dark, {this.elevated = false});
  final double radius;
  final bool dark;
  final bool elevated;

  @override
  void paint(Canvas canvas, Size size) {
    final bounds = Offset.zero & size;
    final shape = RRect.fromRectAndRadius(bounds, Radius.circular(radius));
    final mask = Path()
      ..fillType = PathFillType.evenOdd
      ..addRect(bounds.inflate(80))
      ..addRRect(shape);
    canvas.save();
    canvas.clipPath(mask);
    final baseColor = dark ? Colors.black : const Color(0xFF0F172A);
    final shadow = BoxShadow(
      color: baseColor.withValues(
        alpha: elevated ? (dark ? .62 : .22) : (dark ? .55 : .16),
      ),
      blurRadius: elevated ? (dark ? 38 : 34) : (dark ? 30 : 24),
      offset: Offset(0, elevated ? (dark ? 14 : 12) : (dark ? 10 : 8)),
      spreadRadius: elevated ? -8 : -12,
    );
    canvas.drawRRect(shape.shift(shadow.offset).inflate(shadow.spreadRadius), shadow.toPaint());
    canvas.drawRRect(
      shape.inflate(.5),
      Paint()..color = baseColor.withValues(alpha: dark ? .6 : (elevated ? .11 : .08)),
    );
    canvas.restore();
  }

  @override
  bool shouldRepaint(_GlassCardShadow old) =>
      old.radius != radius || old.dark != dark || old.elevated != elevated;
}

class GlassApiGate extends StatelessWidget {
  final (String, String, bool)? mask;
  final Widget child;
  const GlassApiGate({super.key, required this.mask, required this.child});

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final status = mask;
    return Stack(fit: StackFit.expand, children: [
      ExcludeFocus(excluding: status != null, child: ExcludeSemantics(excluding: status != null, child: IgnorePointer(ignoring: status != null, child: child))),
      AnimatedSwitcher(
        duration: Duration(milliseconds: MediaQuery.disableAnimationsOf(context) ? 0 : 200),
        child: status == null ? const SizedBox.shrink() : ClipRect(
          key: ValueKey(status.$1),
          child: BackdropFilter(filter: ui.ImageFilter.blur(sigmaX: 10, sigmaY: 10),
            child: Container(alignment: Alignment.center, padding: const EdgeInsets.symmetric(horizontal: 24),
              color: c.page.withValues(alpha: .55),
              child: Semantics(liveRegion: true, child: Column(mainAxisSize: MainAxisSize.min, children: [
                if (status.$3) SizedBox.square(dimension: 40, child: CircularProgressIndicator(color: c.muted))
                else PanelIcon('power', size: 56, color: c.ink.withValues(alpha: .5)),
                const SizedBox(height: 16),
                Text(status.$1, textAlign: TextAlign.center, style: TextStyle(color: c.ink, fontWeight: FontWeight.w600, fontSize: 18)),
                const SizedBox(height: 4),
                Text(status.$2, textAlign: TextAlign.center, style: TextStyle(color: c.muted, fontSize: 14)),
              ])))),
        )),
    ]);
  }
}

class GlassAppHeader extends StatelessWidget {
  final String title;
  final String? scrolledTitle;
  final bool scrolled;
  final double scrollProgress;
  final bool showStatusPill;
  final double statusPillOpacity;
  final bool connected;
  final bool busy;
  final String statusText;
  final VoidCallback? onStatusTap;

  const GlassAppHeader({
    super.key,
    required this.title,
    this.scrolledTitle,
    this.scrolled = false,
    this.scrollProgress = 0.0,
    required this.showStatusPill,
    this.statusPillOpacity = 1.0,
    required this.connected,
    this.busy = false,
    required this.statusText,
    this.onStatusTap,
  });

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final scope = GlassScope.of(context);
    final t = scope.tint.clamp(0.0, 1.0);
    final progress = (scrollProgress > 0 ? scrollProgress : (scrolled ? 1.0 : 0.0)).clamp(0.0, 1.0);
    final surface = scope.hasWallpaper ? 1.0 : progress;
    final blur = surface > 0.01 && scope.hasWallpaper ? _glassBlurSigma(t) * surface : 0.0;
    final titleStyle = TextStyle(
      fontSize: 15,
      fontWeight: FontWeight.w600,
      letterSpacing: -0.2,
      color: c.ink,
    );
    final Widget titleWidget = scrolledTitle != null && scrolledTitle != title
        ? Stack(
            alignment: Alignment.centerLeft,
            children: [
              Opacity(
                opacity: (1.0 - progress).clamp(0.0, 1.0),
                child: Transform.translate(
                  offset: Offset(0, -4.0 * progress),
                  child: Text(
                    title,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: titleStyle,
                  ),
                ),
              ),
              Opacity(
                opacity: progress,
                child: Transform.translate(
                  offset: Offset(0, 6.0 * (1.0 - progress)),
                  child: Text(
                    scrolledTitle!,
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: titleStyle,
                  ),
                ),
              ),
            ],
          )
        : Text(
            title,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: titleStyle,
          );
    final bar = Container(
      padding: EdgeInsets.only(
        top: MediaQuery.paddingOf(context).top,
        left: panelGutter(context) + 4 + MediaQuery.paddingOf(context).left,
        right: panelGutter(context) + 4 + MediaQuery.paddingOf(context).right,
      ),
      decoration: BoxDecoration(
        color: surface > 0.01
            ? c.glassRgb.withValues(
                alpha:
                    (scope.hasWallpaper
                        ? scope.alpha(0.35 + 0.45 * t, .6)
                        : (0.72 + 0.16 * t)) *
                    surface,
              )
            : Colors.transparent,
        image: surface > 0.01 ? scope.grain : null,
        border: surface > 0.01
            ? Border(
                bottom: BorderSide(
                  color: c.hairline.withValues(alpha: c.hairline.a * surface),
                  width: 0.5,
                ),
              )
            : null,
      ),
      child: SizedBox(
        height: 48,
        child: Row(
          children: [
            ClipRRect(
              borderRadius: BorderRadius.circular(6),
              child: Image.asset('assets/favicon.png', width: 24, height: 24),
            ),
            const SizedBox(width: 8),
            Expanded(child: titleWidget),
            if (showStatusPill && statusPillOpacity > 0.01)
              Opacity(
                opacity: statusPillOpacity.clamp(0.0, 1.0),
                child: IgnorePointer(
                  ignoring: statusPillOpacity < 0.5,
                  child: GestureDetector(
                    onTap: onStatusTap,
                child: Container(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 12,
                    vertical: 5,
                  ),
                  decoration: BoxDecoration(
                    color: Color.lerp(
                      c.fill,
                      Colors.white.withValues(alpha: c.dark ? .12 : .28),
                      1.0 - t,
                    ),
                    borderRadius: BorderRadius.circular(999),
                    border: Border.all(
                      color: Color.lerp(
                        c.hairline,
                        Colors.white.withValues(alpha: c.dark ? .19 : .55),
                        1.0 - t,
                      )!,
                      width: 0.75,
                    ),
                    boxShadow: [
                      BoxShadow(
                        color: Colors.black.withValues(
                          alpha: c.dark ? (.16 + .14 * (1.0 - t)) : (.04 + .06 * (1.0 - t)),
                        ),
                        blurRadius: 8,
                        spreadRadius: -2,
                        offset: const Offset(0, 2),
                      ),
                    ],
                  ),
                  child: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      if (busy) SizedBox.square(dimension: 12, child: CircularProgressIndicator(strokeWidth: 2, color: c.warning))
                      else Container(
                        width: 14,
                        height: 14,
                        decoration: BoxDecoration(
                          shape: BoxShape.circle,
                          color: connected ? c.successSoft : c.dangerSoft,
                        ),
                        alignment: Alignment.center,
                        child: Container(
                          width: 8,
                          height: 8,
                          decoration: BoxDecoration(
                            shape: BoxShape.circle,
                            color: connected ? c.success : c.error,
                          ),
                        ),
                      ),
                      const SizedBox(width: 5),
                      Text(
                        statusText,
                        style: TextStyle(
                          fontSize: 12,
                          height: 16 / 12,
                          fontFeatures: const [FontFeature.tabularFigures()],
                          fontWeight: FontWeight.w500,
                          color: c.ink,
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
    );
    if (blur > 0) {
      return ClipRect(
        child: _GlassFilter(
          blur: blur, lens: false,
          child: bar,
        ),
      );
    }
    return bar;
  }
}

class GlassSearchField extends StatelessWidget {
  final String hint;
  final String value;
  final ValueChanged<String> onChanged;
  final TextEditingController? controller;
  final FocusNode? focusNode;

  const GlassSearchField({
    super.key,
    required this.hint,
    required this.value,
    required this.onChanged,
    this.controller,
    this.focusNode,
  });

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    return GlassControl(
      radius: 10,
      child: ConstrainedBox(
        constraints: const BoxConstraints(minHeight: 32),
        child: TextField(
          controller: controller,
          focusNode: focusNode,
          onChanged: onChanged,
          style: TextStyle(fontSize: 14, color: c.ink),
          decoration: InputDecoration(
            isDense: true,
            border: InputBorder.none,
            contentPadding: const EdgeInsets.symmetric(vertical: 5),
            prefixIcon: Center(widthFactor: 1, child: PanelIcon('search', size: 16, color: c.muted)),
            prefixIconConstraints: const BoxConstraints(
              minWidth: 32,
              minHeight: 32,
            ),
            hintText: hint,
            hintStyle: TextStyle(fontSize: 14, color: c.muted),
            suffixIcon: value.isNotEmpty
                ? IconButton(
                    icon: PanelIcon('x-small', size: 10, color: c.muted),
                    onPressed: () {
                      controller?.clear();
                      onChanged('');
                    },
                  )
                : null,
          ),
        ),
      ),
    );
  }
}

class GlassSegmented<T> extends StatelessWidget {
  final List<(T, String)> items;
  final T value;
  final ValueChanged<T> onChanged;

  const GlassSegmented({
    super.key,
    required this.items,
    required this.value,
    required this.onChanged,
  });

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    return GlassControl(
      radius: 999,
      child: Padding(
        padding: const EdgeInsets.all(2.5),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            for (final (key, label) in items)
              GestureDetector(
                onTap: () => onChanged(key),
                child: AnimatedContainer(
                  duration: const Duration(milliseconds: 180),
                  curve: Curves.easeOutCubic,
                  padding: const EdgeInsets.symmetric(
                    horizontal: 12,
                    vertical: 5,
                  ),
                  decoration: BoxDecoration(
                    color: key == value ? c.segmentThumb : Colors.transparent,
                    borderRadius: BorderRadius.circular(999),
                    boxShadow: key == value
                        ? [
                            BoxShadow(
                              color: Colors.black.withValues(alpha: 0.12),
                              blurRadius: 4,
                              offset: const Offset(0, 1),
                            ),
                          ]
                        : null,
                  ),
                  child: Text(
                    label,
                    style: TextStyle(
                      fontSize: 12,
                      fontWeight: key == value
                          ? FontWeight.w600
                          : FontWeight.w500,
                      color: key == value ? c.ink : c.muted,
                    ),
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class GlassToggle extends StatelessWidget {
  const GlassToggle({super.key, required this.value, required this.onChanged});
  final bool value;
  final ValueChanged<bool>? onChanged;
  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    return Semantics(toggled: value, enabled: onChanged != null, child: InkResponse(
      onTap: onChanged == null ? null : () => onChanged!(!value),
      radius: 21,
      child: Padding(padding: const EdgeInsets.symmetric(vertical: 6), child: AnimatedOpacity(
        opacity:onChanged == null ? .45 : 1, duration: const Duration(milliseconds:150),
        child: AnimatedContainer(
          width: 32,height:20,padding:const EdgeInsets.all(1.5),duration:const Duration(milliseconds:160),
          decoration: BoxDecoration(color:value ? c.success : c.fill, borderRadius:BorderRadius.circular(12),border:Border.all(color:value ? c.success : c.muted.withValues(alpha:.5),width:1)),
          child: AnimatedAlign(alignment:value ? Alignment.centerRight : Alignment.centerLeft,duration:const Duration(milliseconds:160),curve:Curves.easeOut,
            child: Container(width:14,height:14,decoration:BoxDecoration(shape:BoxShape.circle,color:Colors.white,border:Border.all(width:.5,color:c.muted.withValues(alpha:.3)),boxShadow:[BoxShadow(color:Colors.black.withValues(alpha:.14),blurRadius:2,offset:const Offset(0,1))]))),
        ),
      )),
    ));
  }
}

class GlassTintSliderWidget extends StatelessWidget {
  final double value;
  final ValueChanged<double> onChanged;
  final ValueChanged<double>? onChangeEnd;

  const GlassTintSliderWidget({
    super.key,
    required this.value,
    required this.onChanged,
    this.onChangeEnd,
  });

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    double snap(double v) {
      final clamped = v.clamp(0.0, 1.0);
      if ((clamped - 0.5).abs() <= 0.012) return 0.5;
      return (clamped * 200).round() / 200;
    }
    return Row(children: [
      PanelIcon('glass-clear', size: 22, color: c.muted),
      const SizedBox(width: 12),
      Expanded(child: SizedBox(height: 36, child: LayoutBuilder(builder: (context, limits) =>
        Stack(alignment: Alignment.center, children: [
          for (final tick in [0.0, 0.5, 1.0])
            if ((value - tick).abs() >= 0.055)
              Positioned(left: 18.25 + (limits.maxWidth - 40) * tick, top: 27, child: Container(width: 3.5, height: 3.5, decoration: BoxDecoration(color: c.muted.withValues(alpha: .42), borderRadius: BorderRadius.circular(2)))),
          SliderTheme(data: SliderTheme.of(context).copyWith(
            trackHeight: 4, activeTrackColor: c.primary, inactiveTrackColor: c.fillStrong, thumbColor: Colors.white,
            overlayShape: SliderComponentShape.noOverlay, thumbShape: const _PillThumbShape(),
          ), child: Slider(value: value.clamp(0.0, 1.0), min: 0, max: 1,
            semanticFormatterCallback: (v) => '${(v*100).round()}%',
            onChanged: (v) {
              final next = snap(v);
              if (next == 0.5 && value != 0.5) HapticFeedback.selectionClick();
              onChanged(next);
            },
            onChangeEnd: (v) => onChangeEnd?.call(snap(v)),
          )),
        ])
      ))),
      const SizedBox(width: 12),
      PanelIcon('glass-tinted', size: 22, color: c.muted),
    ]);
  }
}

class GlassFontScaleSliderWidget extends StatelessWidget {
  final double value;
  final ValueChanged<double>? onChangeStart;
  final ValueChanged<double> onChanged;
  final ValueChanged<double>? onChangeEnd;

  const GlassFontScaleSliderWidget({
    super.key,
    required this.value,
    this.onChangeStart,
    required this.onChanged,
    this.onChangeEnd,
  });

  static const double minScale = 0.9;
  static const double maxScale = 1.4;

  static double _snap(double v) {
    final pos = ((v - minScale) / (maxScale - minScale)).clamp(0.0, 1.0);
    if ((pos - 0.2).abs() <= 0.012) return 1.0;
    final smooth = (pos * 200).round() / 200;
    return minScale + (maxScale - minScale) * smooth;
  }

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final pos = ((value - minScale) / (maxScale - minScale)).clamp(0.0, 1.0);
    return Row(children: [
      SizedBox(
        width: 22,
        child: Center(
          child: Text(
            'A',
            textScaler: TextScaler.noScaling,
            style: TextStyle(fontSize: 12, fontWeight: FontWeight.w500, color: c.muted),
          ),
        ),
      ),
      const SizedBox(width: 12),
      Expanded(child: SizedBox(height: 36, child: LayoutBuilder(builder: (context, limits) =>
        Stack(alignment: Alignment.center, children: [
          for (final tick in [0.0, 0.2, 0.6, 1.0])
            if ((pos - tick).abs() >= 0.055)
              Positioned(left: 18.25 + (limits.maxWidth - 40) * tick, top: 27, child: Container(width: 3.5, height: 3.5, decoration: BoxDecoration(color: c.muted.withValues(alpha: .42), borderRadius: BorderRadius.circular(2)))),
          SliderTheme(data: SliderTheme.of(context).copyWith(
            trackHeight: 4, activeTrackColor: c.primary, inactiveTrackColor: c.fillStrong, thumbColor: Colors.white,
            overlayShape: SliderComponentShape.noOverlay, thumbShape: const _PillThumbShape(),
          ), child: Slider(value: value.clamp(minScale, maxScale), min: minScale, max: maxScale,
            semanticFormatterCallback: (v) => '${(v * 100).round()}%',
            onChangeStart: (v) => onChangeStart?.call(_snap(v)),
            onChanged: (v) {
              final next = _snap(v);
              if (next == 1.0 && value != 1.0) HapticFeedback.selectionClick();
              onChanged(next);
            },
            onChangeEnd: (v) => onChangeEnd?.call(_snap(v)),
          )),
        ])
      ))),
      const SizedBox(width: 12),
      SizedBox(
        width: 22,
        child: Center(
          child: Text(
            'A',
            textScaler: TextScaler.noScaling,
            style: TextStyle(fontSize: 19, fontWeight: FontWeight.w500, color: c.muted),
          ),
        ),
      ),
    ]);
  }
}

class _PillThumbShape extends SliderComponentShape {
  const _PillThumbShape();

  @override
  Size getPreferredSize(bool isEnabled, bool isDiscrete) => const Size(40, 26);

  @override
  void paint(
    PaintingContext context,
    Offset center, {
    required Animation<double> activationAnimation,
    required Animation<double> enableAnimation,
    required bool isDiscrete,
    required TextPainter labelPainter,
    required RenderBox parentBox,
    required SliderThemeData sliderTheme,
    required TextDirection textDirection,
    required double value,
    required double textScaleFactor,
    required Size sizeWithOverflow,
  }) {
    final canvas = context.canvas;
    final active = activationAnimation.value;
    final rect = RRect.fromRectAndRadius(
      Rect.fromCenter(
        center: center,
        width: 40 * (1 + .15 * active),
        height: 26 * (1 + .15 * active),
      ),
      const Radius.circular(18),
    );
    canvas.drawShadow(
      Path()..addRRect(rect),
      Colors.black.withValues(alpha: 0.22 - 0.08 * active),
      4 + 2 * active,
      false,
    );
    canvas.drawRRect(
      rect,
      Paint()..color = Colors.white.withValues(alpha: 1.0 - 0.78 * active),
    );
    if (active > 0.02) {
      canvas.drawRRect(
        rect.deflate(0.6),
        Paint()
          ..color = Colors.white.withValues(alpha: 0.78 * active)
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1.2,
      );
    }
    canvas.drawRRect(
      rect,
      Paint()
        ..color = Colors.black.withValues(alpha: 0.08 + 0.06 * active)
        ..style = PaintingStyle.stroke
        ..strokeWidth = 0.6,
    );
  }
}

class _BackdropScrimPainter extends CustomPainter {
  final bool light;
  const _BackdropScrimPainter(this.light);

  @override
  void paint(Canvas canvas, Size size) {
    if (size.isEmpty) return;
    final rx = size.width * 0.5 + 24.0;
    final ry = size.height * 0.5 + 10.0;
    final base = light ? Colors.white : const Color(0xFF0C0E16);
    final peak = light ? 0.52 : 0.46;
    canvas.save();
    canvas.translate(size.width * 0.5, size.height * 0.5);
    canvas.scale(rx, ry);
    canvas.drawCircle(
      Offset.zero,
      1.0,
      Paint()
        ..shader = ui.Gradient.radial(
          Offset.zero,
          1.0,
          [
            base.withValues(alpha: peak),
            base.withValues(alpha: peak * 0.42),
            base.withValues(alpha: 0.0),
          ],
          const [0.0, 0.55, 1.0],
        ),
    );
    canvas.restore();
  }

  @override
  bool shouldRepaint(_BackdropScrimPainter old) => old.light != light;
}

class GlassSubpageHeader extends StatelessWidget {
  final String title;
  final VoidCallback? onBack;
  final Widget? trailing;

  const GlassSubpageHeader({
    super.key,
    required this.title,
    this.onBack,
    this.trailing,
  });

  static double totalHeight(BuildContext context) =>
      MediaQuery.paddingOf(context).top + 48;

  static BoxDecoration pillDecoration(
    SingboardColors c,
    GlassScope scope,
  ) {
    final t = scope.tint.clamp(0.0, 1.0);
    return BoxDecoration(
      color: Color.lerp(
        c.fill,
        Colors.white.withValues(alpha: c.dark ? .12 : .28),
        1.0 - t,
      ),
      borderRadius: BorderRadius.circular(999),
      border: Border.all(
        color: Color.lerp(
          c.hairline,
          Colors.white.withValues(alpha: c.dark ? .19 : .55),
          1.0 - t,
        )!,
        width: 0.75,
      ),
      boxShadow: [
        BoxShadow(
          color: Colors.black.withValues(
            alpha: c.dark ? (.16 + .14 * (1.0 - t)) : (.04 + .06 * (1.0 - t)),
          ),
          blurRadius: 8,
          spreadRadius: -2,
          offset: const Offset(0, 2),
        ),
      ],
    );
  }

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final scope = GlassScope.of(context);
    final t = scope.tint.clamp(0.0, 1.0);
    final pad = MediaQuery.paddingOf(context);
    final gutter = panelGutter(context);
    final blur = scope.hasWallpaper ? _glassBlurSigma(t) : 0.0;
    final bar = Container(
      padding: EdgeInsets.only(
        top: pad.top,
        left: gutter + 4 + pad.left,
        right: gutter + 4 + pad.right,
      ),
      decoration: BoxDecoration(
        color: c.glassRgb.withValues(
          alpha: scope.hasWallpaper
              ? scope.alpha(0.35 + 0.45 * t, .6)
              : (0.72 + 0.16 * t),
        ),
        image: scope.grain,
        border: Border(
          bottom: BorderSide(color: c.hairline, width: 0.5),
        ),
      ),
      child: SizedBox(
        height: 48,
        child: Row(
          children: [
            Tooltip(
              message: '返回',
              child: Material(
                color: Colors.transparent,
                child: InkWell(
                  customBorder: const CircleBorder(),
                  onTap: onBack ?? () => Navigator.maybePop(context),
                  child: Container(
                    width: 28,
                    height: 28,
                    alignment: Alignment.center,
                    decoration: pillDecoration(c, scope),
                    child: PanelIcon('back', size: 14, color: c.ink),
                  ),
                ),
              ),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                title,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  fontSize: 15,
                  fontWeight: FontWeight.w600,
                  letterSpacing: -0.2,
                  color: c.ink,
                ),
              ),
            ),
            if (trailing != null) ...[
              const SizedBox(width: 8),
              trailing!,
            ],
          ],
        ),
      ),
    );
    if (blur > 0) {
      return ClipRect(
        child: _GlassFilter(
          blur: blur,
          lens: false,
          child: bar,
        ),
      );
    }
    return bar;
  }
}

class GlassBackdropHeading extends StatelessWidget {
  final String text;
  final TextStyle style;
  final int? maxLines;
  final TextOverflow? overflow;

  const GlassBackdropHeading(
    this.text, {
    super.key,
    required this.style,
    this.maxLines,
    this.overflow,
  });

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final scope = GlassScope.of(context);
    final ink = scope.backdropHeaderInk(c);
    final label = Text(
      text,
      maxLines: maxLines,
      overflow: overflow,
      style: style.copyWith(color: ink, shadows: null),
    );
    if (!scope.hasWallpaper) return label;
    final light = scope.headerToneLight ?? scope.backdropToneLight ?? !c.dark;
    return CustomPaint(painter: _BackdropScrimPainter(light), child: label);
  }
}

class GlassBackdropPill extends StatelessWidget {
  final String text;
  final TextStyle style;
  final bool muted;
  final Color? colorOverride;
  final EdgeInsetsGeometry padding;
  final int? maxLines;
  final TextOverflow? overflow;
  final TextAlign? textAlign;

  const GlassBackdropPill(
    this.text, {
    super.key,
    required this.style,
    this.muted = false,
    this.colorOverride,
    this.padding = const EdgeInsets.symmetric(horizontal: 10, vertical: 3),
    this.maxLines,
    this.overflow,
    this.textAlign,
  });

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final scope = GlassScope.of(context);
    if (!scope.hasWallpaper) {
      return Text(
        text,
        maxLines: maxLines,
        overflow: overflow,
        textAlign: textAlign,
        style: style.copyWith(
          color: colorOverride ?? (muted ? c.muted : c.ink),
          shadows: null,
        ),
      );
    }
    final t = scope.tint.clamp(0.0, 1.0);
    var alpha = math.max(0.48 + 0.24 * t, c.dark ? 0.56 : 0.60);
    if (scope.contrastingBackdrop) {
      alpha = math.max(alpha, 0.72);
    } else if (scope.busyBackdrop) {
      alpha = math.max(alpha, 0.66);
    }
    final fill = (c.dark ? const Color(0xFF24252C) : Colors.white).withValues(
      alpha: alpha,
    );
    final rim = Colors.white.withValues(alpha: c.dark ? 0.16 : 0.55);
    final ink =
        colorOverride ??
        (muted
            ? (c.dark ? const Color(0xEAF2F2F7) : const Color(0xE61C1C1E))
            : c.ink);
    final shape = BorderRadius.circular(999);
    final box = Container(
      padding: padding,
      decoration: BoxDecoration(
        color: fill,
        borderRadius: shape,
        border: Border.all(color: rim, width: 0.75),
      ),
      child: Text(
        text,
        maxLines: maxLines,
        overflow: overflow,
        textAlign: textAlign,
        style: style.copyWith(color: ink, shadows: null),
      ),
    );
    final wallpaper = scope.blurredWallpaper;
    return DecoratedBox(
      decoration: BoxDecoration(
        borderRadius: shape,
        boxShadow: [
          BoxShadow(
            color: Colors.black.withValues(alpha: c.dark ? 0.28 : 0.08),
            blurRadius: 8,
            spreadRadius: -2,
            offset: const Offset(0, 2),
          ),
        ],
      ),
      child: wallpaper != null
          ? _WallpaperGlass(
              image: wallpaper,
              radius: 999,
              tint: t,
              refract: false,
              child: box,
            )
          : box,
    );
  }
}

class SettingsSectionHeader extends StatelessWidget {
  final String title;
  final bool first;
  const SettingsSectionHeader(this.title, {super.key, this.first = false});

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: EdgeInsets.fromLTRB(4, first ? 0 : 24, 4, 8),
      child: Align(
        alignment: Alignment.centerLeft,
        child: GlassBackdropPill(
          title,
          muted: true,
          style: const TextStyle(fontSize: 12, fontWeight: FontWeight.w600),
        ),
      ),
    );
  }
}

class SettingsRow extends StatelessWidget {
  final String title;
  final String? subtitle;
  final Widget trailing;
  final VoidCallback? onTap;
  final bool inline;

  const SettingsRow({
    super.key,
    required this.title,
    this.subtitle,
    required this.trailing,
    this.onTap,
    this.inline = false,
  });

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final copy = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(
          title,
          style: TextStyle(
            fontSize: 13,
            fontWeight: FontWeight.w500,
            color: c.ink,
          ),
        ),
        if (subtitle != null && subtitle!.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 2),
            child: Text(
              subtitle!,
              style: TextStyle(fontSize: 12, height: 1.45, color: c.muted),
            ),
          ),
      ],
    );
    final row = LayoutBuilder(
      builder: (context, bounds) {
        final stack = !inline && bounds.maxWidth <= 560;
        return Container(
          constraints: const BoxConstraints(minHeight: 56),
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
          child: stack
              ? Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [copy, const SizedBox(height: 10), trailing],
                )
              : Row(
                  children: [
                    Expanded(child: copy),
                    const SizedBox(width: 16),
                    if (inline)
                      trailing
                    else
                      ConstrainedBox(
                        constraints: const BoxConstraints(maxWidth: 300),
                        child: trailing,
                      ),
                  ],
                ),
        );
      },
    );
    return onTap == null ? row : InkWell(onTap: onTap, child: row);
  }
}

class PlaybackIcon extends StatelessWidget {
  final bool paused;
  final bool topology;
  final Color color;

  const PlaybackIcon({
    super.key,
    required this.paused,
    required this.color,
    this.topology = false,
  });

  @override
  Widget build(BuildContext context) {
    final size = topology ? 14.0 : 16.0;
    return Semantics(
      button: true,
      label: paused ? '继续刷新' : '暂停刷新',
      child: Center(
        child: CustomPaint(
          size: Size.square(size),
          painter: _PlaybackPainter(paused, topology, color),
        ),
      ),
    );
  }
}

class _PlaybackPainter extends CustomPainter {
  final bool paused;
  final bool topology;
  final Color color;

  const _PlaybackPainter(this.paused, this.topology, this.color);

  @override
  void paint(Canvas canvas, Size size) {
    final viewBox = topology ? 24.0 : 16.0;
    final paint = Paint()..color = color;
    canvas.save();
    canvas.scale(size.width / viewBox, size.height / viewBox);
    if (paused) {
      final path = Path();
      if (topology) {
        path
          ..moveTo(7, 5.65)
          ..relativeCubicTo(0, -.86, .92, -1.4, 1.67, -.99)
          ..relativeLineTo(10.4, 6.35)
          ..relativeArcToPoint(const Offset(0, 1.97), radius: const Radius.circular(1.13))
          ..lineTo(8.67, 19.34)
          ..arcToPoint(const Offset(7, 18.35), radius: const Radius.circular(1.13))
          ..close();
      } else {
        path
          ..moveTo(5, 3.4)
          ..relativeLineTo(0, 9.2)
          ..relativeArcToPoint(const Offset(1.06, .6), radius: const Radius.circular(.7), clockwise: false)
          ..relativeLineTo(7.4, -4.6)
          ..relativeArcToPoint(const Offset(0, -1.2), radius: const Radius.circular(.7), clockwise: false)
          ..relativeLineTo(-7.4, -4.6)
          ..arcToPoint(const Offset(5, 3.4), radius: const Radius.circular(.7), clockwise: false)
          ..close();
      }
      canvas.drawPath(path, paint);
    } else if (topology) {
      paint.strokeWidth = 2;
      paint.strokeCap = StrokeCap.round;
      for (final x in [9.0, 15.0]) {
        canvas.drawLine(Offset(x, 5.5), Offset(x, 18.5), paint);
      }
    } else {
      for (final x in [4.0, 9.4]) {
        canvas.drawRRect(
          RRect.fromRectAndRadius(Rect.fromLTWH(x, 3, 2.6, 10), const Radius.circular(.8)),
          paint,
        );
      }
    }
    canvas.restore();
  }

  @override
  bool shouldRepaint(_PlaybackPainter old) =>
      paused != old.paused || topology != old.topology || color != old.color;
}

class TrafficWavePainter extends CustomPainter {
  final List<int> downSamples;
  final List<int> upSamples;
  final bool live;
  final Color ink;

  const TrafficWavePainter({
    required this.downSamples,
    required this.upSamples,
    required this.live,
    required this.ink,
  });

  @override
  void paint(Canvas canvas, Size size) {
    final axisY = size.height * 0.62;
    canvas.saveLayer(Offset.zero & size, Paint());
    final drawn = live && downSamples.length >= 2 && upSamples.length >= 2;
    Offset? head;
    if (drawn) {
      final down = _sparkLine(
        downSamples,
        size,
        axisY,
        size.height * 0.58,
        32768,
      );
      final up = _sparkLine(upSamples, size, axisY, -size.height * 0.35, 32768);
      for (final (curve, top, bottom) in [
        (down, 0.16, 0.03),
        (up, 0.02, 0.08),
      ]) {
        final bounds = curve.$2.getBounds();
        if (bounds.height > 0) {
          canvas.drawPath(
            curve.$2,
            Paint()
              ..shader = ui.Gradient.linear(
                bounds.topCenter,
                bounds.bottomCenter,
                [ink.withValues(alpha: top), ink.withValues(alpha: bottom)],
              ),
          );
        }
      }
      canvas.drawLine(
        Offset(0, axisY),
        Offset(size.width, axisY),
        Paint()..color = ink.withValues(alpha: 0.12),
      );
      for (final (curve, alpha, width) in [
        (down, 0.55, 1.5),
        (up, 0.30, 1.25),
      ]) {
        canvas.drawPath(
          curve.$1,
          Paint()
            ..color = ink.withValues(alpha: alpha)
            ..style = PaintingStyle.stroke
            ..strokeWidth = width
            ..strokeJoin = StrokeJoin.round,
        );
      }
      head = down.$3;
    } else {
      final brush = Paint()
        ..color = ink.withValues(alpha: 0.18)
        ..strokeWidth = 1.25;
      for (double x = 0; x < size.width; x += 8) {
        canvas.drawLine(
          Offset(x, axisY),
          Offset(math.min(x + 3, size.width), axisY),
          brush,
        );
      }
    }
    canvas.drawRect(
      Offset.zero & size,
      Paint()
        ..blendMode = BlendMode.dstIn
        ..shader = ui.Gradient.linear(
          Offset.zero,
          Offset(size.width * 0.12, 0),
          [Colors.transparent, Colors.white],
        ),
    );
    canvas.restore();
    if (head != null)
      canvas.drawCircle(head, 3, Paint()..color = ink.withValues(alpha: 0.75));
  }

  @override
  bool shouldRepaint(TrafficWavePainter old) =>
      old.downSamples != downSamples ||
      old.upSamples != upSamples ||
      old.live != live ||
      old.ink != ink;
}

class MiniSparkPainter extends CustomPainter {
  final List<int> samples;
  final int floor;
  final Color color;

  const MiniSparkPainter({
    required this.samples,
    required this.floor,
    required this.color,
  });

  @override
  void paint(Canvas canvas, Size size) {
    if (samples.length < 2) return;
    final curve = _sparkLine(
      samples,
      size,
      size.height,
      size.height * 26 / 30,
      floor,
    );
    canvas.drawPath(curve.$2, Paint()..color = color.withValues(alpha: 0.05));
    canvas.drawPath(
      curve.$1,
      Paint()
        ..color = color.withValues(alpha: 0.38)
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1.25
        ..strokeJoin = StrokeJoin.round,
    );
  }

  @override
  bool shouldRepaint(MiniSparkPainter old) =>
      old.samples != samples || old.color != color || old.floor != floor;
}

(Path, Path, Offset) _sparkLine(
  List<int> samples,
  Size size,
  double base,
  double extent,
  int floor,
) {
  final peak = math.max(floor, samples.reduce(math.max));
  Offset at(int i) => Offset(
    (60 - samples.length + i) * size.width / 59,
    base - extent * samples[i] / peak,
  );
  final first = at(0);
  final line = Path()..moveTo(first.dx, first.dy);
  var previous = first;
  for (var i = 1; i < samples.length; i++) {
    final next = at(i);
    final mid = (previous.dx + next.dx) / 2;
    line.cubicTo(mid, previous.dy, mid, next.dy, next.dx, next.dy);
    previous = next;
  }
  final area = Path.from(line)
    ..lineTo(previous.dx, base)
    ..lineTo(first.dx, base)
    ..close();
  return (line, area, previous);
}

(String, String) _quantityParts(num value, List<String> units, int precision) {
  var number = math.max(0, value).toDouble();
  var unit = 0;
  while (number >= 1024 && unit + 1 < units.length) {
    number /= 1024;
    unit++;
  }
  final text = number
      .toStringAsFixed(precision)
      .replaceFirst(RegExp(r'\.?0+$'), '');
  return (text.isEmpty ? '0' : text, units[unit]);
}

String formatBytes(num value) {
  final (number, unit) = formatBytesParts(value);
  return '$number $unit';
}

(String, String) formatSpeedParts(int bytes) =>
    _quantityParts(bytes, const ['B/s', 'KB/s', 'MB/s', 'GB/s'], 1);

(String, String) formatBytesParts(num value) =>
    _quantityParts(value, const ['B', 'KB', 'MB', 'GB', 'TB'], 2);

String maskIp(String ip) => ip.replaceAll(RegExp(r'[0-9a-fA-F]'), '•');
