import 'dart:convert';
import 'dart:typed_data';
import 'package:flutter/material.dart';
import 'package:mobile_scanner/mobile_scanner.dart';
import 'package:wakelock_plus/wakelock_plus.dart';
import 'history_store.dart';
import 'src/rust/api/fountain.dart';

/// 送信側の wrapPayload を戻す: [uint32be headerLen][json header][body]。
({String name, String type, Uint8List body})? _unwrap(Uint8List payload) {
  if (payload.length < 4) return null;
  final headerLen = ByteData.sublistView(payload, 0, 4).getUint32(0, Endian.big);
  if (payload.length < 4 + headerLen) return null;
  try {
    final header = jsonDecode(utf8.decode(payload.sublist(4, 4 + headerLen)));
    final body = payload.sublist(4 + headerLen);
    return (
      name: header['name'] as String? ?? 'data',
      type: header['type'] as String? ?? 'application/octet-stream',
      body: Uint8List.fromList(body),
    );
  } catch (_) {
    return null;
  }
}

/// mobile_scanner の rawDecodedBytes (sealed) から実バイト列を取り出す。
/// Android=DecodedBarcodeBytes.bytes、Apple=DecodedVisionBarcodeBytes(bytes ?? rawBytes)。
Uint8List? _decodedBytes(BarcodeBytes? b) {
  return switch (b) {
    DecodedBarcodeBytes(:final bytes) => bytes,
    DecodedVisionBarcodeBytes(:final bytes, :final rawBytes) => bytes ?? rawBytes,
    null => null,
  };
}

String _hex(Uint8List b, [int len = 12]) {
  final n = b.length < len ? b.length : len;
  final sb = StringBuffer();
  for (var i = 0; i < n; i++) {
    sb.write(b[i].toRadixString(16).padLeft(2, '0'));
  }
  return sb.toString();
}

class ReceiveScreen extends StatefulWidget {
  const ReceiveScreen({super.key});
  @override
  State<ReceiveScreen> createState() => _ReceiveScreenState();
}

class _ReceiveScreenState extends State<ReceiveScreen> {
  MobileScannerController? _controller;

  FountainDecoder? _decoder;
  String? _lastOti;
  final _unique = <String>{};
  int _payloadSize = 0;
  String? _warning; // 大きすぎる等の注意
  String? _blockedOti; // ガードで弾いた OTI (再プローブ防止)

  ({String name, String type, Uint8List body})? _result;

  Future<void> _startCamera() async {
    await WakelockPlus.enable();
    setState(() {
      _controller = MobileScannerController(
        detectionSpeed: DetectionSpeed.unrestricted, // 毎フレーム処理 (QRが高速に変わるため)
        formats: const [BarcodeFormat.qrCode],
      );
      _decoder = null;
      _lastOti = null;
      _unique.clear();
      _payloadSize = 0;
      _warning = null;
      _blockedOti = null;
      _result = null;
    });
  }

  Future<void> _stopCamera() async {
    await _controller?.dispose();
    await WakelockPlus.disable();
    setState(() => _controller = null);
  }

