# gpth-rs

Rust implementation of [GooglePhotosTakeoutHelper](https://github.com/TheLastGimbus/GooglePhotosTakeoutHelper).

Processes Google Takeout zip files **directly without extraction**, streaming media files in memory and writing them to an organized output folder.

Includes both a **GUI app** (Tauri) and a **CLI tool**.

[日本語版 README](README.ja.md)

## Features

- **No pre-extraction required** - reads zip files directly
- **GUI & CLI** - desktop app with drag & drop, or command-line tool
- **Parallel processing** - uses rayon for EXIF reading, hashing, and file writing
- **Date extraction** - JSON metadata, EXIF, filename pattern guessing (priority order)
- **Duplicate detection** - file size + SHA-256 streaming hash (no file size limit)
- **Multilingual folder recognition** - 32+ language patterns for Google Takeout year folders
- **Japanese ZIP support** - Shift_JIS encoded filenames in ZIP archives
- **Edited file filtering** - skip `-edited`, `-bearbeitet`, `-編集済み`, etc.
- **Date-based organization** - optional YYYY/MM subdirectory output
- **Album support** - process named album folders, output as album directories or JSON index
- **Incremental output** - skips files already present in the output directory (same name & size)

## Installation

### Pre-built binaries

Download from the [Releases](https://github.com/nickel-and-dime/gpth-rs/releases) page:

- `gpth-rs.exe` — GUI (Windows)
- `gpth-rs-cli.exe` — CLI (Windows)

### Build from source

Requires: Rust toolchain, Node.js (for GUI only)

```sh
# CLI only
cargo build --release -p gpth-cli

# GUI (requires Node.js)
npm install
npx tauri build
```

Binaries will be at:
- `target/release/gpth-rs-cli.exe` (CLI)
- `target/release/gpth-rs.exe` (GUI)

## GUI Usage

1. Launch `gpth-rs.exe`
2. Drag & drop ZIP files onto the window, or click **+ Add ZIP**
3. Click **Browse** to select an output directory
4. Toggle options as needed:
   - **Date folders** - Organize into YYYY/MM subdirectories
   - **Skip derivatives** - Exclude edited/effects variants
   - **No guess** - Use only JSON/EXIF metadata
   - **Albums** - Process album information
   - **Album folders** - Create album directories under `output/albums/`
   - **Symlinks** - Use symlinks instead of copies (requires admin on Windows)
5. Click **Run**

## CLI Usage

### Basic

```sh
gpth-rs-cli -o output_dir takeout-*.zip
```

### With date-based subdirectories

```sh
gpth-rs-cli -o output_dir --divide-to-dates takeout-*.zip
```

### With album processing

```sh
gpth-rs-cli -o output_dir --albums takeout-*.zip
```

### Albums in separate folders with symlinks

```sh
gpth-rs-cli -o output_dir --albums --album-dest album --album-link --divide-to-dates takeout-*.zip
```

### All options

```
gpth-rs-cli [OPTIONS] -o <OUTPUT> <ZIP_FILES>...

Arguments:
  <ZIP_FILES>...              Google Takeout zip files

Options:
  -o, --output <DIR>          Output directory (required)
  --divide-to-dates           Organize into YYYY/MM subdirectories
  --skip-extras               Skip derivative images (-edited, -effects, etc.)
  --no-guess                  Disable date guessing from filenames
  --albums                    Process album folders (non-year named folders)
  --album-dest <MODE>         Album output mode: "year" (default) or "album"
  --album-link                Use symlinks instead of copies (--album-dest album only)
  --album-json <PATH>         Output path for albums.json (default: <output>/albums.json)
  -h, --help                  Print help
  -V, --version               Print version
```

### Examples

Process multiple zip files:

```sh
gpth-rs-cli -o ~/Photos takeout-20240101T000000Z-001.zip takeout-20240101T000000Z-002.zip
```

Skip edited variants and organize by date:

```sh
gpth-rs-cli -o ~/Photos --divide-to-dates --skip-extras takeout-*.zip
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

### With `--albums --album-dest album --divide-to-dates`

```
output/
├── 2023/
│   └── 07/
│       ├── IMG_001.jpg
│       └── IMG_002.jpg
├── albums/
│   ├── Vacation 2023/
│   │   ├── IMG_001.jpg      (copy or symlink)
│   │   └── IMG_002.jpg
│   └── Family/
│       └── DSC_100.jpg
├── date-unknown/
│   └── ...
└── albums.json
```

### albums.json

When `--albums` is enabled, an `albums.json` file is written mapping album names to their output files:

```json
{
  "albums": {
    "Vacation 2023": {
      "files": [
        { "filename": "IMG_001.jpg", "output_path": "2023/07/IMG_001.jpg" },
        { "filename": "IMG_002.jpg", "output_path": "2023/07/IMG_002.jpg" }
      ]
    }
  }
}
```

## How It Works

1. **Scan** - Reads all zip entries, collects media files from year folders and JSON metadata. With `--albums`, also collects entries from named album folders.
2. **Date extraction** - Extracts dates in priority order:
   - Google JSON metadata (`photoTakenTime.timestamp`)
   - EXIF (`DateTimeOriginal`, `DateTimeDigitized`, `DateTime`)
   - Filename patterns (`IMG_20230101_120000`, `Screenshot_20230101-120000`, etc.)
3. **Album merge** (with `--albums`) - Matches album entries to year-folder media by filename + size. Unmatched album-only files are added as new media.
4. **Deduplication** - Groups by file size, then SHA-256 hash to remove duplicates
5. **Write** - Streams each file from zip to output directory, sets file modification time. Files already present with matching name and size are skipped. Optionally writes album folders and `albums.json`.

## Project Structure

```
gpth-rs/
├── crates/
│   ├── gpth-core/       # Core library (processing pipeline)
│   └── gpth-cli/        # CLI binary (gpth-rs-cli)
├── src-tauri/           # Tauri GUI backend (gpth-rs)
├── src-frontend/        # GUI frontend (TypeScript + CSS)
└── index.html
```

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
