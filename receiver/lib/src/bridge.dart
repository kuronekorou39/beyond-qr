// beyond_qr_core_ffi.dll への Dart FFI バインディング。
//
// Phase 0d-A: Windows desktop でテスト。Rust 側 (core-ffi) で
// `bqc_encode`, `bqc_decode`, `bqc_palette_rgb` 等を C ABI として公開しているのを呼ぶ。

import 'dart:ffi';
import 'dart:io';
import 'dart:typed_data';

import 'package:ffi/ffi.dart';
import 'package:path/path.dart' as p;

// ---------------- 関数シグネチャ ----------------

typedef BqcEncodeNative = Int32 Function(
  Pointer<Uint8> payload,
  IntPtr payloadLen,
  IntPtr gridWidth,
  IntPtr gridHeight,
  IntPtr cellPx,
  IntPtr finderSize,
  IntPtr calibrationRowStart,
  IntPtr calibrationRows,
  Pointer<Uint8> outCells,
  IntPtr outCellsCapacity,
);

typedef BqcEncode = int Function(
  Pointer<Uint8> payload,
  int payloadLen,
  int gridWidth,
  int gridHeight,
  int cellPx,
  int finderSize,
  int calibrationRowStart,
  int calibrationRows,
  Pointer<Uint8> outCells,
  int outCellsCapacity,
);

typedef BqcDecodeNative = Int32 Function(
  Pointer<Uint8> cells,
  IntPtr cellsLen,
  IntPtr gridWidth,
  IntPtr gridHeight,
  IntPtr cellPx,
  IntPtr finderSize,
  IntPtr calibrationRowStart,
  IntPtr calibrationRows,
  Pointer<Uint8> outPayload,
  IntPtr outPayloadCapacity,
);

typedef BqcDecode = int Function(
  Pointer<Uint8> cells,
  int cellsLen,
  int gridWidth,
  int gridHeight,
  int cellPx,
  int finderSize,
  int calibrationRowStart,
  int calibrationRows,
  Pointer<Uint8> outPayload,
  int outPayloadCapacity,
);

typedef BqcMaxPayloadBytesNative = IntPtr Function(
  IntPtr gridWidth,
  IntPtr gridHeight,
  IntPtr cellPx,
  IntPtr finderSize,
  IntPtr calibrationRowStart,
  IntPtr calibrationRows,
);

typedef BqcMaxPayloadBytes = int Function(
  int gridWidth,
  int gridHeight,
  int cellPx,
  int finderSize,
  int calibrationRowStart,
  int calibrationRows,
);

typedef BqcTotalCellsNative = IntPtr Function(
  IntPtr gridWidth,
  IntPtr gridHeight,
  IntPtr cellPx,
  IntPtr finderSize,
  IntPtr calibrationRowStart,
  IntPtr calibrationRows,
);

typedef BqcTotalCells = int Function(
  int gridWidth,
  int gridHeight,
  int cellPx,
  int finderSize,
  int calibrationRowStart,
  int calibrationRows,
);

typedef BqcPaletteRgbNative = Int32 Function(Pointer<Uint8> outRgb, IntPtr outCapacity);
typedef BqcPaletteRgb = int Function(Pointer<Uint8> outRgb, int outCapacity);

// ---------------- FrameSpec ----------------

class FrameSpec {
  final int gridWidth;
  final int gridHeight;
  final int cellPx;
  final int finderSize;
  final int calibrationRowStart;
  final int calibrationRows;

  const FrameSpec({
    this.gridWidth = 128,
    this.gridHeight = 128,
    this.cellPx = 8,
    this.finderSize = 7,
    this.calibrationRowStart = 64,
    this.calibrationRows = 1,
  });

  static const phase0 = FrameSpec();

  int get totalCells => gridWidth * gridHeight;
  (int, int) get imageDimensions => (gridWidth * cellPx, gridHeight * cellPx);
}

// ---------------- バインディングクラス ----------------

class BeyondQrBridge {
  final DynamicLibrary _lib;
  late final BqcEncode _encode;
  late final BqcDecode _decode;
  late final BqcMaxPayloadBytes _maxPayload;
  late final BqcTotalCells _totalCells;
  late final BqcPaletteRgb _paletteRgb;

  BeyondQrBridge(this._lib) {
    _encode = _lib
        .lookupFunction<BqcEncodeNative, BqcEncode>('bqc_encode');
    _decode = _lib
        .lookupFunction<BqcDecodeNative, BqcDecode>('bqc_decode');
    _maxPayload = _lib.lookupFunction<BqcMaxPayloadBytesNative,
        BqcMaxPayloadBytes>('bqc_max_payload_bytes');
    _totalCells = _lib
        .lookupFunction<BqcTotalCellsNative, BqcTotalCells>('bqc_total_cells');
    _paletteRgb = _lib.lookupFunction<BqcPaletteRgbNative, BqcPaletteRgb>(
        'bqc_palette_rgb');
  }

