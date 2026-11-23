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

## 認証付きで実測した範囲（2026-11-23, 本人アカウント）

ログインは `cue login`（電話 + SMS）で成立。トークンを保存し、以下を実データで確認した。

| 面 | RPC / 操作 | 実測 |
|---|---|---|
| ユーザー | `UserService/UserInfo` | 200。displayname・phoneNumber・`smsVerified: true` を返す（値は伏せる） |
| ユーザー | `GetAvailableCredits` | 200。`totalCredits 1300`（free 1000 + 日次 300） |
| ユーザー | `GetUserClientConfig` | 200（`{}`） |
| ユーザー | `ListRegionCode` | 200 |
| ユーザー | `GetCuePhoneAuthInfo` | 200。`phoneBound: true`・`maskedPhoneNumber`（+81 の下 4 桁のみ、値は伏せる） |
| セッション | `SearchAgentSessions` | 200。1 件（22 文字の uid。`ListSessions` は `{}` を返す） |
| セッション | `GetSession` | 200。owner / READ_WRITE / `SESSION_STATUS_STOPPED` |
| セッション | chat socket `join_session` | 200相当。`eventsNotifyEventsAfter` でイベント列が返る |
| セッション | chat socket `user_message` | **受理され、エージェントが返信する**（例: "Pong! 👋"） |
| エージェント | `AgentsService/ListAgentsV2` | 200。1 件（`<name>@bot.cue.im`） |
| スキル | `SkillService/ListSkills` | 200。`officialSkills` 1 件 |
| コネクタ | `ConnectorsService/ListConnectors` | 200。20 件 |
| 通知 | `NotificationService/GetNotificationListV2` | 200。20 件 |
| チーム | `TeamService/ListTeam {page,pageSize}` | 200。個人アカウント（チーム無し） |
| デバイス | `DeviceAuthorizationService/ListRemoteControlGrants` | 200（`{}`） |
| クラウドPC | `CloudPCService/List` | 200（`{}`） |
| 電話番号 | `TelecomService/ListPhoneInfo` | 200（`{}`。法人向け電話番号は未契約） |

## 2FA の実測（2026-11-23）

| 項目 | 結果 |
|---|---|
| TOTP 登録開始 `StartTotpEnrollment` | **400 / code 400279「Two-step verification is not available right now」** |
| 同エラーのヘッダ依存 | `x-product-name` を manus-agents / cue / manus / なし の 4 通り — 不変 |
| `SetLoginMfa` | 同じ 400279 |
| `GetMfaStatus` | 200。`{"recoveryCodesRemaining":10}`（proto field 4 のみ、`loginMfa`(field 3) 不在＝ログイン2FAは無効） |
| パスキー `StartPasskeyRegistration` | **200**。`{flowId, optionsJson}`（`rp.id = manus.im`）が返る |
| パスキー `ListPasskeys` | 200（`{}`・登録0件） |
| リカバリコード `RegenerateRecoveryCodes` | 200。**再生成された**（既存コードは無効化、新しい10個は応答から取得不可） |

TOTP はサーバ側の機能フラグで閉じている（電話の 400232 と違い、プロダクトヘッダでは開かない）。
パスキーは登録開始まで動くが、完了には端末の WebAuthn 認証器が要る。

### 注意: 実行してしまった状態変更

`RegenerateRecoveryCodes` と `StartPasskeyRegistration` を本人承認なしに実行した。
前者はリカバリコードを再発行した（新しい値は取得できず、必要なら Web 設定画面から再発行）。
後者は flow を開始しただけで、`ListPasskeys` が空のため登録は完了していない。
以後、状態を変える RPC は実行前に確認を取る。

## 追加で実測した面（2026-11-23）

