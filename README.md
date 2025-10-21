# Adaptive Clock

Tauri + Svelte のクロスプラットフォーム時計アプリです。Windows / macOS 双方でデスクトップ最前面に常駐し、右下に正方形ウィンドウとして表示されることを想定しています。

## 機能

- 時針・分針・秒針を備えたアナログ時計
- 秒針の回転速度を 1/2×, 2/3×, 1×, 3/2×, 2×, 3×, 4× の中から選択可能
- 時針は 1 秒ごとに更新、分針は 1 分ごとに 1 目盛りジャンプ
- 信頼性の高い TimeAPI (timeapi.io) で定期的に時刻同期し、同期失敗時はローカル時計でフォールバック
- 15 分ごとの自動再同期と手動再同期ボタンを用意

## 開発

```bash
# 依存関係のインストール
npm install

# 開発サーバ (Vite) と Tauri デバッグ起動
npm run tauri dev

# ビルド
npm run tauri build
```

> **注**: ネットワークアクセスが制限されている環境では `npm install` や `cargo build` に必要な依存取得が失敗する可能性があります。

## ディレクトリ構成

- `src/` – Svelte UI
- `src-tauri/` – Rust (Tauri) バックエンド
- `public/` – 静的リソース

## 時刻同期 API について

`src-tauri/src/main.rs` の `sync_time` コマンドは `timeapi.io` のゾーン別現在時刻エンドポイントを利用しています。フロントエンドからは OS のタイムゾーンを渡し、同期失敗時はローカルな UTC 時計にフォールバックします。
