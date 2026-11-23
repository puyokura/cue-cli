# 機能カバレッジ（実測に基づく）

各行は実サービスに対する実測の有無を示す。未検証は理由とともに残す。
「実測済み」は 2026-11-23 に api.manus.im に対して実プロセスで確認した範囲。

| 機能 | 状態 | 根拠 |
|---|---|---|
| 電話 + SMS ログイン | 実測済み | `cue login` でトークン取得、`UserInfo` で検証 |
| 電話 + パスワードログイン | 未検証 | 実装済み（`cue phone password`）。本人のパスワード未使用 |
| メールログイン | 不可 | Cloudflare Turnstile 必須（`1715 Need CAPTCHA code!`） |
| Google ログイン | 未検証 | 実 id_token / access_token が要る。aud 検証まで到達 |
| 2FA: TOTP | 不可 | サーバ側 400279。認証・時間・ヘッダ・属性を変えても不変 |
| 2FA: パスキー開始 | 実測済み | `StartPasskeyRegistration` が flow と optionsJson を返す |
| 2FA: パスキー完了 | 未検証 | 端末の WebAuthn 認証器が要る（`residentKey`/`userVerification` とも required） |
| セッション作成 | 実測済み | ID をローカル生成して送るとサーバが永続化する |
| プロンプト送信 | 実測済み | `user_message`（camelCase）を送り、応答を得る |
| エージェント応答受信 | 実測済み | `chatDelta` → `chat` → `statusUpdate` |
| 沙箱内コマンド実行 | 実測済み | `uname -a`、ファイル作成と読み取り、Python 3.12.3、curl 200 |
| 沙箱の VNC 画面 | 未検証 | ブリッジが `400 Failed to parse password`。資格情報の発行元が公開面に無い |
| WebDev 生成 | 実測済み | 静的プロジェクト作成、プレビュー 200 |
| WebDev デプロイ | 実測済み | `WEB_DEV_PUBLICATION_STATE_LIVE`、公開ドメイン 200 |
| telecom 読み取り | 実測済み | 14 か国、JP レート 40 credits/min、無料電話 available |
| デスクトップ端末一覧 | 実測済み | 1 台（`deviceId` / `deviceName` / `configVersion`） |
| スペース | 実測済み | `PageGetUserSpaces` は `total: 1` |
| コネクタ一覧 | 実測済み | 20 件（Shopify / GitHub / Outlook ほか）。接続済み 0 件 |
| コネクタ接続 | 未検証 | OAuth 同意が要る（外部アカウントの認可） |
| 課金・サブスク読み取り | 実測済み | Agents 枠 5000、残 4994、planKey `agents_20_monthly`（source: invitation） |
| アプリ内課金カタログ | 不可 | `GetResourceIAPCatalog` は `400 resource purchases require a mobile app` |
| telecom 番号契約 | 未検証 | 番号 0 件。割当は状態変更かつ課金を伴い得るため未実行 |
| cloud_pc 作成 | 実測済み（誤操作） | 誤って 2 台作成。`WAIT_PAYMENT`、課金なし、放置（本人判断） |
| cloud_pc 削除/解除 | 未検証 | 解除は `StripeCloudComputerCancel`。実 ID には未実行（本人が放置を選択） |
| Unix 0700/0600 | 未検証 | Windows ホストのため未コンパイル・未実行。CI の Linux ジョブで実測される |

## この表の使い方

- 「不可」はクライアントの努力で開けないもの。サーバ側の機能ゲート。
- 「未検証」は経路が存在し契約も分かっているが、資格情報や端末操作が要るもの。
- 未検証を実測済みとして扱わない。README と CHANGELOG もこの区別に合わせている。



