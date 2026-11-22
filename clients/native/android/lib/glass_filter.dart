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
  Color? _lightNavRgb, _darkNavRgb;
  int _revision = 0;
  ui.Image? _sourceImage;
  ui.Image? _blurredImage;
  bool? _builtDark;
  double? _builtTint;
  Size? _builtScreen;
  bool _blurBusy = false;
  bool _blurDirty = false;
  bool _currentDark = false;
  Size _currentScreen = Size.zero;

  @override
  void initState() {
    super.initState();
    _sample();
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _currentDark = SingboardColors.of(context).dark;
    _currentScreen = MediaQuery.sizeOf(context);
    _maybeRebuildBlur();
  }

  @override
  void didUpdateWidget(GlassEnvironment old) {
    super.didUpdateWidget(old);
    if (old.wallpaperPath != widget.wallpaperPath ||
        old.wallpaperRevision != widget.wallpaperRevision) {
      _sample();
    } else if (old.tint != widget.tint) {
      _maybeRebuildBlur();
    }
  }

  @override
  void dispose() {
    _revision++;
    _disposeLater(_sourceImage);
    _disposeLater(_blurredImage);
    _sourceImage = null;
    _blurredImage = null;
    super.dispose();
  }

  void _disposeLater(ui.Image? image) {
    if (image == null) return;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      image.dispose();
    });
  }

  void _maybeRebuildBlur() {
    if (_sourceImage == null || _currentScreen.isEmpty) return;
    final t = widget.tint.clamp(0.0, 1.0);
    final sizeChanged =
        _builtScreen == null ||
        (_currentScreen.width - _builtScreen!.width).abs() > 8 ||
        (_currentScreen.height - _builtScreen!.height).abs() > 8;
    if (_currentDark != _builtDark ||
        _builtTint == null ||
        (t - _builtTint!).abs() > 0.002 ||
        sizeChanged) {
      _scheduleBlur();
    }
  }

  void _scheduleBlur() {
    if (_sourceImage == null || _currentScreen.isEmpty) return;
    if (_blurBusy) {
      _blurDirty = true;
      return;
    }
    _blurBusy = true;
    _blurDirty = false;
    _buildBlur();
  }

  Future<void> _buildBlur() async {
    try {
      while (mounted) {
        final source = _sourceImage;
        final screen = _currentScreen;
        final dark = _currentDark;
        final t = widget.tint.clamp(0.0, 1.0);
        final rev = _revision;
        if (source == null || screen.isEmpty) break;
        _blurDirty = false;
        final srcW = source.width;
        final srcH = source.height;
        final screenSigma = _glassBlurSigma(t);
        final maxSide = screenSigma <= 3.5
            ? 1080.0
            : screenSigma <= 10.0
            ? 640.0
            : 420.0;
        final longest = math.max(srcW, srcH);
        final down = longest > maxSide ? maxSide / longest : 1.0;
        final w = math.max(1, (srcW * down).round());
        final h = math.max(1, (srcH * down).round());
        final coverScale = math
            .max(screen.width / w, screen.height / h)
            .clamp(0.01, 100.0);
        final sigma = (screenSigma / coverScale).clamp(0.4, 28.0);
        final saturation = 1.5 + 0.3 * t;
        final r = 0.2126 * (1 - saturation);
        final g = 0.7152 * (1 - saturation);
        final b = 0.0722 * (1 - saturation);
        final filter = ui.ImageFilter.compose(
          outer: ColorFilter.matrix([
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
          ]),
          inner: ui.ImageFilter.blur(
            sigmaX: sigma,
            sigmaY: sigma,
            tileMode: ui.TileMode.clamp,
          ),
        );
        final colors = SingboardColors(dark);
        final rect = Offset.zero & Size(w.toDouble(), h.toDouble());
        final srcRect = Offset.zero & Size(srcW.toDouble(), srcH.toDouble());
        final recorder = ui.PictureRecorder();
        final canvas = ui.Canvas(recorder, rect);
        canvas.drawRect(rect, Paint()..color = colors.page);
        canvas.saveLayer(rect, Paint()..imageFilter = filter);
        canvas.drawRect(rect, Paint()..color = colors.page);
        canvas.drawImageRect(
          source,
          srcRect,
          rect,
          Paint()..filterQuality = FilterQuality.medium,
        );
        canvas.drawRect(rect, Paint()..color = colors.backdropVeil);
        canvas.restore();
        final picture = recorder.endRecording();
        ui.Image? next;
        try {
          next = await picture.toImage(w, h);
        } finally {
          picture.dispose();
        }
        if (!mounted || rev != _revision) {
          next.dispose();
          break;
        }
        final old = _blurredImage;
        setState(() {
          _blurredImage = next;
          _builtDark = dark;
          _builtTint = t;
          _builtScreen = screen;
        });
        _disposeLater(old);
        if (!_blurDirty) break;
      }
    } on Exception {
      return;
    } finally {
      _blurBusy = false;
    }
  }

  Future<void> _sample() async {
    final revision = ++_revision, path = widget.wallpaperPath;
    _lightConflict = _darkConflict = false;
    _lightToneLight = _darkToneLight = null;
    _lightNavRgb = _darkNavRgb = null;
    if (path == null || path.isEmpty) {
      final oldSource = _sourceImage;
      final oldBlur = _blurredImage;
      _sourceImage = null;
      _blurredImage = null;
      _builtDark = null;
      _builtTint = null;
      _builtScreen = null;
      _disposeLater(oldSource);
      _disposeLater(oldBlur);
      return;
    }
    ui.Codec? codec;
    ui.Image? image;
    ui.ImmutableBuffer? buffer;
    ui.ImageDescriptor? descriptor;
    ui.Codec? sourceCodec;
    ui.Image? decodedSource;
    try {
      final fileBytes = await File(path).readAsBytes();
      codec = await ui.instantiateImageCodec(
        fileBytes,
        targetWidth: 32,
        targetHeight: 32,
      );
      image = (await codec.getNextFrame()).image;
      final bytes = await image.toByteData(format: ui.ImageByteFormat.rawRgba);
      buffer = await ui.ImmutableBuffer.fromUint8List(fileBytes);
      descriptor = await ui.ImageDescriptor.encoded(buffer);
      final srcW = descriptor.width;
      final srcH = descriptor.height;
      final longest = math.max(srcW, srcH);
      final scale = longest > 1080 ? 1080.0 / longest : 1.0;
      sourceCodec = await descriptor.instantiateCodec(
        targetWidth: math.max(1, (srcW * scale).round()),
        targetHeight: math.max(1, (srcH * scale).round()),
      );
      decodedSource = (await sourceCodec.getNextFrame()).image;
      if (!mounted || revision != _revision || bytes == null) {
        decodedSource.dispose();
        return;
      }
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

      Color sampleBottom(double veil, double alpha) {
        final w = image!.width, h = image.height;
        final x0 = (w * 10 ~/ 100).clamp(0, w - 1);
        final x1 = (w * 90 ~/ 100).clamp(x0 + 1, w);
        final y0 = (h * 82 ~/ 100).clamp(0, h - 1);
        var sr = 0.0, sg = 0.0, sb = 0.0;
        var count = 0;
        for (var y = y0; y < h; y++) {
          for (var x = x0; x < x1; x++) {
            final idx = (y * w + x) * 4;
            if (idx + 2 >= bytes.lengthInBytes) continue;
            sr += bytes.getUint8(idx) * (1 - alpha) + veil * alpha;
            sg += bytes.getUint8(idx + 1) * (1 - alpha) + veil * alpha;
            sb += bytes.getUint8(idx + 2) * (1 - alpha) + veil * alpha;
            count++;
          }
        }
        if (count == 0) {
          final v = veil.round().clamp(0, 255);
          return Color.fromARGB(255, v, v, v);
        }
        return Color.fromARGB(
          255,
          (sr / count).round().clamp(0, 255),
          (sg / count).round().clamp(0, 255),
          (sb / count).round().clamp(0, 255),
        );
      }

      final lightLuma = luminance(255, .28);
      final darkLuma = luminance(0, .38);
      final lightNav = sampleBottom(255, .28);
      final darkNav = sampleBottom(0, .38);
      final oldSource = _sourceImage;
      _sourceImage = decodedSource;
      _builtDark = null;
      _builtTint = null;
      _builtScreen = null;
      _disposeLater(oldSource);
      setState(() {
        _lightConflict = lightLuma <= .179;
        _darkConflict = darkLuma > .179;
        _lightToneLight = lightLuma > .179;
        _darkToneLight = darkLuma > .179;
        _lightNavRgb = lightNav;
        _darkNavRgb = darkNav;
      });
      _scheduleBlur();
    } on Exception {
      decodedSource?.dispose();
      return;
    } finally {
      sourceCodec?.dispose();
      descriptor?.dispose();
      buffer?.dispose();
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
      navBackdropRgb: dark ? _darkNavRgb : _lightNavRgb,
      blurredWallpaper: _blurredImage,
      child: widget.child,
    );
  }
}

