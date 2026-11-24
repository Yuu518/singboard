import 'dart:async';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:singboard_native/tailscale_page.dart';
import 'package:singboard_native/tailscale_service.dart';

class _Controller extends TailscaleController {
  final opens = <Map<String, dynamic>>[];
  late final handle = Completer<int>();
  final batches = <Map<String, dynamic>>[];
  final closed = <int>[];

  @override
  Future<int> openFeed(Map<String, dynamic> args) {
    opens.add(args);
    return handle.future;
  }

  @override
  Future<Map<String, dynamic>> drain(int handle) async => batches.removeAt(0);

  @override
  Future<void> closeFeed(int handle) async => closed.add(handle);
}

void main() {
  const channel = MethodChannel('singboard/config');
  TestWidgetsFlutterBinding.ensureInitialized();
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;
  late _Controller controller;
  late List<String> cleared;

  setUp(() {
    controller = _Controller();
    for (final tag in ['first', 'second']) {
      controller.endpoints.add({
        'endpointTag': tag,
        'canShareFiles': true,
        'userGroups': [
          {
            'peers': [
              {'hostName': '无标识', 'canReceiveFiles': true},
              {
                'stableId': 'id-$tag',
                'hostName': 'host-$tag',
                'canReceiveFiles': true,
                'tailscaleIps': ['100.64.0.2'],
              },
            ],
          },
        ],
      });
    }
    cleared = [];
    messenger.setMockMethodCallHandler(channel, (call) async {
      if (call.method == 'clearStagedShare')
        cleared.add(call.arguments['batchDir'] as String);
      return null;
    });
  });

  tearDown(() {
    messenger.setMockMethodCallHandler(channel, null);
    controller.dispose();
  });

  Future<void> showSheet(WidgetTester tester) => tester.pumpWidget(
    MaterialApp(
      home: Scaffold(
        body: TaildropQuickShareSheet(
          controller: controller,
          batchDir: 'batch',
          files: const [
            {'name': 'one', 'size': 8, 'path': '/one'},
            {'name': 'two', 'size': 5, 'path': '/two'},
          ],
          fromExternal: true,
        ),
      ),
    ),
  );

  testWidgets('系统分享使用所选端点，连点只创建一个上传并显示跨文件进度', (tester) async {
    await showSheet(tester);
    await tester.tap(find.text('second'));
    await tester.pump();
    expect(find.text('无标识'), findsNothing);
    expect(find.textContaining('100.64.0.2'), findsOneWidget);
    final send = find.text('发送到 host-second');
    await tester.tap(send);
    await tester.tap(send);
    expect(controller.opens, [
      {
        'feed': 'upload',
        'tag': 'second',
        'peerStableId': 'id-second',
        'paths': ['/one', '/two'],
      },
    ]);
    controller.batches.addAll([
      {
        'events': [
          {
            'data': {'sentBytes': 1},
            'totalSentBytes': 9,
            'completedFiles': 1,
          },
        ],
      },
      {'finished': true, 'events': []},
    ]);
    controller.handle.complete(7);
    await tester.pump();
    expect(find.textContaining('1/2 (9 B / 13 B)'), findsOneWidget);
    await tester.pump(const Duration(milliseconds: 220));
    expect(find.textContaining('2 个文件到 host-second'), findsOneWidget);
    expect(controller.closed, [7]);
    await tester.pumpWidget(const SizedBox());
    await tester.pump();
    expect(cleared, ['batch']);
  });

  testWidgets('打开上传途中关闭面板会先关闭迟到的句柄再清理文件', (tester) async {
    await showSheet(tester);
    await tester.tap(find.text('发送到 host-first'));
    await tester.pumpWidget(const SizedBox());
    expect(cleared, isEmpty);
    controller.handle.complete(11);
    await tester.pump();
    expect(controller.closed, [11]);
    expect(cleared, ['batch']);
  });

  testWidgets('失败后释放句柄并保留文件以供重试', (tester) async {
    controller.handle.complete(13);
    controller.batches.add({
      'events': [
        {
          'error': {'message': '连接中断'},
        },
      ],
    });
    await showSheet(tester);
    await tester.tap(find.text('发送到 host-first'));
    await tester.pump();
    expect(find.textContaining('连接中断'), findsOneWidget);
    expect(controller.closed, [13]);
    expect(cleared, isEmpty);
    controller.batches.add({'finished': true, 'events': []});
    await tester.tap(find.text('发送到 host-first'));
    await tester.pump();
    expect(find.textContaining('2 个文件到 host-first'), findsOneWidget);
    expect(controller.opens.length, 2);
    await tester.pumpWidget(const SizedBox());
    await tester.pump();
    expect(cleared, ['batch']);
  });

  test('设备显示名优先采用控制台 dnsName 并保留未改名时的 hostName 大小写', () {
    expect(
      tailscalePeerDisplayName({
        'hostName': 'DESKTOP-ABC',
        'dnsName': 'yuu-desktop.tailnet.ts.net.',
      }),
      'yuu-desktop',
    );
    expect(
      tailscalePeerDisplayName({
        'hostName': 'JP-Oracle-Yuu',
        'dnsName': 'jp-oracle-yuu.tailnet.ts.net.',
      }),
      'JP-Oracle-Yuu',
    );
    expect(
      tailscalePeerDisplayName({'hostName': 'local-box'}),
      'local-box',
    );
    expect(
      tailscalePeerDisplayName({'hostName': 'device-of-shared-to-user'}),
      '共享设备',
    );
  });
}
