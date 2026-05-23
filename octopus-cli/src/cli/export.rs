use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use zip::write::FileOptions;

use crate::constant;
use crate::metadata::{WorkDirMeta, load_metadata};
use crate::session::Session;

#[derive(Serialize)]
struct Manifest {
    version: String,
    rust_version: String,
    os: String,
    platform: String,
    session_start_time: Option<f64>,
    session_end_time: Option<f64>,
}

fn add_dir_to_zip<W: Write + std::io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    dir: &Path,
    prefix: &str,
) -> std::io::Result<()> {
    for entry in walkdir::WalkDir::new(dir).into_iter().flatten() {
        let path = entry.path();
        if path.is_file() {
            let name = format!("{}/{}", prefix, path.strip_prefix(dir).unwrap().display());
            let mut file = File::open(path)?;
            let mut contents = Vec::new();
            file.read_to_end(&mut contents)?;
            zip.start_file(name, FileOptions::<()>::default())?;
            zip.write_all(&contents)?;
        }
    }
    Ok(())
}

pub async fn run_export(
    work_dir: &Path,
    session_id: Option<String>,
    output: Option<PathBuf>,
    yes: bool,
) -> std::io::Result<()> {
    let work_dir = work_dir
        .canonicalize()
        .unwrap_or_else(|_| work_dir.to_path_buf());

    // Resolve session
    let (session_id, is_default) = match session_id {
        Some(id) => (id, false),
        None => {
            let metadata = load_metadata();
            let wd_meta = metadata
                .get_work_dir_meta(&work_dir)
                .cloned()
                .unwrap_or_else(|| WorkDirMeta {
                    path: work_dir.to_string_lossy().to_string(),
                    kaos: "local".to_string(),
                    last_session_id: None,
                });
            match wd_meta.last_session_id {
                Some(id) => (id, true),
                None => {
                    eprintln!("No previous session found for the working directory.");
                    std::process::exit(1);
                }
            }
        }
    };

    let session = match Session::find(&work_dir, &session_id).await {
        Some(s) => s,
        None => {
            eprintln!("Session {} not found.", session_id);
            std::process::exit(1);
        }
    };

    if is_default && !yes {
        eprintln!(
            "Exporting default previous session: {} - {}",
            &session.id[..8.min(session.id.len())],
            session.title
        );
        eprintln!("Use --yes to skip this confirmation.");
        std::process::exit(1);
    }

    let output_path = output.unwrap_or_else(|| {
        PathBuf::from(format!(
            "session-{}.zip",
            &session.id[..8.min(session.id.len())]
        ))
    });

    let file = File::create(&output_path)?;
    let mut zip = zip::ZipWriter::new(file);

    // Add session directory
    let session_dir = session.dir();
    add_dir_to_zip(&mut zip, &session_dir, "session")?;

    // Add manifest
    let manifest = Manifest {
        version: constant::get_version().to_string(),
        rust_version: "unknown".to_string(),
        os: std::env::consts::OS.to_string(),
        platform: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
        session_start_time: None,
        session_end_time: None,
    };
    let manifest_json = serde_json::to_string_pretty(&manifest).unwrap();
    zip.start_file("manifest.json", FileOptions::<()>::default())?;
    zip.write_all(manifest_json.as_bytes())?;

    zip.finish()?;

    println!("Exported session to {}", output_path.display());
    Ok(())
}
