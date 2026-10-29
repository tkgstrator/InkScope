# 2026-10-28 Salmon Run 保存区間の通信解析

この記録は、ユーザーがクリアを報告した試合の `OnlineCoopSetting` から、最後に観測した `Session 6` までを含む。`Session 6` 自体をクリア通知と判定する根拠はない。以下のレコード番号は切り出した PCAP 内の番号で、元の PCAP の番号は **6752 を加算**する。

## 対象と検証

- PCAP: `/Users/devonly/captures/inkscope-user-reported-clear-20261028-setting-6753-session6-19248.pcap`
- SHA-256: `bf680a5dcd8b8f69c5e59b03289eedddb841ecf5a04ba02ff3955d95ccd403b4`
- 元の記録: #6753–#19248、12,496 UDP パケット。先頭は `OnlineCoopSetting`、末尾は `Session 6`。
- InkScope の `inspect --mode debug --details --jsonl` で 29,979 行を認証。鍵不足、認証失敗、不正・非対応フレームはそれぞれ 0。
- `OnlineCoopSetting` が送ったステージは `Cop_Shakehighway`、難易度は 25。これは送信設定の観測であり、ゲーム内での採用やゲーム結果の独立した証明ではない。
- WebUI のローカル保存セッション: `0733a0b9-7bfd-4e45-8a46-704e465df4c5`。無名の Group 1107/1108 配列値と、塗りイベントの座標・生スカラーを抽出するデコーダーで区間内 #1–#12496 を再取り込みした。旧セッション `72225e57-c3a4-4c0d-85e6-6c8231dd9fe0` と `eeb3b1c5-d740-47e1-be76-10482e1b86a5` も保持。PCAP、復号鍵、実パケット JSON はリポジトリに置かない。

## Protocol 5: プレイヤー状態と納品通知

| 認証済み行 | 件数 | 方向・確認した形式 |
| --- | ---: | --- |
| `CoopSequenceNetState`、GroupId 1104 | 5,795 | ホスト→参加側。FrameMode=1、FrameLength=63、Present=1。 |
| `CoopSeqPlayerNetState`、GroupId 1108 | 5,572 | 参加側→ホスト。FrameMode=1、FrameLength=222、Present=1。12 ビット値の配列が 4 要素ずつ 2 組。 |
| `CoopSeqPlayerNetState`、GroupId 1107 | 39 | ホスト→参加側の開始直後。調べた 2 組の配列値はすべて 0。 |
| 未分類 `UnreliableProtocol` | 969 | 78 バイトの mode0 が 743、28 バイトの mode0 が 217、88/118 バイトの mode1 が 2/7。 |

GroupId 1104 は #171 で Sequence=0 / Quota=2、#3942 で Sequence=1 / Quota=2、#7519 で Sequence=2 / Quota=3。`CoopSeqRoundFinishEvent` は各 Sequence の後半、#3793、#7368、#10921 に 1 件ずつ出た。これは 3 WAVE の終了と整合する。

GroupId 1108 の配列 A の index 1（payload bit 87–98）は、Sequence 0/1/2 に沿って **0→9、0→9、0→4** と変化した。#3941→#3943 と #7516→#7518 で 0 に戻り、前者は Sequence=1 の受信直後、後者は Sequence=2 の受信直前だった。配列 B の index 1（bit 138–149）は、試合中に **0→22** まで 22 回、毎回 +1 で増えた。他の 3 要素は両配列ともこの記録では 0 のままだった。

B の 22 回の増加は、参加側→ホストの `CoopItemGoldenIkuraDirectBankEvent` 10 件と `CoopItemShootSleepEvent` 12 件に一対一で対応した。`CoopItemShootSleepEvent` は逆方向にも 8 件あり、全方向では 20 件。同じ UDP パケット、または通知から 71.583 ms 以内に状態値が増え、対応しない参加側→ホストの増加や通知はなかった。22 回すべてで A も +1 になった。**A は当該 WAVE の納品数、B はその参加側の累計納品数の有力候補**。ただし正式なフィールド名、index 1 と画面上のプレイヤー枠の対応、ゲーム画面の確定値は PCAP 単独では証明できない。WebUI の HUD が数える直接納品通知 10 件だけでは、この記録の 22 回の値増加を表せない。

