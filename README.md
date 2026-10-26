# InkScope

Splatoon 3 **11.3.0 のnative-IP LAN Play**で保存したPCAPを読むRustワークスペースです。`s3-packet-capture` はUDP 35000の部屋探索とUDP 49152のPia通信を解析し、共有鍵による認証、復号、Zstd展開、Piaチャンク分割の後に確認済みの型とフィールドを表示します。macOSでは`tcpdump`による受動的なライブ保存もできます。ゲーム機へパケットは送信しません。

`salmon-wave-lookahead` は別のRust crateです。11.3.0から調査したWAVE計算と、生成済みのステージ・レベル・武器候補データを含みます。パケットCLIは確認済みの`OnlineCoopSetting`から最初に`GameSeed[0..3]`を得た時点で、このcrateを使った**条件付きの生成イベント・潮位**を表示します。直近32種類の同じ設定の再送では繰り返さず、ステージ・JobType・難易度・シードが変われば再表示します。

## ビルド

```sh
cargo build --release --locked --workspace
mkdir -p dist
install -m 755 target/release/s3-packet-capture dist/s3-packet-capture
install -m 755 target/release/salmon-wave-lookahead dist/salmon-wave-lookahead
cargo clean
```

`cargo clean`後も`dist/`のローカル実行ファイルは残ります。`dist/`、`target/`、PCAP、ELF、鍵、参照表はGit管理から除外します。

## 保存PCAPの解析

`capture.pcap`を手元の**実在するファイルのパス**に置き換えてください。`/path/to/capture.pcap`は例示用の文字列で、そのままでは動きません。

```sh
PCAP="$HOME/captures/capture.pcap"
test -f "$PCAP" || { printf 'PCAPが見つかりません: %s\n' "$PCAP" >&2; exit 1; }
dist/s3-packet-capture inspect "$PCAP" \
  --random-table "$HOME/.config/s3-packet-capture/table-11.3.0.bin" \
  --target-mac 02:00:00:00:00:01 \
  --hide-cycle
```

`--target-mac`の値は対象マシンの実際のMACに置き換えます。`->`は対象からの送信、`<-`は対象への送信、`--`は対象外、`<->`は両端が対象です。ブロードキャストの実受信はPCAPだけでは確定しません。通常表示にはISO 8601 UTC時刻、方向、型名、確認済みの主な値を出し、IPアドレスは省略します。`--jsonl`には元の送受信アドレス、全フィールドとビット範囲が入ります。

### リピーターハブ経由のリアルタイム解析

リピーターハブ自体をソフトウェアから選択するのではなく、**そのハブにつないだMacの有線NIC**を`--interface`で選びます。`networksetup -listallhardwareports`と`ifconfig <NIC名>`などでNIC名・リンク状態を確認してください。以下の`en18`は例なので、現在の配線に合わせて変更します。`--target-mac`も解析対象の実際のMACへ置き換えます。

```sh
mkdir -p "$HOME/captures"
sudo dist/s3-packet-capture capture \
  --interface en18 \
  --output "$HOME/captures/inkscope-$(date -u +%Y%m%dT%H%M%SZ).pcap" \
  --random-table "$HOME/.config/s3-packet-capture/table-11.3.0.bin" \
  --target-mac 02:00:00:00:00:01 \
  --hide-cycle
```

`capture`はmacOSの`/usr/sbin/tcpdump`を起動し、UDP 35000/49152のPCAPを権限0600の新規ファイルへ保存しながら、同じデータを逐次復号・表示します。設定通知が届けば上記のWAVE先読みもその場で表示します。`Ctrl-C`でこのCLIが起動した`tcpdump`を終了します。BPF権限がない環境向けに例には`sudo`を付けています。PCAPはroot所有になるので、後で一般ユーザーとして扱う場合は表示された保存先を`sudo chown "$(id -un)" 保存先.pcap`で変更します。CLIは内部で`sudo`を起動せず、既存PCAPを上書きしません。ハブから対象の通信がNICへ流れていなければ、表示されるデータもありません。

