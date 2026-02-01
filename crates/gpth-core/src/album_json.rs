use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::media::Media;

#[derive(Serialize)]
struct AlbumFile {
    filename: String,
    output_path: String,
}

#[derive(Serialize)]
struct AlbumInfo {
    files: Vec<AlbumFile>,
}

#[derive(Serialize)]
struct AlbumsJson {
    albums: BTreeMap<String, AlbumInfo>,
}

/// Write albums.json mapping album names to their files and output paths.
pub fn write_albums_json(
    media: &[Media],
    assignments: &[std::path::PathBuf],
    output_dir: &Path,
    album_json_path: &Path,
) -> anyhow::Result<()> {
    let mut albums: BTreeMap<String, Vec<AlbumFile>> = BTreeMap::new();

    for (m, dest) in media.iter().zip(assignments.iter()) {
        for album_name in &m.albums {
            let relative = dest
                .strip_prefix(output_dir)
                .unwrap_or(dest)
                .to_string_lossy()
                .replace('\\', "/");
            albums.entry(album_name.clone()).or_default().push(AlbumFile {
                filename: m.filename.clone(),
                output_path: relative,
            });
        }
    }

    let json = AlbumsJson {
        albums: albums
            .into_iter()
            .map(|(name, files)| (name, AlbumInfo { files }))
            .collect(),
    };

    let file = std::fs::File::create(album_json_path)?;
    serde_json::to_writer_pretty(file, &json)?;

    Ok(())
}
