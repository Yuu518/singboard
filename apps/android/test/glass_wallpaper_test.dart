import 'dart:typed_data';
import 'dart:ui' as ui;

import 'package:flutter/rendering.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:singboard_native/glass_ui.dart';

Future<ui.Image> _buildVerticalBandImage(int width, int height) async {
  final recorder = ui.PictureRecorder();
  final canvas = ui.Canvas(
    recorder,
    Rect.fromLTWH(0, 0, width.toDouble(), height.toDouble()),
  );
  canvas.drawRect(
    Rect.fromLTWH(0, 0, width.toDouble(), height * 0.5),
    Paint()..color = const Color(0xFFFF0000),
  );
  canvas.drawRect(
    Rect.fromLTWH(0, height * 0.5, width.toDouble(), height * 0.5 - 4),
    Paint()..color = const Color(0xFF00FF00),
  );
  canvas.drawRect(
    Rect.fromLTWH(0, height - 4.0, width.toDouble(), 4),
    Paint()..color = const Color(0xFF0000FF),
  );
  final picture = recorder.endRecording();
  final image = await picture.toImage(width, height);
  picture.dispose();
  return image;
}

Future<Color> _samplePixel(
  WidgetTester tester,
  GlobalKey boundaryKey,
  Offset position,
) async {
  final data = await tester.runAsync<ByteData?>(() async {
    final boundary =
        boundaryKey.currentContext!.findRenderObject()! as RenderRepaintBoundary;
    final image = await boundary.toImage();
    try {
      return await image.toByteData(format: ui.ImageByteFormat.rawRgba);
    } finally {
      image.dispose();
    }
  });
  final boundary =
      boundaryKey.currentContext!.findRenderObject()! as RenderRepaintBoundary;
  final bytes = data!;
  final width = boundary.size.width.round();
  final x = position.dx.round().clamp(0, width - 1);
  final y = position.dy.round().clamp(0, boundary.size.height.round() - 1);
  final offset = (y * width + x) * 4;
  return Color.fromARGB(
    bytes.getUint8(offset + 3),
    bytes.getUint8(offset),
    bytes.getUint8(offset + 1),
    bytes.getUint8(offset + 2),
  );
}

void main() {
  testWidgets('上方卡片展开将下方玻璃卡片挤出底部再收回时，背景保持实时对齐不出现拉伸', (tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(360, 600);
    addTearDown(tester.view.reset);

    final wallpaper = (await tester.runAsync(
      () => _buildVerticalBandImage(360, 600),
    ))!;
    addTearDown(wallpaper.dispose);

    final rootKey = GlobalKey();
    final lastCardKey = GlobalKey();
    var topHeight = 120.0;
    late StateSetter setPageState;

    await tester.pumpWidget(
      MaterialApp(
        home: RepaintBoundary(
          key: rootKey,
          child: GlassScope(
            tint: 0.0,
            wallpaperPath: 'test.png',
            wallpaperRevision: 1,
            sourceWallpaper: wallpaper,
            blurredWallpaper: wallpaper,
            child: StatefulBuilder(
              builder: (context, setState) {
                setPageState = setState;
                return ListView.builder(
                  padding: const EdgeInsets.all(12),
                  itemCount: 2,
                  itemBuilder: (_, index) {
                    if (index == 0) {
                      return Padding(
                        padding: const EdgeInsets.only(bottom: 12),
                        child: GlassCard(
                          padding: EdgeInsets.zero,
                          child: AnimatedSize(
                            alignment: Alignment.topCenter,
                            duration: const Duration(milliseconds: 260),
                            child: SizedBox(
                              height: topHeight,
                              width: double.infinity,
                            ),
                          ),
                        ),
                      );
                    }
                    return GlassCard(
                      key: lastCardKey,
                      padding: EdgeInsets.zero,
                      child: const SizedBox(
                        height: 120,
                        width: double.infinity,
                      ),
                    );
                  },
                );
              },
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    final initialCenter = tester.getCenter(find.byKey(lastCardKey));
    final initialColor = await _samplePixel(tester, rootKey, initialCenter);
    expect(initialColor.r, greaterThan(0.7));
    expect(initialColor.g, lessThan(0.3));
    expect(initialColor.b, lessThan(0.3));

    setPageState(() => topHeight = 360.0);
    await tester.pump();
    for (var i = 0; i < 20; i++) {
      await tester.pump(const Duration(milliseconds: 16));
    }

    final midCenter = tester.getCenter(find.byKey(lastCardKey));
    final midColor = await _samplePixel(tester, rootKey, midCenter);
    expect(midColor.g, greaterThan(0.7));
    expect(midColor.r, lessThan(0.3));
    expect(midColor.b, lessThan(0.3));

    setPageState(() => topHeight = 900.0);
    await tester.pump();
    for (var i = 0; i < 20; i++) {
      await tester.pump(const Duration(milliseconds: 16));
    }

    setPageState(() => topHeight = 120.0);
    await tester.pump();
    for (var i = 0; i < 20; i++) {
      await tester.pump(const Duration(milliseconds: 16));
    }

    final restoredCenter = tester.getCenter(find.byKey(lastCardKey));
    expect(restoredCenter.dy, closeTo(initialCenter.dy, 1.0));
    final restoredColor = await _samplePixel(tester, rootKey, restoredCenter);
    expect(restoredColor.r, greaterThan(0.7));
    expect(restoredColor.g, lessThan(0.3));
    expect(restoredColor.b, lessThan(0.3));
    expect(tester.takeException(), isNull);
  });
}
