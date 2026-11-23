# Cue (ai.manus.agents) プロトコル仕様 — 実測版

対象ビルド: `ai.manus.agents` versionName 26.19.1 / versionCode 447（XAPK, armeabi-v7a スプリット）
実測日: 2026-11-23 / 実測ホスト: このワークステーション（Windows, Asia/Tokyo）
本書の数値・文字列はすべて、同梱 APK の静的抽出か `api.manus.im` への実リクエストで得た観測値。推測は「未確定」と明記する。

## 1. 転送

| 項目 | 実測値 |
|---|---|
| サービス原点 | `https://api.manus.im` |
| RPC 形式 | Connect（gRPC 互換）。`POST /<package>.<Service>/<Method>` |
| 必須ヘッダ | `connect-protocol-version: 1` |
| unary JSON | `content-type: application/json` / `accept: application/json` |
| unary proto | `content-type: application/proto`（応答もバイナリ proto） |
| ストリーミング | `application/connect+json` / `application/connect+proto`（クライアント実装から観測） |
| エラー | 本文 `{"code":"...","message":"..."}`。HTTP 400/401/403/500 に写像 |
| レート制限 | `X-RateLimit-Limit` / `-Remaining` / `-Reset` を応答ヘッダで返す |
| ゲートウェイ | `Server: APISIX/3.11.0` |

未認証時に返る代表例（すべて実測）:

```
POST /user.v1.UserService/UserInfo            -> 401 unauthenticated "missing authorization header"
POST /user.v1.UserService/UserInfo  (Bearer x) -> 401 unauthenticated "invalid token"
POST /user.v1.UserPublicService/CheckRegion   -> 200 {"iso":"JP","code":"+81",...}
```

## 2. 認証ヘッダ

Web クライアント実装（`manus.im` の Next.js バンドル）で確認した形:

```
authorization: Bearer <token>
x-client-type: <platform>
x-client-version: <version>
x-client-id: <device id>
x-client-locale: <locale>
x-client-timezone: <IANA tz>
x-client-timezone-offset: <文字列>
x-product-name: <product>
```

トークンは Web 版では `localStorage["sessionId"]` に保存され、`Bearer` で送られる。
本 CLI は同じ値を受け取り、`authorization: Bearer` で送る（`cue login --token`）。

## 3. RPC 面

全数は 2 ファイルに分かれている。出所が違うので混ぜない。

- `docs/rpc-inventory.txt` — APK の文字列から静的に抽出した **252 本**
- `docs/rpc-discovered.txt` — 実行時の存在判定で静的全数の外側に足した **149 本**

合計 401 本が名前として確定している。ただしこれは「名前が分かっている数」であって
「サーバに実在する全ルート数」ではない（10 節参照）。

静的分の内訳（実測で到達性を確認したものは「未認証での挙動」列）:

| サービス | 本数 | 未認証での挙動 |
|---|---|---|
| `user.v1.UserPublicService` | 3 | 200（公開情報のみ） |
| `user.v1.UserAuthPublicService` | 8 | 400/401/403（下記 4 節） |
| `user.v1.UserService` | 16 | 401 |
| `user.v1.UserAuthService` | 20 | 401 |
| `session.v1.*` | 20 | 401 |
| `agents.v1.*` | 20 | 400（バリデーション）/401 |
| `telecom.v1.TelecomService` | 27 | 401 |
| `cloud_pc.v1.*` | 15 | 400/401 |
| `skill.v1.*` | 6 | 401 |
| `connectors.v1.*` | 5 | 401 |
| `notifier.v1.*` | 3 | 400/200 |
| その他（space/team/webdev/device_authorization ほか） | 109 | 401 |

実行時発見分の内訳:

| サービス | 本数 |
|---|---|
| `webdev.v1.WebDevService` | 34 |
| `user.v1.UserService` | 32 |
| `user.v1.UserAuthService` | 19 |
| `session.v1.SessionService` | 19 |
| `session.v1.ProjectService` | 12 |
| `connectors.v1.ConnectorsService` | 9 |
| `session.v1.SessionCollaborateService` | 7 |
| `cloud_pc.v1.CloudPCService` | 6 |
| `device_authorization.v1.DeviceAuthorizationService` | 4 |
| `user.v1.UserAuthPublicService` | 4 |
| その他（space/team/telecom/file/skill ほか） | 9 |

