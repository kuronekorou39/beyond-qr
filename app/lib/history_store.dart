import 'dart:convert';
import 'dart:io';
import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';

/// 履歴の1エントリ。received は本体ファイルを端末に保存 (file に相対パス)、
/// sent は試行の記録 (grid/ec など設定)。
class HistoryItem {
  final String id;
  final String name;
  final String type;
  final int size;
  final int tsMs; // epoch ms
  final String? file; // received: 保存ファイルの相対パス
  final String? grid; // sent: 使用グリッド
  final String? ec; // sent: 使用 EC

  HistoryItem({
    required this.id,
    required this.name,
    required this.type,
    required this.size,
    required this.tsMs,
    this.file,
    this.grid,
    this.ec,
  });

  DateTime get time => DateTime.fromMillisecondsSinceEpoch(tsMs);

  Map<String, dynamic> toJson() => {
        'id': id,
        'name': name,
        'type': type,
        'size': size,
        'tsMs': tsMs,
        if (file != null) 'file': file,
        if (grid != null) 'grid': grid,
        if (ec != null) 'ec': ec,
      };

  factory HistoryItem.fromJson(Map<String, dynamic> j) => HistoryItem(
        id: j['id'] as String,
        name: j['name'] as String? ?? 'data',
        type: j['type'] as String? ?? 'application/octet-stream',
        size: (j['size'] as num?)?.toInt() ?? 0,
        tsMs: (j['tsMs'] as num?)?.toInt() ?? 0,
        file: j['file'] as String?,
        grid: j['grid'] as String?,
        ec: j['ec'] as String?,
      );
}

/// ローカル履歴 (オフライン)。受信成功と送信試行を JSON インデックスで管理。
class HistoryStore {
  HistoryStore._();
  static final HistoryStore instance = HistoryStore._();

  late Directory _dir;
  File get _index => File('${_dir.path}/history.json');
  Directory get _received => Directory('${_dir.path}/received');

  final List<HistoryItem> received = [];
  final List<HistoryItem> sent = [];

  /// 履歴が変わるたびに増える (UI の再描画トリガ)。
  final ValueNotifier<int> revision = ValueNotifier<int>(0);

  Future<void> init() async {
    _dir = await getApplicationSupportDirectory();
    if (!await _received.exists()) await _received.create(recursive: true);
    if (await _index.exists()) {
      try {
        final j = jsonDecode(await _index.readAsString()) as Map<String, dynamic>;
        received
          ..clear()
          ..addAll((j['received'] as List? ?? [])
              .map((e) => HistoryItem.fromJson(e as Map<String, dynamic>)));
        sent
          ..clear()
          ..addAll((j['sent'] as List? ?? [])
              .map((e) => HistoryItem.fromJson(e as Map<String, dynamic>)));
      } catch (_) {/* 壊れていたら無視 */}
    }
    revision.value++;
  }

  String _newId() => DateTime.now().microsecondsSinceEpoch.toString();

  Future<void> _persist() async {
    final j = {
      'received': received.map((e) => e.toJson()).toList(),
      'sent': sent.map((e) => e.toJson()).toList(),
    };
    await _index.writeAsString(jsonEncode(j));
  }

  /// ストリーミング受信用: 出力先パスを確保 (実データはここへ直接書き込む)。
  ({String id, String path}) reserveReceivedPath() {
    final id = _newId();
    return (id: id, path: '${_received.path}/$id');
  }

  /// 受信完了を履歴に登録 (実データは reserveReceivedPath のパスに書き込み済み)。
  Future<void> registerReceived(String id, String name, String type, int size) async {
    received.insert(
      0,
      HistoryItem(
          id: id,
          name: name,
          type: type,
          size: size,
          tsMs: DateTime.now().millisecondsSinceEpoch,
          file: 'received/$id'),
    );
    await _persist();
    revision.value++;
  }

  Future<void> addSent(String name, String type, int size, String grid, String ec) async {
    sent.insert(
      0,
      HistoryItem(
          id: _newId(),
          name: name,
          type: type,
          size: size,
          tsMs: DateTime.now().millisecondsSinceEpoch,
          grid: grid,
          ec: ec),
    );
    await _persist();
    revision.value++;
  }

  /// 受信データのファイル (画像は Image.file でストリーム表示、テキストは小さい時だけ読む)。
  File? receivedFile(HistoryItem item) =>
      item.file == null ? null : File('${_dir.path}/${item.file}');

  Future<Uint8List?> readReceived(HistoryItem item) async {
    final f = receivedFile(item);
    if (f == null || !await f.exists()) return null;
    return f.readAsBytes();
  }
}
