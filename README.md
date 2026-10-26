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

## コマンド・オプション

`inspect PCAP` は既存の classic PCAP を解析します。`capture` は macOS、Linux、Windows の有線 NIC から保存しながら解析します。`export-table --elf FILE --output FILE` は確認済み 11.3.0 ELF から参照表を作り、ほかの鍵源は受け付けません。

| オプション・引数 | 説明 |
| --- | --- |
| `inspect PCAP` | 読み込む PCAP のパス。`inspect` で必須 |
| `--interface IF` | 取得する NIC 名。Windows では `dumpcap -D` の番号も指定可能。`capture` で必須 |
| `--output FILE` | 保存先。`capture` または `export-table` で必須。既存ファイルは上書きしない |
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
| `--mode MODE` | `normal`（既定）は Empty、BroadcastReliableProtocol、型不明の UnreliableProtocol を省略。`debug` は表示する |
| `--details` | 通常表示に全フィールド、ビット範囲、復号済み本文の HEX、先読みの名目 tick などを追加。JSONL にはヘッダーの送受信 ID と本文 HEX を追加 |
| `--hide-cycle` | RttProtocol の全行と、本文の確認が済んでいない UnreliableProtocol の行を省略 |
| `-h`、`--help` | ヘルプを表示 |

`inspect` と `capture` では、`--random-table`、`--game-key`、`--game-key-file`、`--elf` のちょうど1つが必要です。別バージョンの GameKey は `--game-key` または `--game-key-file` で指定できますが、そのバージョンの本文解析は未検証です。鍵ファイルと参照表は利用者本人だけが読める権限で保管してください。`--game-key` はシェル履歴・プロセス引数に残ります。`--mode` と `--hide-cycle` の省略設定は `--jsonl` にも適用されます。方向の `->` は対象からの送信、`<-` は対象への送信です。

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
| 3 | RttProtocol | 21 バイト形の Type、OriginTick、ElapsedMicroseconds | 他の形 |
| 4 | SyncProtocol | 型名 | 本文 |
| 5 | UnreliableProtocol | CoopSequenceNetState、CoopSeqPlayerNetState の一部 | 残る配列・Group |
| 6 | CloneProtocol | 型名 | 本文 |
| 7 | EventProtocol | 型名 | 本文 |
| 8 | BroadcastEventProtocol | 観測済み 39 型の名前、7 型の本文の一部 | 残るイベント本文 |
| 9 | ClockProtocol | 型名 | 本文 |
| 10 | ReliableProtocol | 型名 | 本文 |
| 11 | BroadcastReliableProtocol | 161 バイト形の値候補 | PrivateSetting 本文と値の意味 |
| 12 | StreamBroadcastReliableProtocol | 型名 | 本文 |
| 13 | SessionProtocol | subtype 番号 | subtype 本文 |
| 14 | NatTraversalResultProtocol | 型名 | 本文 |
| 15 | MonitoringDataProtocol | 型名 | 本文 |
| 16 | WanNatTraversalProtocol | 型名 | 本文 |
| 17 | Reckoning1DProtocol | 型名 | 本文 |
| 18 | Reckoning3DProtocol | 型名 | 本文 |

ELF に登録された Coop 系イベント 54 種のうち、通信で型名を確認できたのは 16 種、本文の一部を解析できたのは 6 種です。入力は classic PCAP の Ethernet/IPv4/UDP、ライブ保存は macOS・Linux・Windows に対応します。

.devcontainer と GitHub Actions は [qtmleap/devcontainers の Rust example](https://github.com/qtmleap/devcontainers/tree/789699a52c594e0a0015441c6d57c602f994dc9a/examples/rust)を基にしています。Docker の実行イメージは distroless です。データの由来は[DATA_NOTICE.md](DATA_NOTICE.md)を参照してください。
