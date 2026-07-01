import 'dart:typed_data';
import 'package:flutter/material.dart';
import 'history_store.dart';

class HistoryScreen extends StatelessWidget {
  const HistoryScreen({super.key});

  @override
  Widget build(BuildContext context) {
    return ValueListenableBuilder<int>(
      valueListenable: HistoryStore.instance.revision,
      builder: (context, _, _) {
        final store = HistoryStore.instance;
        return DefaultTabController(
          length: 2,
          child: Column(
            children: [
              const TabBar(tabs: [Tab(text: '受信'), Tab(text: '送信')]),
              Expanded(
                child: TabBarView(
                  children: [
                    _ReceivedList(items: store.received),
                    _SentList(items: store.sent),
                  ],
                ),
              ),
            ],
          ),
        );
      },
    );
  }
}

String _fmtTime(DateTime t) {
  String two(int n) => n.toString().padLeft(2, '0');
  return '${t.month}/${t.day} ${two(t.hour)}:${two(t.minute)}';
}

String _fmtSize(int n) {
  if (n >= 1024 * 1024) return '${(n / 1024 / 1024).toStringAsFixed(1)}MB';
  if (n >= 1024) return '${(n / 1024).toStringAsFixed(1)}KB';
  return '${n}B';
}

class _ReceivedList extends StatelessWidget {
  final List<HistoryItem> items;
  const _ReceivedList({required this.items});

  @override
  Widget build(BuildContext context) {
    if (items.isEmpty) return const Center(child: Text('受信履歴はまだありません'));
    return ListView.separated(
      itemCount: items.length,
      separatorBuilder: (_, _) => const Divider(height: 1),
      itemBuilder: (context, i) {
        final it = items[i];
        final isImage = it.type.startsWith('image/');
        return ListTile(
          leading: Icon(isImage ? Icons.image : Icons.insert_drive_file),
          title: Text(it.name, overflow: TextOverflow.ellipsis),
          subtitle: Text('${_fmtSize(it.size)}  ·  ${_fmtTime(it.time)}'),
          onTap: () => Navigator.of(context).push(MaterialPageRoute(
            builder: (_) => _ViewerPage(item: it),
          )),
        );
      },
    );
  }
}

class _SentList extends StatelessWidget {
  final List<HistoryItem> items;
  const _SentList({required this.items});

  @override
  Widget build(BuildContext context) {
    if (items.isEmpty) return const Center(child: Text('送信履歴はまだありません'));
    return ListView.separated(
      itemCount: items.length,
      separatorBuilder: (_, _) => const Divider(height: 1),
      itemBuilder: (context, i) {
        final it = items[i];
        return ListTile(
          leading: const Icon(Icons.upload),
          title: Text(it.name, overflow: TextOverflow.ellipsis),
          subtitle: Text(
              '${_fmtSize(it.size)}  ·  grid ${it.grid ?? "-"} / EC ${it.ec ?? "-"}  ·  ${_fmtTime(it.time)}'),
        );
      },
    );
  }
}

class _ViewerPage extends StatelessWidget {
  final HistoryItem item;
  const _ViewerPage({required this.item});

  @override
  Widget build(BuildContext context) {
    final file = HistoryStore.instance.receivedFile(item);
    Widget body;
    if (file == null) {
      body = const Center(child: Text('ファイルが見つかりません'));
    } else if (item.type.startsWith('image/')) {
      // 画像は Image.file でストリーム表示 (大きくてもメモリに全読みしない)。
      body = Center(child: InteractiveViewer(child: Image.file(file)));
    } else if (item.type.startsWith('text/') && item.size <= 1024 * 1024) {
      body = FutureBuilder<Uint8List?>(
        future: HistoryStore.instance.readReceived(item),
        builder: (context, snap) {
          if (!snap.hasData || snap.data == null) {
            return const Center(child: CircularProgressIndicator());
          }
          return SingleChildScrollView(
            padding: const EdgeInsets.all(16),
            child: SelectableText(String.fromCharCodes(snap.data!)),
          );
        },
      );
    } else {
      body = Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const Icon(Icons.check_circle, color: Colors.green, size: 48),
            const SizedBox(height: 8),
            Text(item.name),
            Text('${item.type}  ${_fmtSize(item.size)}'),
            const SizedBox(height: 8),
            Text('保存先: ${file.path}', style: Theme.of(context).textTheme.bodySmall),
          ],
        ),
      );
    }
    return Scaffold(appBar: AppBar(title: Text(item.name)), body: body);
  }
}