## 4. 認証経路の実測結果

| 経路 | RPC | 実測 |
|---|---|---|
| 電話 + SMS コード | `SendCuePhoneCode` | **403 permission_denied, code 400232 "Phone authentication is not available for this account"** |
| 電話 + パスワード | `LoginCueByPhonePassword` | 同上 403 |
| 電話コード検証 | `LoginCueByPhoneCode` | 同上 403 |
| メール + パスワード | `LoginByEmail` | 400 `Error<1715> Need CAPTCHA code!` |
| メール登録 | `RegisterByEmail` | 400 `incorrect verification code`（コードが必要） |
| Google ID トークン | `GoogleGSIAuth` | 400 `idtoken: invalid token, token must have three segments` |
| Google アクセストークン | `GoogleAppOauthCallback` | 401 `native oauth token is not trustworthy: google access token is invalid` |
| Microsoft | `MicrosoftOauth2Callback` | 400 failed_precondition "This sign-in method requires a newer version" |
| OAuth 同意 URL 生成 | `Oauth2Authorize` | **200**（Google/Apple/Microsoft の同意 URL を返す） |
| Apple | `Oauth2Authorize type=2` | 200 |
| Facebook | `Oauth2Authorize type=5` | 400 `META_LOGIN_DISCONTINUED` |

### 4.1 電話認証ゲートの切り分け（実測マトリクス）

結論: **サーバ側のアカウント／機能フラグで閉じており、クライアント側から開放できない。**

次の全組合せで 403 / 400232 が不変:

- 電話番号 10 か国（JP/US/SG/GB/CN/IN/KR/TW/HK/DE）— すべて同一エラー
- `X-Forwarded-For` を JP/SG/US/GB/CN に差し替え — 不変（`CheckRegion` は XFF に応じて正しく国を返すため、XFF は実際に評価されている）
- `x-client-type` を agents/cue/CUE/mobile/android/flutter/ios/web/app/desktop の 10 種 — 不変
- `x-product-name` を cue / manus — 不変
- `Origin: https://manus.im` / `https://cue.im` — 不変
- `User-Agent: okhttp/4.12.0` / Cue/26.19.1 — 不変
- `Authorization: Bearer <不正トークン>` 付き — 不変（電話系はトークン有無より前に 403）
- 本文に `authCommandCmd` / `userId` を追加 — 不変
- `purpose` は 1 と 2 が定義済み（3 は `enum.defined_only` 違反で 400）

エラー詳細（`common.v1.ErrorDetail` の base64 protobuf）を復号すると
`field1 varint=400232`, `field2 "Phone authentication is not available for this account"`。
つまり専用のエラーコードであり、汎用の権限エラーではない。

電話側の表示文字列も APK に存在する（`Phone authentication unavailable`）。
アプリ側は「このアカウントでは使えない」を前提に UI を出している。

### 4.2 開放条件として残る仮説（未確定）

実測で否定できなかったものだけを挙げる。いずれも**確認していない**。

1. 電話ログインは特定リージョンのテナントでのみ有効（`CheckRegion` の field 9 が JP=3 / US=1 と異なるため、リージョン別の設定差は存在する）。
2. 電話ログインは招待制／ウェイトリストで、アカウント側にフラグが立っていない。
3. 電話ログインは Cue アプリ（`tech.butterfly.app`）系列のアカウントだけに開放され、`ai.manus.agents` の新規アカウントには無効。

いずれもサーバ側の状態であり、クライアントから観測できる材料はこの 403 だけ。**アカウントを所有する本人の操作（アプリ上での設定確認、サポートへの問い合わせ）以外に前進手段はない。**

### 4.3 現に使える経路

`Oauth2Authorize` が 200 で同意 URL を返すため、**ブラウザでの Google 同意 → `api.manus.im/api/oauth2_callback/google` → セッショントークン**が唯一の実用的経路。

- 同意 URL には `state`（22 文字）が付く。`state` はサーバ発行で有効期限あり（不正 state では `Your sign-in session has expired` に 302）。
- コールバックは `redirect_uri=https://api.manus.im/api/oauth2_callback/google` 固定。**任意の redirect_uri（カスタムスキーム・外部ドメイン）は `invalid auth redirect url` で拒否**（実測）。
- したがってトークンは「ブラウザセッション内」で受け取る形になり、CLI へは本人が値を渡す必要がある。

