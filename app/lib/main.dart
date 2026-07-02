import 'dart:io' show Platform;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'src/rust/frb_generated.dart';
import 'history_store.dart';
import 'send_screen.dart';
import 'receive_screen.dart';
import 'history_screen.dart';
import 'vcode_send_screen.dart';
import 'vcode_receive_screen.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  // 縦向き固定: QR/vcode の受信はカメラ向きを前提に検出するため、端末回転で
  // 前提が崩れて読めなくなるのを防ぐ (スキャナ系アプリの定石)。
  await SystemChrome.setPreferredOrientations(
      [DeviceOrientation.portraitUp, DeviceOrientation.portraitDown]);
  await RustLib.init();
  await HistoryStore.instance.init();
  runApp(const BeyondQrApp());
}

class BeyondQrApp extends StatelessWidget {
  const BeyondQrApp({super.key});

  @override
  Widget build(BuildContext context) {
    final scheme = ColorScheme.fromSeed(
      seedColor: const Color(0xFF5B8CFF),
      brightness: Brightness.dark,
    );
    return MaterialApp(
      title: 'beyond-qr',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(colorScheme: scheme, useMaterial3: true),
      home: const HomeShell(),
    );
  }
}

class HomeShell extends StatefulWidget {
  const HomeShell({super.key});
  @override
  State<HomeShell> createState() => _HomeShellState();
}

class _HomeShellState extends State<HomeShell> {
  int _index = 0;

  /// カメラ受信はモバイルのみ。デスクトップ (Windows = PC 送信機として使用) では
  /// mobile_scanner / camera が動かないため差し替える (IndexedStack は全子を即ビルドする)。
  static final bool _hasCamera = Platform.isAndroid || Platform.isIOS;

  static final _screens = <Widget>[
    const SendScreen(),
    _hasCamera
        ? const ReceiveScreen()
        : const Center(child: Text('この環境ではカメラ受信は使えません')),
    const VcodeSendScreen(),
    _hasCamera
        ? const VcodeReceiveScreen()
        : const Center(child: Text('この環境ではカメラ受信は使えません')),
    const HistoryScreen(),
  ];

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('beyond-qr')),
      body: IndexedStack(index: _index, children: _screens),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _index,
        onDestinationSelected: (i) => setState(() => _index = i),
        destinations: const [
          NavigationDestination(icon: Icon(Icons.qr_code_2), label: '送信'),
          NavigationDestination(icon: Icon(Icons.photo_camera), label: '受信'),
          NavigationDestination(icon: Icon(Icons.grid_on), label: 'V送信'),
          NavigationDestination(icon: Icon(Icons.center_focus_strong), label: 'V受信'),
          NavigationDestination(icon: Icon(Icons.history), label: '履歴'),
        ],
      ),
    );
  }
}
