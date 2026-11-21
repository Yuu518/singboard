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
  ui.Image? _sourceImage;
  ui.Image? _blurredImage;
  bool? _builtDark;
  double? _builtTint;
  Size? _builtScreen;
  bool _blurBusy = false;
  bool _blurDirty = false;
  Timer? _blurTimer;
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
    _blurTimer?.cancel();
    _blurTimer = null;
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
        (t - _builtTint!).abs() > 0.005 ||
        sizeChanged) {
      _scheduleBlur();
    }
  }

  void _scheduleBlur({bool immediate = false}) {
    if (_sourceImage == null || _currentScreen.isEmpty) return;
    if (!immediate && _blurredImage != null) {
      _blurTimer?.cancel();
      _blurTimer = Timer(const Duration(milliseconds: 48), () {
        if (mounted) _scheduleBlur(immediate: true);
      });
      return;
    }
    _blurTimer?.cancel();
    _blurTimer = null;
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
        final w = source.width;
        final h = source.height;
        final coverScale = math
            .max(screen.width / w, screen.height / h)
            .clamp(0.01, 100.0);
        final screenSigma = 3.0 + 14.0 * t;
        final sigma = (screenSigma / coverScale).clamp(0.5, 32.0);
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
        final recorder = ui.PictureRecorder();
        final canvas = ui.Canvas(recorder, rect);
        canvas.drawRect(rect, Paint()..color = colors.page);
        canvas.saveLayer(rect, Paint()..imageFilter = filter);
        canvas.drawRect(rect, Paint()..color = colors.page);
        canvas.drawImage(
          source,
          Offset.zero,
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
      final scale = longest > 720 ? 720.0 / longest : 1.0;
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

      final lightLuma = luminance(255, .28);
      final darkLuma = luminance(0, .38);
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
      blurredWallpaper: _blurredImage,
      child: widget.child,
    );
  }
}

class _WallpaperGlass extends StatelessWidget {
  final ui.Image image;
  final double radius;
  final Widget child;

  const _WallpaperGlass({
    required this.image,
    required this.radius,
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
  final Size screenSize;
  final Listenable? verticalScroll;
  final Listenable? horizontalScroll;

  const _BlurredWallpaperBox({
    required this.image,
    required this.radius,
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
      ..screenSize = screenSize
      ..verticalScroll = verticalScroll
      ..horizontalScroll = horizontalScroll;
  }
}

class _RenderBlurredWallpaper extends RenderProxyBox {
  _RenderBlurredWallpaper({
    required ui.Image image,
    required double radius,
    required Size screenSize,
    required Listenable? verticalScroll,
    required Listenable? horizontalScroll,
  }) : _image = image,
       _radius = radius,
       _screenSize = screenSize,
       _verticalScroll = verticalScroll,
       _horizontalScroll = horizontalScroll;

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
      final dst = Rect.fromLTWH(
        offset.dx + coverLeft - global.dx,
        offset.dy + coverTop - global.dy,
        drawW,
        drawH,
      );
      final clampedRadius = math.min(
        _radius,
        math.min(size.width, size.height) * 0.5,
      );
      final rrect = RRect.fromRectAndRadius(
        offset & size,
        Radius.circular(clampedRadius),
      );
      final canvas = context.canvas;
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
