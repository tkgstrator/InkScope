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

リピーターハブに接続した Mac の有線 NIC から保存しながら解析する場合:

~~~sh
mkdir -p "$HOME/captures"
sudo dist/inkscope capture --interface en18 \
  --output "$HOME/captures/capture.pcap" \
  --random-table "$HOME/.config/inkscope/table.bin" \
  --target-mac 02:00:00:00:00:01
~~~

出力の -> は対象からの送信、<- は対象への送信です。必要に応じて --hide-cycle、--mode debug、--details、--jsonl を指定できます。

復号には **--random-table FILE または --game-key HEX** が必要です。--game-key-file FILE、--elf FILE も選べますが、4種類からちょうど1つを指定します。--elf による素材抽出は確認済みの 11.3.0 ELF 向けです。--game-key はシェル履歴・プロセス引数に残り、sudo で保存した PCAP は root 所有になります。

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

ELF に登録された Coop 系イベント 54 種のうち、通信で型名を確認できたのは 16 種、本文の一部を解析できたのは 6 種です。入力は classic PCAP の Ethernet/IPv4/UDP、ライブ保存は macOS に対応します。

.devcontainer と GitHub Actions は [qtmleap/devcontainers の Rust example](https://github.com/qtmleap/devcontainers/tree/789699a52c594e0a0015441c6d57c602f994dc9a/examples/rust)を基にしています。Docker の実行イメージは distroless です。データの由来は[DATA_NOTICE.md](DATA_NOTICE.md)を参照してください。