double _glassLensStrength(double tint) =>
    math.pow((1.0 - tint.clamp(0.0, 1.0)), 1.35).toDouble();

double _glassBlurSigma(double tint) {
  final t = tint.clamp(0.0, 1.0);
  return 1.2 + 7.8 * t + 16.0 * t * t;
}

class _WallpaperGlass extends StatelessWidget {
  final ui.Image image;
  final double radius;
  final double tint;
  final bool refract;
  final Widget child;

  const _WallpaperGlass({
    required this.image,
    required this.radius,
    required this.tint,
    this.refract = true,
    required this.child,
  });

  @override
  Widget build(BuildContext context) {
    final screenSize = MediaQuery.sizeOf(context);
    final vertical = Scrollable.maybeOf(context, axis: Axis.vertical)?.position;
    final horizontal = GlassScrollScope.maybeOf(context);
    return _BlurredWallpaperBox(
      image: image,
      radius: radius,
      tint: tint,
      refract: refract,
      screenSize: screenSize,
      verticalScroll: vertical,
      horizontalScroll: horizontal,
      child: RepaintBoundary(child: child),
    );
  }
}

class _BlurredWallpaperBox extends SingleChildRenderObjectWidget {
  final ui.Image image;
  final double radius;
  final double tint;
  final bool refract;
  final Size screenSize;
  final Listenable? verticalScroll;
  final Listenable? horizontalScroll;

