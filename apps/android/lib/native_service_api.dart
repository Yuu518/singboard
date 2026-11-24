import 'dart:convert';
import 'dart:ffi';
import 'dart:isolate';

import 'package:ffi/ffi.dart';

final _library = DynamicLibrary.open('libsingboard_core.so');
final _open = _library
    .lookupFunction<
      Uint64 Function(Pointer<Utf8>),
      int Function(Pointer<Utf8>)
    >('sb_service_api_open');
final _request = _library
    .lookupFunction<
      Pointer<Utf8> Function(Uint64, Pointer<Utf8>),
      Pointer<Utf8> Function(int, Pointer<Utf8>)
    >('sb_service_api_request');
final _close = _library
    .lookupFunction<Void Function(Uint64), void Function(int)>(
      'sb_service_api_close',
    );
final _free = _library
    .lookupFunction<Void Function(Pointer<Utf8>), void Function(Pointer<Utf8>)>(
      'sb_string_free',
    );
final _finalizer = Finalizer<int>(_close);

class ServiceApiException implements Exception {
  final String code;
  final String message;
  const ServiceApiException(this.code, this.message);
  @override
  String toString() => message;
}

class NativeServiceApi implements Finalizable {
  int _handle;

  NativeServiceApi(String directory)
    : _handle = using(
        (arena) => _open(directory.toNativeUtf8(allocator: arena)),
      ) {
    if (_handle == 0) throw StateError('无法创建 Service API 会话');
    _finalizer.attach(this, _handle, detach: this);
  }

  Future<dynamic> request(
    String op, [
    Map<String, dynamic> args = const {},
  ]) async {
    if (_handle == 0) throw StateError('会话已关闭');
    return await _execute(_handle, jsonEncode({...args, 'op': op}));
  }

  void close() {
    if (_handle == 0) return;
    _finalizer.detach(this);
    _close(_handle);
    _handle = 0;
  }
}

Future<dynamic> _execute(int handle, String request) => Isolate.run(() {
  return using((arena) {
    final reply = _request(handle, request.toNativeUtf8(allocator: arena));
    if (reply == nullptr) throw StateError('Rust 接口未返回数据');
    try {
      final envelope = jsonDecode(reply.toDartString()) as Map<String, dynamic>;
      if (envelope['error'] case final Map error) {
        throw ServiceApiException(
          error['code'] as String,
          error['message'] as String,
        );
      }
      return envelope['data'];
    } finally {
      _free(reply);
    }
  });
});
