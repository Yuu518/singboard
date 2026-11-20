import 'dart:convert';
import 'dart:ffi';
import 'dart:isolate';

import 'package:ffi/ffi.dart';

final _library = DynamicLibrary.open('libsingboard_stream.so');
final _open = _library
    .lookupFunction<
      Pointer<Void> Function(Pointer<Utf8>, Pointer<Utf8>, Bool),
      Pointer<Void> Function(Pointer<Utf8>, Pointer<Utf8>, bool)
    >('sb_stream_open');
final _openLogs = _library
    .lookupFunction<
      Pointer<Void> Function(Pointer<Utf8>, Pointer<Utf8>, Pointer<Utf8>),
      Pointer<Void> Function(Pointer<Utf8>, Pointer<Utf8>, Pointer<Utf8>)
    >('sb_logs_open');
final _drain = _library
    .lookupFunction<
      Pointer<Utf8> Function(Pointer<Void>),
      Pointer<Utf8> Function(Pointer<Void>)
    >('sb_stream_drain');
final _close = _library
    .lookupFunction<Void Function(Pointer<Void>), void Function(Pointer<Void>)>(
      'sb_stream_close',
    );
final _free = _library
    .lookupFunction<Void Function(Pointer<Utf8>), void Function(Pointer<Utf8>)>(
      'sb_string_free',
    );

final _connections = _library
    .lookupFunction<
      Pointer<Void> Function(Pointer<Utf8>, Pointer<Utf8>),
      Pointer<Void> Function(Pointer<Utf8>, Pointer<Utf8>)
    >('sb_connections_open');
final _request = _library
    .lookupFunction<
      Pointer<Utf8> Function(Pointer<Utf8>, Pointer<Utf8>, Pointer<Utf8>),
      Pointer<Utf8> Function(Pointer<Utf8>, Pointer<Utf8>, Pointer<Utf8>)
    >('sb_clash_request');

Future<dynamic> clashRequest(
  String endpoint,
  String secret,
  String op, [
  Map<String, dynamic> args = const {},
]) {
  return Isolate.run(
    () => using((arena) {
      final response = _request(
        endpoint.toNativeUtf8(allocator: arena),
        secret.toNativeUtf8(allocator: arena),
        jsonEncode({'op': op, ...args}).toNativeUtf8(allocator: arena),
      );
      final result = jsonDecode(_takeString(response)) as Map<String, dynamic>;
      if (result['error'] case final String error) throw StateError(error);
      return result['data'];
    }),
  );
}

String _takeString(Pointer<Utf8> pointer) {
  if (pointer == nullptr) throw StateError('Rust 接口未返回数据');
  try {
    return pointer.toDartString();
  } finally {
    _free(pointer);
  }
}

class NativeStream {
  Pointer<Void> _handle = nullptr;

  NativeStream.connections(String endpoint, String secret) {
    _handle = using(
      (arena) => _connections(
        endpoint.toNativeUtf8(allocator: arena),
        secret.toNativeUtf8(allocator: arena),
      ),
    );
    if (_handle == nullptr) throw const FormatException('端点地址无效');
  }

  NativeStream.logs(String endpoint, String secret, String level) {
    _handle = using(
      (arena) => _openLogs(
        endpoint.toNativeUtf8(allocator: arena),
        secret.toNativeUtf8(allocator: arena),
        level.toNativeUtf8(allocator: arena),
      ),
    );
    if (_handle == nullptr) throw const FormatException('端点地址无效');
  }

  NativeStream(String endpoint, String secret, {required bool traffic}) {
    _handle = using(
      (arena) => _open(
        endpoint.toNativeUtf8(allocator: arena),
        secret.toNativeUtf8(allocator: arena),
        traffic,
      ),
    );
    if (_handle == nullptr) throw const FormatException('端点地址无效');
  }

  Map<String, dynamic> drain() =>
      jsonDecode(_takeString(_drain(_handle))) as Map<String, dynamic>;

  void close() {
    _close(_handle);
    _handle = nullptr;
  }
}

String readRootStatus() {
  final read = _library
      .lookupFunction<Pointer<Utf8> Function(), Pointer<Utf8> Function()>(
        'sb_root_status',
      );
  return _takeString(read());
}

Future<dynamic> configRequest(
  String directory,
  String op, [
  Map<String, dynamic> args = const {},
]) {
  return Isolate.run(
    () => using((arena) {
      final call = _library
          .lookupFunction<
            Pointer<Utf8> Function(Pointer<Utf8>, Pointer<Utf8>),
            Pointer<Utf8> Function(Pointer<Utf8>, Pointer<Utf8>)
          >('sb_config_request');
      final value = call(
        directory.toNativeUtf8(allocator: arena),
        jsonEncode({'op': op, ...args}).toNativeUtf8(allocator: arena),
      );
      final result = jsonDecode(_takeString(value)) as Map;
      if (result['error'] case final String error) throw StateError(error);
      return result['data'];
    }),
  );
}
