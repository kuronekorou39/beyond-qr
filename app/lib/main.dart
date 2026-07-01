import 'package:flutter/material.dart';
import 'src/rust/frb_generated.dart';
import 'history_store.dart';
import 'send_screen.dart';
import 'receive_screen.dart';
import 'history_screen.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
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

  static const _screens = [SendScreen(), ReceiveScreen(), HistoryScreen()];

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
          NavigationDestination(icon: Icon(Icons.history), label: '履歴'),
        ],
      ),
    );
  }
}