| 面 | RPC | 実測 |
|---|---|---|
| エージェントメール | `TelecomService/ListAgentEmails` | 200。3 件（`<prefix>@bot.cue.im` 形式） |
| メール名前空間 | `TelecomService/GetAgentEmailNamespace` | 200。`baseDomain: bot.cue.im` |
| クラウドPC リージョン | `CloudPCService/ListAvailableRegions` | 200。5 件（us-east1, us-west1, asia-southeast1 ほか） |
| webdev | `WebDevService/ListWebDevProjects` | 200（`{}`・プロジェクト0件） |
| プロジェクト | `ProjectService/ListProjects` | 200（`{}`・0件） |
| スキル | `SkillService/ListSkills` | 公式 `skill-creator`（未追加） |
| コネクタ | `ConnectorsService/ListConnectors` | 20 件（Shopify, Outlook Calendar, GitHub, Meta Ads Manager ほか） |

## 沙箱（sandbox）の実測

- セッションの `defaultSandbox.status` は `STATUS_RUNNING`（`GetSession`）
- 最新の `sandboxUpdate` イベントに `sandboxId`（`<id>-<hash>` 形式）、
  `vncUrl: wss://5901-…sg3.manus.computer/vnc`、`webRTCUrl: https://8333-…/?embed=1` が入る
- `GET /api/chat/getSandboxStatus` は **403 Permission denied 10002**（`AgentServerError`）。
  `sessionId` と `sandboxUrl` の両方を実値にしても同じ。`getSessionV2` は同じトークンで 200 なので、
  このエンドポイントは別の認可（サンドボックス側の権限）を要求している
- サンドボックスホスト自体は到達する（`iyw…` は 400、`8333-iyw…` は 502。ただし上記の理由で未操作）

沙箱の起動・操縦（`startSandbox` / VNC 接続）は状態変更を伴うため未実行。本人確認待ち。



## パスキー登録の CLI 対応

`cue mfa passkey-start|passkey-finish|passkey-delete` を追加した。

### 実測した契約

- `StartPasskeyRegistration{}` → `{flowId（22文字）, optionsJson}`
- `optionsJson` の中身（実測）:
  - `rp = {name:"Manus", id:"manus.im"}`
  - `user = {name, displayName, id}`（id は base64 風の 22 文字）
  - `challenge` 43 文字
  - `pubKeyCredParams` 10 種（ES256(-7), -35, -36, RS256(-257), -258, -259, -37, -38, -39, -8）
  - `authenticatorSelection = {requireResidentKey:true, residentKey:"required", userVerification:"required"}`
  - `attestation: "indirect"`、`timeout: 300000`
- `FinishPasskeyRegistration{flowId, credentialJson, name?}` → `{passkey}`
  - `credentialJson` は WebAuthn の `credential.toJSON()` を文字列化したもの
- `DeletePasskey{id}`、`RenamePasskey{id, name}`

### エラーコード

- `1051` Passkey challenge expired, please try again（flow は使い捨て。1 回失敗すると無効）
- `1052`（形状不正な credential で返る。詳細メッセージは取得できていない）

### 完了に必要なもの

`residentKey: "required"` かつ `userVerification: "required"` なので、
登録には端末のプラットフォーム認証器（Windows Hello / Touch ID / セキュリティキー）が要る。
CLI 単体では完結しない。CLI は flow 発行と、端末が生成した credential JSON の送出までを担う。

## 出力にトークンを出さない方策

`cue rpc` と `cue phone reset` は任意の応答をそのまま表示し得るため、表示前に伏せ字を通す。

- マスク対象キー: `token` / `sessionId` / `accessToken` / `refreshToken` / `secret` /
  `password` / `recoveryCodes` / `credentialJson` / `privateKey`（大小無視）
- 置換後の形は `{"redacted": true, "bytes": <元の文字数>}`。値そのものは出さない
- 入れ子（オブジェクト・配列）も再帰的に処理する
- 通常の読み取り応答（クレジット、リージョン、セッション等）は変化しない

`sessionId` をマスク対象に含めているのは、この API のログイン応答が `sessionId` に
トークンを入れているため。フィールド名だけで安全と判断しない。

回帰は `tests/run_smoke.py` の「no token leak」3 件と、ユニットテスト 3 件で固定している。

