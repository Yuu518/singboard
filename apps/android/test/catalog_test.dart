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
}