### 復号に必要な鍵源

**`inspect`と`capture`には、`random_tables`に相当する外部参照表ファイルか`game_key`が必要です。** CLIでは`--random-table FILE`または`--game-key 32桁HEX`を指定します。正確には次の4種類から**ちょうど1つ**を指定します。

| オプション | 入力 |
| --- | --- |
| `--random-table FILE` | 272バイトの外部v1参照表。バイナリ内の`generate_game_key()`でGameKeyを計算 |
| `--game-key HEX` | 16バイトのGameKeyを32桁HEXで指定。`--game_key`も可 |
| `--game-key-file FILE` | 16バイトの生GameKeyをファイルから読む |
| `--elf FILE` | 確認済み11.3.0 ELFからGameKeyを計算 |

参照表は先頭16バイトが4個のlittle-endian u32初期状態、続く256バイトが64個のlittle-endian u32表です。権限0600でリポジトリ外に保管してください。手元に確認済みのELFがある場合は、存在しない出力先を指定して一度だけ作成できます。

```sh
dist/s3-packet-capture export-table \
  --elf "$HOME/private/Splatoon3/11.3.0/exefs/main.elf" \
  --output "$HOME/.config/s3-packet-capture/table-11.3.0.bin"
```

このリポジトリと配布バイナリには実際のGameKey、参照表、ELFを含めません。`--game-key`の値はシェル履歴とプロセス引数に残り得るため、秘匿する場合はファイル入力を使います。途中から録画したPCAPで部屋探索返信がない場合は、**上の鍵源に加えて**16バイトの生`PiaKey`を`--pia-key-file FILE --peer IPv4`で与えます。PiaKeyだけでは現在のCLIの鍵源要件を満たしません。GameKeyは検証済み11.3.0 ELFに対して固定ですが、PiaKeyは部屋の`SessionParameter`に依存します。

## `GameSeed`を得た時点の先読み表示

認証済みProtocol 8の**確認済み211バイト**`OnlineCoopSetting`だけが先読みの起点です。表示には`IsBigRun`の0/1と、イベント候補に影響する`global_word >> 8`の2区分を組み合わせた4行が出ます。各行にWAVE 0〜4の生成イベントIDと潮位IDを表示します。`--details`では対応するイベント0・5・6の名目上のRare予定tickも表示し、`--jsonl`では`wave_preview`に構造化して保存します。初期の特別潮位状態と予定表の分岐は通常経路を仮定します。

これは**条件付き計算**です。ネットワーク上のGameSeedが実効scene seedとして採用されたかは未確認です。設定通知の難易度が実効難易度に等しいこと、JobType 1でraw MatchModeが11であること、11.3.0の元リソース値がそのまま有効であることも仮定します。JobType 0の参加者平均と特別潮位スコアは未取得です。生成値は適用後のイベント・潮位、実際のオオモノ出現、位置ではありません。未知のステージや非対応JobTypeでは、結果を作らず理由を表示します。設定通知を含まない途中開始PCAPでは先読みは出ません。

独立の先読みCLIも利用できます。4語は十進数または`0x`付き16進数です。各入力条件の意味と未確認範囲は[先読みcrateのREADME](crates/wave-lookahead/README.md)を参照してください。

```sh
dist/salmon-wave-lookahead waves 1 2 3 4 Cop_Shakedent 1 150 0 0x10100
```

## プロトコル解析範囲

下表の「型名のみ」はProtocol IDを認識するだけで、本文の意味を解析したことを示しません。**意味フィールドまで完全に解析済みのプロトコルはまだありません。** 復号・フレーム分割と、個々のProtocol本文の意味解析は別の進捗です。