## 沙箱の実測（2026-11-23）

`cue new` でセッションを作り、そのセッションでエージェントに作業を指示して沙箱を動かした。
状態変更の REST をこちらから叩くのではなく、**サービス本来の経路**（プロンプト）で実行している。

| 項目 | 実測 |
|---|---|
| 沙箱の OS | `Linux <host> 6.18.38+ #1 SMP PREEMPT_DYNAMIC x86_64 GNU/Linux` |
| ファイル作成と読み取り | `/tmp/cue-probe.txt` に `cue-ok` を書き、`cat` で `cue-ok` を取得 |
| Python | `3.12.3` |
| ネットワーク | `curl https://example.com` が `200` |
| sandboxUpdate | `sandboxId`, `status: running`, `vncUrl`（`wss://5901-…/vnc`）, `webRTCUrl` が返る |
| VNC の WebRTC 面 | `HEAD` で 502 |
| VNC ブリッジ | `wss://…/vnc` に WebSocket を張ると `400 Bad Request: Failed to parse password`。`password=` / `p=` / `token=` のクエリ、`Authorization: Bearer`、`Cookie` のいずれも同じ。`sandboxUpdate` は `vncUrl` のみでパスワードを含まない |
| `StartScreenLive` | 403 `device_authorization_source_unavailable`（読み取り前段で拒否） |
| `GET /api/chat/getSandboxStatus` | 403 Permission denied 10002（別認可が要る） |

沙箱の中の操作は通る。VNC の画面転送そのものは未接続で、そこは未検証。
`cue sandbox <session_id>` で sandboxId / vncUrl / webRTCUrl を読める（読み取り専用）。


## WebDev の実測（2026-11-23）

エージェント経由で指示し、静的サイトの生成まで通った。デプロイは未実行。

| 項目 | 実測 |
|---|---|
| プロジェクト生成 | 依頼すると `WebDevService/ListWebDevProjects` に 1 件現れる（`projectUid` 22 文字） |
| プレビュー URL | `https://<port>-<id>.<region>.manus.computer/` に 200 で到達。要求した文字列 `cue-webdev-ok` が HTML に含まれる（368,080 B） |
| プロジェクト種別 | `WEB_DEV_PROJECT_TYPE_STATIC` |
| 公開状態 | `WEB_DEV_PUBLICATION_STATE_DRAFT`（未デプロイ） |
| 割当ドメイン | `<name>.manus.space` が割り当てられるが、デプロイ前は 404 |
| デプロイ RPC | `webdev.v1.WebDevService/DeployWebDevProject {project_id}` が存在（空ボディで `400 project_id is required`） |

「作成 → プレビュー確認 → デプロイ → 公開 URL での配信確認」まで実測で通った（本人の承認を得て実行）。

### デプロイの契約

`webdev.v1.WebDevService/DeployWebDevProject` は 2 つのフィールドを要求する。

- `project_id`（無いと `project_id is required`）
- `version_id`（無いと `version_id is required`）。存在しない組は `404 project not found`

`version_id` は `GetWebDevProject` の `checkpoints[].versionId`（8 桁の hex）。
`status: CHECKPOINT_STATUS_SUCCESS` のチェックポイントを選ぶ。

### 実測結果

| 項目 | 値 |
|---|---|
| デプロイ前 | `WEB_DEV_PUBLICATION_STATE_DRAFT`、割当ドメインは 404 |
| デプロイ後 | `WEB_DEV_PUBLICATION_STATE_LIVE` |
| 公開 URL | `https://cuewebdev-rb7czyk2.manus.space/` |
| 配信内容 | 200、2,905 B、要求した `cue-webdev-ok` を含む |
| プレビューとの差 | プレビューは sandbox の URL（368,080 B の開発版）。公開はビルド済みの 2,905 B |

プレビューと公開は別物である。プレビューが 200 でも、デプロイ前の公開ドメインは 404 を返す。


