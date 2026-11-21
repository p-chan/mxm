# mxm

[English](README.md) | 日本語

Logitech MX Master シリーズのマウスの設定を、HID++ 2.0 プロトコルで直接読み書きする CLI ツールです。MX Master 4 と MX Master 3S で動作を確認しています。Logi Options+ を使わずに DPI、SmartShift、ホイール、触覚フィードバック、ボタン割り当てなどを変更できます。

このプロジェクトは Logitech とは関係のない非公式のツールです。

プロトコルの解析結果は [PROTOCOL.ja.md](PROTOCOL.ja.md) にまとめています。

## 動作環境

- macOS（Linux / Windows でも [hidapi](https://crates.io/crates/hidapi) が動けば動作する見込みですが未検証です）
- Rust（ソースからビルドするときのみ）
- MX Master シリーズのマウスを Bluetooth または Logi Bolt レシーバーで接続（動作確認は MX Master 4 と MX Master 3S。MX Master 3S の触覚フィードバックのように機種にない機能は、エラーになります）

## セットアップ

[GitHub Releases](https://github.com/p-chan/mxm/releases) から、macOS（Apple Silicon / Intel）向けのビルド済みバイナリをインストールします。

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/p-chan/mxm/releases/latest/download/mxm-installer.sh | sh   # ~/.local/bin/mxm にインストール
```

アップデートするときも、同じコマンドを実行します。ソースからビルドする場合は次のとおりです。

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

## 設定ファイルと常駐モード

マウスは電源を切ると、ほとんどの設定を忘れます（[注意](#注意) を参照）。`mxm daemon` は常駐して、マウスが接続するたびに設定ファイルの内容を書き込みます。

設定は `~/.config/mxm/config.toml`（`$XDG_CONFIG_HOME` があれば `$XDG_CONFIG_HOME/mxm/config.toml`）に書きます。書いていない設定には触れません。

```toml
# 機種ごと。`mxm list` に表示される名前で指定（大文字小文字は区別しない）
[device."MX Master 4"]
dpi = 1600
smartshift = { mode = "ratchet", threshold = 12, torque = 75 }  # threshold = "off" で SmartShift を無効にする
wheel = { hires = true, invert = false }
thumbwheel = { invert = true }
haptic = 60                 # 1〜100、または "off"
force = "default"           # ハプティックセンスパネルの押下圧。値、または "default"

[device."MX Master 4".buttons]
back = "middle"             # ボタンの割り当て。"default" で既定の動作に戻す

# 1 台ごと。`mxm info` に表示されるシリアル番号で指定。機種の設定より優先される
[serial."XXXXXXXXXXXX"]
dpi = 2000
```

```sh
mxm apply                   # 接続中のすべてのマウスに設定ファイルを 1 回書き込む
mxm daemon                  # 常駐して、接続のたびに設定ファイルを書き込む
```

`mxm daemon` は、起動したとき、マウスが Bluetooth または Logi Bolt レシーバー経由で接続したとき、電源を入れたマウスが再設定を求めてきたときに、設定ファイルを書き込みます。設定ファイルは毎回読み直すので、編集した内容は次の接続から反映されます。MX Master 3S の触覚フィードバックのように機種にない設定は、「not supported」と表示して飛ばします。Logi Options+ も接続のたびに自分の設定を書き込むので、使うときは Options+ を終了してください。

### ログイン時に起動する（macOS）

```sh
mxm service install       # mxm daemon を LaunchAgent に登録して起動する（以降はログインのたびに起動）
mxm service status        # 登録されているか、動いているか
mxm service uninstall     # 止めて登録を解除する
tail -f ~/Library/Logs/mxm.log
```

`mxm service install` は、実行中の `mxm` の実行ファイルを指す `~/Library/LaunchAgents/io.github.p-chan.mxm.plist` を書き込みます。別の設定ファイルを使うときは `--config <path>` を付けます。実行ファイルを移動したら、もう一度実行してください。plist を自分で管理したいとき（dotfiles など）は、`mxm service install --print` で書き出せます。

launchd から起動した `mxm` が Bluetooth 接続のマウスを開くには、入力監視の権限が必要です（ないとログに `not permitted` と出ます）。`mxm service install` が、システム設定 → プライバシーとセキュリティ → 入力監視 に追加すべき実行ファイルを表示します。Logi Bolt レシーバー経由のマウスは、権限がなくても動きます。

macOS 以外では、まだ `mxm service` に対応していません。`mxm daemon` を各 OS のサービス管理（Linux なら systemd のユーザーサービスなど）から起動してください。

## 注意

- Logi Options+ が起動していると、Options+ が設定を上書きしたり、ボタンやホイールを HID++ に転送（divert）したりします。このツールで変更した設定を保ちたいときは、Options+ を終了してください。
- マウスの電源を入れ直すと、触覚の強さ、SmartShift、ハプティックセンスパネルの押下圧は残りますが、DPI、高解像度スクロール、サムホイールの方向、ボタンの割り当てと横取り（divert）は既定に戻ります（[PROTOCOL.ja.md](PROTOCOL.ja.md#電源の入れ直し後の設定) を参照）。`mxm daemon` を使うと、接続のたびに書き直せます（[設定ファイルと常駐モード](#設定ファイルと常駐モード) を参照）。
- 左右クリックは割り当てを変更できません（デバイスが許可していません）。一方、ほかのボタンを左右クリックとして割り当てることはできます。

## 謝辞

Logitech は HID++ 2.0 の仕様書を公開していません。このツールで使っている feature の名前、メッセージの形式、デバイスの動作は、Logitech デバイスの管理ツール（主に Linux 向けで、macOS も一部対応） [Solaar](https://github.com/pwr-Solaar/Solaar) と照らし合わせ、可能な範囲で実機の MX Master 4 と MX Master 3S で確認しました（未検証の内容は [PROTOCOL.ja.md](PROTOCOL.ja.md) に記載）。Solaar のコードは、このプロジェクトに含まれていません。

## ライセンス

[MIT](LICENSE)
