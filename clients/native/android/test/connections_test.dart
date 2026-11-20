import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:singboard_native/connections_page.dart';

void main() {
  test('逐条完成筛选快照，即使部分断开失败', () async {
    final visited = <String>[];
    final last = Completer<void>();
    final failure = StateError('断开失败');
    final finished = disconnectSelected(['first', 'second', 'third'], (
      id,
    ) async {
      visited.add(id);
      if (id == 'first') throw failure;
      if (id == 'third') await last.future;
    });
    var resolved = false;
    finished.then((_) => resolved = true);
    await Future<void>.delayed(Duration.zero);
    expect(visited, ['first', 'second', 'third']);
    expect(resolved, isFalse);
    last.complete();
    expect(await finished, (2, 1, failure));
  });
}
