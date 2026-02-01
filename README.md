# gpth-rs

Rust implementation of [GooglePhotosTakeoutHelper](https://github.com/TheLastGimbus/GooglePhotosTakeoutHelper).

Processes Google Takeout zip files **directly without extraction**, streaming media files in memory and writing them to an organized output folder.

[日本語版 README](README.ja.md)

## Features

- **No pre-extraction required** - reads zip files directly
- **Parallel processing** - uses rayon for EXIF reading, hashing, and file writing
- **Date extraction** - JSON metadata, EXIF, filename pattern guessing (priority order)
- **Duplicate detection** - file size + SHA-256 deduplication
- **Multilingual folder recognition** - 32+ language patterns for Google Takeout year folders
- **Edited file filtering** - skip `-edited`, `-bearbeitet`, `-編集済み`, etc.
- **Date-based organization** - optional YYYY/MM subdirectory output

## Installation

### From source

```sh
cargo install --path .
```

### Build

```sh
cargo build --release
```

The binary will be at `target/release/gpth-rs` (or `gpth-rs.exe` on Windows).

## Usage

### Basic

```sh
gpth-rs -o output_dir takeout-*.zip
```

### With date-based subdirectories

```sh
gpth-rs -o output_dir --divide-to-dates takeout-*.zip
```

### All options

```
gpth-rs [OPTIONS] -o <OUTPUT> <ZIP_FILES>...

Arguments:
  <ZIP_FILES>...       Google Takeout zip files

Options:
  -o, --output <DIR>   Output directory (required)
  --divide-to-dates    Organize into YYYY/MM subdirectories
  --skip-extras        Skip derivative images (-edited, -effects, etc.)
  --no-guess           Disable date guessing from filenames
  -h, --help           Print help
  -V, --version        Print version
```

### Examples

Process multiple zip files:

```sh
gpth-rs -o ~/Photos takeout-20240101T000000Z-001.zip takeout-20240101T000000Z-002.zip
```

Skip edited variants and organize by date:

```sh
gpth-rs -o ~/Photos --divide-to-dates --skip-extras takeout-*.zip
```

## Output Structure

### Flat (default)

```
output/
├── IMG_20230101_120000.jpg
├── IMG_20230102_140000.jpg
└── ...
```

### With `--divide-to-dates`

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

## How It Works

1. **Scan** - Reads all zip entries, collects media files from year folders and JSON metadata
2. **Date extraction** - Extracts dates in priority order:
   - Google JSON metadata (`photoTakenTime.timestamp`)
   - EXIF (`DateTimeOriginal`, `DateTimeDigitized`, `DateTime`)
   - Filename patterns (`IMG_20230101_120000`, `Screenshot_20230101-120000`, etc.)
3. **Deduplication** - Groups by file size, then SHA-256 hash to remove duplicates
4. **Write** - Streams each file from zip to output directory, sets file modification time

## Date Extraction Details

### JSON Metadata

Google Takeout includes `.json` files alongside media files. The tool handles various naming quirks:

- Standard: `IMG_1234.jpg.json`
- Truncated filenames (>46 chars)
- Bracket-swapped: `image(1).jpg` → `image.jpg(1).json`
- Edited variants: `image-edited.jpg` → `image.jpg.json`

### Supported Filename Patterns

| Pattern | Example |
|---------|---------|
| `YYYYMMDD-hhmmss` | `Screenshot_20190919-053857.jpg` |
| `YYYYMMDD_hhmmss` | `IMG_20190509_154733.jpg` |
| `YYYY-MM-DD-hh-mm-ss` | `Screenshot_2019-04-16-11-19-37.jpg` |
| `YYYY-MM-DD-hhmmss` | `signal-2020-10-26-163832.jpg` |
| `YYYYMMDDhhmmss` | `201801261147521000.jpg` |
| `YYYY_MM_DD_hh_mm_ss` | `2016_01_30_11_49_15.mp4` |

## Supported Year Folder Languages

EN, DE, FR, ES, PT, CA, NL, IT, PL, RU, CS, RO, SV, NO, DA, FI, HU, TR, JA, KO, ZH-CN, ZH-TW

## License

Apache License 2.0 - Same as the [original project](https://github.com/TheLastGimbus/GooglePhotosTakeoutHelper).

## Credits

Based on [GooglePhotosTakeoutHelper](https://github.com/TheLastGimbus/GooglePhotosTakeoutHelper) by TheLastGimbus.