未分類 mode1 の 9 件には FrameLength=60、Present=1 と GroupId=2（ホスト→参加側）または 3（逆方向）が読めたが、意味は `Unknown`。78/28 バイトの mode0 は別形式であり、mode1 と同じ bit 位置を GroupId として読まない。28 バイトの 217 件はすべて参加側→ホストで、最終 WAVE 後の #11283–#11717 に集中した。

## Protocol 8 / 11 の未分類行

Protocol 8 の `BroadcastEventProtocol` 2,273 行のうち、2,215 行は 190 バイト、先頭バイト 0、イベント長 0 の形であり、型付きイベントとして解釈しない。残る **58 行**は型付きイベントのエンベロープ長条件に一致し、現行 InkScope の型名表にない ID が 12 種類あった。

| 型 ID | 件数 | 照合結果 |
| --- | ---: | --- |
| `0xde1a711a` | 31 | `Unknown`。#9191–#10334 の Sequence 2 に集中。 |
| `0x0855cf93` | 6 | `Unknown`。 |
| `0xa9cd6c5a` | 5 | `Unknown`。 |
| `0xd303a96b` | 3 | `Unknown`。 |
| `0x1c8decf9` | 3 | `Unknown`。 |
| `0x1efbaa50` | 2 | ELF 登録名 `CoopEnemySakeDolphinCapturedPointEvent` と一致。 |
| `0x80c2ef2b` | 2 | `Unknown`。 |
| `0x5db54d9f` | 2 | ELF 登録名 `CoopEnemySakelienShieldDownEvent` と一致。 |
| `0xf41baa27` | 1 | ELF 登録名 `CoopEnemySakelienShieldHitPlayerEvent` と一致。 |
| `0x0e623724` | 1 | `Unknown`。#10252 の `PlayerNetEvent::PerformSpecial` と同じ UDP パケット。 |
| `0x9380c77b` | 1 | `Unknown`。 |
| `0x83f114b0` | 1 | ELF 登録名 `CoopHelicopterStopEvent` と一致。 |

4 種の名前はローカルの InkScope-Wiki `docs/protocol/type.md` にある ELF 登録 ID 表との照合結果であり、イベント本文のフィールドを復号した結果ではない。`0xde1a711a` と塗り・ヒット行の同時出現や、`0x0e623724` とスペシャル行の同時出現だけでイベントの意味を決めない。

Protocol 11 の未分類 `BroadcastReliableProtocol` は 523 行。190 バイト形が 259、83 バイト形が 200、18 バイト形が 51、その他が 13。83 バイト形は両方向に存在し、調べた bit 100–107 は全行 128 だったが、値の意味は `Unknown`。

## そのほかの確認と制限

- `CoopEnemySpawnNetEvent` は 138 行で、Sequence 0/1/2 に 30/38/70 行。座標は読めるが、既存の静的地形データと一意に近接する方向候補は 7 行のみ。通信の観測と実際の出現・湧き方向は別に扱う。
- `CoopItemSpawnEvent` は 29 行。`CoopItemGoldenIkuraDirectBankEvent` 10 行はすべて同じ UDP パケット内に `PlayerNetEvent::DirectBankClam` があった。
- `PlayerNetEvent::TroubleDie` と `PlayerNetEvent::Revival` は各 2 行、`PlayerNetEvent::PerformSpecial` は 1 行。これらはイベント名の同定であり、各プレイヤーの HP、スペシャル残量、ブキ、ネームプレート、バッジの復号ではない。
- 最後の `Session 5` / `Session 6` は #12495 / #12496。元 PCAP では試合前にも同じ組があるため、`Session 6` 単独を終了・クリアの識別子としない。

この 1 試合には画面上の納品数との時刻同期、各プレイヤーを入れ替えた比較、ELF の GroupId 1108 の encoder/decoder の追跡がない。配列 A/B の正式名称とプレイヤー枠の対応は、これらの追加証拠で確定する。

## Replay のプレイヤー情報・塗りに向けた追加照合

