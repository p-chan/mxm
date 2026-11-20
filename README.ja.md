# mxm

[English](README.md) | 日本語

Logitech MX Master シリーズのマウスの設定を、HID++ 2.0 プロトコルで直接読み書きする CLI ツールです。MX Master 4 で動作を確認しています。Logi Options+ を使わずに DPI、SmartShift、ホイール、触覚フィードバック、ボタン割り当てなどを変更できます。

このプロジェクトは Logitech とは関係のない非公式のツールです。

プロトコルの解析結果は [PROTOCOL.ja.md](PROTOCOL.ja.md) にまとめています。

## 動作環境

- macOS（Linux / Windows でも [hidapi](https://crates.io/crates/hidapi) が動けば動作する見込みですが未検証です）
- Rust（ビルド時のみ）
- MX Master シリーズのマウスを Bluetooth または Logi Bolt レシーバーで接続（動作確認は MX Master 4 のみ。ほかの機種では、触覚フィードバックなど一部の機能がない場合があります）

## セットアップ

```sh
cargo install --path .   # ~/.cargo/bin/mxm にインストール
```

インストールせずに `cargo run -- <command>` でも実行できます。

## 開発

```sh
cargo test     # 実機から記録した応答を使ったデコードのテスト
cargo clippy --all-targets
cargo fmt
```

## 使い方

```sh
mxm list                                # 接続中の HID++ 2.0 デバイス
mxm info                                # 名前、ファームウェア、シリアル、電池、ホスト
mxm features                            # feature テーブル
mxm battery

mxm dpi                                 # 現在の DPI
mxm dpi 1600                            # DPI を変更（200〜8000、50 刻み）

mxm smartshift                          # SmartShift の状態
mxm smartshift --mode ratchet --threshold 12 --torque 75
mxm smartshift --threshold off          # SmartShift を無効化（常にラチェット）

mxm wheel --invert on                   # メインホイールの方向を反転
mxm wheel --hires off                   # 高解像度スクロールを無効化
mxm thumbwheel --invert on              # サムホイールの方向を反転

mxm haptic                              # 触覚フィードバックの状態と波形一覧
mxm haptic 60                           # 強さを変更（1〜100）
mxm haptic off
mxm haptic play happy-alert             # 波形を再生

mxm force                               # ハプティックセンスパネルの押下圧
mxm force default

mxm buttons                             # ボタン一覧
mxm buttons remap back middle           # 戻るボタンを中クリックにする
mxm buttons reset back

mxm host                                # Easy-Switch のホスト一覧
mxm host 2                              # ホスト 2 に切り替え（このマシンとの接続は切れる）
```

### プロトコル解析用のコマンド

```sh
mxm --trace dpi                         # 送受信した HID++ レポートを stderr に出力
mxm raw 0x2201 2 00                     # 任意の feature / function を呼ぶ（ここでは getSensorDpi）
mxm raw '#20' 1                         # feature index を直接指定
mxm monitor                             # デバイスからの通知を表示
mxm monitor --divert back,gesture --wheel --thumbwheel
                                        # 指定したボタンやホイールを一時的に HID++ へ転送して観察（終了時に元に戻す）
mxm dump                                # 読み取れる設定をすべて JSON で出力
```

`--json` を付けると、各コマンドの結果を JSON で出力します。

## 注意

- Logi Options+ が起動していると、Options+ が設定を上書きしたり、ボタンやホイールを HID++ に転送（divert）したりします。このツールで変更した設定を保ちたいときは、Options+ を終了してください。
- マウスの電源を入れ直すと、触覚の強さ、SmartShift、ハプティックセンスパネルの押下圧は残りますが、DPI、高解像度スクロール、サムホイールの方向、ボタンの割り当てと横取り（divert）は既定に戻ります（[PROTOCOL.ja.md](PROTOCOL.ja.md#電源の入れ直し後の設定) を参照）。接続のたびに設定を書き直す常駐モードを追加する予定です。
- 左右クリックは割り当てを変更できません（デバイスが許可していません）。一方、ほかのボタンを左右クリックとして割り当てることはできます。

## 謝辞

Logitech は HID++ 2.0 の仕様書を公開していません。このツールで使っている feature の名前、メッセージの形式、デバイスの動作は、Logitech デバイスの管理ツール（主に Linux 向けで、macOS も一部対応） [Solaar](https://github.com/pwr-Solaar/Solaar) と照らし合わせ、可能な範囲で実機の MX Master 4 で確認しました（未検証の内容は [PROTOCOL.ja.md](PROTOCOL.ja.md) に記載）。Solaar のコードは、このプロジェクトに含まれていません。
