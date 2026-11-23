# mxm

[English](README.md) | 日本語

mxm は、Logitech MX Master シリーズのマウスを HID++ 2.0 プロトコルで直接設定する CLI とデーモンです。Logi Options+ を使わずに DPI、SmartShift、ホイール、触覚フィードバック、ボタン割り当てなどを変更できます。設定ファイルに書いた設定は、一度だけ書き込むことも、マウスがつながるたびにデーモンに書き込ませることもできます。macOS では、`mxm service` でデーモンをログイン時に起動でき、デーモンは Logi Options+ と同じ加速でホイールをスクロールします。MX Master 4 と MX Master 3S で動作を確認しています。

このプロジェクトは Logitech とは関係のない非公式のツールです。

プロトコルの解析結果は [PROTOCOL.ja.md](PROTOCOL.ja.md) にまとめています。

## 動作環境

- Apple Silicon または Intel の macOS（Linux / Windows でも [hidapi](https://crates.io/crates/hidapi) が動けば動作する見込みですが、未検証で、ビルド済みバイナリもありません）
- MX Master シリーズのマウスを Bluetooth または Logi Bolt レシーバーで接続（動作確認は MX Master 4 と MX Master 3S。MX Master 3S の触覚フィードバックのように機種にない機能は、エラーになります）

## セットアップ

macOS 向けのビルド済みバイナリは、Developer ID で署名し、Apple の公証を受けています。次のどれかでインストールします。

### Homebrew

```sh
brew tap p-chan/tap
brew trust --tap p-chan/tap   # 初回だけ。Homebrew は信頼した tap の cask だけを読み込む
brew install --cask mxm
```

`brew upgrade --cask mxm` でアップデートします。[daemon をログイン時に起動](#ログイン時に起動するmacos)している場合は、daemon もあわせて更新されます。

### mise

```sh
mise use -g github:p-chan/mxm
```

daemon をログイン時に起動して、mise で更新するときは、[ログイン時に起動する](#ログイン時に起動するmacos)を参照してください。

### インストールスクリプト

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/p-chan/mxm/releases/latest/download/mxm-installer.sh | sh   # ~/.local/bin/mxm にインストール
```

アップデートするときも、同じコマンドを実行します。

### ソースからビルド

[Rust](https://www.rust-lang.org/ja/tools/install) をインストールしたうえで、次を実行します。

```sh
git clone https://github.com/p-chan/mxm.git
cd mxm
cargo install --path .   # ~/.cargo/bin/mxm にインストール
```

自分でビルドしたバイナリは Developer ID で署名されていないので、ビルドし直すたびに、daemon の入力監視とアクセシビリティの権限が外れます。インストールせずに `cargo run -- <command>` でも実行できます。

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
wheel = { hires = true, invert = false, speed = 85, smooth = true }  # speed と smooth は下を参照
thumbwheel = { invert = true, speed = 50 }
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

### ソフトウェアスクロール（macOS）

`mxm daemon` は Logi Options+ と同じように、ホイールを HID++ に divert して、スクロールのイベントを自分で送ります。加速のしかたも Options+ と同じです（[PROTOCOL.md](PROTOCOL.md#software-scrolling-of-logi-options-macos) を参照）。設定の意味も Options+ と同じです。

- `wheel.speed`：「スクロール速度」。`0`〜`100`（既定は `50`）。速く回したときの加速の強さを決めます。ゆっくり回したときの距離は変わりません。
- `wheel.smooth`：「スムーズスクロール」（既定は `true`）。`false` にすると、ホイールを 1 ノッチ単位（低解像度）で扱います。`mxm daemon` では `hires` の代わりにこの値を使います。
- `thumbwheel.speed`：サムホイールの「速度」。`0`〜`100`（既定は `50`）。
- スクロールの向きは、Options+ の「スクロール方向」と同じく `invert` だけで決まります。macOS のナチュラルスクロールの設定では変わりません。

`mxm apply` は終了するとスクロールを送れないので、スクロールは扱いません。Options+ との違いは 2 つです。Options+ はサムホイールの速度が低いとき、「ページ間をスワイプ」のジェスチャーのためにスクロールを少し捨てますが、mxm は捨てません。また、Options+ はフリースピン中に中クリックした直後にホイールを止めますが、mxm は止めません。

- スクロールのイベントを送るには、アクセシビリティの権限が必要です。ログに表示される実行ファイルを、システム設定 → プライバシーとセキュリティ → アクセシビリティ で許可してください。許可すると、数秒以内にソフトウェアスクロールが始まります。それまでは、ホイールは OS がそのまま扱います。
- daemon が止まるとき（`mxm service uninstall` など）は、ホイールを OS に戻します。強制終了やクラッシュで止まったときは divert されたままになり、マウスの電源を入れ直すか、`mxm wheel --divert off` と `mxm thumbwheel --divert off` を実行するまでスクロールしません。`mxm service install` で登録した daemon は、終了しても起動し直されます。
- Logi Options+ もホイールを divert するので、終了してください。両方が動いていると、スクロールが 2 回ずつ送られます。

### ログイン時に起動する（macOS）

```sh
mxm service install       # mxm daemon を LaunchAgent に登録して起動する（以降はログインのたびに起動）
mxm service update        # mxm をアップデートしたあとに、daemon をこの mxm に更新する
mxm service status        # 登録されているか、動いているか
mxm service uninstall     # 止めて登録を解除する
tail -f ~/Library/Logs/mxm.log
```

`mxm service install` は、実行中の `mxm` を `~/Library/Application Support/mxm/mxm` にコピーし、そのコピーを起動する `~/Library/LaunchAgents/io.github.p-chan.mxm.plist` を書き込みます。`mxm` をアップデートしたら、`mxm service update` を実行してコピーを置き換え、daemon を再起動してください（コピーがないときや、手元の `mxm` と違うときは `mxm service status` が知らせます）。`mxm service update` は plist を書き換えず、サービスを登録していなければ何もしません。設定ファイルのパスは実行した時点で決まります（`--config <path>` を付けなければ既定の `~/.config/mxm/config.toml`。`$XDG_CONFIG_HOME` があれば `$XDG_CONFIG_HOME/mxm/config.toml`）。設定ファイルのパスを変えたときも、もう一度実行してください。plist を自分で管理したいとき（dotfiles など）は、`mxm service install --print` で書き出せます。書き出した plist は上のパスのコピーを起動しますが、`--print` はコピーを作りません。

launchd から起動した `mxm` が Bluetooth 接続のマウスを開くには、入力監視の権限が必要です（ないとログに `not permitted` と出ます）。`mxm service install` が、システム設定 → プライバシーとセキュリティ → 入力監視 に追加すべき実行ファイルを表示します。Logi Bolt レシーバー経由のマウスは、権限がなくても動きます。[ソフトウェアスクロール](#ソフトウェアスクロールmacos)には、同じ実行ファイルにアクセシビリティの権限も必要です。macOS は権限を実行ファイルのパスごとに管理します。リリースのバイナリは Developer ID で署名しているので、`mxm service update` がアップデートした `mxm` でコピーを置き換えても、権限は引き継がれます。インストール方法は問いません（たとえば mise はバージョンごとに別のディレクトリに入れます）。

[mise](https://mise.jdx.dev/) で入れるなら、`postinstall` を使うと、mise がバージョンを入れるたびに daemon を登録、または更新できます。

```toml
[tools]
"github:p-chan/mxm" = { version = "latest", postinstall = "mxm service install" }
```

`mxm service install` は毎回 plist を書き直すので、`--config <path>` を使うときはこのコマンドにも付けてください。自分で登録した daemon を更新するだけにしたいときは、代わりに `mxm service update` を使います。

Homebrew の cask で入れた場合は、`brew upgrade --cask mxm` が `mxm service update` を実行します。

macOS 以外では、まだ `mxm service` に対応していません。`mxm daemon` を各 OS のサービス管理（Linux なら systemd のユーザーサービスなど）から起動してください。

## 注意

- Logi Options+ が起動していると、Options+ が設定を上書きしたり、ボタンやホイールを HID++ に転送（divert）したりします。このツールで変更した設定を保ちたいときは、Options+ を終了してください。
- マウスの電源を入れ直すと、触覚の強さ、SmartShift、ハプティックセンスパネルの押下圧は残りますが、DPI、高解像度スクロール、サムホイールの方向、ボタンの割り当てと横取り（divert）は既定に戻ります（[PROTOCOL.ja.md](PROTOCOL.ja.md#電源の入れ直し後の設定) を参照）。`mxm daemon` を使うと、接続のたびに書き直せます（[設定ファイルと常駐モード](#設定ファイルと常駐モード) を参照）。
- 左右クリックは割り当てを変更できません（デバイスが許可していません）。一方、ほかのボタンを左右クリックとして割り当てることはできます。

## 謝辞

Logitech は HID++ 2.0 の仕様書を公開していません。このツールで使っている feature の名前、メッセージの形式、デバイスの動作は、Logitech デバイスの管理ツール（主に Linux 向けで、macOS も一部対応） [Solaar](https://github.com/pwr-Solaar/Solaar) と照らし合わせ、可能な範囲で実機の MX Master 4 と MX Master 3S で確認しました（未検証の内容は [PROTOCOL.ja.md](PROTOCOL.ja.md) に記載）。Solaar のコードは、このプロジェクトに含まれていません。

## ライセンス

[MIT](LICENSE)