  void _onDetect(BarcodeCapture capture) {
    if (_result != null) return;
    var changed = false;
    for (final bc in capture.barcodes) {
      final bytes = _decodedBytes(bc.rawDecodedBytes);
      if (bytes == null || bytes.length < 16) continue;
      final otiHex = _hex(bytes, 12);
      final oti = Uint8List.sublistView(bytes, 0, 12);
      final packet = Uint8List.sublistView(bytes, 12);

      if (otiHex == _blockedOti) continue; // 大きすぎでブロック済み
      // OTI が変わったら復号やり直し (送信側の設定変更に追従)
      if (_decoder == null || otiHex != _lastOti) {
        try {
          final dec = FountainDecoder(otiBytes: oti);
          final sz = dec.payloadSize().toInt();
          // 大きすぎる payload は RaptorQ 復号が破綻し UI が固まるのでブロック
          if (sz > 800 * 1024) {
            _blockedOti = otiHex;
            _warning = '送信データが大きすぎます (${(sz / 1024).round()}KB)。'
                'QR転送は ~200KB 向けです';
            changed = true;
            continue;
          }
          _decoder = dec;
          _lastOti = otiHex;
          _unique.clear();
          _payloadSize = sz;
          _warning = null;
        } catch (_) {
          continue;
        }
      }

      final key = _hex(packet, 4);
      if (_unique.contains(key)) continue;
      _unique.add(key);
      changed = true;

      try {
        if (_decoder!.addPacket(packet: Uint8List.fromList(packet))) {
          final p = _decoder!.payload();
          if (p != null) {
            _result = _unwrap(p);
            final r = _result;
            if (r != null) {
              HistoryStore.instance.addReceived(r.name, r.type, r.body);
            }
            WakelockPlus.disable();
            break;
          }
        }
      } catch (_) {}
    }
    if (changed || _result != null) setState(() {});
  }

  @override
  void dispose() {
    _controller?.dispose();
    WakelockPlus.disable();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    if (_result != null) return _buildResult(context);
    if (_controller == null) return _buildIdle(context);
    return _buildScanning(context);
  }

  Widget _buildIdle(BuildContext context) {
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(Icons.photo_camera,
              size: 64, color: Theme.of(context).colorScheme.primary),
          const SizedBox(height: 16),
          const Text('送信側の QR にカメラを向けて受信します'),
          const SizedBox(height: 16),
          FilledButton.icon(
            onPressed: _startCamera,
            icon: const Icon(Icons.play_arrow),
            label: const Text('カメラ開始'),
          ),
        ],
      ),
    );
  }

  Widget _buildScanning(BuildContext context) {
    final needed =
        (_payloadSize > 0) ? '~${(_payloadSize / 300).ceil()}' : '?';
    return Column(
      children: [
        Expanded(
          child: MobileScanner(controller: _controller!, onDetect: _onDetect),
        ),
        Padding(
          padding: const EdgeInsets.all(12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              if (_warning != null)
                Padding(
                  padding: const EdgeInsets.only(bottom: 6),
                  child: Text('⚠ $_warning',
                      style: const TextStyle(color: Colors.orange)),
                ),
              Text('OTI: ${_lastOti ?? "-"}',
                  style: Theme.of(context).textTheme.bodySmall),
              Text('ユニーク: ${_unique.length} / $needed'
                  '${_payloadSize > 0 ? "   payload ${_payloadSize}B" : ""}'),
              const SizedBox(height: 8),
              FilledButton.tonalIcon(
                onPressed: _stopCamera,
                icon: const Icon(Icons.stop),
                label: const Text('停止'),
              ),
            ],
          ),
        ),
      ],
    );
  }

  Widget _buildResult(BuildContext context) {
    final r = _result!;
    final isImage = r.type.startsWith('image/');
    final isText = r.type.startsWith('text/');
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Card(
          color: Colors.green.withValues(alpha: 0.15),
          child: ListTile(
            leading: const Icon(Icons.check_circle, color: Colors.green),
            title: Text('復元成功: ${r.name}'),
            subtitle: Text('${r.type}  ${r.body.length}B'),
          ),
        ),
        const SizedBox(height: 12),
        if (isImage)
          ClipRRect(
            borderRadius: BorderRadius.circular(8),
            child: Image.memory(r.body, fit: BoxFit.contain),
          )
        else if (isText)
          Card(
            child: Padding(
              padding: const EdgeInsets.all(12),
              child: Text(utf8.decode(r.body, allowMalformed: true)),
            ),
          )
        else
          Text('バイナリデータ (${r.body.length}B)'),
        const SizedBox(height: 16),
        FilledButton.icon(
          onPressed: _startCamera,
          icon: const Icon(Icons.replay),
          label: const Text('もう一度受信'),
        ),
      ],
    );
  }
}
