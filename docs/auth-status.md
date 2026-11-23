# 認証の現状と、通すために必要なもの

2026-11-23 時点の実測。ここに書いた以外の経路は試していない。

## 通る経路: ブラウザの Google 同意のみ

`user.v1.UserAuthPublicService/Oauth2Authorize {"type":1}` が 200 で同意 URL を返す。

```
https://accounts.google.com/o/oauth2/auth
  ?access_type=offline
  &client_id=362133815767-ohl3m6958nbtn2j092984r9nakj4f5eg.apps.googleusercontent.com
  &include_granted_scopes=true
  &prompt=select_account
  &redirect_uri=https%3A%2F%2Fapi.manus.im%2Fapi%2Foauth2_callback%2Fgoogle
  &response_type=code
  &scope=openid .../userinfo.email .../userinfo.profile
  &state=<22 文字>
```

フロー:

1. `Oauth2Authorize` で同意 URL を得る（state 発行）
2. ブラウザで同意 → Google → `api.manus.im/api/oauth2_callback/google?code=…&state=…`
3. コールバックがセッションを確立し、`https://manus.im/...` へ 302 で戻す
4. そのブラウザセッションの `localStorage["sessionId"]` が Bearer トークン

実測した制約:

- `redirect_uri` はサーバ固定。`rea-cue://callback` や `https://example.com/cb` は
  `400 invalid auth redirect url` で拒否。相対パス `/auth_landing` のみ受理。
  → **CLI が直接コールバックを受ける経路は作れない。**
- state は使い捨て。`code=bogus&state=<valid>` は
  `302 https://manus.im/login?errMessage=We+couldn%27t+complete+sign-in+with+the+provider.`、
  `state=y` では `Your sign-in session has expired.` に 302。
- 署名付き `id_token`（`GoogleGSIAuth`）は audience 検証まで進む:
  `400 validate google id_token failed: idtoken: audience provided does not match aud claim`。

## 通らない経路と、その理由（すべて実測）

### 電話 + SMS（本人が先に調べたいと言った経路）

`SendCuePhoneCode` / `LoginCueByPhoneCode` / `LoginCueByPhonePassword` は
**403 permission_denied, 400232 "Phone authentication is not available for this account"**。

切り分けの結果、リクエスト側の要素はすべて無関係:

| 変えた軸 | 値 | 結果 |
|---|---|---|
| 電話番号の国 | JP/US/SG/GB/CN/IN/KR/TW/HK/DE | 不変 |
| `X-Forwarded-For` | JP/SG/US/GB/CN | 不変（`CheckRegion` は XFF を正しく反映） |
| `x-client-type` | agents/cue/CUE/mobile/android/flutter/ios/web/app/desktop | 不変 |
| `x-product-name` | cue / manus | 不変 |
| Origin | manus.im / cue.im | 不変 |
| User-Agent | okhttp / Cue | 不変 |
| Authorization | なし / 不正 Bearer | 不変 |
| 本文 | `authCommandCmd` / `userId` 追加 | 不変 |
| `purpose` | 1, 2（3 は enum 違反） | 1,2 とも 403 |

エラー詳細の protobuf は `field1=400232`, `field2="Phone authentication is not available for this account"`。

`CheckRegion` の proto を復号すると field 9 が **JP=3 / US=1 / SG=1** と異なる
（`privacyPageType` の差）。リージョン別設定が存在することの証拠だが、
電話ゲートがリージョンで開くかは**確認していない**。

APK 側にも `Phone authentication unavailable` という表示文字列があり、
`GetCuePhoneAuthInfo` という「電話認証の可否を問い合わせる」RPC が存在する
（要認証のため未実行）。

**結論: サーバ側のアカウント／機能フラグ。クライアントからは開放できない。
前進には本人のアカウント状態の確認（アプリ上の表示、サポート照会）が要る。**

### メール + パスワード

```
LoginByEmail {"email":..., "password":...}                    -> 400 1715 "Need CAPTCHA code!"
LoginByEmail {..., "captchaCode":"x"}                          -> 400 1015 "CAPTCHA verification failed"
LoginByEmail {..., "captcha":{"value":"x","type":"cloudflare"}} -> 400 1715（フィールド名が違う）
```

Cloudflare Turnstile。sitekey は Web バンドルに埋まっている
（prod: `0x4AAAAAAA_sd0eRNCinWBgU`、cue 側: `0x4AAAAAAA_s…`）。
Turnstile はブラウザでしか解けず、CLI からは通せない。

`x-client-dcr` は Web 実装から再現できる（`tools/dcr.py`）。
ただし CAPTCHA が先に要求されるため、dcr だけでは通らない。

### メール新規登録

```
RegisterByEmail {"email":...,"password":...,"verifyCode":"123456","name":...}
  -> 400 "incorrect verification code"
```

コード送信 RPC は公開されていない（`SendEmailVerifyCodeWithAuth` は要認証、
`SendXxx` 系の推測名はすべて 404）。登録は Web の Turnstile 付き画面が前提。

### Google ID トークン（GSI）

`GoogleGSIAuth` は aud 検証まで到達する。正規の Google サインインで得た
id_token が必要で、それ自体がブラウザ操作を要する。

## つまり

CLI から自前でログインを完結させる経路は、現時点で**存在しない**。
Turnstile とサーバ固定 redirect_uri がその理由で、どちらも迂回ではなく
サービス側の設計として閉じている。

通すための現実的な選択肢は 3 つ:

1. 本人がブラウザでログインし、`localStorage["sessionId"]` を `cue login --token` に渡す
2. ブラウザを CDP で操作する補助ツールを同梱し、同意とトークン取得を自動化する
   （Turnstile が通るかは未検証）
3. 電話認証の開放を待つ／サポートに確認する（サーバ側の状態変更が前提）
