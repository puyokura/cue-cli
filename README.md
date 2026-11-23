# rea-cue

Cue（`ai.manus.agents`）の API を独立クライアントから扱うための解析成果物と CLI。

このリポジトリは、市販アプリのクライアント／自動化を自作する目的で、
公開 APK（Cue+ by Manus 26.19.1）を静的解析し、実サービスへのリクエストで
観測した事実だけを記録する。推測は「未確定」と明記する。

## 収録物

| パス | 内容 |
|---|---|
| `docs/protocol.md` | 転送方式・認証・RPC/REST/WebSocket の実測仕様 |
| `docs/auth-status.md` | 認証経路ごとの実測結果と、通らない理由 |
| `docs/probe-log.md` | 全 probe の再現手順と観測値 |
| `docs/rpc-inventory.txt` | 静的抽出した RPC 252 本の全数 |
| `docs/rpc-fields.tsv` | サーバのバリデーション応答から復号した実フィールド名 |
| `docs/rest-routes.txt` | Web クライアントの REST route + method + request schema |
| `crates/cue-cli/` | Rust 製 CLI（Connect RPC の JSON / proto、Socket.IO チャット） |
| `tools/` | 静的解析・実サービス probe 用スクリプト（Python, 標準ライブラリのみ） |
| `tests/run_smoke.py` | 実サービスに対する CLI の実プロセス検証 |
| `tests/run_live.py` | ログイン後の認証付き検証（未認証なら即座に失敗する） |
| `CHANGELOG.md` | 実測済み／未検証の範囲の記録 |
| `docs/coverage.md` | 機能ごとの実測状態（実測済み／未検証／不可） |

## 使い方（CLI）

```
cargo build --release

# ログインは 1 プロセスで完結する（引数なしで対話入力）
target/release/cue login
#   phone number (E.164, e.g. +819012345678): <番号を入力>
#   SMS code: <届いた 6 桁を入力>
#   -> トークンを検証してから保存し、user を表示する

cue settings                  # 公開 RPC（認証不要）
cue region                    # 公開 RPC（認証不要）
cue rpc user.v1.UserPublicService/GetExchangeInfo
cue raw user.v1.UserPublicService/CheckRegion   # proto コーデック
cue watch --seconds 10        # Socket.IO を開いてフレームを観測

cue phone start --number +8190...   # SMS コード送信（verificationId を返す）
cue phone verify --id <v> --code <c>  # コードでログインしトークンを保存
cue phone password --number +8190...  # パスワードでログイン（stdin から読む）

cue whoami
cue sessions --limit 20
cue new --text "やあ"         # 新規セッション（ID はクライアントが生成）
cue sandbox <session_id>      # 沙箱の状態と VNC/WebRTC の URL（読み取り専用）
cue telecom                   # 対応国・通話レート・無料電話設定（読み取り専用）
cue webdev [--limit n]        # WebDev プロジェクト一覧（読み取り専用）
cue webdev --detail <id>      # チェックポイント（versionId）を含む詳細
cue webdev-deploy --project <id> --version <vid>   # 公開ドメインへデプロイ（外部公開）
cue send <session_id> --text "やあ"   # 既存セッションに join_session + user_message
cue mfa status                # 2FA の有効状態
cue mfa totp-start            # 認証アプリ登録の開始（otpauthUri を返す）
cue mfa totp-finish --flow <id> --code <6桁>
cue discover user.v1.UserAuthService GetMfaStatus CreateTotp   # ルート存在判定
```

接続先は `--base`、`CUE_BASE_URL`、`CUE_WS_URL` で差し替えられる。

### MCP サーバー

`cue mcp` は stdio で動く MCP サーバーになる。`initialize` / `tools/list` / `tools/call` を実装し、
ツールは CLI の非対話コマンドをそのまま公開する（`cue_region`、`cue_whoami`、`cue_sessions`、
`cue_session`、`cue_agents`、`cue_rpc`、`cue_send`、`cue_join`、`cue_coverage`、`cue_mfa_status`、
`cue_discover`）。