以下も同じ `OnlineCoopSetting` → `Session 6` 区間の認証済み通信と、所有する 11.3.0 ELF の照合による。候補値を四人のプレイヤー枠に割り当てる証拠や同期した画面記録はまだない。

- `CoopItemRequestGetByPlayerEvent` と `CoopItemResponseGetByPlayerEvent` はそれぞれ 32 件で、先頭 32 bit の item ID は 29 種類。応答本文の payload bit 268–269 が `1` のものは 29 件ですべて異なる ID、`0` のものは 3 件で先の ID と重複した。`1` の応答を取得 +1、参加側の `CoopItemGoldenIkuraShotEvent` 18 件と `CoopItemGoldenIkuraDirectBankEvent` 10 件を各 −1 として時系列に並べると、推定所持数は全区間で 0 または 1、最後は 1 になる。これは参加側の金イクラ所持状態の有力候補だが、応答 2 bit の正式な意味、再送の扱い、画面上の所持表示は未確認。
- 参加側 `CoopItemShootSleepEvent` 12 件の item ID は先行する Shot の payload bit 363–394 と一致する。これらを別の所持数減少として二重計上しない。`CoopItemSpawnEvent` の payload bit 385–416 の ID は取得要求の 29 ID のうち 22 と一致した。取得・投げ・納品のイベント鎖を追う材料になるが、他プレイヤーの所持数や橙イクラの個人得点ではない。
- `CoopSeqPlayerNetState` の後続 61 bit は、この記録では WAVE らしい 3 bit、状態らしい 2 bit、単調増加する 20 bit、26 bit のスカラー、一定の 10 bit に分かれる。この 61 bit だけからプレイヤー XYZ は特定できない。未分類 Protocol 5 mode0 の 78 byte は 743 件（試合前 16、最終 RoundFinish 後 727）、28 byte は 217 件（すべて最終 WAVE 後 #11283–#11717）で、いずれも進行中の三つの WAVE には出ない。28 byte で主に変化するのは bits 167–178 の 12 bit で、216 隣接組のうち 211 組が +4。78 byte の bits 293–303 も主に +4 で、三軸の移動とは整合しない。mode0 に mode1 の GroupId 位置を流用しない。この区間には Protocol 6、17、18 の行がなく、現時点で四人の位置を表示できない。既知フレーム以降の本文と ELF の `PlayerNetState` serializer との追加照合を下に記す。

#### Protocol 5 の `PlayerNetState` 三軸候補（追加照合）

所有する 11.3.0 ELF は `spl::PlayerNetState` を TypeId `0x2d9946d6`、本文 496–586 bit で登録しており、encoder `0x27fca10` は object `+0x20/+0x24/+0x28` の三つの float を符号付き 17 bit で書く。大きさは下位 16 bit × `256 / 65535`、bit 16 が負号。TypeId は調べた Pia wire header の値ではなく、登録側の識別子である。

| 認証済み形 | 行数 | 形の検査 | 三軸の payload bit |
| --- | ---: | --- | --- |
| GroupId 1107、109 byte | 39 | 先頭の 222-bit frame、bits 235–247=`1,309`、bits 248–279=`0x800186a0`、末尾 6 bit=0 | 280–296、297–313、314–330 |
| GroupId 1108、109 byte | 5,572 | 同じ frame、bits 248–279=`0x80019a28`、末尾 6 bit=0 | 同上 |
| GroupId 1104 内の 1107 frame、118 byte 以上 | 5,578 | bit 75 から mode 0、length 222、GroupId 1107。bits 310–322=`1,309`、bits 323–354=`0x800186a0`。最短 118-byte 形の末尾 3 bit=0 | 355–371、372–388、389–405 |

