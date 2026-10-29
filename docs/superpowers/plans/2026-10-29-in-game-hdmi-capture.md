# InkScope ゲーム内HDMI記録 実装計画

ユーザー指定: HDMIデバイスを指定し、ゲーム中の画像と通信を `captures/in_game/<UUID>/screeshots/*.png` と `capture.pcap` に保存します。リサイズ・WebP変換は行いません。`screeshots` は指定された綴りを保持します。

## 実装方針

- `capture --hdmi-device DEVICE` を追加し、通常の `--output FILE` と択一にします。保存ルートは `--in-game-dir` で指定でき、既定は `captures/in_game` です。
- 認証済み・正しい形式の `OnlineCoopSetting` でUUIDを開始し、正しい `CoopSequenceNetState` で画像保存を開始します。`CoopSeqRoundFinishEvent` で画像を停止し、同じSequenceの残留通信では再開しません。次WAVEは同じUUIDで再開します。
- `Session 5/6` を終了やクリアの判定に使いません。新試合・ロビー設定・無通信・明示停止の終了理由を記録し、成功や失敗は推定しません。無通信停止は欠落通知への保護であり、画面の厳密な境界ではありません。
- FFmpeg AVFoundationをmacOSのHDMI取得に使い、PNG圧縮レベル0・予測なし・元の入力解像度で保存します。取得モード指定はリサイズではありません。PNG符号化と入力画素形式からの色変換は残ります。
- 画像ファイル名にFFmpeg取得時のwallclock由来PTS（microseconds）を残し、その意味をmanifestに記録します。ハードウェア露光時刻やゲームフレームとみなしません。
- 画像保存と異常監視をPCAP読み取りの待ちから独立させます。異常時は所有子プロセスを停止し、不完全記録を保持します。
- PCAPの元レコードを保持し、鍵を含まない `packets.jsonl` と `manifest.json` を併記します。新規ディレクトリは0700、生成ファイルは本人だけが参照できる場所に置きます。

## 作業と検証

- [x] 純粋な区間判定をテストから実装: ロビー、認証失敗、短い形式、再送、WAVE間、別部屋、次試合。
- [x] HDMI子プロセスをテストから実装: shellを介さない引数、ネイティブPNG、時刻、異常終了、停止・wait、模擬FFmpeg。
- [x] CLIとPCAPループを統合: HDMIなしの既存動作、択一の引数、UUID出力、元PCAP・JSONL保存、独立watchdog。
- [x] 全workspaceテスト、fmt、clippyと独立した実差分レビューを行う。
- [ ] 実デバイスで短時間の有界記録を行い、PNGを開き、幅・高さ・実保存fps・PTS間隔・書込量を確認する。入力60fpsの認識だけでは保存60fpsと報告しない。

## 調査・レビュー状況

2026-10-29: LocalGPTはbrowser未接続かつ新規generation不可。Claude Code MCPは保持ジョブ128件の上限でレビュー開始前に拒否。代替として独立したCodexワーカーがプロトコル境界とHDMI経路を読み取り専用レビューし、上記の区間・PNG・同期・復号コンテキストの制約を反映しました。

Ruling: 現在のInkScopeに未コミットの解析・取得機能があり、それらを前提に統合するため、元ファイルを一時領域へ保存し、現在の作業ツリーへ必要な変更だけを追加します。既存変更の破棄・コミット・pushは行いません。

## 実装後の検証

2026-10-29: `cargo fmt --all -- --check`、`cargo clippy --locked --workspace --all-targets -- -D warnings`、`cargo test --locked --workspace --quiet`が成功し、テストは585件成功・0件失敗でした。リリースビルドも成功しました。ローカルRustツールチェーンのstrip用LLVMライブラリ不足を避け、このビルドは`CARGO_PROFILE_RELEASE_STRIP=none`を指定しています。

実差分レビューの指摘した、watchdogと区間判定の競合、端末Ctrl-CのFFmpegへの直接到達、PCAP取得失敗時の不完全フラグを回帰テストで再現して修正しました。Unix専用テストのWindows除外も修正し、独立した再レビューですべて解消を確認しました。

実FFmpegと合成入力で時刻付きPNG12枚・320×180の寸法維持・枚数一致・画像表示を確認しました。実HDMIは名前とindexの両方で`Could not lock device for configuration`となり取得できませんでした。同時にOBSの起動を確認しており競合の候補ですが、原因は確定していません。実HDMIの保存fps・プレイ画面と通信境界の一致は未検証です。デバイス一覧には`[0] HDMI capture device`があります。
