# InkScope

Splatoon 3 (0100C2500FC20000) の LAN Play パケットをキャプチャーし、コンソールに表示するアプリケーションです。鍵を指定すると復号結果も表示できます。鍵はリポジトリに含みません。ocean-calc を使って WAVE 情報を先読みできます。

## 使い方

~~~sh
cargo build --release --locked --workspace
mkdir -p dist
install -m 755 target/release/inkscope dist/inkscope
install -m 755 target/release/ocean-calc dist/ocean-calc
cargo clean
~~~

保存した PCAP を解析します。PCAP・参照表のパスと MAC アドレスは手元の値に変更してください。

~~~sh
dist/inkscope inspect "$HOME/captures/capture.pcap" \
  --random-table "$HOME/.config/inkscope/table.bin" \
  --target-mac 02:00:00:00:00:01
~~~

macOS では、初回に `brew install --cask wireshark-chmodbpf` を実行して再起動すると、通常ユーザーで BPF にアクセスできます。インストール時には管理者認証が必要です。リピーターハブに接続した有線 NIC から保存しながら解析する例:

~~~sh
mkdir -p "$HOME/captures"
dist/inkscope capture --interface en18 \
  --output "$HOME/captures/capture.pcap" \
  --random-table "$HOME/.config/inkscope/table.bin" \
  --target-mac 02:00:00:00:00:01
~~~

開発中に最新のデバッグビルドで継続取得する場合は、このリポジトリで `./capture.sh` を実行します。既定の `normal` モードは取得対象の全パケットを新しい PCAP に保存します。`--mode debug` でも全パケットを保存し、`--mode salmon` は認証済みの既知 Salmon Run パケットと対象のプレイヤー復活・デス通知を保存します。起動時とソース・Cargo 設定・ビルド入力の変更時にビルドし、成功したときだけ新しいキャプチャを起動してから、このスクリプトが起動した前のキャプチャを停止します。ビルドに失敗した場合は前のキャプチャを続けます。キャプチャが予期せず終了した場合は、最後にビルドが成功したバイナリから間隔を空けて再起動します。再起動ごとに新しい 0600 の PCAP を `$HOME/captures` に保存し、Ctrl-C でこのスクリプトが起動したプロセスだけを止めます。別の端末で既に動いているキャプチャは操作しないため、重複を避ける場合は先にその端末で停止してください。`./capture.sh --build-only` は取得を始めずビルドだけを確認します。

同じ Mac の `../InkScope-Visualizer/.env.local` に `INKSCOPE_VISUALIZER_TOKEN` が設定されていれば、`capture.sh` は保存と同時に解析イベントを Visualizer の LIVE へ送ります。同ファイルに `INKSCOPE_CONTROL_TOKEN` もあれば、Settings 用の制御 API を `127.0.0.1:10254` で起動します（`INKSCOPE_CONTROL_PORT` で変更可）。起動時の GameKey ファイルは制御 API から変更できず、Settings では鍵を再入力せずに接続します。PCAP の保存先は引き続き非公開の `$HOME/captures` です。Visualizer は PCAP ディレクトリを監視しません。送信済みイベントは Visualizer の PostgreSQL にも保存され、Replay のセッション一覧から選択できます。既に動いている `capture.sh` には変更が反映されないため、その端末で停止して再起動してください。

スクリプトの既定値はインターフェース `en18`、通信相手 `192.168.1.180`、保存・表示モード `normal` です。必要なら `INKSCOPE_INTERFACE`、`INKSCOPE_PEER`、`INKSCOPE_MODE`、`INKSCOPE_CAPTURE_DIR` を環境変数で変更できます。`--mode` 引数は `INKSCOPE_MODE` より優先します。GameKey は `$HOME/.config/inkscope/game_key.bin`、保存する PiaKey は `$HOME/.config/inkscope/pia_key.bin` を使います。

## ゲーム内のHDMI画像と通信を保存

macOSではFFmpegのAVFoundationデバイス名または番号を指定できます。デバイス一覧は鍵なしで照会できます。

~~~sh
dist/inkscope hdmi-devices
dist/inkscope capture --interface en18 \
  --hdmi-device "HDMI capture device" \
  --in-game-dir "$HOME/captures/in_game" \
  --game-key-file "$HOME/.config/inkscope/game_key.bin"
~~~

