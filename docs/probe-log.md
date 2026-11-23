# 解析・probe 手順（再現用）

すべて実測。コマンドは実行した形のまま。値は観測値。

## 1. XAPK 展開と静的抽出

```powershell
Expand-Archive Cue+by+Manus_26.19.1_APKPure.xapk -DestinationPath work/xapk
# libapp.so は config.armeabi_v7a.apk の中
```

```
ai.manus.agents.apk      44,637,214 bytes  sha256 f563a27c…9440
config.armeabi_v7a.apk   75,499,077 bytes  sha256 78309fa8…b3b7
  lib/armeabi-v7a/libapp.so  48,398,920 bytes  sha256 fde12b80…e8ce
    snapshot hash 0451907c2eaa8467e848c0067bfe8ed4
  lib/armeabi-v7a/libflutter.so 8,625,652 bytes build id f7cb49be5c9d07cb8634eca1ba37ae02a019bc71
```

文字列抽出（重複除去・最小長 6）:

```powershell
python tools/extract_strings.py work/lib/libapp.so work/lib/libapp_strings.txt
python tools/extract_strings_offsets.py work/lib/libapp.so work/lib/libapp_strings_off.tsv
python tools/triage.py work/lib/libapp_strings_off.tsv work/inv
```

結果: 48,398,920 バイトから 178,837 本の ASCII run（重複除去後 141,675）。
`work/inv` に RPC 252 / REST 23 / URL 164 / host 62 / header 48 を分類出力。

## 2. RPC 到達性とフィールド復号

```powershell
python tools/harvest_rpc.py work/inv/rpc_paths.txt work/inv/rpc_schema.json
python tools/harvest_fields.py work/inv/rpc_schema.json work/inv/rpc_fields.tsv
```

252 本に `{}` を POST した結果:

| HTTP | 本数 | 意味 |
|---|---|---|
| 200 | 4 | 公開（`CheckRegion`, `GetGlobalSettings`, `GetExchangeInfo`, `Logout`） |
| 400 | 74 | バリデーション失敗（＝フィールド名が返る） |
| 401 | 171 | 認証必須 |
| 500 | 3 | 実装側の前提条件エラー（`filename is required` 等） |

`buf.validate.Violations` の base64 を protobuf として復号し、
`FieldPathElement.field_number` / `field_name` を取り出して 114 行の実フィールド表を得た（`docs/rpc-fields.tsv`）。
例: `RemoveAgentFromAgentGroup` → `group_id#1`, `agent_id#2`。

## 3. 電話認証ゲートの切り分け

```powershell
# 基本
Invoke-WebRequest -Method POST https://api.manus.im/user.v1.UserAuthPublicService/SendCuePhoneCode `
  -Body '{"phone_number":"+819012345678","purpose":1}' `
  -ContentType application/json -Headers @{'connect-protocol-version'='1'}
```

→ `403 {"code":"permission_denied","message":"Error<400232>","details":[{"type":"common.v1.ErrorDetail",...}]}`

エラー詳細を復号:

```powershell
python tools/walk_pb.py work/inv/phone_gate.bin
# field=1 wt=0 varint=400232
# field=2 wt=2 str='Phone authentication is not available for this account'
```

変化させても不変だった軸（各 1 回以上実測）:

| 軸 | 試した値 | 結果 |
|---|---|---|
| 電話番号の国 | JP/US/SG/GB/CN/IN/KR/TW/HK/DE | すべて 400232 |
| `X-Forwarded-For` | JP/SG/US/GB/CN | すべて 400232（`CheckRegion` は XFF を正しく反映するので XFF は評価されている） |
| `x-client-type` | agents/cue/CUE/mobile/android/flutter/ios/web/app/desktop | すべて 400232 |
| `x-product-name` | cue / manus | すべて 400232 |
| `Origin` | https://manus.im / https://cue.im | すべて 400232 |
| `User-Agent` | okhttp/4.12.0 / Cue/26.19.1 | すべて 400232 |
| `Authorization` | なし / 不正 Bearer | どちらも 400232（トークン評価より前） |
| 本文 | `authCommandCmd` / `userId` 追加 | すべて 400232 |
| `purpose` | 1, 2 は定義済み / 3 は enum 違反 | 1,2 は 400232、3 は 400 バリデーション |

