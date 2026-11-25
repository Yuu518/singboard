import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:singboard_native/catalog_page.dart';
import 'package:singboard_native/glass_ui.dart';

void main() {
  test('formatUpdatedRelative handles RFC3339 timestamps and zero times', () {
    final now = DateTime.utc(2026, 11, 23, 3, 0, 0);
    expect(
      formatUpdatedRelative('2026-11-23T02:59:45Z', now: now),
      '15 秒前',
    );
    expect(
      formatUpdatedRelative('2026-11-23T10:54:59.123456789+08:00', now: now),
      '5 分钟前',
    );
    expect(
      formatUpdatedRelative('2026-11-23T01:00:00Z', now: now),
      '2 小时前',
    );
    expect(
      formatUpdatedRelative('2026-11-20T03:00:00Z', now: now),
      '3 天前',
    );
    expect(formatUpdatedRelative('0001-01-01T00:00:00Z', now: now), '');
    expect(formatUpdatedRelative('', now: now), '');
  });

  testWidgets('SpinningPanelIcon rotates while spinning and settles when stopped', (
    tester,
  ) async {
    Future<void> pumpIcon(bool spinning) {
      return tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: Center(
              child: SpinningPanelIcon(
                'refresh',
                size: 16,
                color: Colors.blue,
                spinning: spinning,
              ),
            ),
          ),
        ),
      );
    }

    await pumpIcon(false);
    final transition = () => tester.widget<RotationTransition>(
      find.descendant(
        of: find.byType(SpinningPanelIcon),
        matching: find.byType(RotationTransition),
      ),
    );
    expect(transition().turns.value, 0.0);

    await pumpIcon(true);
    await tester.pump(const Duration(milliseconds: 250));
    expect(transition().turns.value, greaterThan(0.0));

    await pumpIcon(false);
    await tester.pumpAndSettle();
    expect(transition().turns.value, 0.0);
  });

  testWidgets('GlassSegmented renders count badges and slides on selection change', (
    tester,
  ) async {
    var selected = false;
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: StatefulBuilder(
            builder: (context, setState) => Center(
              child: GlassSegmented<bool>(
                items: const [(false, '代理组'), (true, '代理提供商')],
                counts: const {false: 15, true: 5},
                value: selected,
                onChanged: (v) => setState(() => selected = v),
              ),
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('代理组'), findsOneWidget);
    expect(find.text('15'), findsOneWidget);
    expect(find.text('代理提供商'), findsOneWidget);
    expect(find.text('5'), findsOneWidget);
    final before = tester.widget<AnimatedPositioned>(find.byType(AnimatedPositioned));
    expect(before.left, 0.0);

    await tester.tap(find.text('代理提供商'));
    await tester.pumpAndSettle();
    expect(selected, isTrue);
    final after = tester.widget<AnimatedPositioned>(find.byType(AnimatedPositioned));
    expect(after.left, greaterThan(0.0));
  });
}
