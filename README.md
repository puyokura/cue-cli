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

## 使い方（CLI）

```
cargo build --release --manifest-path crates/cue-cli/Cargo.toml

# ログインは 1 プロセスで完結する（引数なしで対話入力）
crates/cue-cli/target/release/cue login
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
cue send <session_id> --text "やあ"   # join_session + user_message を送出
cue mfa status                # 2FA の有効状態
cue mfa totp-start            # 認証アプリ登録の開始（otpauthUri を返す）
cue mfa totp-finish --flow <id> --code <6桁>
cue discover user.v1.UserAuthService GetMfaStatus CreateTotp   # ルート存在判定
```

`--base` もしくは `CUE_BASE_URL`、`CUE_WS_URL` で接続先を差し替えられる。

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
python tests/run_smoke.py                               # 40/40 PASS
cargo test --manifest-path crates/cue-cli/Cargo.toml    # 12/12 PASS
```

実バイナリを別プロセスで起動し、公開 RPC の応答、proto コーデック、
Socket.IO の 101 ハンドシェイクと名前空間拒否、電話ゲートが開いていること、
ルート存在判定、不正トークンが永続化されないこと、未認証時の終了コードを実測で確認する。

## ライセンス

AGPL-3.0-or-later。`LICENSE` を参照。

本リポジトリは非公式の独立実装であり、Manus / Butterfly Effect とは関係がない。
各商標はそれぞれの所有者に帰属する。サービスの利用には当該サービスの利用規約が適用される。