デコーダーは厳密な長さ・GroupId・ヘッダー・末尾検査を通過した認証済み Protocol 5 port 101 行にだけ `PlayerStateX/Y/Z` と `PlayerStateCoordinateMeaning=Unknown` を出す。#169 の 1107 値 `(-9.242,25.594,-33.395)` と #170 の 1108 値 `(-8.824,25.805,-34.235)` は、#171/#172 でそれぞれ `(18.887,38.094,-76.118)`、`(19.078,38.305,-76.872)` に移る。別ストリームの動きだが、1108 の #635→#637→#643 は `(18.360,11.996,73.462)`→`(0.660,0.051,1.480)`→`(18.910,13.117,74.353)` と約 75 単位離れて戻る。近傍の `DokanWarp` イベントや塗り座標では説明できていない。Pia dispatch から `PlayerNetState` 型への選択経路、座標系と適用時点、画面の P1〜P4 枠は未確認なので、四人の現在地としては扱わない。
- `ColPaintEvent` 6,972 件と `ObjPaintEvent` 68 件について、所有 ELF の decoder `0x3729394` / `0x3728be4` は typed body の先頭を chunk LSB0 bit 200 とし、相対 bit 0–16、17–33、34–50 を座標、51–62 と 63–74 を 12 bit の生スカラーとして復号する。座標は 17 bit の符号と大きさから `±(magnitude × 256 / 65535)`、スカラーは `raw × 64 / 4095` に換算できる。InkScope はスカラーを意味未確定の `PaintScalarRawA/B` として生値で出す。相対 bit 116–136 の 21 bit は 7,040 件すべてで共通メタデータの `GameFrame` と一致。相対 bit 137–140 の 4 bit は送信元 `.174` の 6,939 件ですべて `1`、`.180` の 101 件ですべて `0` だった。送信側の枠を示す候補だが、正式な PlayerId や HUD の枠と確定していない。形状、地形面への投影、重複・上書き、色も未確認なので塗り面積や面全体の塗り状況へ変換しない。
- `CoopEnemyStateEvent` は 4 件で、所有 ELF の decoder `0x1c3151c` が 6 bit と 20 bit を読む。本文は `0x3ffffc2` と `0x3ffffc9` が各 2 件で、20 bit はすべてセンチネル。#1387 / #2522 は同じパケットに `CoopItemSpawnEvent` が 3 件ずつあるが、#9602 / #9813 にはない。一方、アイテム 3 件の同時生成は計 9 回。敵状態イベント数を撃破数として数えられず、討伐者の帰属も不明。`AttackEvent` 353 件、`AttackEventMulti8` 21 件、`HitEffectNetEvent` 384 件も攻撃・ヒットの型名だけで個人討伐数に変換しない。

Replay に出す値には、認証された通信とプレイヤー枠の対応、画面または結果との照合が必要。現段階で確定していない個人値は `Unknown` とし、観測済みのゼロと区別する。次の制御記録では、各プレイヤーが順に移動・金イクラ取得・投げ・直接納品・単独でオオモノを倒す・未塗装地点を塗る操作を画面時刻と PCAP で同期して比較する。

#### PlayerNetState の水平方向候補（2026-10-29 追加）

上記109 byte形式で従来「bits 235=`1`、236–247=`309`、248–279=`0x800186a0`／`0x80019a28`」として検査していた部分は、同じビット列を通常の枠境界で読むと、bit 235から12 bitの長さ619、bit 247から32 bitのgroup 200000／210000、bit 279のpresent 1である。三軸の開始bit 280はこの後続状態の本文先頭であり、先行するCoopSeqPlayerNetState本文の三軸ではない。ホスト複合形式も長さ619がbit 310、group 200000がbit 322、presentがbit 354、本文先頭がbit 355となる。既存の厳密な長さ・枠・末尾検査は維持する。

所有11.3.0 ELFのPlayerNetState encoder `0x27fcb68..0x27fcb70` はobject `+0x5c` のベクトルをhelper `0x31cc060` に渡す。helperとdecoder `0x27fd700..0x27fd7bc` の照合により、本文bit 159–171の13 bitは、bit 12が符号、下位12 bitが大きさの水平角と読める。角度は `sign × magnitude × 180 / 4095` 度で、水平ベクトルはおおむね `[sin(angle), 0, cos(angle)]` である。ゲーム側はfloat32と三角関数表を使うため、表示用の通常のsin/cos計算とのビット一致は主張しない。後続の本文bit 172–183はobject `+0x68` の-1～1スカラーだが、縦の照準角とは確定していない。

