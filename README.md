# rea-cue

Cue（`ai.manus.agents`）の API を独立クライアントから扱うための解析成果物と CLI。

このリポジトリは、市販アプリのクライアント／自動化を自作する目的で、
公開 APK（Cue+ by Manus 26.19.1）を静的解析し、実サービスへのリクエストで
観測した事実だけを記録する。推測は「未確定」と明記する。

## 収録物

| パス | 内容 |
|---|---|
| `docs/protocol.md` | 転送方式・認証・RPC/REST/WebSocket の実測仕様 |
| `docs/rpc-inventory.txt` | 静的抽出した RPC 252 本の全数 |
| `docs/rpc-fields.tsv` | サーバのバリデーション応答から復号した実フィールド名 |
| `docs/rest-routes.txt` | Web クライアントの REST route + method + request schema |
| `crates/cue-cli/` | Rust 製 CLI（Connect RPC の JSON / proto 両対応） |
| `tools/` | 静的解析・実サービス probe 用スクリプト（Python, 標準ライブラリのみ） |
| `tests/run_smoke.py` | 実サービスに対する CLI の実プロセス検証 |

## 使い方（CLI）

```
cargo build --release --manifest-path crates/cue-cli/Cargo.toml

cue settings                  # 公開 RPC（認証不要）
cue region                    # 公開 RPC（認証不要）
cue rpc user.v1.UserPublicService/GetExchangeInfo
cue raw user.v1.UserPublicService/CheckRegion   # proto コーデック
cue login --token <sessionId> # Web クライアントの localStorage["sessionId"]
cue whoami
cue sessions --limit 20
```

`--base` もしくは `CUE_BASE_URL` で接続先を差し替えられる。

## 認証について（実測）

現時点で通るのは **ブラウザでの Google 同意 → セッショントークン** の経路のみ。

- 電話＋SMS: サーバが `403 / code 400232 "Phone authentication is not available for this account"` を返す。
  電話番号の国・`X-Forwarded-For`・client-type/product ヘッダ・UA・Origin を変えても不変。
- メール＋パスワード: `Need CAPTCHA code!`（`x-client-dcr` のデバイスチェックが必要）。
- Google ID トークン: `GoogleGSIAuth` は実トークンを要求。
- `Oauth2Authorize` は 200 で同意 URL を返す。`redirect_uri` はサーバ固定で、任意の URL は `invalid auth redirect url` で拒否。

詳細と実測ログは `docs/protocol.md` を参照。

## 検証

```
python tests/run_smoke.py
```

実バイナリを別プロセスで起動し、公開 RPC の応答、proto コーデック、
不正トークンが永続化されないこと、未認証時の終了コードを実測で確認する。

## ライセンス

AGPL-3.0-or-later。`LICENSE` を参照。

本リポジトリは非公式の独立実装であり、Manus / Butterfly Effect とは関係がない。
各商標はそれぞれの所有者に帰属する。サービスの利用には当該サービスの利用規約が適用される。