  const _BlurredWallpaperBox({
    required this.image,
    required this.radius,
    required this.tint,
    required this.refract,
    required this.screenSize,
    required this.verticalScroll,
    required this.horizontalScroll,
    required super.child,
  });

  @override
  _RenderBlurredWallpaper createRenderObject(BuildContext context) =>
      _RenderBlurredWallpaper(
        image: image,
        radius: radius,
        tint: tint,
        refract: refract,
        screenSize: screenSize,
        verticalScroll: verticalScroll,
        horizontalScroll: horizontalScroll,
      );

  @override
  void updateRenderObject(
    BuildContext context,
    _RenderBlurredWallpaper renderObject,
  ) {
    renderObject
      ..image = image
      ..radius = radius
      ..tint = tint
      ..refract = refract
      ..screenSize = screenSize
      ..verticalScroll = verticalScroll
      ..horizontalScroll = horizontalScroll;
  }
}

class _RenderBlurredWallpaper extends RenderProxyBox {
  static Future<ui.FragmentProgram>? _programFuture;
  static ui.FragmentProgram? _cachedProgram;
  static bool _loadFailed = false;

  _RenderBlurredWallpaper({
    required ui.Image image,
    required double radius,
    required double tint,
    required bool refract,
    required Size screenSize,
    required Listenable? verticalScroll,
    required Listenable? horizontalScroll,
  }) : _image = image,
       _radius = radius,
       _tint = tint,
       _refract = refract,
       _screenSize = screenSize,
       _verticalScroll = verticalScroll,
       _horizontalScroll = horizontalScroll;

  ui.FragmentShader? _shader;
  bool _disposed = false;
  bool _loadingShader = false;

  ui.Image _image;
  set image(ui.Image value) {
    if (identical(_image, value)) return;
    _image = value;
    markNeedsPaint();
  }

  double _radius;
  set radius(double value) {
    if (_radius == value) return;
    _radius = value;
    markNeedsPaint();
  }

  double _tint;
  set tint(double value) {
    if ((_tint - value).abs() < 0.001) return;
    _tint = value;
    markNeedsPaint();
  }

  bool _refract;
  set refract(bool value) {
    if (_refract == value) return;
    _refract = value;
    markNeedsPaint();
  }

  Size _screenSize;
  set screenSize(Size value) {
    if (_screenSize == value) return;
    _screenSize = value;
    markNeedsPaint();
  }

  Listenable? _verticalScroll;
  set verticalScroll(Listenable? value) {
    if (_verticalScroll == value) return;
    if (attached) _verticalScroll?.removeListener(markNeedsPaint);
    _verticalScroll = value;
    if (attached) _verticalScroll?.addListener(markNeedsPaint);
  }

  Listenable? _horizontalScroll;
  set horizontalScroll(Listenable? value) {
    if (_horizontalScroll == value) return;
    if (attached) _horizontalScroll?.removeListener(markNeedsPaint);
    _horizontalScroll = value;
    if (attached) _horizontalScroll?.addListener(markNeedsPaint);
  }

