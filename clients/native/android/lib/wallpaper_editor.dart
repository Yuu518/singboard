import 'dart:async';
import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'glass_ui.dart';

const _channel = MethodChannel('singboard/config');

enum _AspectPreset { free, screen }

class WallpaperEditorDialog extends StatefulWidget {
  final Uint8List bytes;
  const WallpaperEditorDialog({super.key, required this.bytes});
  @override
  State<WallpaperEditorDialog> createState() => _WallpaperEditorDialogState();
}

class _WallpaperEditorDialogState extends State<WallpaperEditorDialog> {
  ui.Image? _decoded;
  int _quarterTurns = 0;
  bool _flipH = false;
  bool _flipV = false;
  _AspectPreset _aspect = _AspectPreset.free;
  Rect? _crop;
  Rect? _initialCrop;
  Offset _dragStart = Offset.zero;
  Alignment? _handle;
  bool _busy = false;
  String _status = '';

  @override
  void initState() {
    super.initState();
    unawaited(
      _decodeBytes(widget.bytes).catchError((Object e) {
        if (mounted) setState(() => _status = '读取图片失败：$e');
      }),
    );
  }

  @override
  void dispose() {
    _decoded?.dispose();
    super.dispose();
  }

  Future<void> _decodeBytes(Uint8List bytes) async {
    final codec = await ui.instantiateImageCodec(bytes);
    final frame = await codec.getNextFrame();
    codec.dispose();
    if (!mounted) {
      frame.image.dispose();
      return;
    }
    setState(() {
      _decoded?.dispose();
      _decoded = frame.image;
    });
  }

  double _targetRatio(Size screenSize, double srcRatio) => switch (_aspect) {
    _AspectPreset.free => srcRatio,
    _AspectPreset.screen => screenSize.width / math.max(1.0, screenSize.height),
  };

  Size get _imageSize {
    final image = _decoded!;
    return _quarterTurns.isOdd
        ? Size(image.height.toDouble(), image.width.toDouble())
        : Size(image.width.toDouble(), image.height.toDouble());
  }

  void _rotate(bool clockwise) {
    final crop = _crop;
    _quarterTurns = (_quarterTurns + (clockwise ? 1 : 3)) % 4;
    final previous = _flipH;
    _flipH = _flipV;
    _flipV = previous;
    _crop = crop == null || _aspect == _AspectPreset.screen
        ? null
        : Rect.fromLTWH(
            clockwise ? 1 - crop.bottom : crop.top,
            clockwise ? crop.left : 1 - crop.right,
            crop.height,
            crop.width,
          );
  }

  void _flip(bool horizontal) {
    final crop = _crop;
    if (horizontal) {
      _flipH = !_flipH;
    } else {
      _flipV = !_flipV;
    }
    if (crop != null)
      _crop = Rect.fromLTWH(
        horizontal ? 1 - crop.right : crop.left,
        horizontal ? crop.top : 1 - crop.bottom,
        crop.width,
        crop.height,
      );
  }

  Rect _selection(Size screenSize) {
    final rect = _crop ?? const Rect.fromLTWH(0, 0, 1, 1);
    if (_aspect == _AspectPreset.free) return rect;
    final aspect =
        _targetRatio(screenSize, _imageSize.aspectRatio) /
        _imageSize.aspectRatio;
    return Rect.fromCenter(
      center: rect.center,
      width: math.min(rect.width, rect.height * aspect),
      height: math.min(rect.height, rect.width / aspect),
    );
  }

