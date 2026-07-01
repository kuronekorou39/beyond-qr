import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

/// チャンク・ストリーミング転送プロトコル。
///
/// ファイルを固定 [blockSize] のブロックに分割し、各ブロックを独立に Fountain 符号化する。
/// QR には 1 バイトの種別プレフィックスを付ける:
///   - 0x01 マニフェスト: [0x01][utf8(JSON)]   … 名前/総サイズ/ブロック数/各ブロックの OTI
///   - 0x02 データ:       [0x02][blockIndex u32LE][fountain packet]
/// 全ブロックが full 長 (=blockSize) なので OTI は共通 (otiFull)。最後のブロックだけ別 (otiLast)。
/// これにより受信側はマニフェストの OTI からブロックごとに decoder を構築できる。

const int kBlockSize = 512 * 1024; // 512KB / ブロック

const int kFrameManifest = 0x01;
const int kFrameData = 0x02;

class StreamManifest {
  final String name;
  final String type;
  final int totalSize;
  final int blockSize;
  final int blockCount;
  final Uint8List otiFull;
  final Uint8List otiLast;

  StreamManifest({
    required this.name,
    required this.type,
    required this.totalSize,
    required this.blockSize,
    required this.blockCount,
    required this.otiFull,
    required this.otiLast,
  });

  int get lastBlockSize => totalSize - (blockCount - 1) * blockSize;

  /// blockIndex に対応する OTI (最後のブロックだけ otiLast)。
  Uint8List otiFor(int index) => index == blockCount - 1 ? otiLast : otiFull;

  /// そのブロックの実バイト長。
  int blockLen(int index) => index == blockCount - 1 ? lastBlockSize : blockSize;

  Uint8List toQr() {
    final json = jsonEncode({
      'n': name,
      't': type,
      's': totalSize,
      'bs': blockSize,
      'bc': blockCount,
      'of': base64Encode(otiFull),
      'ol': base64Encode(otiLast),
    });
    final body = utf8.encode(json);
    final out = Uint8List(1 + body.length);
    out[0] = kFrameManifest;
    out.setRange(1, out.length, body);
    return out;
  }

  static StreamManifest? tryParse(Uint8List bytes) {
    if (bytes.isEmpty || bytes[0] != kFrameManifest) return null;
    try {
      final j = jsonDecode(utf8.decode(bytes.sublist(1))) as Map<String, dynamic>;
      return StreamManifest(
        name: j['n'] as String? ?? 'data',
        type: j['t'] as String? ?? 'application/octet-stream',
        totalSize: (j['s'] as num).toInt(),
        blockSize: (j['bs'] as num).toInt(),
        blockCount: (j['bc'] as num).toInt(),
        otiFull: base64Decode(j['of'] as String),
        otiLast: base64Decode(j['ol'] as String),
      );
    } catch (_) {
      return null;
    }
  }
}

/// データQR を組み立てる: [0x02][blockIndex u32LE][packet]。
Uint8List buildDataQr(int blockIndex, Uint8List packet) {
  final out = Uint8List(1 + 4 + packet.length);
  out[0] = kFrameData;
  ByteData.sublistView(out, 1, 5).setUint32(0, blockIndex, Endian.little);
  out.setRange(5, out.length, packet);
  return out;
}

/// データQR を解析。マニフェスト等なら null。
({int blockIndex, Uint8List packet})? parseDataQr(Uint8List bytes) {
  if (bytes.length < 5 || bytes[0] != kFrameData) return null;
  final idx = ByteData.sublistView(bytes, 1, 5).getUint32(0, Endian.little);
  return (blockIndex: idx, packet: Uint8List.sublistView(bytes, 5));
}

/// 送信元 (メモリ or ディスク) を統一的にブロック読みするための抽象。
abstract class BlockSource {
  int get length;
  String get name;
  String get type;
  Future<Uint8List> readBlock(int offset, int len);
  Future<void> close() async {}
}

class MemoryBlockSource extends BlockSource {
  final Uint8List bytes;
  @override
  final String name;
  @override
  final String type;
  MemoryBlockSource(this.bytes, this.name, this.type);
  @override
  int get length => bytes.length;
  @override
  Future<Uint8List> readBlock(int offset, int len) async =>
      Uint8List.sublistView(bytes, offset, offset + len);
}

class FileBlockSource extends BlockSource {
  final String path;
  @override
  final String name;
  @override
  final String type;
  final int _length;
  RandomAccessFile? _raf;
  FileBlockSource(this.path, this.name, this.type, this._length);
  @override
  int get length => _length;
  @override
  Future<Uint8List> readBlock(int offset, int len) async {
    _raf ??= await File(path).open();
    await _raf!.setPosition(offset);
    return _raf!.read(len);
  }

  @override
  Future<void> close() async {
    await _raf?.close();
    _raf = null;
  }
}