  ui.FragmentShader? _obtainShader() {
    if (_disposed || _loadFailed) return null;
    if (_shader != null) return _shader;
    final program = _cachedProgram;
    if (program != null) {
      _shader = program.fragmentShader();
      return _shader;
    }
    if (!_loadingShader) {
      _loadingShader = true;
      (_programFuture ??= ui.FragmentProgram.fromAsset(
        'assets/glass_wallpaper.frag',
      )).then(
        (prog) {
          _cachedProgram = prog;
          _loadingShader = false;
          if (!_disposed && attached) {
            _shader ??= prog.fragmentShader();
            markNeedsPaint();
          }
        },
        onError: (Object _) {
          _loadFailed = true;
          _loadingShader = false;
        },
      );
    }
    return null;
  }

  @override
  void attach(PipelineOwner owner) {
    super.attach(owner);
    _verticalScroll?.addListener(markNeedsPaint);
    _horizontalScroll?.addListener(markNeedsPaint);
  }

  @override
  void detach() {
    _verticalScroll?.removeListener(markNeedsPaint);
    _horizontalScroll?.removeListener(markNeedsPaint);
    super.detach();
  }

  @override
  void dispose() {
    _disposed = true;
    _shader?.dispose();
    _shader = null;
    super.dispose();
  }

  @override
  void paint(PaintingContext context, Offset offset) {
    if (size.isEmpty) {
      super.paint(context, offset);
      return;
    }
    final imgW = _image.width.toDouble();
    final imgH = _image.height.toDouble();
    if (imgW > 0 && imgH > 0 && !_screenSize.isEmpty) {
      final global = localToGlobal(Offset.zero);
      final scale = math.max(
        _screenSize.width / imgW,
        _screenSize.height / imgH,
      );
      final drawW = imgW * scale;
      final drawH = imgH * scale;
      final coverLeft = (_screenSize.width - drawW) / 2;
      final coverTop = (_screenSize.height - drawH) / 2;
      final clampedRadius = math.min(
        _radius,
        math.min(size.width, size.height) * 0.5,
      );
      final lens = _refract ? _glassLensStrength(_tint) : 0.0;
      final useShader =
          lens > 0.02 &&
          size.width > 48 &&
          size.height > 48 &&
          clampedRadius >= 6;
      final shader = useShader ? _obtainShader() : null;
      final canvas = context.canvas;
      if (shader != null) {
        final rim = math.min(
          clampedRadius,
          math.min(18.0, math.min(size.width, size.height) * 0.24),
        );
        final amount = math.min(rim * 0.36 * lens, 7.0 * lens);
        final spread = 0.03 * lens;
        shader
          ..setFloat(0, size.width)
          ..setFloat(1, size.height)
          ..setFloat(2, coverLeft - global.dx)
          ..setFloat(3, coverTop - global.dy)
          ..setFloat(4, drawW)
          ..setFloat(5, drawH)
          ..setFloat(6, clampedRadius)
          ..setFloat(7, rim)
          ..setFloat(8, amount)
          ..setFloat(9, spread)
          ..setImageSampler(0, _image, filterQuality: FilterQuality.low);
        final localRRect = RRect.fromRectAndRadius(
          Offset.zero & size,
          Radius.circular(clampedRadius),
        );
        canvas.save();
        canvas.translate(offset.dx, offset.dy);
        canvas.drawRRect(localRRect, Paint()..shader = shader);
        canvas.restore();
      } else {
        final dst = Rect.fromLTWH(
          offset.dx + coverLeft - global.dx,
          offset.dy + coverTop - global.dy,
          drawW,
          drawH,
        );
        final rrect = RRect.fromRectAndRadius(
          offset & size,
          Radius.circular(clampedRadius),
        );
        canvas.save();
        canvas.clipRRect(rrect);
        canvas.drawImageRect(
          _image,
          Rect.fromLTWH(0, 0, imgW, imgH),
          dst,
          Paint()..filterQuality = FilterQuality.low,
        );
        canvas.restore();
      }
    }
    super.paint(context, offset);
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
    final lensStrength = _glassLensStrength(tint);
    final effectiveAmount = widget.blur > 0
        ? math.min(widget.amount * lensStrength, widget.rim * 0.24)
        : widget.amount;
    final needsLens =
        widget.lens &&
        (widget.blur == 0 || lensStrength > 0.02) &&
        effectiveAmount > 0.05 &&
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
        ..setFloat(5, effectiveAmount)
        ..setFloat(6, widget.spread * (widget.blur > 0 ? lensStrength : 1.0))
        ..setFloat(7, widget.depth);
      filter = ui.ImageFilter.compose(
        outer: ui.ImageFilter.shader(shader),
        inner: filter,
      );
    }
    return BackdropFilter(filter: filter, child: widget.child);
  }
}