  Widget _cropPreview(Size screen) => LayoutBuilder(
    builder: (context, limits) {
      final size = limits.biggest;
      final image = _imageSize;
      final frame = Alignment.center.inscribe(
        applyBoxFit(
          BoxFit.contain,
          image,
          Size(math.max(1, size.width - 28), math.max(1, size.height - 28)),
        ).destination,
        Offset.zero & size,
      );
      final selection = _selection(screen);
      final rect = Rect.fromLTWH(
        frame.left + selection.left * frame.width,
        frame.top + selection.top * frame.height,
        selection.width * frame.width,
        selection.height * frame.height,
      );
      final handles = [
        Alignment.topLeft,
        Alignment.topRight,
        Alignment.bottomLeft,
        Alignment.bottomRight,
        if (_aspect == _AspectPreset.free) ...[
          Alignment.topCenter,
          Alignment.centerLeft,
          Alignment.centerRight,
          Alignment.bottomCenter,
        ],
      ];
      return GestureDetector(
        behavior: HitTestBehavior.opaque,
        onPanDown: _busy
            ? null
            : (d) {
                _initialCrop = selection;
                _dragStart = d.localPosition;
                _handle = rect.contains(_dragStart) ? Alignment.center : null;
                var distance = 18.0;
                for (final handle in handles) {
                  final delta = (handle.withinRect(rect) - _dragStart).distance;
                  if (delta < distance) {
                    distance = delta;
                    _handle = handle;
                  }
                }
              },
        onPanUpdate: _busy
            ? null
            : (d) {
                final start = _initialCrop;
                final h = _handle;
                if (start == null || h == null) return;
                final delta = d.localPosition - _dragStart;
                final dx = delta.dx / frame.width, dy = delta.dy / frame.height;
                final minW = math.min(start.width, 64 / _imageSize.width),
                    minH = math.min(start.height, 64 / _imageSize.height);
                final Rect next;
                if (h == Alignment.center) {
                  next = Rect.fromLTWH(
                    (start.left + dx)
                        .clamp(0.0, math.max(0.0, 1 - start.width))
                        .toDouble(),
                    (start.top + dy)
                        .clamp(0.0, math.max(0.0, 1 - start.height))
                        .toDouble(),
                    start.width,
                    start.height,
                  );
                } else if (_aspect != _AspectPreset.free) {
                  final ratio = start.width / start.height;
                  final maxW = math.min(
                    h.x > 0 ? 1 - start.left : start.right,
                    (h.y > 0 ? 1 - start.top : start.bottom) * ratio,
                  );
                  final w = math
                      .max(
                        start.width + h.x * dx,
                        (start.height + h.y * dy) * ratio,
                      )
                      .clamp(math.min(maxW, math.max(minW, minH * ratio)), maxW)
                      .toDouble();
                  next = Rect.fromLTWH(
                    h.x > 0 ? start.left : start.right - w,
                    h.y > 0 ? start.top : start.bottom - w / ratio,
                    w,
                    w / ratio,
                  );
                } else {
                  next = Rect.fromLTRB(
                    h.x < 0
                        ? (start.left + dx)
                              .clamp(0.0, start.right - minW)
                              .toDouble()
                        : start.left,
                    h.y < 0
                        ? (start.top + dy)
                              .clamp(0.0, start.bottom - minH)
                              .toDouble()
                        : start.top,
                    h.x > 0
                        ? (start.right + dx)
                              .clamp(start.left + minW, 1.0)
                              .toDouble()
                        : start.right,
                    h.y > 0
                        ? (start.bottom + dy)
                              .clamp(start.top + minH, 1.0)
                              .toDouble()
                        : start.bottom,
                  );
                }
                setState(() => _crop = next);
              },
        child: CustomPaint(
          size: size,
          painter: _WallpaperPreviewPainter(
            image: _decoded!,
            quarterTurns: _quarterTurns,
            flipH: _flipH,
            flipV: _flipV,
            frame: frame,
            selection: rect,
            handles: handles,
          ),
        ),
      );
    },
  );

