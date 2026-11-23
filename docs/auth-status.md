# 認証: ネイティブ経路（Web 経由は不採用）

方針: **CLI の認証はアプリ本体と同じネイティブ経路で完結させる。ブラウザ・Web セッションのトークン流用は使わない。**
（本人指示 2026-11-23: 「CLIでweb経由のログインは嫌いだ」。ブラウザで同意させる補助ツールは削除済み。）

## 訂正: 電話認証は「閉じていない」

以前この文書に「サーバ側のアカウントフラグで閉じており、クライアントからは開放できない」と書いた。**これは誤りだった。撤回する。**

原因は `x-product-name` ヘッダ。付けずに呼ぶと Manus 系のプロダクト面に落ち、
`403 / 400232 "Phone authentication is not available for this account"` が返る。
Cue（`ai.manus.agents`）の値 `manus-agents` を付けると、同じリクエストが
Cue 面に入り、通常の検証に進む。

実測（同一の本文・同一の番号、ヘッダのみ差分）:

| リクエスト | `x-product-name` なし | `x-product-name: manus-agents` |
|---|---|---|
| `SendCuePhoneCode {"phone_number":"+","purpose":1}` | 403 / 400232 | **400 / 400230「国コード付きの有効な番号を入力」** |
| `SendCuePhoneCode {"phone_number":"+819012345678","purpose":1}` | 403 / 400232 | **200 `{verificationId, expiresInSeconds:299, retryAfterSeconds:60}`** |
| `LoginCueByPhoneCode`（偽 ID + 偽コード） | 403 / 400232 | **400 / 1013「Incorrect verification code」** |
| `LoginCueByPhonePassword`（不正な番号） | 403 / 400232 | **400 / 400230** |
| `ResetCuePhonePassword`（偽 ID + 偽コード） | 403 / 400232 | **400 / 1013** |

## 検証とゲートの順序（実測）

```
1. リクエスト検証 (buf.validate)   -> 空本文なら 400 + 「phone_number: must be at least 1 characters」
2. プロダクトゲート                -> x-product-name が Cue でなければ 403 / 400232
3. 業務検証                        -> 番号形式なら 400 / 400230、コード不一致なら 400 / 1013
```

1 が先に走るため、空本文を送る全数ハーベスト（252 本）では
400232 と 400230 の差が出ない。ゲートを可視化するには
「min_len は満たすが形式が不正」な本文が必要だった。

## レート制限

`SendCuePhoneCode` は 1 番号あたり 60 秒間隔（応答の `retryAfterSeconds`）。
超えると `429 / 400233 "Too many attempts. Please try again later"`。
ハーベスト中の連投で実際に踏んだ。

## ネイティブアプリが備えるログイン手段

```
Continue with Google / Apple / Email / phone / Manus / Microsoft / Singpass
```

## 経路ごとの現状

| 経路 | RPC | 状態 |
|---|---|---|
| 電話 + SMS コード | `SendCuePhoneCode` → `LoginCueByPhoneCode` | **通る**（プロダクトヘッダ必須。実コードでの完走は未実施） |
| 電話 + パスワード | `LoginCueByPhonePassword` | **通る**（同上） |
| パスワード再設定 | `ResetCuePhonePassword` | 検証まで到達 |
| メール + パスワード | `LoginByEmail` | Turnstile 必須（`captchaCode` フィールド） |
| メール新規登録 | `RegisterByEmail` | 検証コードが必要。送信 RPC は非公開 |
| Google（ID トークン） | `GoogleGSIAuth` | aud 検証まで到達。期待 aud は Web 型クライアント ID |
| Google（アクセストークン） | `GoogleAppOauthCallback` | 実 access_token 必須 |
| Apple / Microsoft / SSO / Facebook | `Oauth2Authorize type=2,3,4,5` | Apple は同意 URL を返す。Microsoft は版要求。SSO は組織ドメインのみ。Facebook は廃止 |

## 電話ログインの CLI 実装

`crates/cue-cli/src/phone.rs` と `commands::phone`。

```
cue phone start --number +8190... [--purpose login|reset_password]
  -> {"verificationId": "...", "expiresInSeconds": 299, "retryAfterSeconds": 60}

cue phone verify --id <verificationId> --code <sms code>
  -> トークンを取り、UserInfo で検証してから保存

cue phone password --number +8190...      （パスワードは stdin から読む）
cue phone reset --id <verificationId> --code <sms code>
```

トークンは保存前に `user.v1.UserService/UserInfo` で検証する。
不正トークンを残さない方針は `login --token` と共通。

## 未実施（本人の電話番号とコードが要る）

- 実番号への SMS 送信と、届いたコードでの `LoginCueByPhoneCode` 完走
- 上記で得たトークンを使った認証付き RPC（セッション一覧・送信・telecom 等）
- 2FA の有無と方式（ログイン応答に `loginMfaRequired` 相当のイベントがあるかは未確認）

実番号を使うか、パスワード経路で先に通すかは本人の判断。