InkScopeは三軸を出す同じ検査済み経路で `PlayerStateHorizontalAngleRaw`、`PlayerStateHorizontalAngleDeg`、`PlayerStateDirectionX/Z` を出す。身体の向きと照準方向のどちらかは未確定なので `PlayerStateDirectionMeaning=Unknown` を維持する。静止したユーザーが視点を回した制御キャプチャとの照合はNXCTFの `docs/security/2026-10-29-s3-coop-sequence-player-state-audit.md` にあり、移動差分から向きを作った値ではない。

2026-10-29の完全な保存スナップショット `inkscope-coop-audit-20261029-035850-041848.pcap` を更新デコーダーで読取専用に再検査し、35,597 UDPレコード、認証済み72,355行、proof verified 23行、認証失敗0件のうち21,491行で水平角を取得した。既存のVisualizer保存イベントはこの追加前のfieldsしか持たず、保存された `packet.payload_hex` は暗号化UDPなのでブラウザーだけでは角度を復元できない。必要な鍵と元PCAPから `inspect` で再復号し、新しいセッションへ取り込む必要がある。元のセッションを書き換える処理は行っていない。

#### PlayerNetState のカメラ pitch と形態 selector（2026-10-29 追加）

同じ所有11.3.0 ELF（SHA-256 `a641a94f74b832ffb6f28e216147fb5a867790246809b270507611d554294357`）の送信側を追跡し、object `+0x68` はPlayerCameraの正規化pitch入力と同じ状態propertyを送ると確認した。`0x273d5f4`は`S=*(SplPlayerActor+0x108)`を取得し、wrapperからsenderへ渡すbiased pointerを`0x2874880..0x28748a4`で補正すると`x28=S`となる。`0x287e2c0`が構築したPlayerNetStateに、`0x287e4fc`が読む`S+0x584`を`0x287e504`でobject `+0x68`へ直接コピーする。PlayerCameraも`0x28ee490..0x28ee4a8`で同じowner stateの`+0x584`をcamera `+0x16c`へコピーし、geometry呼出`0x28f065c -> 0x28eb664`がこれをpitch制御値`t`として受け取る。これは値域・画面相関からの推定ではない。カメラ初期コピーは条件付きで、通常updateには補間・制御があるため全フレームの描画カメラとの完全一致は主張しない。

| 出力field | object / 本文bit（LSB0、両端含む） | standalone payload bit | nested host payload bit | 復号 |
| --- | --- | --- | --- | --- |
| `PlayerStateCameraPitchRaw` | `+0x68` / 172–183 | 452–463 | 527–538 | unsigned 12 bit、0–4095 |
| `PlayerStateCameraPitchControl` | 同上 | 同上 | 同上 | doubleで`2 * (raw / 4095.0) - 1`を計算後float32へ変換、小数9桁 |
| `PlayerStateForm` | `+0xf8` / 318 | 598 | 673 | 0=`Squid`、1=`Human` |

pitch encoder `0x27fcb74..0x27fcbdc`は有限値を-1～1へclampし、float32中間演算で`(t+1)*4095*.5`に`.5`を加えて整数化する。符号bitと大きさに分かれた欄ではない。decoder `0x27fd7c0..0x27fd804`はunsigned 12 bitをdoubleへ変換し、4095で割り、`2*x-1`の後float32へ戻す。端点raw0/4095は-1/+1、raw2047/2048はおよそ-0.000244200/+0.000244200となり、正確な0は表せない。`t=0`を送るとraw2048となる。

形態の根拠は親decoder `0x27fdb00..0x27fdbd8`が選択値0で`PlayerNetState_Squid`のvtable `0x5d2bef8`、1で`PlayerNetState_Human`のvtable `0x5d2bdd8`を選ぶことにある。senderもobject `+0xf8`へ`0x287e76c`で0、`0x287f368`で1を書き込む。`+0x98`の3 bit値とは別欄である。camera `+0x17ac`の切替を制御するstate `+0xee0`は別のcountdownで、weapon VFX/animationのexpiry処理を持つが、その意味やHuman/Squidとの対応は未確定。このform fieldから`+0x17ac`を選ばない。

