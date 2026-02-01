# gpth-rs

Google Photos Takeout Helper の Rust 実装。ZIP ファイルを事前展開せずストリーミング処理する。

## ビルド
```
cargo build --release
```

## 実行
```
gpth-rs -o output_dir takeout-*.zip
gpth-rs -o output_dir --divide-to-dates takeout-*.zip
```

## テスト
```
cargo test
```

## アーキテクチャ
- `main.rs` - CLI (clap) + パイプライン制御
- `media.rs` - Media struct
- `zip_scan.rs` - ZIP エントリ列挙、JSON メタデータ収集
- `dedup.rs` - サイズ→SHA-256 重複除去
- `date/` - 日付抽出 (json, exif, filename guess)
- `extras.rs` - 派生画像フィルタ (多言語)
- `folder_classify.rs` - 年フォルダ判定 (多言語)
- `writer.rs` - 出力書き出し
