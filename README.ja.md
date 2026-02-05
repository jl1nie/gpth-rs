# gpth-rs

[GooglePhotosTakeoutHelper](https://github.com/TheLastGimbus/GooglePhotosTakeoutHelper) の Rust 実装。

Google Takeout の zip ファイルを**事前展開なしで直接処理**し、メディアファイルをストリーミングで読み出して整理されたフォルダに出力します。

**GUI アプリ**（Tauri）と**CLI ツール**の両方を提供。

[English README](README.md)

## 特徴

- **事前展開不要** - zip ファイルを直接読み込み
- **GUI & CLI** - ドラッグ&ドロップ対応のデスクトップアプリ、またはコマンドラインツール
- **並列処理** - rayon による EXIF 読み取り・ハッシュ計算・ファイル書き出しの並列化
- **日付抽出** - JSON メタデータ、EXIF、ファイル名パターン推測（優先順）
- **重複検出** - ファイルサイズ + SHA-256 ストリーミングハッシュ（ファイルサイズ制限なし）
- **多言語フォルダ認識** - Google Takeout の年フォルダを 32 以上の言語パターンで認識
- **日本語 ZIP 対応** - Shift_JIS エンコードされたファイル名に対応
- **派生画像フィルタ** - `-edited`、`-bearbeitet`、`-編集済み` 等をスキップ
- **日付別整理** - YYYY/MM サブフォルダへの出力に対応
- **アルバム対応** - 名前付きアルバムフォルダの処理、アルバムディレクトリまたは JSON インデックスとして出力
- **差分出力** - 出力先に既に存在するファイル（同名・同サイズ）をスキップし、再実行が高速

## インストール

### ビルド済みバイナリ

[Releases](https://github.com/nickel-and-dime/gpth-rs/releases) ページからダウンロード:

- `gpth-rs.exe` — GUI（Windows）
- `gpth-rs-cli.exe` — CLI（Windows）

### ソースからビルド

必要: Rust ツールチェイン、Node.js（GUI のみ）

```sh
# CLI のみ
cargo build --release -p gpth-cli

# GUI（Node.js が必要）
npm install
npx tauri build
```

バイナリの出力先:
- `target/release/gpth-rs-cli.exe`（CLI）
- `target/release/gpth-rs.exe`（GUI）

## GUI の使い方

1. `gpth-rs.exe` を起動
2. ZIP ファイルをウィンドウにドラッグ&ドロップ、または **+ Add ZIP** をクリック
3. **Browse** をクリックして出力先ディレクトリを選択
4. 必要に応じてオプションを切り替え:
   - **Date folders** - YYYY/MM サブフォルダに分割
   - **Skip derivatives** - 派生画像をスキップ
   - **No guess** - JSON/EXIF のみ使用（ファイル名推測無効）
   - **Albums** - アルバム情報を処理
   - **Album folders** - `output/albums/` にアルバムフォルダを作成
   - **Symlinks** - コピーではなくシンボリックリンクを使用（Windows では管理者権限が必要）
5. **Run** をクリック

## CLI の使い方

### 基本

```sh
gpth-rs-cli -o 出力先 takeout-*.zip
```

### 日付別サブフォルダに整理

```sh
gpth-rs-cli -o 出力先 --divide-to-dates takeout-*.zip
```

### アルバムを処理

```sh
gpth-rs-cli -o 出力先 --albums takeout-*.zip
```

### アルバムを別フォルダにシンボリックリンクで出力

```sh
gpth-rs-cli -o 出力先 --albums --album-dest album --album-link --divide-to-dates takeout-*.zip
```

### 全オプション

```
gpth-rs-cli [OPTIONS] -o <OUTPUT> <ZIP_FILES>...

引数:
  <ZIP_FILES>...              Google Takeout の zip ファイル

オプション:
  -o, --output <DIR>          出力ディレクトリ（必須）
  --divide-to-dates           YYYY/MM サブフォルダに分割
  --skip-extras               派生画像をスキップ（-edited, -effects 等）
  --no-guess                  ファイル名からの日付推測を無効化
  --albums                    アルバムフォルダも処理する
  --album-dest <MODE>         アルバム出力モード: "year"（デフォルト）または "album"
  --album-link                コピーではなくシンボリックリンクを使用（--album-dest album 時のみ）
  --album-json <PATH>         albums.json の出力パス（デフォルト: <output>/albums.json）
  -h, --help                  ヘルプを表示
  -V, --version               バージョンを表示
```

### 使用例

複数の zip ファイルを処理:

```sh
gpth-rs-cli -o ~/Photos takeout-20240101T000000Z-001.zip takeout-20240101T000000Z-002.zip
```

編集済み画像をスキップして日付別に整理:

```sh
gpth-rs-cli -o ~/Photos --divide-to-dates --skip-extras takeout-*.zip
```

## 出力構造

### フラット（デフォルト）

```
output/
├── IMG_20230101_120000.jpg
├── IMG_20230102_140000.jpg
└── ...
```

### `--divide-to-dates` 指定時

```
output/
├── 2023/
│   ├── 01/
│   │   ├── IMG_20230101_120000.jpg
│   │   └── ...
│   └── 02/
│       └── ...
├── 2024/
│   └── ...
└── date-unknown/
    └── ...
```

### `--albums --album-dest album --divide-to-dates` 指定時

```
output/
├── 2023/
│   └── 07/
│       ├── IMG_001.jpg
│       └── IMG_002.jpg
├── albums/
│   ├── 旅行2023/
│   │   ├── IMG_001.jpg      (コピーまたはシンボリックリンク)
│   │   └── IMG_002.jpg
│   └── 家族/
│       └── DSC_100.jpg
├── date-unknown/
│   └── ...
└── albums.json
```

### albums.json

`--albums` を有効にすると、アルバム名と出力ファイルの対応を記述した `albums.json` が出力されます:

```json
{
  "albums": {
    "旅行2023": {
      "files": [
        { "filename": "IMG_001.jpg", "output_path": "2023/07/IMG_001.jpg" },
        { "filename": "IMG_002.jpg", "output_path": "2023/07/IMG_002.jpg" }
      ]
    }
  }
}
```

## 処理の流れ

1. **スキャン** - 全 zip エントリを読み込み、年フォルダ内のメディアファイルと JSON メタデータを収集。`--albums` 指定時はアルバムフォルダのエントリも収集。
2. **日付抽出** - 以下の優先順で日付を取得:
   - Google JSON メタデータ (`photoTakenTime.timestamp`)
   - EXIF (`DateTimeOriginal`, `DateTimeDigitized`, `DateTime`)
   - ファイル名パターン (`IMG_20230101_120000`, `Screenshot_20230101-120000` 等)
3. **アルバムマージ**（`--albums` 指定時）- アルバムエントリをファイル名+サイズで年フォルダのメディアとマッチング。マッチしないアルバム専用ファイルは新規メディアとして追加。
4. **重複除去** - ファイルサイズでグループ化 → SHA-256 ハッシュで重複を除去
5. **書き出し** - zip から1ファイルずつストリーミングで出力、ファイル更新日時を設定。出力先に同名・同サイズのファイルが既にある場合はスキップ。オプションでアルバムフォルダと `albums.json` を出力。

## プロジェクト構成

```
gpth-rs/
├── crates/
│   ├── gpth-core/       # コアライブラリ（処理パイプライン）
│   └── gpth-cli/        # CLI バイナリ（gpth-rs-cli）
├── src-tauri/           # Tauri GUI バックエンド（gpth-rs）
├── src-frontend/        # GUI フロントエンド（TypeScript + CSS）
└── index.html
```

## 日付抽出の詳細

### JSON メタデータ

Google Takeout はメディアファイルに対応する `.json` ファイルを含みます。以下の命名規則に対応:

- 標準: `IMG_1234.jpg.json`
- ファイル名切り詰め（46文字超）
- カッコ入れ替え: `image(1).jpg` → `image.jpg(1).json`
- 編集版: `image-edited.jpg` → `image.jpg.json`

### 対応するファイル名パターン

| パターン | 例 |
|---------|---------|
| `YYYYMMDD-hhmmss` | `Screenshot_20190919-053857.jpg` |
| `YYYYMMDD_hhmmss` | `IMG_20190509_154733.jpg` |
| `YYYY-MM-DD-hh-mm-ss` | `Screenshot_2019-04-16-11-19-37.jpg` |
| `YYYY-MM-DD-hhmmss` | `signal-2020-10-26-163832.jpg` |
| `YYYYMMDDhhmmss` | `201801261147521000.jpg` |
| `YYYY_MM_DD_hh_mm_ss` | `2016_01_30_11_49_15.mp4` |

## 対応言語（年フォルダ認識）

EN, DE, FR, ES, PT, CA, NL, IT, PL, RU, CS, RO, SV, NO, DA, FI, HU, TR, JA, KO, ZH-CN, ZH-TW

## ライセンス

Apache License 2.0 - [オリジナルプロジェクト](https://github.com/TheLastGimbus/GooglePhotosTakeoutHelper)と同一。

## クレジット

TheLastGimbus による [GooglePhotosTakeoutHelper](https://github.com/TheLastGimbus/GooglePhotosTakeoutHelper) に基づいています。
