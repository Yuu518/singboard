import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:singboard_native/glass_ui.dart';

void main() {
  testWidgets('底栏拖动松手后切页，取消保留当前页，重复点选会回调当前页', (tester) async {
    const destinations = [
      GlassBottomNavItem('home', '概览'),
      GlassBottomNavItem('proxies', '代理'),
      GlassBottomNavItem('connections', '连接'),
      GlassBottomNavItem('logs', '日志'),
      GlassBottomNavItem('rules', '规则'),
      GlassBottomNavItem('settings', '设置'),
    ];
    var index = 0;
    final changes = <int>[];
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          bottomNavigationBar: StatefulBuilder(
            builder: (_, update) => GlassBottomNav(
              index: index,
              items: destinations,
              onChanged: (next) => update(() {
                index = next;
                changes.add(next);
              }),
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    Offset at(String text) => tester.getCenter(find.text(text).first);
    final first = at('概览'), third = at('连接');
    final cancelled = await tester.startGesture(first);
    await cancelled.moveTo(third);
    await tester.pump(const Duration(milliseconds: 80));
    expect(changes, isEmpty);
    await cancelled.cancel();
    await tester.pumpAndSettle();
    expect(changes, isEmpty);
    final chosen = await tester.startGesture(first);
    await chosen.moveTo(third);
    await tester.pump();
    expect(changes, isEmpty);
    await chosen.up();
    await tester.pumpAndSettle();
    expect(changes, [2]);
    await tester.tapAt(third);
    await tester.pumpAndSettle();
    expect(changes, [2, 2]);
  });

  testWidgets('API 不可用时拦截操作，恢复后可在同一页面继续', (tester) async {
    var clicks = 0;
    Widget page((String, String, bool)? mask) => MaterialApp(
      home: GlassApiGate(
        mask: mask,
        child: TextButton(onPressed: () => clicks++, child: const Text('断开连接')),
      ),
    );
    await tester.pumpWidget(
      page(('Clash API 密钥不匹配', '请在设置中检查 Clash API 密钥', false)),
    );
    expect(find.text('Clash API 密钥不匹配'), findsOneWidget);
    await tester.tap(find.text('断开连接'), warnIfMissed: false);
    expect(clicks, 0);
    await tester.pumpWidget(page(null));
    await tester.pumpAndSettle();
    await tester.tap(find.text('断开连接'));
    expect(clicks, 1);
  });
}
