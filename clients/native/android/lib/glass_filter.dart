part of 'glass_ui.dart';

class GlassEnvironment extends StatefulWidget {
  final double tint;
  final String? wallpaperPath;
  final int wallpaperRevision;
  final Widget child;
  const GlassEnvironment({
    super.key,
    required this.tint,
    required this.wallpaperPath,
    required this.wallpaperRevision,
    required this.child,
  });

  @override
  State<GlassEnvironment> createState() => _GlassEnvironmentState();
}

class _GlassEnvironmentState extends State<GlassEnvironment> {
  bool _lightConflict = false, _darkConflict = false;
  bool? _lightToneLight, _darkToneLight;
  int _revision = 0;

  @override
  void initState() {
    super.initState();
    _sample();
  }

  @override
  void didUpdateWidget(GlassEnvironment old) {
    super.didUpdateWidget(old);
    if (old.wallpaperPath != widget.wallpaperPath ||
        old.wallpaperRevision != widget.wallpaperRevision)
      _sample();
  }

  Future<void> _sample() async {
    final revision = ++_revision, path = widget.wallpaperPath;
    _lightConflict = _darkConflict = false;
    _lightToneLight = _darkToneLight = null;
    if (path == null || path.isEmpty) return;
    ui.Codec? codec;
    ui.Image? image;
    try {
      codec = await ui.instantiateImageCodec(
        await File(path).readAsBytes(),
        targetWidth: 32,
        targetHeight: 32,
      );
      image = (await codec.getNextFrame()).image;
      final bytes = await image.toByteData(format: ui.ImageByteFormat.rawRgba);
      if (!mounted || revision != _revision || bytes == null) return;
      double luminance(double veil, double alpha) {
        double linear(int channel) {
          final v = (channel * (1 - alpha) + veil * alpha) / 255;
          return v <= .04045
              ? v / 12.92
              : math.pow((v + .055) / 1.055, 2.4).toDouble();
        }

        var luma = 0.0;
        for (var i = 0; i < bytes.lengthInBytes; i += 4) {
          luma +=
              .2126 * linear(bytes.getUint8(i)) +
              .7152 * linear(bytes.getUint8(i + 1)) +
              .0722 * linear(bytes.getUint8(i + 2));
        }
        return luma / (bytes.lengthInBytes / 4);
      }

      final lightLuma = luminance(255, .28);
      final darkLuma = luminance(0, .38);
      setState(() {
        _lightConflict = lightLuma <= .179;
        _darkConflict = darkLuma > .179;
        _lightToneLight = lightLuma > .179;
        _darkToneLight = darkLuma > .179;
      });
    } on Exception {
      return;
    } finally {
      image?.dispose();
      codec?.dispose();
    }
  }

  @override
  Widget build(BuildContext context) {
    final dark = SingboardColors.of(context).dark;
    return GlassScope(
      tint: widget.tint,
      wallpaperPath: widget.wallpaperPath,
      wallpaperRevision: widget.wallpaperRevision,
      contrastingBackdrop: dark ? _darkConflict : _lightConflict,
      backdropToneLight: dark ? _darkToneLight : _lightToneLight,
      child: widget.child,
    );
  }
}

class _GlassFilter extends StatefulWidget {
  final Widget child;
  final double blur, radius, rim, amount, spread, depth;
  final bool lens;
  const _GlassFilter({
    required this.child,
    required this.blur,
    this.radius = 32,
    this.rim = 24,
    this.amount = 24,
    this.spread = .06,
    this.depth = 0,
    this.lens = true,
  });

  @override
  State<_GlassFilter> createState() => _GlassFilterState();
}

class _GlassFilterState extends State<_GlassFilter> {
  static Future<ui.FragmentProgram>? _program;
  ui.FragmentShader? _shader;
  bool _attempted = false;

  void _load() async {
    _attempted = true;
    try {
      final program = await (_program ??= ui.FragmentProgram.fromAsset(
        'assets/glass_lens.frag',
      ));
      if (mounted) setState(() => _shader = program.fragmentShader());
    } on Exception {
      return;
    }
  }

  @override
  void dispose() {
    _shader?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final tint = GlassScope.of(context).tint;
    final needsLens =
        widget.lens &&
        tint < .5 &&
        widget.amount > 0 &&
        ui.ImageFilter.isShaderFilterSupported;
    if (widget.blur <= 0 && !needsLens) return widget.child;
    ui.FragmentShader? shader;
    if (needsLens) {
      shader = _shader;
      if (shader == null) {
        if (!_attempted) _load();
        if (widget.blur <= 0) return widget.child;
      }
    }
    final saturation = 1.5 + .3 * tint;
    final r = .2126 * (1 - saturation),
        g = .7152 * (1 - saturation),
        b = .0722 * (1 - saturation);
    ui.ImageFilter filter = ColorFilter.matrix([
      r + saturation,
      g,
      b,
      0,
      0,
      r,
      g + saturation,
      b,
      0,
      0,
      r,
      g,
      b + saturation,
      0,
      0,
      0,
      0,
      0,
      1,
      0,
    ]);
    if (widget.blur > 0)
      filter = ui.ImageFilter.compose(
        outer: filter,
        inner: ui.ImageFilter.blur(sigmaX: widget.blur, sigmaY: widget.blur),
      );
    if (shader != null) {
      shader
        ..setFloat(2, MediaQuery.devicePixelRatioOf(context))
        ..setFloat(3, widget.radius)
        ..setFloat(4, widget.rim)
        ..setFloat(5, widget.amount)
        ..setFloat(6, widget.spread)
        ..setFloat(7, widget.depth);
      filter = ui.ImageFilter.compose(
        outer: ui.ImageFilter.shader(shader),
        inner: filter,
      );
    }
    return BackdropFilter(filter: filter, child: widget.child);
  }
}
