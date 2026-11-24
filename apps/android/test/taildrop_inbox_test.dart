import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:singboard_native/tailscale_page.dart';
import 'package:singboard_native/tailscale_service.dart';

class _InboxController extends TailscaleController {
  Map<String, dynamic> snapshot = {};

  @override
  Future<int> openFeed(Map<String, dynamic> args) async {
    expect(args, {'feed': 'inbox', 'tag': 'ts'});
    return 1;
  }

  @override
  Future<Map<String, dynamic>> drain(int handle) async => {
    'events': [
      {'data': snapshot},
    ],
  };

  @override
  Future<void> closeFeed(int handle) async {}
}

void main() {
  testWidgets('收件箱区分端点已保存的文件和正在接收的文件', (tester) async {
    final controller = _InboxController();
    addTearDown(controller.dispose);
    controller.endpoints = [
      {'endpointTag': 'ts', 'backendState': 'Running', 'canShareFiles': true},
    ];
    final files = [
      for (var i = 1; i <= 4; i++) {'name': 'photo-$i.jpg', 'size': 512},
    ];
    final receiving = [
      {'name': 'incoming.jpg', 'size': 512, 'receivedBytes': 128},
    ];
    controller.snapshot = {'files': files, 'receiving': []};
    await tester.pumpWidget(
      MaterialApp(home: TailscalePage(controller: controller)),
    );
    await tester.tap(find.text('Taildrop'));
    await tester.pump();
    expect(find.text('等待读取收件箱'), findsOneWidget);
    expect(find.text('收件箱为空'), findsNothing);
    await tester.pump(const Duration(milliseconds: 250));
    await tester.pump();
    expect(find.text('photo-4.jpg'), findsOneWidget);
    expect(find.text('已接收 4 个文件'), findsOneWidget);
    expect(find.text('文件已保存在此端点的 Taildrop 接收目录。'), findsOneWidget);
    expect(find.text('另存为'), findsNWidgets(4));
    expect(find.text('删除原文件'), findsNWidgets(4));

    controller.snapshot = {'files': files, 'receiving': receiving};
    await tester.pump(const Duration(milliseconds: 250));
    await tester.pump();
    expect(find.text('已接收 4 个文件'), findsOneWidget);
    expect(find.text('incoming.jpg'), findsOneWidget);

    controller.snapshot = {'files': [], 'receiving': receiving};
    await tester.pump(const Duration(milliseconds: 250));
    await tester.pump();
    expect(find.text('收件箱为空'), findsNothing);
    expect(find.text('取消'), findsOneWidget);
    expect(find.text('另存为'), findsNothing);

    controller.snapshot = {'files': [], 'receiving': []};
    await tester.pump(const Duration(milliseconds: 250));
    await tester.pump();
    expect(find.text('收件箱为空'), findsOneWidget);
    await tester.pumpWidget(const SizedBox());
  });
}