  /// 既定のパス (workspace の target/release/beyond_qr_core_ffi.dll) から DLL をロードする。
  factory BeyondQrBridge.loadDefault() {
    final dllPath = _findDll();
    return BeyondQrBridge(DynamicLibrary.open(dllPath));
  }

  static String _findDll() {
    if (Platform.isWindows) {
      // テスト実行時は receiver ディレクトリが cwd 想定
      final candidates = [
        p.join('..', 'target', 'release', 'beyond_qr_core_ffi.dll'),
        p.join('target', 'release', 'beyond_qr_core_ffi.dll'),
        'beyond_qr_core_ffi.dll',
      ];
      for (final c in candidates) {
        if (File(c).existsSync()) return c;
      }
      throw Exception('DLL not found in any of: ${candidates.join(", ")}');
    }
    throw UnsupportedError('Platform ${Platform.operatingSystem} not supported yet');
  }

  int maxPayloadBytes(FrameSpec s) => _maxPayload(
      s.gridWidth,
      s.gridHeight,
      s.cellPx,
      s.finderSize,
      s.calibrationRowStart,
      s.calibrationRows);

  int totalCells(FrameSpec s) => _totalCells(
      s.gridWidth,
      s.gridHeight,
      s.cellPx,
      s.finderSize,
      s.calibrationRowStart,
      s.calibrationRows);

  /// 8 色のパレット RGB を Uint8List (長さ 24) で返す。
  Uint8List paletteRgb() {
    final out = calloc<Uint8>(24);
    try {
      final n = _paletteRgb(out, 24);
      if (n != 24) {
        throw StateError('bqc_palette_rgb returned $n');
      }
      final result = Uint8List(24);
      for (int i = 0; i < 24; i++) {
        result[i] = out[i];
      }
      return result;
    } finally {
      calloc.free(out);
    }
  }

  /// ペイロードを spec に応じてセル列に符号化する。返り値は Uint8List(spec.totalCells)。
  Uint8List encode(Uint8List payload, FrameSpec s) {
    final total = totalCells(s);
    final payloadPtr = calloc<Uint8>(payload.length == 0 ? 1 : payload.length);
    final outPtr = calloc<Uint8>(total);
    try {
      for (int i = 0; i < payload.length; i++) {
        payloadPtr[i] = payload[i];
      }
      final n = _encode(
        payloadPtr,
        payload.length,
        s.gridWidth,
        s.gridHeight,
        s.cellPx,
        s.finderSize,
        s.calibrationRowStart,
        s.calibrationRows,
        outPtr,
        total,
      );
      if (n < 0) {
        throw BridgeError('encode failed: code $n');
      }
      if (n != total) {
        throw BridgeError('encode returned $n cells (expected $total)');
      }
      final result = Uint8List(total);
      for (int i = 0; i < total; i++) {
        result[i] = outPtr[i];
      }
      return result;
    } finally {
      calloc.free(payloadPtr);
      calloc.free(outPtr);
    }
  }

  /// セル列を spec に応じて復号する。
  Uint8List decode(Uint8List cells, FrameSpec s) {
    if (cells.length != s.totalCells) {
      throw ArgumentError(
          'cells length ${cells.length} != totalCells ${s.totalCells}');
    }
    final maxOut = maxPayloadBytes(s);
    final cellsPtr = calloc<Uint8>(cells.length);
    final outPtr = calloc<Uint8>(maxOut);
    try {
      for (int i = 0; i < cells.length; i++) {
        cellsPtr[i] = cells[i];
      }
      final n = _decode(
        cellsPtr,
        cells.length,
        s.gridWidth,
        s.gridHeight,
        s.cellPx,
        s.finderSize,
        s.calibrationRowStart,
        s.calibrationRows,
        outPtr,
        maxOut,
      );
      if (n < 0) {
        throw BridgeError('decode failed: code $n');
      }
      final result = Uint8List(n);
      for (int i = 0; i < n; i++) {
        result[i] = outPtr[i];
      }
      return result;
    } finally {
      calloc.free(cellsPtr);
      calloc.free(outPtr);
    }
  }
}

class BridgeError implements Exception {
  final String message;
  BridgeError(this.message);
  @override
  String toString() => 'BridgeError: $message';
}
