# CHANGELOG

## 0.1.0 — 2026-11-23

最初の公開版。主要面を実サービスに対して実測した上で公開する。
実測済み・未検証・不可の区別は `docs/coverage.md` にある。

### 変更点

- `cue` CLI（Rust）を追加。Connect RPC を JSON と proto の両方で扱える。
- 対話ログイン `cue login`（電話番号 + SMS コード）を追加。
- `cue phone start|verify|password|reset`、`cue mfa ...`、`cue join`、`cue send`、
  `cue watch`、`cue coverage`、`cue discover` を追加。
- `cue telecom`: 対応国・通話レート・無料電話設定・履歴の読み取り。- `cue webdev-deploy --project <id> --version <vid>`: WebDev プロジェクトを公開ドメインへデプロイする（外部公開を伴う明示コマンド）。- `cue webdev --detail <id>`: チェックポイント（versionId）を含む詳細。- `cue webdev [--limit n]`: WebDev プロジェクトの一覧（読み取り専用）。- `cue sandbox <session_id>`: 沙箱の sandboxId / 状態 / VNC / WebRTC の URL を読む（読み取り専用）。- `cue new --text`: セッション ID をローカル生成して新規セッションを開始する（サーバは ID を発行しない）。- `cue mcp`: stdio の MCP サーバー。`initialize` / `tools/list` / `tools/call` を実装し、11 ツールを公開する。遅いツールが読み取りループを塞がない。- Socket.IO（Engine.IO v4）クライアントを実装。`join_session` と `user_message` を送出し、
  `chat` / `chatDelta` / `statusUpdate` / `liveStatus` を受信する。

### 新機能

- RPC 目録 401 本（APK 由来 252 + 実行時発見 149）。出所を分けて 2 ファイルに記録。
- 404/401 オラクルによるルート存在判定（`cue discover`）。
- 実測に基づくプロトコル仕様 `docs/protocol.md`、認証状況 `docs/auth-status.md`、
  再現手順 `docs/probe-log.md`。

### 注意点

- **未検証（環境）**: Unix の 0700/0600 設定は Windows ホストのため未コンパイル・未実行（CI の Linux ジョブで実測される）。
- **未検証（機能）**: 沙箱の中の操作は通るが VNC の画面転送は未達（ブリッジが password を要求し、発行元が公開面に無い）、telecom の実番号契約、webdev のデプロイ、
  cloud_pc の生成、2FA の TOTP 登録（サーバが 400279 で拒否）、パスキー登録の完了（端末の認証器が要る）。
- 認証は電話 + SMS のみ実測済み。メールは Turnstile、Google は実トークンが前提で未検証。
- 本リポジトリは非公式の独立実装。サービスの規約は利用者自身の責任で確認すること。











### 配布物の実測

Release v0.1.0 に添付した 3 資産を取得して中身を確認した。

| 資産 | 中身 | 形式 |
|---|---|---|
| `cue-linux-x86_64.tar.gz` | `cue`（3,463,368 B）+ README + LICENSE + CHANGELOG | ELF64, machine 0x3e (x86-64) |
| `cue-macos-aarch64.tar.gz` | `cue`（3,018,080 B）+ README + LICENSE + CHANGELOG | Mach-O 64, cpu 0xc (arm64) |
| `cue-windows-x86_64.zip` | `cue.exe`（3,232,256 B）+ README + LICENSE + CHANGELOG | PE、`cue version` が exit 0 で `cue 0.1.0` |

Linux と macOS は開発ホスト（Windows）では実行できないため、形式の確認までで止めた。
Windows 版は展開後に実際に起動して `version` を確認している。
