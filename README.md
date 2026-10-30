# InkScope

Splatoon 3（`0100C2500FC20000`）のLAN Play通信（Piaプロトコル / UDP 35000, 49152）をリアルタイムにキャプチャ・認証・復号し、コンソールやWeb UIに表示するツールです。

- **パケットキャプチャ＆リアルタイム復号**: 部屋発見（Browse）からPia暗号鍵を自動導出してゲーム中パケットを復号・解析。
- **WAVE情報の先読み**: シーンシードからイベント・潮位・名目オオモノ出現時刻を事前計算（`ocean-calc` 連携）。
- **HDMI映像と通信の同期保存**: 1080p 60fpsのロスレスPNG画面とPCAP・復号JSONLを試合単位（UUID）で記録。
- **Visualizer連携**: ローカルのInkScope Visualizerと連携し、LIVE表示・Replay再生・Settingsからの安全な鍵管理に対応。

> [!NOTE]
> 本リポジトリに暗号鍵ファイルは含まれません。利用には所有するゲーム環境から取得した正規の鍵が必要です。

---

## 目次

1. [ビルド手順](#ビルド手順)
2. [基本的な使い方](#基本的な使い方)
3. [開発・継続取得スクリプト (capture.sh)](#開発継続取得スクリプト-capturesh)
4. [ゲーム内HDMI画像と通信の同期保存](#ゲーム内hdmi画像と通信の同期保存)
5. [Visualizer連携](#visualizer連携)
6. [コマンド・オプション一覧](#コマンドオプション一覧)
7. [プロトコル対応状況](#プロトコル対応状況)
8. [OS別の実行要件](#os別の実行要件)

---

## ビルド手順

Rust（Cargo）ツールチェーンが必要です。

```sh
# ワークスペース全体のリリースビルド
cargo build --release --locked --workspace

# バイナリを作業用ディレクトリへ配置
mkdir -p dist
install -m 755 target/release/inkscope dist/inkscope
install -m 755 target/release/ocean-calc dist/ocean-calc

# ビルド成果物の一時ファイルを削除（任意）
cargo clean
```

---

## 基本的な使い方

### 1. 保存済みPCAPの解析 (`inspect`)

既存のPCAPファイルを読み込み、通信構造や復号本文を表示します。

```sh
dist/inkscope inspect "$HOME/captures/capture.pcap" \
  --random-table "$HOME/.config/inkscope/table.bin" \
  --target-mac 02:00:00:00:00:01
```

### 2. 有線NICからのライブキャプチャ (`capture`)

ネットワークインターフェースを指定して直接キャプチャと解析を行います。

```sh
mkdir -p "$HOME/captures"
dist/inkscope capture --interface en18 \
  --output "$HOME/captures/capture.pcap" \
  --random-table "$HOME/.config/inkscope/table.bin" \
  --target-mac 02:00:00:00:00:01
```

> [!TIP]
> **macOS での権限設定**:
> 一般ユーザーでパケットキャプチャを行うには、初回のみ `brew install --cask wireshark-chmodbpf` を実行してOSを再起動してください（インストール時に管理者権限が必要です）。

---

## 開発・継続取得スクリプト (`capture.sh`)

開発時にソースコードの変更を監視し、自動リビルドと安全なキャプチャ再起動を行うスクリプトです。

```sh
./capture.sh
```

### 主な特徴と動作仕様

- **安全なホットリロード**:
  ソースコードやCargo設定の変更を検知するとバックグラウンドでビルドを実行します。ビルドに成功した場合のみ新しいキャプチャプロセスを起動し、正常稼働を確認してから古いプロセスを停止します（ビルド失敗時は既存のキャプチャを中断しません）。
- **クラッシュ時の自動復帰**:
  キャプチャプロセスが予期せず終了した場合は、直前の正常バイナリを用いてバックオフ間隔を空けて自動再起動します。
- **排他制御とファイル保護**:
  再起動ごとにパーミッション `0600` の新しいPCAPファイルを `$HOME/captures` に生成します。Ctrl-C でスクリプトが自身で起動したプロセスのみを停止し、他の端末で実行中のキャプチャには影響を与えません。
- **ビルド事前確認**:
  `./capture.sh --build-only` を実行すると、キャプチャを開始せずにビルドの成否のみをテストできます。

### 取得モード (`--mode`)

- `normal`（既定）: 取得対象の全パケットを新しいPCAPに保存します（ノイズとなる一部制御パケットは画面表示のみ省略）。
- `debug`: 全パケットを保存し、コンソールにも全パケットの詳細を出力します。
- `salmon`: 認証済みのサーモンラン通信、およびプレイヤーの復活・デス通知のみを選別してPCAPに保存します。

### インターフェース自動判定と環境変数 (macOS)

macOS では通信相手（デフォルト: `192.168.1.180`）へのルーティング情報に基づき、同一ネットワークに直結されている有効な `en` デバイスを自動選択します（VPNや仮想NIC、別ネットワークへのゲートウェイしかない場合は誤動作を防ぐため起動を中断します）。

ミラーポート等でIPを持たないNICを明示指定する場合は、環境変数で上書きしてください。

| 環境変数 | 既定値 | 説明 |
| --- | --- | --- |
| `INKSCOPE_INTERFACE` | 自動判定 | キャプチャ対象のNIC名（例: `en13`） |
| `INKSCOPE_PEER` | `192.168.1.180` | 通信相手のSwitchのIPv4アドレス |
| `INKSCOPE_MODE` | `normal` | 動作モード（`normal`, `debug`, `salmon`。コマンド引数 `--mode` が優先） |
| `INKSCOPE_CAPTURE_DIR` | `$HOME/captures` | PCAPファイルの保存先ディレクトリ |

- **鍵の参照先**:
  GameKey: `$HOME/.config/inkscope/game_key.bin`  
  PiaKeyキャッシュ: `$HOME/.config/inkscope/pia_key.bin`（パーミッション `0600`）

---

## ゲーム内HDMI画像と通信の同期保存

HDMIキャプチャデバイスからの非圧縮映像とパケット通信を、試合単位のUUIDディレクトリへ同時記録します。

### 実行例

```sh
# 利用可能なHDMIキャプチャデバイスの一覧表示（鍵不要）
dist/inkscope hdmi-devices

# HDMI映像とパケットの同時キャプチャ開始
dist/inkscope capture --interface en18 \
  --hdmi-device "HDMI capture device" \
  --in-game-dir "$HOME/captures/in_game" \
  --game-key-file "$HOME/.config/inkscope/game_key.bin"
```

### 保存ディレクトリ構造

```text
captures/in_game/<UUID>/
  capture.pcap          # 試合中の暗号化パケット通信
  packets.jsonl         # 認証・復号済みフィールドとPCAPレコード番号の対応
  manifest.json         # 機器設定、実保存フレーム数、終了理由
  screeshots/           # 各フレームの非圧縮PNG画像（※綴りは仕様準拠）
    segment-000001-<UNIXマイクロ秒>.png
    segment-000002-<UNIXマイクロ秒>.png
```

### ライフサイクルとトリガー条件

- **開始条件**:
  部屋探索前からキャプチャを起動しておきます。認証済みの `OnlineCoopSetting` 受信時にセッションUUIDを発行し、ホストからの正規な `CoopSequenceNetState` を受信した瞬間に画像保存を開始します（ロビー待機中の `PrivateSetting` や事前ハンドシェイクでは画像保存は開始されません）。
- **WAVE進行と終了**:
  各WAVEの終了（`CoopSeqRoundFinishEvent`）で画像保存を一時停止します。次WAVE開始時は同一UUIDディレクトリ内に連番で保存を再開します（WAVE間およびリザルト画面は保存されません）。
- **中断・閉鎖判定**:
  ゲーム状態パケットが2秒間途絶した場合は不完全な記録として停止します。また、120秒間の無通信、新しい開始設定、または明示的な停止操作によってセッションが正常にクローズされます。

### 映像仕様と容量に関する注意

- **既定設定**: 1920×1080・60fps（`--hdmi-video-size` と `--hdmi-framerate` で変更可能）。
- **画像形式**: 入力UYVYからRGBへ変換した圧縮なしPNG（圧縮レベル0）。
- **帯域・ストレージ要件**:
  1080p / RGB / 60fpsの非圧縮記録では**約373 MB/秒**のデータが生成されます。ディスク空き容量と書き込み速度を事前にご確認ください。
- **時刻情報**:
  画像ファイル名のタイムスタンプは、ホストPCがFFmpeg経由で入力を受け取ったUNIXマイクロ秒（wallclock）です。露光時刻やゲーム内フレーム（`GameFrame`）そのものではなく、キャプチャ機器の伝送遅延を含みます。

---

## Visualizer連携

ローカルで起動した [InkScope Visualizer](file:///Users/devonly/Developer/personal/InkScope-Visualizer) とWebSocket経由で接続し、リアルタイム表示や過去セッションの再生を行えます。

```sh
dist/inkscope capture --interface en18 \
  --output "$HOME/captures/capture.pcap" \
  --visualizer-url ws://127.0.0.1:10252/api/ingest \
  --control-port 10254
```

- **リアルタイム表示 (LIVE)**:
  `../InkScope-Visualizer/.env.local` に `INKSCOPE_VISUALIZER_TOKEN` を設定することで、キャプチャと同時に解析イベントがVisualizerへストリーミング送信されます。
- **安全な鍵管理 (Settings API)**:
  `INKSCOPE_CONTROL_TOKEN`（32文字以上の秘密文字列）を設定して `--control-port 10254` を起動すると、VisualizerのSettings画面からWeb経由でGameKeyを登録できます。コマンドライン引数やプロセスリストに秘密鍵を残さずに運用可能です。
- **リプレイ再生 (Replay)**:
  送信されたイベントはVisualizer背後のPostgreSQLに蓄積され、過去のセッションを選択して再生・検証できます。

---

## コマンド・オプション一覧

| オプション・引数 | 必須条件 | 説明 |
| --- | :---: | --- |
| `inspect PCAP` | `inspect` 時必須 | 解析するPCAPファイルのパス |
| `--interface IF` | `capture` 時必須 | キャプチャ対象の有線NIC名（Windowsでは `dumpcap -D` の番号） |
| `--output FILE` | 通常 `capture` 必須 | PCAPファイルの保存先パス（既存ファイルは上書き不可） |
| `hdmi-devices` | - | macOS AVFoundationのHDMIデバイス一覧を表示（鍵不要） |
| `--hdmi-device DEVICE` | HDMI保存時 | HDMIキャプチャデバイス名または番号。UUIDディレクトリへ保存 |
| `--in-game-dir DIR` | - | HDMI記録の保存ルート（既定: `captures/in_game`） |
| `--hdmi-framerate FPS` | - | 要求fps（既定: `60`） |
| `--hdmi-video-size WxH` | - | 入力解像度（既定: `1920x1080`） |
| `--ffmpeg FILE` | - | FFmpegバイナリのパス（既定: `ffmpeg`） |
| `--random-table FILE` | ※1 | 272バイトの参照表から11.3.0のGameKeyを算出 |
| `--game-key HEX` | ※1 | 32桁の16進数GameKey文字列（プロセス引数に残るため注意） |
| `--game-key-file FILE` | ※1 | 16バイトの生GameKeyバイナリファイル |
| `--elf FILE` | `export-table` 必須 | 11.3.0 ELFバイナリからGameKeyを算出 |
| `--pia-key-file FILE` | ※2 | 16バイトの生PiaKeyバイナリファイル（途中参加・復号用） |
| `--peer IPv4` | ※2 | 手動指定したPiaKeyの通信相手SwitchのIPv4アドレス |
| `--broadcast-ip IPv4` | - | ブロードキャストIPの自動推定値を上書き（Browse検証用） |
| `--target-mac MAC` | 排他（※3） | 対象SwitchのMACアドレスを基準に送受信方向（`->` / `<-`）を表示 |
| `--target-ip IPv4` | 排他（※3） | 対象SwitchのIPv4アドレスを基準に送受信方向を表示 |
| `--mode MODE` | - | 出力モード（`normal` / `debug` / `salmon`、詳細は前述） |
| `--details` | - | 全ビット範囲、復号HEXダンプ、WAVE先読みtickなどを追加表示 |
| `--hide-cycle` | - | debugモード時に未対応のRTT要求や未確認Unreliableパケットを省略 |
| `--visualizer-url URL` | - | InkScope VisualizerのWebSocketインジェストURL |
| `--control-port PORT` | - | Visualizerからの鍵登録を受け付ける制御APIポート（`127.0.0.1`） |
| `--jsonl` | - | 1行1JSON形式で全フィールドと送受信アドレスを出力 |
| `-h`, `--help` | - | ヘルプメッセージを表示 |

- **※1 鍵の指定要件**: `inspect` および通常 `capture` では、`--random-table`、`--game-key`、`--game-key-file`、`--elf` のうち**いずれか1つ**が必須です（`--control-port` 使用時は起動後のWeb登録が可能）。
- **※2 途中参加時のPiaKey**: `--pia-key-file` と `--peer` は必ずペアで指定します。
- **※3 送受信方向の基準**: `--target-mac` と `--target-ip` は併用できません。

---

## プロトコル対応状況

Piaプロトコル各層の解析状況です（型名のみ表示されるパケットは本文未デコードです）。

| ID | プロトコル名 | 現在の解析状況 | 主な未解明・検証中フィールド |
| ---: | --- | --- | --- |
| 0 | `Empty` | 空のkeepaliveフレーム | 他のバリアントの有無 |
| 1 | `NetProtocol` | 一部subtype、Sequence、Generation、Reason、Detail | 未対応subtypeおよび本文ペイロード |
| 2 | `TurnProtocol` | 型名のみ識別 | 本文ペイロード |
| 3 | `RTTProtocol` | 21バイト形（Type, OriginTick, ElapsedMicroseconds, VariableId, 応答遅延Ms） | 他のフレーム長バリアント |
| 4 | `SyncProtocol` | 型名のみ識別 | 本文ペイロード |
| 5 | `UnreliableProtocol` | `CoopSequenceNetState`, `CoopSeqPlayerNetState`（Group 1104/1107/1108） | 未知配列・内部フラグ |
| 6 | `CloneProtocol` | 型名のみ識別 | 本文ペイロード |
| 7 | `EventProtocol` | 型名のみ識別 | 本文ペイロード |
| 8 | `BroadcastEventProtocol` | **観測済み39種類のイベント型**（アイテム生成・取得・納品、オオモノ生成等） | 残るイベントの本文構造 |
| 9 | `ClockProtocol` | 型名のみ識別 | 本文ペイロード |
| 10 | `ReliableProtocol` | 型名のみ識別 | 本文ペイロード |
| 11 | `BroadcastReliableProtocol` | 78/161/226バイト形（`PrivateSetting`, `StageId`, 内部難易度） | 未知のPrivateSettingバリアント |
| 12 | `StreamBroadcastReliableProtocol` | 型名のみ識別 | 本文ペイロード |
| 13 | `SessionProtocol` | subtype番号の識別 | 各subtypeの内部ペイロード |
| 14 | `NatTraversalResultProtocol` | 型名のみ識別 | 本文ペイロード |
| 15 | `MonitoringDataProtocol` | 型名のみ識別 | 本文ペイロード |
| 16 | `WanNatTraversalProtocol` | 型名のみ識別 | 本文ペイロード |
| 17 | `Reckoning1DProtocol` | 型名のみ識別 | 本文ペイロード |
| 18 | `Reckoning3DProtocol` | 型名のみ識別 | 本文ペイロード |

### プレイヤー状態・座標に関する留意点

- Protocol 5（Group 1107 / 1108）から抽出される `PlayerStateX/Y/Z` は、ELFの `PlayerNetState` エンコーダーとビット幅・位置が一致する三軸候補値ですが、画面上のプレイヤー枠（P1〜P4）やワールド座標への完全なマッピングは未確定です（`PlayerStateCoordinateMeaning=Unknown` として出力されます）。
- `CoopEnemySpawnNetEvent` の `BossRequestObservationOrder` は受信した生成要求の順序であり、実際の出現成功や再送除外を意味するものではありません。

---

## OS別の実行要件

ライブキャプチャにはOSごとのキャプチャツールと管理者/実行権限が必要です（Ethernet/IPv4/UDPパケットのみ対応）。

| OS | 使用ツール | インターフェース確認方法と必要権限 |
| --- | --- | --- |
| **macOS** | `/usr/sbin/tcpdump` | `ifconfig -l`<br>一般ユーザーでの実行には [ChmodBPF](https://formulae.brew.sh/cask/wireshark-chmodbpf) のインストールとOS再起動が必要 |
| **Linux** | `tcpdump` | `tcpdump -D`<br>実行ユーザーへのキャプチャ権限（Linux Capabilities 等）の付与が必要 |
| **Windows** | `dumpcap.exe` | `dumpcap -D`<br>WiresharkおよびNpcapのインストールとキャプチャ権限が必要（`--interface` に番号またはデバイス名を指定） |

> [!WARNING]
> Dockerコンテナ（Docker Desktop）環境では物理NICへの直接パケットアクセスが制限されるため、Dockerイメージは保存済みPCAPの解析（`inspect`）専用となります。

---

## 関連ドキュメント・ライセンス

- **WAVE先読みロジック**: [`crates/ocean-calc/README.md`](crates/ocean-calc/README.md)
- **データの取り扱い基準**: [`DATA_NOTICE.md`](DATA_NOTICE.md)
- **開発ガイドライン**: [`AGENTS.md`](AGENTS.md)