| ID | プロトコル | 現在の表示・解析 | 残る不明点 |
| ---: | --- | --- | --- |
| 0 | Empty | 確認済みの空keepalive形 | 他の形の有無 |
| 1 | NetProtocol | 一部のsubtype、Sequence/Generation、Reason/Detail | subtype全体と残りの本文 |
| 2 | TurnProtocol | 型名のみ | 本文全体 |
| 3 | RttProtocol | 確認済み21バイト形のType、OriginTick、ElapsedMicroseconds | 他の長さ・型の意味 |
| 4 | SyncProtocol | 型名のみ | 本文全体 |
| 5 | UnreliableProtocol | Group 1104の`CoopSequenceNetState`、1107/1108の`CoopSeqPlayerNetState`と一部フィールド | 残りの配列・player対応・未確認Group |
| 6 | CloneProtocol | 型名のみ | 本文全体 |
| 7 | EventProtocol | 型名のみ | 本文全体 |
| 8 | BroadcastEventProtocol | 観測済み39型IDの名前、7型の確認済み本文長と一部フィールド | 残るイベント本文と未観測型 |
| 9 | ClockProtocol | 型名のみ | 本文全体 |
| 10 | ReliableProtocol | 型名のみ | 本文全体 |
| 11 | BroadcastReliableProtocol | 特定161バイト形の`HazardCandidatePercent`候補 | PrivateSetting本文全体。候補値の意味も要検証 |
| 12 | StreamBroadcastReliableProtocol | 型名のみ | 本文全体 |
| 13 | SessionProtocol | subtype番号 | 各subtypeの本文 |
| 14 | NatTraversalResultProtocol | 型名のみ | 本文全体 |
| 15 | MonitoringDataProtocol | 型名のみ | 本文全体 |
| 16 | WanNatTraversalProtocol | 型名のみ | 本文全体 |
| 17 | Reckoning1DProtocol | 型名のみ | 本文全体 |
| 18 | Reckoning3DProtocol | 型名のみ | 本文全体 |

Protocol 8の39型は観測した型IDと名前の対応です。そのうち意味フィールドが確認できたのは7型で、残りは名前または型フィールドのみです。ELFに登録された54種類の`Coop*`イベントのうち、このProtocol 8として観測・名前対応できたのは16型、本文フィールドまで扱えるのは6型です。残る38型の通信上のProtocol IDは確認していないため、Protocol 8対応数に含めません。

通常モードでは`Empty`、`BroadcastReliableProtocol`、型を判別できない`UnreliableProtocol`を省略し、`--mode debug`で表示します。このフィルタは`--jsonl`にも適用されます。`--hide-cycle`は周期的なRTTと未確認のProtocol 5行を省略します。`--details`は認証済み本文のHEXとビット範囲を表示します。認証失敗した本文を平文扱いしません。JSONLと詳細出力にはGameSeed、個体識別子などが入る可能性があります。

classic PCAPのEthernet（VLAN最大2段）、IPv4、非断片UDPを対象にしています。pcapng、他リンク型、IPv6、IP断片は未対応です。ライブ保存はmacOSの`/usr/sbin/tcpdump`を使い、新規PCAPを権限0600で作ります。

## 開発環境

`.devcontainer/`と`.github/workflows/`は[qtmleap/devcontainers のRust example](https://github.com/qtmleap/devcontainers/tree/789699a52c594e0a0015441c6d57c602f994dc9a/examples/rust)を基に、2-crate workspace、バイナリ名、CI対象を調整しています。元exampleのMIT表示は[LICENSE](LICENSE)に保持しています。ゲーム由来の表については[DATA_NOTICE.md](DATA_NOTICE.md)を参照してください。devcontainerはホストの認証情報を自動マウントしません。GitHubや開発エージェントを使う場合はコンテナ内でログインします。Integration workflowはpushとPRでrustfmt、Clippy、ワークスペースのテストを実行します。コンテナ画像のGHCR公開は手動workflowのみで、実行段はdistrolessです。Docker画像は保存PCAPの`inspect`向けで、macOS専用ライブ`capture`には使いません。
