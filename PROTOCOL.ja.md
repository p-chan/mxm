# MX Master の HID++ 2.0 プロトコル

[English](PROTOCOL.md) | 日本語

MX Master 4（ファームウェア RBM27.02_0017、Bluetooth LE 接続、PID `0xB042`）で実際に送受信して確認した内容です。MX Master 3S との違いは「[MX Master 3S](#mx-master-3s)」にまとめています。feature の意味は [Solaar](https://github.com/pwr-Solaar/Solaar) の実装と照らし合わせました。実機で確かめていない内容は、その旨を書いています。

## 通信経路

| 接続 | HID コレクション | device index |
| --- | --- | --- |
| Bluetooth LE | usage page `0xFF43`, usage `0x0202` | `0xFF` |
| Logi Bolt レシーバー（PID `0xC548`） | usage page `0xFF00`, usage `0x0002` | ペアリングのスロット `1`〜`6` |

レシーバーの HID++ 1.0 のレジスター `0xB5`（スロット N のペアリング情報はサブアドレス `0x5N`、名前は `0x6N`）を読むと、ペアリング済みのデバイスが分かります。観察したレシーバーでは、スロット 2 に MX Master 4 がペアリングされていて、unit ID は Bluetooth 接続時と同じでした。マウスをホスト 3 に切り替えると、device index `0x02` として通信できました。45 秒ほど操作しなかった後は、レシーバー経由の最初の要求への応答に最大で約 1.4 秒かかりました（省電力状態から起きるためと推測）。そのため、このツールはスロットごとに最大 2 秒待ちます。

macOS では、Bluetooth LE のマウスもレシーバーも、OS のドライバーを外さずに hidapi から読み書きできます。Bluetooth LE のマウスでは、マウス移動などの通常の入力レポートも同じデバイスから読めるため、report ID で HID++ のレポートだけを選びます。

## メッセージ形式

long report（20 bytes）を使います。

| byte | 内容 |
| --- | --- |
| 0 | report ID（`0x10` = short 7 bytes、`0x11` = long 20 bytes） |
| 1 | device index |
| 2 | feature index |
| 3 | 上位 4 bit: function ID、下位 4 bit: software ID |
| 4〜19 | パラメーター |

- 応答は要求と同じ feature index と function / software ID で返ります。software ID が `0` のレポートは、デバイスが自発的に送る通知です。
- Logi Options+ も同時に通信しているため、応答を取り違えないよう、このツールは software ID `0xB` を使います。
- HID++ 2.0 のエラーは `[0x11, idx, 0xFF, featureIndex, fn|sw, errorCode]` の形で返ります。
- feature ID から feature index を引くには、Root（index 0）の function 0 `getFeature(featureId)` を呼びます。

例: DPI を読む

```text
-> 11 ff 14 2b 00 00 00 ...    feature index 0x14 (AdjustableDpi), fn 2 getSensorDpi(sensor 0), sw 0xB
<- 11 ff 14 2b 00 09 60 03 e8  sensor 0, dpi 0x0960 = 2400, default 0x03E8 = 1000
```

## feature テーブル

`mxm features` の出力です。名前は Solaar に合わせていて、Solaar に名前がない feature は「不明」としています。feature index は機種やファームウェアで変わるため、毎回 feature ID から引きます。

| index | ID | 名前 | 備考 |
| ---: | --- | --- | --- |
| 1 | `0x0001` | FeatureSet | |
| 2 | `0x0003` | DeviceInfo | |
| 3 | `0x0005` | DeviceName | |
| 4 | `0x1D4B` | WirelessDeviceStatus | |
| 5 | `0x0020` | ConfigChange | |
| 6 | `0x0021` | CryptoId | |
| 7 | `0x0007` | DeviceFriendlyName | |
| 8 | `0x0011` | PropertyAccess | |
| 9 | `0x1004` | UnifiedBattery | |
| 10 | `0x1701` | 不明 | |
| 11 | `0x19B0` | Haptic | |
| 12 | `0x19C0` | ForceSensingButton | |
| 13 | `0x1B04` | ReprogControlsV4 | |
| 14 | `0x1814` | ChangeHost | |
| 15 | `0x1815` | HostsInfo | |
| 16 | `0x2250` | XYStats | |
| 17 | `0x2111` | SmartShiftEnhanced | |
| 18 | `0x2121` | HiResWheel | |
| 19 | `0x2150` | ThumbWheel | |
| 20 | `0x2201` | AdjustableDpi | |
| 21 | `0x2251` | WheelStats | |
| 22 | `0x00D1` | 不明 | |
| 23〜45 | `0x18xx`, `0x1Exx`, `0x9xxx` ほか | 診断・製造用 | hidden フラグ付き（多くは engineering フラグも付く） |

## 電源の入れ直し後の設定

Logi Options+ を止めた状態で設定を変え、マウスの電源スイッチで電源を入れ直してから、Bluetooth LE 経由で設定を読み直して確認しました。

| 設定 | 電源の入れ直し後 |
| --- | --- |
| 触覚の強さ（`0x19B0`） | 残る |
| SmartShift のモード・閾値・トルク（`0x2111`） | 残る |
| ハプティックセンスパネルの押下圧（`0x19C0`） | 残る |
| DPI（`0x2201`） | 既定値（1000）に戻る |
| 高解像度スクロール（`0x2121`） | オフに戻る |
| サムホイールの方向（`0x2150`） | 通常に戻る |
| ボタンの割り当てと divert（`0x1B04`） | 解除される |
| analyticsKeyEvents の報告（`0x1B04`） | 無効に戻る |

Logi Options+ は、終了するときに divert を解除しません。止めたあとも、メインホイール、サムホイール、戻る・進む・ジェスチャー・ハプティックセンスパネルのボタンが divert されたままで、divert を解除するまで OS に届きませんでした。

## 各 feature

パラメーターは、レポートの byte 4 以降を 0 始まりで数えます。

### AdjustableDpi `0x2201`

| fn | 名前 | 要求 | 応答 |
| --- | --- | --- | --- |
| 1 | getSensorDpiList | `sensor` | `sensor, dpi[]`（u16 の並び、`0` で終端） |
| 2 | getSensorDpi | `sensor` | `sensor, dpi (u16), defaultDpi (u16)` |
| 3 | setSensorDpi | `sensor, dpi (u16)` | |

DPI リストの上位 3 bit が `111` の値は「直前の値から直後の値まで、下位 13 bit の刻み」を表します。MX Master 4 は `00c8 e032 1f40` で、200〜8000 を 50 刻みです。

### SmartShiftEnhanced `0x2111`

MX Master 3S は、代わりに古い SmartShift `0x2110` を持っています（「[MX Master 3S](#mx-master-3s)」を参照）。

| fn | 名前 | 要求 / 応答 |
| --- | --- | --- |
| 0 | getCapabilities | `flags (bit0: トルク調整可), 既定の閾値, 既定のトルク, ...` |
| 1 | getRatchetControlMode | `wheelMode (1: フリースピン, 2: ラチェット), autoDisengage, torque` |
| 2 | setRatchetControlMode | 同上。`0` は「変更しない」 |

- `autoDisengage` は SmartShift の閾値で、`1`〜`254`。`255` は SmartShift 無効（常に `wheelMode` のまま）です。
- `torque` はラチェットの重さで `1`〜`100`（既定 75）です。Solaar によると、MX Master 4 はトルクだけを書いても反映されず、`wheelMode` も一緒に書く必要があります。観察したデバイスでは、トルクだけを書いた（`00 00 32`）ところ読み戻した値は 50 に変わりましたが、ホイールの重さが実際に変わったかは確かめていません。このツールは常に 3 つの値をまとめて書きます。

### HiResWheel `0x2121`

| fn | 名前 | 要求 / 応答 |
| --- | --- | --- |
| 0 | getWheelCapability | `multiplier (15), flags` |
| 1 | getWheelMode | `mode` |
| 2 | setWheelMode | `mode` |
| 3 | getRatchetSwitchState | `state (bit0: ラチェット)` |

`mode` は bit0 = HID++ に転送（divert）、bit1 = 高解像度、bit2 = 方向反転です。ホイールを divert している間は、通知 fn 0 で `flags (bit4: 高解像度, bit0〜3: periods), delta (s16)` が届きます（実機で観察。例: `11 fffe` = 高解像度、1 period、delta -2）。通知 fn 1 では、SmartShift ボタンを押した直後に新しいラチェット状態（`state`、bit0: ラチェット）が届きます（ホイールを divert した状態で実機で観察。divert していないときにも届くかは未確認）。

### ThumbWheel `0x2150`

| fn | 名前 | 要求 / 応答 |
| --- | --- | --- |
| 0 | getInfo | `nativeRes (u16), divertedRes (u16), defaultDirection, capabilities, timeElapsed (u16)` |
| 1 | getStatus | `reportingMode (0: HID, 1: HID++), invert (bit0)` |
| 2 | setStatus | 同上 |

サムホイールを divert している間は、通知 fn 0 で `rotation (s16), timestamp (u16), status` が届きます（実機で観察）。1 回の操作で `status` は `1` start → `2` active → `3` stop → `0` inactive と変わります。

### Haptic `0x19B0`

| fn | 名前 | 要求 / 応答 |
| --- | --- | --- |
| 0 | getCapabilities | byte 4〜7 が再生できる波形のビットマスク（`0x08007FFF`） |
| 1 | getConfig | `enabled (bit0), level (0〜100), flags (bit0: 4 段階のみ)` |
| 2 | setConfig | `enabled, level` |
| 4 | playWaveform | `waveform` |

波形 ID は `0x00`〜`0x0E` と `0x1B`（`mxm haptic` で一覧を表示）です。

### ForceSensingButton `0x19C0`

ハプティックセンスパネル（親指の押し込みボタン）の押下圧の閾値です。

| fn | 名前 | 要求 / 応答 |
| --- | --- | --- |
| 0 | getCount | `count (1)` |
| 1 | getButtonInfo | 要求 `button`、応答 `flags (u16, bit0: 変更可), default (u16), max (u16), min (u16)` |
| 2 | getButtonConfig | 要求 `button`、応答 `current (u16)` |
| 3 | setButtonConfig | `button, value (u16)` |

MX Master 4 の範囲は 4578〜7610、既定値は 5722 です。

### ReprogControlsV4 `0x1B04`

| fn | 名前 | 要求 / 応答 |
| --- | --- | --- |
| 0 | getCount | `count` |
| 1 | getCidInfo | 要求 `index`、応答 `cid (u16), taskId (u16), flags, position, group, groupMask, flags2` |
| 2 | getCidReporting | 要求 `cid (u16)`、応答 `cid (u16), flags, remap (u16), flags2` |
| 3 | setCidReporting | `cid (u16), flags, remap (u16)` |

`setCidReporting` の `flags` は、bit0 divert / bit1 divert 有効、bit2 persist / bit3 persist 有効、bit4 rawXY / bit5 rawXY 有効です。`remap` が `0` なら割り当てを変更せず、自身の CID を書くと既定に戻ります。割り当て先は、`groupMask` のビットに対応する group の CID に限られます。

MX Master 4 のボタン:

| CID | ボタン | group | groupMask |
| --- | --- | ---: | ---: |
| `0x0050` | 左クリック | 1 | `0x00` |
| `0x0051` | 右クリック | 1 | `0x00` |
| `0x0052` | 中クリック（ホイールの押し込み） | 2 | `0x03` |
| `0x0053` | 戻る | 2 | `0x03` |
| `0x0056` | 進む | 2 | `0x03` |
| `0x00C3` | ジェスチャーボタン（側面のボタン群、戻る・進むの前） | 2 | `0x03` |
| `0x00C4` | SmartShift ボタン（ホイールの後ろ） | 2 | `0x03` |
| `0x01A0` | ハプティックセンスパネル（親指を置く部分） | 2 | `0x03` |
| `0x00D7` | 仮想ジェスチャーボタン | 3 | `0x00` |

group 2 のボタンは groupMask `0x03` なので、group 1（左右クリック）と group 2 のどれにでも割り当てられます。左右クリック自体は groupMask が `0x00` で、割り当てを変更できません。

通知（実機で観察）:

- fn 0: divert したボタンを押したときと離したときに、押されている CID（u16 × 最大 4）が届きます。すべて 0 なら、全ボタンが離されています。
- fn 2（analyticsKeyEvents）: divert していない左右クリックを含む全ボタンについて、`cid (u16), event (1: 押下, 0: 離す)` が届きます。Solaar は名前を付けているだけで、中身を解釈していません。観察したデバイスでは、全ボタンの `getCidReporting` の `flags2` の bit0 が `1` で、これがこの通知を有効にしていると考えられます（推測。Logi Options+ が起動していて、有効にしたとみられます）。
### UnifiedBattery `0x1004`

fn 1 getStatus の応答は `stateOfCharge (%), level (bit0 critical, bit1 low, bit2 good, bit3 full), chargingStatus (0 放電, 1 充電中, 2 低速充電, 3 完了, 4 エラー), externalPower` です。同じ形式の通知が fn 0 で届きます（実機で観察。USB-C ケーブルをつなぐと、約 6 秒後に `charging` が届きました）。

### ChangeHost `0x1814`

fn 0 getHostInfo は `hostCount (3), currentHost (0 始まり)`、fn 1 setCurrentHost は `host` です。観察したデバイスでは、setCurrentHost は応答を返してから現在のホストとの接続を切りました。同じ Mac のホスト 3（Logi Bolt レシーバー）からホスト 2（Bluetooth LE）に切り替えると、約 2 秒で Bluetooth で再接続されました。

HostsInfo `0x1815` の fn 1 getHostInfo の応答の 3 byte 目は、バスの種類を表しているようです。ホスト 1・2（Bluetooth LE）は `0x04`、ホスト 3（Logi Bolt レシーバー）は `0x05` でした。この対応は 1 台のデバイスからの推測です。

### DeviceInfo `0x0003` / DeviceName `0x0005`

- DeviceInfo fn 0: `entityCount, unitId (4), transport (u16), modelId (u16 × 3), extendedModelId, capabilities`
- DeviceInfo fn 1 getFwInfo(entity): `type, prefix (3 文字), number (BCD), revision (BCD), build (BCD u16), active, pid (u16)`
- DeviceInfo fn 2: シリアル番号（ASCII 12 文字）
- DeviceName fn 0: 名前の長さ、fn 1 (offset): 名前の一部、fn 2: 種別（`3` = mouse）

## MX Master 3S

MX Master 3S（ファームウェア RBM22.00_0003、Logi Bolt レシーバー接続、モデル ID `0xB034`）で確認した内容です。ここに書いていないことは、このドキュメントのほかの部分と同じです。

- SmartShiftEnhanced `0x2111`、Haptic `0x19B0`、ForceSensingButton `0x19C0` がありません。
- 代わりに、トルクの項目がない古い SmartShift `0x2110` があります。

  | fn | 名前 | 要求 / 応答 |
  | --- | --- | --- |
  | 0 | getRatchetControlMode | `wheelMode (1: フリースピン, 2: ラチェット), autoDisengage, autoDisengageDefault` |
  | 1 | setRatchetControlMode | `wheelMode, autoDisengage, autoDisengageDefault`。`0` は「変更しない」（`00 00 00` を書いても状態は変わりませんでした） |

- ボタンは、ハプティックセンスパネル（`0x01A0`）がない以外は MX Master 4 と同じです。
- サムホイールのネイティブの解像度は 18 です（MX Master 4 は 20）。