追加fieldは従来の三軸・水平角と同じ厳密な認証済みport101／standalone・nested frame検査を通った経路だけに出す。既存のheader、末尾、角度fieldを変更しない。座標については送信時に行列変換を適用する条件分岐`0x287e2c4..0x287e384`があり、wire XYZを常にworld座標・モデル足元と同一視する根拠にはならない。保存済みVisualizerイベントには新fieldがないため、同じPCAPを更新バイナリーで再検査し、新規sessionとして取り込む必要がある。

同じ保存スナップショット`inkscope-coop-audit-20261029-035850-041848.pcap`（8,401,605 byte、SHA-256 `d864e207202fc2ec63c95499467095d3a9569173e0f7a3ec0bef11e733ad597d`）を更新バイナリーでネットワーク送信なしに再検査した。72,355認証済み行、23 proof-verified行、3 unsupported-layout行、認証失敗0件のうち、三つの追加fieldは各21,491行で取得できた。formはHuman16,256行／Squid5,235行、pitch controlの観測範囲は-0.965811968～0.880341887だった。保存先への新session取り込みは別工程であり、この再検査では送信・既存session更新をしていない。

追加fieldを用いる現在のReplay用に、別の既存PCAP`inkscope-player-20261029-050137-watch-30396-23.pcap`（13,765,299 byte、SHA-256 `841cea506c9b70089c21fd0380021286e1605f5d0ca0009c399e3151dce28289`、全64,537 record）のrecord64,536までを、新規session `272070b6-e23c-4486-9dda-82cf16d4bf83`へ取り込んだ。認証済み111,099行、proof_verified36行、authenticated_malformed_chunks1行、missing key／認証失敗0件。pitch raw/control/formは各21,258行（Human16,233／Squid5,025）で、最初の該当行はrecord33,348のraw2048／control0.000244200／Humanだった。元のsession `ea17a502-69d4-44e2-9d64-61cd565df35f`は維持した。

この取り込みでは1送信のACKが未確認となったため、全体を再送せずAPI全件readbackでrecord61,431だけが保存されていないと特定した。この行は追加fieldを持たない認証済みUnreliableProtocolであり、元PCAPと旧sessionの全packet field・timestampが一致することを確認し、既存の`PostgresPersistence.save`の`INSERT ON CONFLICT DO NOTHING`でその1行だけ補った。最終API再読では1～64,536の全recordが一意に存在し、全packet fieldとtimestampが元PCAPに一致した。順序付きの`record/timestamp_ns/packet`をkey順canonical JSON＋改行にしたSHA-256は、元PCAPとAPI双方で`f82f1a89727ef89f42f89f1c2e459a5fcd13d55601dab9f848f338d95d77905c`。API・schemaの変更、既存sessionの書換え、managed key cacheへの書込み、UI選択変更はしていない。

### 終盤の個人結果データ候補

所有 ELF の `0x3808c9c` 付近には、`GoldenIkuraNum`、`IkuraNum`、`KillNum`、`AssistGoldenIkuraNum`、`WeaponIdArray`、`SpecialId` などをキーとする構造化データの読み取り処理がある。`0x3808f84` から `CoopPersonalReportScore` の読み取り処理 `0x3806324` を呼ぶ。ただし、これは結果データの形式の候補であり、LAN パケットへのシリアライズや `KillNum` がオオモノ専用かは未確認。`CoopGameResult` の文字列参照は型名登録で、通信との接続を示さない。

RoundFinish #10921 の後の未分類 Protocol 11 に #11732（94 byte）、#11741 / #11742（各 226 byte、逆方向に約 7.5 ms 間隔）、#11762（15 byte）、#11768（96 byte）、#11770（78 byte）がある。#11741 / #11742 の bit 97–99 は両方 `0` で、既知の 226 byte `PrivateSetting` が要求する `4` と違う。個人結果と結び付く型 ID・フィールド位置はまだない。`CoopPersonalReportScore` 付近の型 ID `0x82a5df04` の生バイト列は認証済み 29,979 行に見つからなかったが、型 ID がそのまま送られるとは限らず、結果データ不在の証明にはならない。次は `0x3808c9c` の呼出元と Protocol 11 の該当レコードを照合する。
