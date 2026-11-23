# CHANGELOG

## 0.1.0 — 未公開

まだタグもリリースも作っていない段階の記録。公開は全機能の実測が終わった後に行う。

### 変更点

- `cue` CLI（Rust）を追加。Connect RPC を JSON と proto の両方で扱える。
- 対話ログイン `cue login`（電話番号 + SMS コード）を追加。
- `cue phone start|verify|password|reset`、`cue mfa ...`、`cue join`、`cue send`、
  `cue watch`、`cue coverage`、`cue discover` を追加。
- `cue mcp`: stdio の MCP サーバー。`initialize` / `tools/list` / `tools/call` を実装し、11 ツールを公開する。遅いツールが読み取りループを塞がない。- Socket.IO（Engine.IO v4）クライアントを実装。`join_session` と `user_message` を送出し、
  `chat` / `chatDelta` / `statusUpdate` / `liveStatus` を受信する。

### 新機能

- RPC 目録 401 本（APK 由来 252 + 実行時発見 149）。出所を分けて 2 ファイルに記録。
- 404/401 オラクルによるルート存在判定（`cue discover`）。
- 実測に基づくプロトコル仕様 `docs/protocol.md`、認証状況 `docs/auth-status.md`、
  再現手順 `docs/probe-log.md`。

### 注意点

- **未検証（環境）**: Unix の 0700/0600 設定は Windows ホストのため未コンパイル・未実行（CI の Linux ジョブで実測される）。
- **未検証（機能）**: 沙箱の起動と操縦、telecom の実番号契約、webdev のデプロイ、
  cloud_pc の生成、2FA の TOTP 登録（サーバが 400279 で拒否）、パスキー登録の完了（端末の認証器が要る）。
- 認証は電話 + SMS のみ実測済み。メールは Turnstile、Google は実トークンが前提で未検証。
- 本リポジトリは非公式の独立実装。サービスの規約は利用者自身の責任で確認すること。