## 5. REST 面

`/api/chat/*` などは `api.manus.im` 上に存在する（未認証 401、存在しない ID で 404 `session not found`）。
Web バンドルから route + method + request schema を 33 本抽出済み（`docs/rest-routes.txt`）。
主なもの: `getSessionV2`(GET), `getSessionViews`(POST), `getSessionOutline`(GET), `deleteSession`(POST),
`getPresignedUploadUrl`(POST), `uploadComplete`(POST), `scheduleTask`(POST), `listMainModelCandidates`(GET), `startSandbox`(POST)。

## 6. WebSocket

`wss://api.manus.im` + **Socket.IO (Engine.IO v4)**, path `/socket.io`。

実測ハンドシェイク（未認証）:

```
GET /socket.io/?EIO=4&transport=websocket  -> 101
S: 0{"sid":"...","upgrades":[],"pingInterval":15000,"pingTimeout":20000,"maxPayload":2097152}
C: 40{"token":"..."}
S: 44{"message":"Invalid Token"}
```

接続オプション（Web 実装で確認）: `withCredentials:true, reconnection:false, auth:{token}, query:{locale,tz,clientType,clientVersion,"x-product-name"}, transports:["websocket"]`。

### 6.1 フレーム形式

Engine.IO プレフィックス `0` open / `1` close / `2` ping / `3` pong / `4` message、
Socket.IO プレフィックス `0` connect / `1` disconnect / `2` event / `3` ack / `4` error。

イベントは `42["<event>", <payload>]`。**キーは snake_case**。
Web クライアントは送信時に `convertObjectNames("server", …)` で camelCase→snake_case、
受信時に `convertObjectNames("client", …)` で snake_case→camelCase に変換する。
つまりワイヤ上は常に snake_case。

### 6.2 イベント（Web バンドルから抽出した全数）

送信（19 種）: `user_message`, `join_session`, `leave_session`, `sync_sessions`,
`change_task_mode`, `message_queue_action`, `side_chat_request`, `follow_up_end`,
`addon_widget_notify`, `watch_session_pull_requests`, `check_git_sync_status`,
`create_git_repo`, `kill_cascade_job`, `kill_cascade_job_group`,
`set_workspace_working_directory`, `switch_deploy_version`,
`confirm_connector_re_oauth`, `resume_map_reduce`

受信（応答の対応表）:

| 送信 type | 待つ受信イベント |
|---|---|
| `user_message` | `chat`（`type:"chat"`, `sender:"user"`, 同 `id`） |
| `join_session` | `eventsNotifyEventsAfter` |
| `change_task_mode` | `taskModeChanged` |
| `change_queue` | `queueStatusChange` |
| `sync_sessions` | `sessions_synced` |
| `stop_session` | `userStop` |
| `start_sandbox` | `sandboxUpdate` / `sandboxLimitReached` |
| `turn_preparation_action` | `turnPreparationStatus` |
| `retry_upload` | `userRetryUploadResult` |
| `switch_deploy_version` | `deploySwitchVersionResult` |

受信フレームの外形は `{"type":"event","session_id":...,"event":{...}}`
または `{"type":..., "session_id":..., ...}`。エージェントの失敗は `type:"agent_error"` +
`code`/`message`。

### 6.3 プロンプト送信の実ペイロード（Web 実装から）

```json
{"type":"user_message","id":"<22桁>","session_id":"<sid>","timestamp":1732…,
 "message_type":"text","task_mode":"standard",
 "content":"<本文>","contents":[{"type":"text","value":"<本文>"}],
 "attachments":[],"delivery_mode":"interrupt","country_iso_code":"JP",
 "ext_data":{"mode":"…","maxEffort":"…"}}
```

`task_mode` は `standard` / `max` / `lite` / `flex`（アカウント種別で変わる）。
`delivery_mode` は `interrupt`（既定）ほか `queue`。

セッション購読:

```json
{"type":"join_session","session_id":"<sid>","last_message_id":"new_session",
 "version":2,"session_view":{"version":…,"from":…}}
```

`sender` の値は `user` / `agent`。`chat` イベントに `content` / `delta` が入る。

## 7. 未確定事項

- 認証後の `join_session` 応答（`eventsNotifyEventsAfter`）の内部構造
- メッセージ送信が socket のみか、REST の併用が必要か
- `x-client-dcr` の Turnstile 連携部分（`fgRequestId` に何が入るか）
- `telecom.v1`（電話番号・SMS・通話）の実挙動（要認証）

