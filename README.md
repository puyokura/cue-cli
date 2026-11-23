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

cue settings                  # 公開 RPC（認証不要）
cue region                    # 公開 RPC（認証不要）
cue rpc user.v1.UserPublicService/GetExchangeInfo
cue raw user.v1.UserPublicService/CheckRegion   # proto コーデック
cue watch --seconds 10        # Socket.IO を開いてフレームを観測
cue login --token <token>     # サービスが発行した Bearer トークン（Web 経由は非対応）
cue whoami
cue sessions --limit 20
cue send <session_id> --text "やあ"   # join_session + user_message を送出
```

`--base` もしくは `CUE_BASE_URL`、`CUE_WS_URL` で接続先を差し替えられる。

## 認証について（実測）

現時点で通るのは **ブラウザでの Google 同意 → セッショントークン** の経路のみ。
CLI からログインを完結させる経路は、Turnstile とサーバ固定 redirect_uri のため存在しない。
詳細と切り分けの全記録は `docs/auth-status.md`。

- 電話＋SMS: `403 / code 400232 "Phone authentication is not available for this account"`。
  電話番号の国・`X-Forwarded-For`・client-type/product ヘッダ・UA・Origin・Bearer 有無を
  変えても不変。サーバ側のアカウント状態で決まっている。
- メール＋パスワード: `Need CAPTCHA code!`（Cloudflare Turnstile）。
- メール新規登録: 検証コードが必要で、コード送信 RPC は公開されていない。
- Google ID トークン: `GoogleGSIAuth` は aud 検証まで到達する（実トークン必須）。
- `Oauth2Authorize` は 200 で同意 URL を返す。`redirect_uri` はサーバ固定で、
  任意の URL は `invalid auth redirect url` で拒否。

## 検証

```
python tests/run_smoke.py            # 25/25 PASS
cargo test --manifest-path crates/cue-cli/Cargo.toml   # 9/9 PASS
```

実バイナリを別プロセスで起動し、公開 RPC の応答、proto コーデック、
Socket.IO の 101 ハンドシェイクと名前空間拒否、不正トークンが永続化されないこと、
未認証時の終了コードを実測で確認する。

## ライセンス

AGPL-3.0-or-later。`LICENSE` を参照。

本リポジトリは非公式の独立実装であり、Manus / Butterfly Effect とは関係がない。
各商標はそれぞれの所有者に帰属する。サービスの利用には当該サービスの利用規約が適用される。
