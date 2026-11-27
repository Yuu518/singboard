import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:singboard_native/native_service_api.dart';
import 'package:singboard_native/tailscale_service.dart';

void main() {
  const channel = MethodChannel('singboard/config');
  TestWidgetsFlutterBinding.ensureInitialized();
  final messenger =
      TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;

  setUp(() {
    messenger.setMockMethodCallHandler(channel, (call) async => null);
  });

  tearDown(() {
    messenger.setMockMethodCallHandler(channel, null);
  });

  Map<String, dynamic> runningSnapshot() => {
    'endpoints': [
      {
        'endpointTag': 'ts-main',
        'backendState': 'Running',
        'canShareFiles': true,
        'networkName': 'tailnet',
        'userGroups': [],
      },
    ],
  };

  test('状态流收到 reconnecting 或 error 或 finished 时立即清除连接与端点', () async {
    final ops = <String>[];
    final drainBatches = <Map<String, dynamic>>[
      {
        'events': [
          {
            'reconnecting': true,
            'error': {'code': 'unavailable', 'message': '状态订阅已断开'},
          },
        ],
        'finished': false,
      },
    ];
    final controller = TailscaleController(
      pollInterval: const Duration(milliseconds: 20),
      retryDelay: const Duration(hours: 1),
      requestHandler: (op, args) async {
        ops.add(op);
        return switch (op) {
          'disconnect' => null,
          'discover' => [
            {'serviceTag': 'api', 'url': 'http://127.0.0.1:8080'},
          ],
          'connect' => {'version': '1.0'},
          'status' => runningSnapshot(),
          'open' => 1,
          'drain' =>
            drainBatches.isNotEmpty
                ? drainBatches.removeAt(0)
                : {'events': const [], 'finished': false},
          _ => null,
        };
      },
    );
    addTearDown(controller.dispose);

    await controller.sync();
    expect(controller.hasEndpoints, isTrue);
    expect(controller.chipTone, ServiceChipTone.ok);

    await Future<void>.delayed(const Duration(milliseconds: 60));

    expect(controller.hasEndpoints, isFalse);
    expect(controller.endpoints, isEmpty);
    expect(controller.chipText, '不可达');
    expect(controller.chipTone, ServiceChipTone.error);
    expect(ops.where((op) => op == 'disconnect').length, greaterThanOrEqualTo(2));
  });

  test('本地核心关闭时立即断开为未连接且重启后自动重连', () async {
    var coreRunning = true;
    var connectCalls = 0;
    final controller = TailscaleController(
      pollInterval: const Duration(milliseconds: 20),
      retryDelay: const Duration(milliseconds: 30),
      requestHandler: (op, args) async {
        return switch (op) {
          'disconnect' => null,
          'discover' => [
            {'serviceTag': 'api', 'url': 'http://127.0.0.1:8080'},
          ],
          'connect' => () {
            connectCalls++;
            if (!coreRunning) {
              throw const ServiceApiException('unavailable', 'connection refused');
            }
            return {'version': '1.0'};
          }(),
          'status' => runningSnapshot(),
          'open' => 2,
          'drain' => {'events': const [], 'finished': false},
          _ => null,
        };
      },
    );
    addTearDown(controller.dispose);

    controller.updateCoreState(
      localBackend: true,
      coreRunning: true,
      knownStopped: false,
    );
    await controller.sync();
    expect(controller.hasEndpoints, isTrue);
    expect(controller.chipTone, ServiceChipTone.ok);
    expect(connectCalls, 1);

    coreRunning = false;
    controller.updateCoreState(
      localBackend: true,
      coreRunning: false,
      knownStopped: true,
    );
    expect(controller.hasEndpoints, isFalse);
    expect(controller.chipText, '未连接');
    expect(controller.chipTone, ServiceChipTone.muted);

    await Future<void>.delayed(const Duration(milliseconds: 80));
    expect(connectCalls, 1);

    coreRunning = true;
    controller.updateCoreState(
      localBackend: true,
      coreRunning: true,
      knownStopped: false,
    );
    await Future<void>.delayed(const Duration(milliseconds: 60));
    expect(connectCalls, 2);
    expect(controller.hasEndpoints, isTrue);
    expect(controller.chipTone, ServiceChipTone.ok);
  });

  test('远程 Service API 在断开后自动重试恢复且不受本地核心停止影响', () async {
    var remoteUp = true;
    var drainCount = 0;
    final controller = TailscaleController(
      pollInterval: const Duration(milliseconds: 20),
      retryDelay: const Duration(milliseconds: 40),
      requestHandler: (op, args) async {
        return switch (op) {
          'disconnect' => null,
          'discover' => [
            {'serviceTag': '', 'url': 'https://router.example:8443'},
          ],
          'connect' => () {
            if (!remoteUp) {
              throw const ServiceApiException('unavailable', 'down');
            }
            return {'version': '1.0'};
          }(),
          'status' => runningSnapshot(),
          'open' => 3,
          'drain' => () {
            drainCount++;
            if (!remoteUp) {
              return {'events': const [], 'finished': true};
            }
            return {'events': const [], 'finished': false};
          }(),
          _ => null,
        };
      },
    );
    addTearDown(controller.dispose);

    await controller.sync();
    expect(controller.hasEndpoints, isTrue);

    controller.updateCoreState(
      localBackend: true,
      coreRunning: false,
      knownStopped: true,
    );
    expect(controller.hasEndpoints, isTrue);
    expect(controller.chipTone, ServiceChipTone.ok);

    remoteUp = false;
    await Future<void>.delayed(const Duration(milliseconds: 50));
    expect(drainCount, greaterThan(0));
    expect(controller.hasEndpoints, isFalse);
    expect(controller.chipText, '不可达');
    expect(controller.chipTone, ServiceChipTone.error);

    remoteUp = true;
    await Future<void>.delayed(const Duration(milliseconds: 90));
    expect(controller.hasEndpoints, isTrue);
    expect(controller.chipTone, ServiceChipTone.ok);
  });
}