  Future<void> _applyWallpaper() async {
    final image = _decoded;
    if (image == null || _busy) return;
    final screenSize = MediaQuery.sizeOf(context);
    setState(() {
      _busy = true;
      _status = '';
    });
    try {
      final rotatedW = _quarterTurns.isOdd
          ? image.height.toDouble()
          : image.width.toDouble();
      final rotatedH = _quarterTurns.isOdd
          ? image.width.toDouble()
          : image.height.toDouble();
      final crop = _selection(screenSize);
      final cropW = crop.width * rotatedW;
      final cropH = crop.height * rotatedH;
      const maxDim = 4096.0;
      final outScale = math.min(1.0, maxDim / math.max(cropW, cropH));
      final outW = math.max(1, (cropW * outScale).round());
      final outH = math.max(1, (cropH * outScale).round());

      final recorder = ui.PictureRecorder();
      final canvas = Canvas(recorder);
      canvas.scale(outScale, outScale);
      canvas.translate(
        rotatedW * (0.5 - crop.left),
        rotatedH * (0.5 - crop.top),
      );
      canvas.scale(_flipH ? -1.0 : 1.0, _flipV ? -1.0 : 1.0);
      canvas.rotate(_quarterTurns * math.pi * 0.5);
      canvas.drawImage(
        image,
        Offset(-image.width * 0.5, -image.height * 0.5),
        Paint()..filterQuality = FilterQuality.high,
      );
      final picture = recorder.endRecording();
      final outImage = await picture.toImage(outW, outH);
      final data = await outImage.toByteData(format: ui.ImageByteFormat.png);
      outImage.dispose();
      picture.dispose();
      if (data == null) throw StateError('PNG 编码失败');
      final saved = await _channel.invokeMethod<String>('saveWallpaper', {
        'bytes': data.buffer.asUint8List(),
      });
      if (!mounted) return;
      Navigator.pop(context, saved);
    } catch (e) {
      if (mounted) setState(() => _status = '保存壁纸失败：$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  KeyEventResult _cropKey(FocusNode node, KeyEvent event) {
    if (_busy || _decoded == null || event is KeyUpEvent)
      return KeyEventResult.ignored;
    if (event.logicalKey == LogicalKeyboardKey.enter) {
      unawaited(_applyWallpaper());
      return KeyEventResult.handled;
    }
    final delta = {
      LogicalKeyboardKey.arrowLeft: const Offset(-.01, 0),
      LogicalKeyboardKey.arrowRight: const Offset(.01, 0),
      LogicalKeyboardKey.arrowUp: const Offset(0, -.01),
      LogicalKeyboardKey.arrowDown: const Offset(0, .01),
    }[event.logicalKey];
    if (delta == null) return KeyEventResult.ignored;
    final initial = _selection(MediaQuery.sizeOf(context));
    if (HardwareKeyboard.instance.isShiftPressed) {
      final minW = math.min(initial.width, 64 / _imageSize.width);
      final minH = math.min(initial.height, 64 / _imageSize.height);
      double w = (initial.width + delta.dx)
          .clamp(minW, 1 - initial.left)
          .toDouble();
      double h = (initial.height + delta.dy)
          .clamp(minH, 1 - initial.top)
          .toDouble();
      if (_aspect == _AspectPreset.screen) {
        final ratio = initial.width / initial.height;
        final maxW = math.min(1 - initial.left, (1 - initial.top) * ratio);
        w = math
            .max(w, h * ratio)
            .clamp(math.min(maxW, math.max(minW, minH * ratio)), maxW)
            .toDouble();
        h = w / ratio;
      }
      _crop = Rect.fromLTWH(initial.left, initial.top, w, h);
    } else {
      _crop = Rect.fromLTWH(
        (initial.left + delta.dx).clamp(0.0, 1 - initial.width).toDouble(),
        (initial.top + delta.dy).clamp(0.0, 1 - initial.height).toDouble(),
        initial.width,
        initial.height,
      );
    }
    setState(() {});
    return KeyEventResult.handled;
  }

  @override
  Widget build(BuildContext context) {
    final c = SingboardColors.of(context);
    final screen = MediaQuery.sizeOf(context);
    return PopScope(
      canPop: !_busy,
      child: Dialog(
        backgroundColor: Colors.transparent,
        insetPadding: const EdgeInsets.all(16),
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 768),
          child: SingleChildScrollView(
            child: GlassCard(
              popover: true,
              padding: const EdgeInsets.all(20),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text(
                    '编辑背景',
                    style: TextStyle(
                      color: c.ink,
                      fontWeight: FontWeight.w600,
                      fontSize: 18,
                    ),
                  ),
                  const SizedBox(height: 12),
                  ClipRRect(
                    borderRadius: BorderRadius.circular(14),
                    child: Container(
                      height: math.min(screen.height * .58, 460),
                      color: c.fill,
                      child: _decoded == null
                          ? Center(
                              child: _status.isEmpty
                                  ? const CircularProgressIndicator()
                                  : Text(_status),
                            )
                          : Focus(
                              autofocus: true,
                              onKeyEvent: _cropKey,
                              child: Semantics(
                                label: '裁剪区域',
                                child: _cropPreview(screen),
                              ),
                            ),
                    ),
                  ),
                  const SizedBox(height: 12),
                  Wrap(
                    alignment: WrapAlignment.start,
                    crossAxisAlignment: WrapCrossAlignment.center,
                    spacing: 4,
                    runSpacing: 4,
                    children: [
                      for (final (icon, label, action) in [
                        ('rotate-left', '左转 90°', () => _rotate(false)),
                        ('rotate-right', '右转 90°', () => _rotate(true)),
                        ('flip-x', '水平镜像', () => _flip(true)),
                        ('flip-y', '垂直镜像', () => _flip(false)),
                      ])
                        IconButton(
                          tooltip: label,
                          onPressed: _busy ? null : () => setState(action),
                          icon: PanelIcon(icon, size: 18, color: c.ink),
                        ),
                      GlassSegmented<_AspectPreset>(
                        items: const [
                          (_AspectPreset.free, '自由'),
                          (_AspectPreset.screen, '适配窗口'),
                        ],
                        value: _aspect,
                        onChanged: (v) => setState(() {
                          if (v == _aspect) return;
                          final previous = _decoded == null
                              ? null
                              : _selection(screen);
                          _aspect = v;
                          _crop = v == _AspectPreset.free ? previous : null;
                        }),
                      ),
                      TextButton(
                        onPressed: _busy
                            ? null
                            : () => setState(() {
                                _quarterTurns = 0;
                                _flipH = false;
                                _flipV = false;
                                _crop = null;
                              }),
                        child: const Text('重置'),
                      ),
                    ],
                  ),
                  if (_status.isNotEmpty && _decoded != null)
                    Text(
                      _status,
                      style: TextStyle(color: c.muted, fontSize: 12),
                    ),
                  const SizedBox(height: 12),
                  Wrap(
                    alignment: WrapAlignment.end,
                    spacing: 8,
                    children: [
                      TextButton(
                        onPressed: _busy ? null : () => Navigator.pop(context),
                        child: const Text('取消'),
                      ),
                      FilledButton(
                        onPressed: _busy || _decoded == null
                            ? null
                            : _applyWallpaper,
                        child: Text(_busy ? '处理中…' : '使用这张背景'),
                      ),
                    ],
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _WallpaperPreviewPainter extends CustomPainter {
  final ui.Image image;
  final int quarterTurns;
  final bool flipH;
  final bool flipV;
  final Rect frame;
  final Rect selection;
  final List<Alignment> handles;

  const _WallpaperPreviewPainter({
    required this.image,
    required this.quarterTurns,
    required this.flipH,
    required this.flipV,
    required this.frame,
    required this.selection,
    required this.handles,
  });

  @override
  void paint(Canvas canvas, Size size) {
    final scale =
        frame.width / (quarterTurns.isOdd ? image.height : image.width);
    canvas.save();
    canvas.translate(frame.center.dx, frame.center.dy);
    canvas.scale(scale * (flipH ? -1 : 1), scale * (flipV ? -1 : 1));
    canvas.rotate(quarterTurns * math.pi / 2);
    canvas.drawImage(
      image,
      Offset(-image.width / 2, -image.height / 2),
      Paint()..filterQuality = FilterQuality.medium,
    );
    canvas.restore();
    canvas.drawPath(
      Path()
        ..fillType = PathFillType.evenOdd
        ..addRect(frame)
        ..addRect(selection),
      Paint()..color = Colors.black.withValues(alpha: .46),
    );
    final line = Paint()
      ..style = PaintingStyle.stroke
      ..strokeWidth = 1
      ..color = Colors.white.withValues(alpha: .7);
    canvas.drawRect(selection, line);
    for (final fraction in [1 / 3, 2 / 3]) {
      final x = selection.left + selection.width * fraction;
      final y = selection.top + selection.height * fraction;
      canvas.drawLine(
        Offset(x, selection.top),
        Offset(x, selection.bottom),
        line,
      );
      canvas.drawLine(
        Offset(selection.left, y),
        Offset(selection.right, y),
        line,
      );
    }
    for (final handle in handles) {
      canvas.drawCircle(
        handle.withinRect(selection),
        4,
        Paint()..color = Colors.white,
      );
    }
  }

  @override
  bool shouldRepaint(_WallpaperPreviewPainter old) =>
      old.image != image ||
      old.quarterTurns != quarterTurns ||
      old.flipH != flipH ||
      old.flipV != flipV ||
      old.frame != frame ||
      old.selection != selection ||
      old.handles.length != handles.length;
}