`--interface`とデバイス名は実環境に合わせてください。部屋の探索前から起動します。途中から復号する場合は、通常と同じくその部屋の`--pia-key-file`と`--peer`が必要です。`--hdmi-device`と通常の単一PCAP用`--output`は併用しません。`--in-game-dir`を省略すると現在のディレクトリの`captures/in_game`へ保存します。

~~~text
captures/in_game/<UUID>/
  capture.pcap
  packets.jsonl
  manifest.json
  screeshots/
    segment-000001-<UNIXマイクロ秒>.png
    segment-000002-<UNIXマイクロ秒>.png
~~~

`screeshots`は保存先として指定された綴りです。認証済みの正しい`OnlineCoopSetting`で試合IDを作り、ホストからの正しい`CoopSequenceNetState`で画像保存を開始します。`CoopSeqRoundFinishEvent`で停止し、同じSequenceの残留通信では再開せず、新しいSequenceで次WAVEを同じUUIDへ保存します。ロビーの`PrivateSetting`だけでは開始しません。`Session 5/6`を終了やクリアと解釈しません。

HDMI記録時の`--mode`と`--hide-cycle`は表示を選別し、UUID内の保存区間は変えません。HDMI未指定の単一PCAP保存は従来の動作です。

WAVE中は対象ホスト／クライアントのUDP 49152レコードを元のバイト・PCAP時刻で保存し、開始・WAVE終了の通知も含めます。WAVE間と結果画面は画像を保存しません。正しい終了通知が欠落した場合は、ゲーム状態・既知イベントが2秒途絶すると不完全な記録として停止します。待機・停止中のセッションは120秒の無通信、新しい開始設定、ロビー設定、明示停止で閉じます。これらは通信に基づく区間であり、画面上の厳密なプレイ境界やクリア判定ではありません。`manifest.json`に閉鎖理由と不完全状態を残します。

要求するHDMI入力モードの既定は1920×1080・60fpsで、`--hdmi-video-size`と`--hdmi-framerate`で変更できます。入力モードをそのままPNGに保存し、リサイズ・WebP変換・出力fpsの複製は行いません。PNGの圧縮レベルは0、予測はなしです。入力のUYVYからRGBへの変換とPNG符号化は必要です。60fpsの保存成立はデバイス・CPU・ストレージに依存します。1080p・RGB・60fpsの圧縮なし画像本体は約373MB/秒となるため、保存容量とmanifestの実フレーム数・PNG枚数を確認してください。

画像名の時刻はFFmpegが入力を受けたホストwallclock由来のUNIXマイクロ秒です。HDMI露光時刻やゲーム内`GameFrame`ではなく、映像経路の遅延は未校正です。`packets.jsonl`は元のレコード番号と保存PCAP内の番号を対応させ、当時の認証済み復号フィールドを残します。鍵交換を除くPCAP単独では後から復号できない場合があるため、学習生成にはこのJSONLとmanifestを利用できます。鍵はデータセットへ保存しません。PNG数とFFmpegの報告数の不一致、取得停止・書込エラーは不完全として扱い、所有する取得プロセスを停止します。

## コマンド・オプション

`inspect PCAP` は既存の classic PCAP を解析します。`capture` は macOS、Linux、Windows の有線 NIC から保存しながら解析します。`export-table --elf FILE --output FILE` は確認済み 11.3.0 ELF から参照表を作り、ほかの鍵源は受け付けません。