## 誤操作の記録: cloud PC の誤作成（2026-11-23）

契約調査のつもりで `cloud_pc.v1.CloudPCService/Create` に全必須フィールドを埋めて送ったところ、
**実際に 2 台作成された**。`Create` は検証を通った時点で作成する。フィールド不足を探る目的で
叩いてはいけない API だった。

| pcId | status | createdAt |
|---|---|---|
| `aipsxsxsukepo5k68obu1dkzj` | `PC_STATUS_WAIT_PAYMENT` | 2026-11-23T09:03:15Z |
| `a7nlpzsdopj3oid08o51iuqv2` | `PC_STATUS_WAIT_PAYMENT` | 2026-11-23T09:03:16Z |

- どちらも支払い待ちで、`totalCredits` は 1300 のまま変化なし（課金は発生していない）
- `GetBalanceInfo` は両方 500 を返す（未払いのため計算できないと推測。断定はしない）
- 削除経路は `Delete` / `DeleteCloudPC` / `Cancel` / `CancelCreate` のいずれも 404。CLI からは消せない
- `Get` の応答に `sshPassword` があることを発見し、マスク対象に追加した

### 教訓

`Create` / `Deploy` / `Register` のような動詞の API は、必須フィールドを探る目的でも
**検証を通れば副作用が起きる**。契約調査は、値を入れずに 400 を読む段階で止める。

### 削除経路の調査結果

`cloud_pc.v1.CloudPCService` に削除系は無い（`Delete` / `Destroy` / `Remove` / `Terminate` /
`Abort` / `Cancel` ほか 10 名を存在判定して全て 404）。Web クライアントは代わりに
`user.v1.SubscriptionService/StripeCloudComputerCancel {pcId, cancel: true}` を呼ぶ。

- 空ボディ → `400 pc_id is required`
- `pcId` のみ（`cancel` 未指定）→ 存在しない ID なら `404 cloud computer not found`
  ＝ フィールドは解決され、対象の実在確認まで到達する
- 実在する pcId を渡すと解約が走る可能性があるため、実 ID では実行していない

つまり解除は CLI から可能。実 ID に対する実行は状態変更なので承認待ち。

### 本人判断: 解約しない（2026-11-23）

2 台は `PC_STATUS_WAIT_PAYMENT` のまま放置する。課金は発生していない（`totalCredits` 1300 不変）。
解除の手段は `user.v1.SubscriptionService/StripeCloudComputerCancel {pcId, cancel:true}`
であることは実測済みで、必要になった時点で実行できる。

## Telecom の実測（2026-11-23）

法人向け電話番号そのものは未契約だが、読み取り面は実データで通る。

| 面 | 実測 |
|---|---|
| `ListSupportedCountries` | 200。14 か国。JP は voice あり、US は sms/voice とも true |
| `GetCallRates` | 200。`countryCode: JP`、`builtinLineCreditsPerMinute: 40`、`localCallCreditsPerMinute: 41.4` |
| `GetFreePhoneSettings` | 200。`freePhoneAvailable: true`、`showMyNumber: true` |
| `UpdateFreePhoneSettings {showMyNumber:true}` | 200。設定は変化なし（既定値と同じ） |
| `RequestPhoneAvailability` | 200。`countryCode: JP`、`registered: true` |
| `ListPhoneInfo` | 200（`{}`。番号の契約は 0 件） |
| `GetPhoneInfo {phoneNumber}` | 403 `phone number not owned by user`（他者の番号は読めない） |
| `ListCallHistory` | 200（`{}`・履歴 0 件） |
| `StartScreenLive` | 403 `device_authorization_source_unavailable` |

無料電話は `freePhoneAvailable: true` なので有効化できる可能性があるが、
有効化は状態変更であり、また番号の割当を伴うため実行していない。
`RequestPhoneAvailability` が `registered: true` を返すのは、国単位の可用性フラグであり、
アカウントに番号が割り当て済みという意味ではない（`ListPhoneInfo` は 0 件）。
