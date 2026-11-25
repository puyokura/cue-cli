# CHANGELOG

## 0.1.1 — 2026-11-26

安全性と操作性の修正版。0.1.0 の `--confirm` は「動詞の一覧に載っているものだけ」を
止めていたため、一覧に無い書き込みが素通りしていた。判定を反転し、読み取りの一覧に
載っているもの以外はすべて状態変更として扱う。サービスが新しい動詞を追加しても、
コードを直さない限りゲートが漏れない。

### 変更点

- **`--confirm` の判定を反転**。`Get`/`List`/`Search`/`Page`/`Check`/`Query`/`View`/`Read`/
  `User` で始まるメソッドだけを読み取りとし、それ以外は `--confirm` を要求する。
  これで `ResetSSHPassword`、`Restart`、`AcceptInvite`、`VerifyEmail`、`SendCuePhoneCode`、
  `RegenerateRecoveryCodes` などが止まる（0.1.0 では止まらなかった）。
- **MCP `cue_rpc` も同じゲート**。状態変更メソッドは `"confirm": true` を要求する。
  0.1.0 では MCP 経由だけゲートを迂回でき、`instructions` の「迂回できない」という記述が
  実装と食い違っていた。
- **本文による判定を撤去**。`--json` の本文に `updatedAt` のような語が含まれるだけで
  読み取りが `--confirm` を要求し、しかも `--set` / `--json-file` / `--json -` では
  同じ本文が検査されなかった。判定はメソッド名だけにする。
- `phone reset` と `mfa codes` をゲート対象に追加（前者はパスワード変更、後者は
  リカバリコードの再発行で、印刷済みのコードを無効化する）。
- 値の無いフラグ（`--limit` だけ等）を「既定値で実行」ではなくエラーにする。
  `--json` は `doctor`/`status` ではスイッチ、`rpc` では値必須として扱う。
- `--state=DIR` をコマンド前でも受け付ける（`--state DIR` と同義。0.1.0 では
  `unknown command: --state=…` になっていた）。
- `doctor`/`status` が保存済みの `base` を尊重するようにした（0.1.0 は既定の接続先を
  点検していたため、`login --base` した環境では別ホストを検査していた）。
- `doctor` は失敗時に終了コード 1 を返す。`status` は状態ディレクトリを作らない。
- `doctor` の `state` 項目がトークン欠如を `warn` として報告する（常に `ok` では点検にならない）。
- 通信失敗・5xx は `identity` を `warn` とし、再ログインを促さない。4xx のみ `fail`。

### 新機能

- `doctor [--live] [--json]`：状態・認証・接続先を 6 項目で点検する。`--live` で実サービスまで往復。
- `status [--json]`：この CLI が認識している状態を返す。
- MCP ツール `cue_doctor` / `cue_status`、`initialize` の `instructions`、`ping` 応答。
- `rpc` が `--set key=value`、`--json-file PATH`、`--json -`（stdin UTF-8）を受ける。
  Windows PowerShell 5.1 が引用符を壊す環境でも本文を渡せる。
- `--flag=value` 形式。`--text='-- leading dashes'` のように値がダッシュで始まる場合に必要。
- 状態パスは `--state` > `CUE_STATE` > `CUE_MCP_STATE` > OS 既定。ディレクトリ指定とファイル指定の
  両方を受け、ファイル指定時はそのファイルをトークンとして扱う。
- `methods --list` が目録を全件返す。`methods` はホスト絶対パスを出さない。

### 注意点

- **`--confirm` の既定が厳しくなった**。読み取りの一覧に無い `rpc`/`raw` は
  `--confirm` なしで拒否される。`Get`/`List` 等で始まらない読み取り系が将来現れた場合は、
  `body.rs` の `READ_LEADERS` に追加する。判定を「読み取りの許可リスト」にしているのは、
  新しい書き込みが既定で止まる方向に倒すため。
- 未検証は 0.1.0 から変わらない：沙箱の VNC 画面転送、telecom の実番号契約、
  webdev 以外のデプロイ、cloud_pc の解除（`StripeCloudComputerCancel`）、TOTP 登録
  （サーバが 400279 で拒否）、パスキー登録の完了（端末の認証器が要る）、
  Unix の 0700/0600（Windows ホストのため未実行、CI で実測）。詳細は `docs/coverage.md`。
- `mfa codes` はリカバリコードを再発行する。`--confirm` を要求するが、実行すると
  印刷済みのコードは無効になる。

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