| オプション・引数 | 説明 |
| --- | --- |
| `inspect PCAP` | 読み込む PCAP のパス。`inspect` で必須 |
| `--interface IF` | 取得する NIC 名。Windows では `dumpcap -D` の番号も指定可能。`capture` で必須 |
| `--output FILE` | 単一PCAPの保存先。HDMI未指定の`capture`、または`export-table`で必須。既存ファイルは上書きしない |
| `hdmi-devices` | macOSのAVFoundationデバイス一覧。鍵・パケット取得は不要 |
| `capture --hdmi-device DEVICE` | macOSのHDMIデバイス名または番号。ゲーム内のUUIDディレクトリへPNGとPCAPを保存 |
| `--in-game-dir DIR` | HDMI記録のルート。既定`captures/in_game`、HDMI指定時のみ使用 |
| `--hdmi-framerate FPS` | HDMI入力に要求するfps。既定60。実保存fpsの保証ではない |
| `--hdmi-video-size WIDTHxHEIGHT` | HDMI入力モード。既定1920x1080。リサイズはしない |
| `--ffmpeg FILE` | HDMI取得・デバイス一覧で使うFFmpeg実行ファイル。既定`ffmpeg` |
| `--random-table FILE` | 272 バイトの外部参照表から GameKey を計算。現実装では 11.3.0 の確認済み GameKey に一致する表のみ受け付ける |
| `--game-key HEX` | 32 桁の 16 進 GameKey。`--game_key` も同じ指定 |
| `--game-key-file FILE` | 16 バイトの生 GameKey を読み込む |
| `--elf FILE` | 確認済み 11.3.0 ELF から GameKey を計算。`export-table` では必須 |
| `--pia-key-file FILE` | 途中から記録した通信向けに、16 バイトの生 PiaKey を読み込む。`--peer` と併用 |
| `--peer IPv4` | 手動指定した PiaKey の通信相手。`--pia-key-file` と併用 |
| `--broadcast-ip IPv4` | ブロードキャスト IP の自動推定を上書きし、BrowseRequest と BrowseReply の認証に使う |
| `--target-mac MAC` | 対象機の MAC を基準に送受信方向を表示。`--target-ip` とは併用不可 |
| `--target-ip IPv4` | 対象機の IPv4 を基準に送受信方向を表示。`--target-mac` とは併用不可 |
| `--jsonl` | 1 行 1 JSON オブジェクトで出力。送受信アドレスと全フィールドを含む |
| `--mode MODE` | `normal`（既定）は RTTProtocol、Empty、識別できない BroadcastReliableProtocol、GroupId を確認できない UnreliableProtocol を省略。Protocol 5 の GroupId 1104・1107・1108 と認証済み PrivateSetting は表示する。`debug` は全行を表示する。`salmon` は認証済みの既知 Salmon Run 行と対象のプレイヤー復活・デス通知を表示し、`capture` では該当する元の PCAP レコードだけを保存する |
| `--details` | 通常表示に全フィールド、ビット範囲、復号済み本文の HEX、先読みの名目 tick などを追加。JSONL にはヘッダーの送受信 ID と本文 HEX を追加 |
| `--hide-cycle` | `--mode debug` で、対応する応答から遅延を計算できた RTTProtocol 行を残し、他の RTTProtocol と本文未確認の UnreliableProtocol を省略 |
| `--visualizer-url URL` | 解析イベントをローカルの InkScope Visualizer に WebSocket 送信。`INKSCOPE_VISUALIZER_TOKEN` と併用 |
| `capture --control-port PORT` | `127.0.0.1` に鍵制御 API を起動。`INKSCOPE_CONTROL_TOKEN` と併用 |
| `-h`、`--help` | ヘルプを表示 |

`inspect` と `capture` では通常、`--random-table`、`--game-key`、`--game-key-file`、`--elf` のちょうど1つが必要です。`capture --control-port` の場合だけ、鍵なしで起動して後から Visualizer の Settings で登録できます。起動時に指定した鍵は制御 API から上書きできません。登録・解除は次の有効な UDP レコードを処理する直前に反映されます。別バージョンの GameKey は `--game-key` または `--game-key-file` で指定できますが、そのバージョンの本文解析は未検証です。鍵ファイルと参照表は利用者本人だけが読める権限で保管してください。`--game-key` はシェル履歴・プロセス引数に残ります。`--mode` と `--hide-cycle` の省略設定は `--jsonl` にも適用されます。方向の `->` は対象からの送信、`<-` は対象への送信です。

## Visualizer 接続

Visualizer を `127.0.0.1:10252` で起動し、両プロセスに同じ `INKSCOPE_VISUALIZER_TOKEN` を設定します。Settings から GameKey を登録する場合は、別の `INKSCOPE_CONTROL_TOKEN`（32 文字以上の秘密の ASCII 文字列）を両プロセスへ設定します。鍵をコマンドライン引数に載せずに起動する例です。

~~~sh
dist/inkscope capture --interface en18 \
  --output "$HOME/captures/capture.pcap" \
  --visualizer-url ws://127.0.0.1:10252/api/ingest \
  --control-port 10254
~~~