対照実験（`CheckRegion` は XFF に応答）:

```
XFF 126.71.1.1 -> iso JP code +81
XFF 8.8.8.8    -> iso US code +1
XFF 85.214.132.117 -> iso DE code +49
XFF 114.114.114.114 -> isUnsupportedV2 true, iso CN
```

## 4. 認証経路の分類

```powershell
# メール
POST /user.v1.UserAuthPublicService/LoginByEmail {"email":"probe@example.com","password":"xxxxxxxxxxxx"}
# -> 400 invalid_argument "Error<1715> Need CAPTCHA code!"

POST /user.v1.UserAuthPublicService/RegisterByEmail {"email":"...","password":"...","verifyCode":"123456","name":"probe"}
# -> 400 "incorrect verification code"

# Google
POST /user.v1.UserAuthPublicService/GoogleGSIAuth {"credential":"x"}
# -> 400 "idtoken: invalid token, token must have three segments; found 1"
POST /user.v1.UserAuthPublicService/GoogleAppOauthCallback {"accessToken":"x"}
# -> 401 "failed to verify google token: ... google access token is invalid"

# OAuth 同意 URL
POST /user.v1.UserAuthPublicService/Oauth2Authorize {"type":1}
# -> 200 {"redirectUrl":"https://accounts.google.com/o/oauth2/auth?...&redirect_uri=https%3A%2F%2Fapi.manus.im%2Fapi%2Foauth2_callback%2Fgoogle&response_type=code&scope=openid+...&state=<22chars>"}
```

`redirectUrl` に任意 URL を渡すと拒否される:

```
{"type":1,"redirectUrl":"rea-cue://callback"}    -> 400 "invalid auth redirect url"
{"type":1,"redirectUrl":"https://example.com/cb"} -> 400 "invalid auth redirect url"
{"type":1,"redirectUrl":"/auth_landing"}          -> 200（相対パスのみ受理）
```

コールバックの `state` 検証:

```
GET /api/oauth2_callback/google?code=bogus&state=<valid>  -> 302 https://manus.im/login?errMessage=We+couldn%27t+complete+sign-in+with+the+provider.
GET /api/oauth2_callback/google?code=x&state=y            -> 302 https://manus.im/login?errMessage=Your+sign-in+session+has+expired.+Please+sign+in+again.
```

## 5. WebSocket

```powershell
python tools/probe_ws.py api.manus.im /ws /event /events /socket /session/ws
# すべて 404 {"error_msg":"404 Route Not Found"}

python tools/probe_ws.py api.manus.im '/socket.io/?EIO=4&transport=websocket'
# 101 Switching Protocols

python tools/socketio_probe.py api.manus.im '/socket.io/?EIO=4&transport=websocket' --send '40{"token":"invalid"}'
# RECV 0{"sid":"...","pingInterval":15000,...}
# RECV 44{"message":"Invalid Token"}
```

## 6. REST

```
POST https://api.manus.im/api/chat/getSessionV2  {}                       -> 404 {"status":"not found"}
GET  https://api.manus.im/api/chat/getSessionV2?sessionId=x              -> 404 {"error":{"message":"session not found","name":"NotFoundError"}}
GET  https://api.manus.im/api/chat/listMainModelCandidates               -> 401 {"name":"http_exception","message":"Unauthorized"}
```

Web バンドル（`https://manus.im/login` の `/_next/static/chunks/*.js` 63 本）から
route + method + zod request schema を抽出 → `docs/rest-routes.txt`。

## 7. CLI 実測

```powershell
cargo build --manifest-path crates/cue-cli/Cargo.toml
python tests/run_smoke.py        # 18/18 PASS
```
