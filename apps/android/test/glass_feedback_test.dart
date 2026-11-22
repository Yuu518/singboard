import 'package:flutter/gestures.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:singboard_native/glass_ui.dart';
import 'package:singboard_native/setup_wizard.dart';

void main() {
  testWidgets('首次向导在面板遮罩中显示并可输入后端地址', (tester) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(360, 640);
    addTearDown(tester.view.reset);
    await tester.pumpWidget(
      MaterialApp(
        home: Stack(
          children: [
            SetupWizardOverlay(
              preferences: const {},
              initialUrl: '',
              initialSecret: '',
              onCompleted: () {},
              onGoToSettings: () {},
            ),
          ],
        ),
      ),
    );
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byType(TextField).first,
      'http://127.0.0.1:9091',
    );
    expect(find.text('http://127.0.0.1:9091'), findsOneWidget);
    expect(find.text('稍后在设置中配置'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('通知悬停暂停计时，关闭后不拦截页面', (tester) async {
    await tester.pumpWidget(
      MaterialApp(
        builder: (_, child) => GlassToastHost(child: child!),
        home: Scaffold(
          body: Builder(
            builder: (context) => TextButton(
              onPressed: () => showGlassToast(context, '操作已完成', error: false),
              child: const Text('触发'),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text('触发'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));
    expect(find.text('操作已完成'), findsOneWidget);
    final mouse = await tester.createGesture(kind: PointerDeviceKind.mouse);
    await mouse.addPointer();
    await mouse.moveTo(tester.getCenter(find.text('操作已完成')));
    await tester.pump(const Duration(seconds: 6));
    expect(find.text('操作已完成'), findsOneWidget);
    await mouse.moveTo(Offset.zero);
    await tester.pump();
    await tester.pump(const Duration(seconds: 6));
    await tester.pump(const Duration(milliseconds: 300));
    await tester.pumpAndSettle();
    expect(find.text('操作已完成'), findsNothing);
    await tester.tap(find.text('触发'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));
    await tester.tap(find.bySemanticsLabel('关闭'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));
    await tester.pumpAndSettle();
    expect(find.text('操作已完成'), findsNothing);
    await mouse.removePointer();
  });

  testWidgets('玻璃确认框支持遮罩取消，必须确认的操作只能由按钮关闭', (tester) async {
    bool? answer;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Builder(
            builder: (context) => Column(
              children: [
                for (final dismissible in [true, false])
                  TextButton(
                    onPressed: () async {
                      answer = await showGlassDialog<bool>(
                        context: context,
                        barrierDismissible: dismissible,
                        builder: (ctx) => GlassConfirmDialog(
                          title: const Text('确认操作'),
                          actions: [
                            TextButton(
                              onPressed: () => Navigator.pop(ctx, true),
                              child: const Text('确认'),
                            ),
                          ],
                        ),
                      );
                    },
                    child: Text(dismissible ? '普通' : '必须确认'),
                  ),
              ],
            ),
          ),
        ),
      ),
    );
    for (final label in ['普通', '必须确认']) {
      await tester.tap(find.text(label));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 220));
      expect(find.text('确认操作'), findsOneWidget);
      await tester.tapAt(const Offset(12, 300));
      await tester.pumpAndSettle();
      if (label == '必须确认') {
        expect(find.text('确认操作'), findsOneWidget);
        await tester.tap(find.text('确认'));
        await tester.pumpAndSettle();
        expect(answer, isTrue);
      }
      expect(find.text('确认操作'), findsNothing);
    }
  });
}