PCAP 保存は表示用のフィルターと独立しており、`--mode normal` でも取得対象の全パケットを保存します。テキスト表示では、確認済みの Protocol 5 状態を `CoopSequenceNetState GroupId=1104 ...`、`CoopSeqPlayerNetState GroupId=1107 ...` のように示します。GroupId を確認できない行に推測値は付けません。JSONL の `kind` にも識別済みの状態名が残ります。`CaptureResponseDelayMs` は認証済み RTTProtocol 要求から対応する応答までのキャプチャ時刻の差で、送信側の `ElapsedMicroseconds` や通信路だけの RTT とは異なります。

`capture --mode salmon` は例外的に PCAP 保存も選別します。認証できた `Coop...`、`PlayerNetEvent::Coop...`、`OnlineCoopSetting`、`PrivateSetting`、および正確な名前の `PlayerNetEvent::Revival`・`PlayerNetEvent::TroubleDie` の行がある UDP レコードを、元の暗号化済みバイト列のまま保存します。対象には汎用のプレイヤー通知2種を含みます。1 つの UDP に複数の行がある場合はレコード全体を保存します。鍵交換用の Browse 通信や識別できない通信は含めません。出力は classic PCAP として開けますが、後から `inspect` で復号するにはその部屋の PiaKey を別途 `--pia-key-file` と `--peer` で渡す必要があります。`inspect` 自体が復号表示を行うため、追加の `decrypt` コマンドは不要です。鍵が不足している間は関連パケットも選別できません。該当レコードがなければ 24-byte ヘッダーだけの PCAP になります。取得対象の全記録を残したい場合は `normal` または `debug` を使ってください。

Protocol 5 の GroupId 1107/1108 の 109-byte 形と GroupId 1104 内の 1107 入れ子形について、所有する 11.3.0 ELF の `PlayerNetState` encoder と形が一致する先頭三軸を `PlayerStateX/Y/Z` として抽出します。行には `PlayerStateCoordinateMeaning=Unknown` を付けます。GroupId と通信元で観測ストリームを区別できますが、画面の P1〜P4 枠や現在のワールド座標との対応は未検証です。四人の位置として表示・集計しないでください。

`OnlineCoopSetting` の通常表示では GameFrame、StageResource、StageName、JobType、Difficulty と4語の GameSeed を各項目1行ずつ表示します。HeaderPacketId、HeaderCompressedFlag、PayloadBytes、TypeId、DifficultyWire、値が0の LifeNumber は通常表示から省き、`--details` と JSONL には保持します。`GameSeed[0]`〜`GameSeed[3]` は順番どおり、テキストでは `0x` 付き8桁の HEX で表示します。JSONL の値はデコーダーの十進表記のままです。

認証済み `CoopEnemySpawnNetEvent` のオオモノ ID 4〜14 には `BossRequestObservationOrder` を付けます。これは受信した**生成要求の行順**であり、再送などの重複は除外していません。要求座標が 11.3.0 の静的配置点に一意かつ近距離で一致したときだけ、内部の方向グループ・潮位の候補を `static position candidate` として表示します。配置点との一致は実際の敵の生成、現在座標、実行時の方向確定を意味しません。

管理対象の `$HOME/.config/inkscope/pia_key.bin` を `capture` に `--pia-key-file` と `--peer` で渡すと、同じ部屋の途中からでも復号できます。保存鍵が対象相手の正しい形式のパケットを3件連続で認証できず、ほかの候補鍵でも認証できない場合は、ファイルが起動後に変更されていないことを確認して削除し、入り直しを案内します。新しい部屋の鍵を認証できた場合は 0600 で保存し直します。`inspect` は鍵ファイルを更新・削除しません。

`PrivateSetting` の `StageId` は固定長文字列です。空文字はテキストでは `StageId=""` と表示し、JSONL では空文字のまま保持します。Random 選択との対応は画面操作と通信の照合に基づく解釈で、通信に数値 `-1` が入っているという意味ではありません。

`capture` の外部ツールと権限は OS によって異なります。`--interface` には対象の **Ethernet** インターフェースを指定してください。PCAP リーダーは現在 Ethernet/IPv4/UDP のみ対応し、Linux の `any` やループバックなど別のリンク形式は解析できません。Unix の新規 PCAP は 0600 で作成し、Windows では保存先の ACL を引き継ぎます。