## 8. 実装状況

`crates/cue-cli` は上記のうち、公開 RPC・proto コーデック・Bearer 認証・
Socket.IO ハンドシェイク・`join_session` / `user_message` の送出までを実装済み。
認証後の応答処理は実トークンが必要なため未検証（`docs/auth-status.md`）。

## 9. 2FA（MFA）の実測

アプリはログイン応答が MFA 要求になった場合、Web 側は `permissionDenied` の
`debug.code = LOGIN_MFA_REQUIRED` を `loginMfaRequired` として受け、
`/two-step-verification` へ遷移する。CLI 側は同じ判定を
「ログイン RPC が返したエラーコード」で行う必要がある（未認証のため未実測）。

### 9.1 MFA 系 RPC（実行時に発見。静的全数には載っていない）

発見方法は 404/401 オラクル。`docs/rpc-discovered.txt` に一覧がある。

| RPC | 認証 | 実フィールド |
|---|---|---|
| `user.v1.UserAuthService/GetMfaStatus` | 要 | 未取得（空本文で 401） |
| `user.v1.UserAuthService/SetLoginMfa` | 要 | `enabled`（Web 実装の呼び出し形） |
| `user.v1.UserAuthService/StartTotpEnrollment` | 要 | 未取得 |
| `user.v1.UserAuthService/FinishTotpEnrollment` | 不要で検証まで到達 | `flow_id`(1), `code`(2, 6 文字以上) |
| `user.v1.UserAuthService/DeleteTotp` | 要 | 未取得 |
| `user.v1.UserAuthService/RegenerateRecoveryCodes` | 要 | 未取得 |
| `user.v1.UserAuthService/StartPasskeyRegistration` | 要 | 未取得 |
| `user.v1.UserAuthService/FinishPasskeyRegistration` | 不要で検証まで到達 | `flow_id`(1), `credential_json`(2) |
| `user.v1.UserAuthService/ListPasskeys` | 要 | 未取得 |
| `user.v1.UserAuthService/DeletePasskey` | 不要で検証まで到達 | `id`(1) |
| `user.v1.UserAuthService/RenamePasskey` | 不要で検証まで到達 | `id`(1), `name`(2) |

### 9.2 TOTP 登録の手順（Web 実装から）

1. `StartTotpEnrollment` → `{flowId, otpauthUri, secret}`
2. 認証アプリで QR（`otpauthUri`）を読み、6 桁コードを入力
3. `FinishTotpEnrollment {flowId, code}` → 登録完了
4. `SetLoginMfa {enabled:true}` → `{recoveryCodes}`（リカバリコードが返る）

### 9.3 ログイン時の MFA チャレンジ

コード検証は `LoginCueByPhoneCode` の応答側で判定する。実測では
偽 ID + 偽コードで `400 / 1013 Incorrect verification code` が返り、
MFA 要求かどうかは未認証では区別できなかった。
`loginMfaRequired` に対応する公開 RPC（`VerifyLoginMfa` 等）は 404 で存在しない。
MFA はログイン応答のエラーコードとして現れる設計と読めるが、**実アカウントでの確認は未実施**。

## 10. RPC 全数の数え方（重要な但し書き）

`docs/rpc-inventory.txt` の 252 本は **APK の libapp.so から抽出した文字列** が上限で、
サーバに実在する全ルートではない。この差は実測で確認した。

- APK 文字列に無いメソッドでも、Web バンドルが `getApi(<Service>).<camelCase>` で
  呼んでいるものは実在する。例: `user.v1.UserService/GetBackupSuggestion` は
  認証不要で 200 を返す。
- 404/401 オラクル（存在すれば 401/400/200、存在しなければ Connect 本文なしの 404）で
  名前を当てれば、認証前に存在を確定できる。

`docs/rpc-discovered.txt` はその方法で静的全数の外側に足したもの。
出所が違う（静的抽出 vs 実行時発見）ので物理的に分けてある。

### 10.1 誤検出の扱い

`404` でも Connect 本文（`{"code":"not_found",...}`）が返る場合は「ルートは存在し、
対象レコードが無い」を意味する。オラクルは本文の有無で判定するので、
`CheckInvitationCodeRemains` のような業務 404 を誤って「存在しない」と判定しない。