```
{
  "mcpServers": {
    "cue": { "command": "<repo>/target/release/cue", "args": ["mcp"] }
  }
}
```

各 `tools/call` は別スレッドで走る。遅いツール（`cue_join` など）の実行中でも、
後続の軽いツールは待たされずに応答する（`tests/mcp_probe.py` で実測）。

### 状態ファイル

トークンは OS 標準の設定ディレクトリに置く。

| OS | 保存先 |
|---|---|
| Windows | `%APPDATA%\cue-cli\` |
| macOS / Linux | `$XDG_CONFIG_HOME/cue-cli/`（未設定なら `~/.config/cue-cli/`） |

上書きは `--state <dir>` または環境変数 `CUE_STATE`。書き込みは一時ファイル + rename で行い、
Unix ではディレクトリ `0700`・ファイル `0600` に設定する（この実装は Linux で未実行。
CI の Linux ジョブで初めて実測される）。トークンの値は標準出力・エラー・`--json` の
いずれにも出さない（存在と長さのみ）。

## 認証について（実測）

ネイティブ経路（アプリ本体と同じ）のみ。ブラウザを介したログインは使わない。

- 電話 + SMS / 電話 + パスワードは **通る**。ただし `x-product-name: manus-agents` が必須で、
  これが無いと `403 / 400232` で Manus 系の面に落ちる（CLI は常に送る）。
- 2FA は TOTP とパスキーの両方が存在する（`user.v1.UserAuthService` 配下、要認証）。
- メールは Cloudflare Turnstile 必須。Google は実 id_token / access_token 必須。
- 実 SMS でのログイン完走は未実施（電話番号とコードが要る）。

詳細と切り分けの全記録は `docs/auth-status.md`、RPC の実測仕様は `docs/protocol.md`。

## 検証

```
cargo build --release          # ルートからビルドできる（ワークスペース）
cargo test                     # 21/21 PASS（オフライン）
python tests/run_smoke.py      # 68/68 PASS（実サービス、認証不要の範囲）
python tests/run_live.py       # 17 PASS / 2 SKIP / 0 FAIL（要ログイン、読み取りのみ）
python tests/mcp_probe.py      # 遅いツールが読み取りループを塞がないこと
```

`run_live.py` は既定で**読み取りのみ**。`--create-session` と `--send` を付けると、
署名済みアカウントに実際のセッションや投稿が残る。アカウントを汚したくないときは付けない。

`run_smoke.py` は実バイナリを別プロセスで起動し、公開 RPC の応答、proto コーデック、
Socket.IO の 101 ハンドシェイクと名前空間拒否、電話ゲートが開いていること、ルート存在判定、
未知フラグの拒否、help/version の終了コード、MCP の 3 メソッド、
トークンが出力に現れないこと、不正トークンが永続化されないことを実測で確認する。

## リリース

タグ `v*` を push すると `.github/workflows/release.yml` が動き、Linux x86_64 / Windows x86_64 /
macOS aarch64 の 3 バイナリをビルドして同じタグに添付する。テストはホストターゲットでのみ実行し、
クロスビルドした成果物を「テスト済み」とは書かない。チェックサムは別添しない（ホスティング側が
各資産の digest を表示する）。

タグ作成と公開は依頼があって初めて行う。このリポジトリはまだタグを持っていない。

## ライセンス

AGPL-3.0-or-later。全文は `LICENSE`、各ソースファイルの先頭に
`SPDX-License-Identifier: AGPL-3.0-or-later` を置いている。

- 本リポジトリのコード・文書・解析ツールはすべて同じ条件で利用できる。
- AGPL 第 13 条に従い、ネットワーク越しに提供する改変版は対応するソースを利用者に提供すること。
- 本リポジトリは非公式の独立実装であり、Manus / Butterfly Effect とは関係がない。
- 各商標はそれぞれの所有者に帰属する。サービスの利用には当該サービスの利用規約が適用される。
- 同梱・参照している第三者コンポーネントのライセンスは `Cargo.lock` の各クレートに従う
  （本リポジトリのコードは MIT/Apache-2.0 のクレートを依存として使うのみで、取り込みはしていない）。