| OS | 外部ツール | インターフェース一覧と権限 |
| --- | --- | --- |
| macOS | `/usr/sbin/tcpdump` | `ifconfig -l` で一覧表示。通常ユーザーでの取得には [ChmodBPF](https://formulae.brew.sh/cask/wireshark-chmodbpf) の初回設定が必要 |
| Linux | `/usr/sbin/tcpdump` または `/usr/bin/tcpdump` | `tcpdump -D` で一覧表示。キャプチャ権限が必要。例えば tcpdump に必要な Linux capabilities を管理者が設定する |
| Windows | `%ProgramFiles%\Wireshark\dumpcap.exe` または `%ProgramFiles(x86)%\Wireshark\dumpcap.exe` | `dumpcap -D` で一覧表示。Wireshark と Npcap のインストール、および Npcap で許可されたキャプチャ権限が必要。番号またはデバイス名を `--interface` に指定する。ポータブル版や別の場所へのインストールは現状検出しない |

例えば Linux では `inkscope capture --interface eth0 --output capture.pcap --random-table table.bin`、Windows では `inkscope.exe capture --interface 1 --output capture.pcap --random-table table.bin` と実行できます。[Docker Desktop のホストネットワーク](https://docs.docker.com/engine/network/drivers/host/)は物理 NIC にパケット層でアクセスできず、distroless 実行イメージにもキャプチャツールはないため、Docker イメージは保存済み PCAP の `inspect` 用です。

## WAVE 先読み

OnlineCoopSetting から GameSeed を得ると、WAVE 0〜4 のイベントと潮位の**条件付き候補**を表示します。計算に使うステージなどの参照データは 11.3.0 由来で、他バージョンや実際に適用された結果との一致は未確認です。計算条件は[先読み crate](crates/ocean-calc/README.md)に記載しています。

## プロトコル対応状況

型名だけの行は本文を解析していません。意味フィールドまで完全に解析済みのプロトコルはまだありません。

| ID | プロトコル | 現在の解析 | 主な未解明部分 |
| ---: | --- | --- | --- |
| 0 | Empty | 空の keepalive 形 | 別の形の有無 |
| 1 | NetProtocol | 一部 subtype、Sequence、Generation、Reason、Detail | 残る subtype と本文 |
| 2 | TurnProtocol | 型名 | 本文 |
| 3 | RTTProtocol | 21 バイト形の Type、OriginTick、ElapsedMicroseconds、DestinationVariableId、ProtocolStationVariableId、対応する応答の CaptureResponseDelayMs | 他の形 |
| 4 | SyncProtocol | 型名 | 本文 |
| 5 | UnreliableProtocol | CoopSequenceNetState、CoopSeqPlayerNetState の一部 | 残る配列・Group |
| 6 | CloneProtocol | 型名 | 本文 |
| 7 | EventProtocol | 型名 | 本文 |
| 8 | BroadcastEventProtocol | 観測済み 39 型の名前、7 型の本文の一部 | 残るイベント本文 |
| 9 | ClockProtocol | 型名 | 本文 |
| 10 | ReliableProtocol | 型名 | 本文 |
| 11 | BroadcastReliableProtocol | 確認済み 78/161/226 バイト形の PrivateSetting、StageId、内部整数 Difficulty | 残る PrivateSetting 形式と本文 |
| 12 | StreamBroadcastReliableProtocol | 型名 | 本文 |
| 13 | SessionProtocol | subtype 番号 | subtype 本文 |
| 14 | NatTraversalResultProtocol | 型名 | 本文 |
| 15 | MonitoringDataProtocol | 型名 | 本文 |
| 16 | WanNatTraversalProtocol | 型名 | 本文 |
| 17 | Reckoning1DProtocol | 型名 | 本文 |
| 18 | Reckoning3DProtocol | 型名 | 本文 |

ELF に登録された Coop 系イベント 54 種のうち、通信で型名を確認できたのは 16 種、本文の一部を解析できたのは 6 種です。入力は classic PCAP の Ethernet/IPv4/UDP、ライブ保存は macOS・Linux・Windows に対応します。

.devcontainer と GitHub Actions は [qtmleap/devcontainers の Rust example](https://github.com/qtmleap/devcontainers/tree/789699a52c594e0a0015441c6d57c602f994dc9a/examples/rust)を基にしています。Docker の実行イメージは distroless です。データの由来は[DATA_NOTICE.md](DATA_NOTICE.md)を参照してください。
