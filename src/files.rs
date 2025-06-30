use anyhow::{anyhow, Context};
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

/// Retrieve the home directory for the user running this program.
///
/// This may not exist on certain platforms, hence the error.
fn home_dir() -> anyhow::Result<PathBuf> {
    Ok(directories::UserDirs::new()
        .ok_or(anyhow!("no user directories on platform"))?
        .home_dir()
        .to_path_buf())
}

/// Return the default directory for penumbra.
///
/// This can fail if home directories aren't available on the machine, for some reason.
pub fn default_penumbra_home() -> anyhow::Result<PathBuf> {
    Ok(home_dir()?.join(".penumbra/network_data/node0"))
}

/// Return the default fullpath to a reindexer archive sqlite3 db.
///
/// This can fail if home directories aren't available on the machine, for some reason.
pub(crate) fn default_reindexer_archive_filepath(chain_id: &str) -> anyhow::Result<PathBuf> {
    Ok(default_reindexer_home()?
        .join(chain_id)
        .join(REINDEXER_FILE_NAME))
}

/// Return the default fullpath to a reindexer data directory,
/// where large archives will be downloaded and saved. Defaults to
/// `~/.local/share/penumbra-reindexer`.
pub fn default_reindexer_home() -> anyhow::Result<PathBuf> {
    let path = ProjectDirs::from("zone", "penumbra", "penumbra-reindexer")
        .context("failed to get platform data dir")?
        .data_dir()
        .to_path_buf();
    Ok(path)
}

/// Return the path to the working directory for a regen operation,
/// which should be in a subdir of the home directory.
pub fn default_regen_working_dir(home: &Path, chain_id: &str) -> PathBuf {
    home.join(chain_id).join("regen-working-dir")
}

/// Get the archive file, based on optional overrides to reindexer home directory,
/// or an explicit path to the archive sqlite3 db. Reused by several subcommands.
pub fn archive_filepath_from_opts(
    home: Option<PathBuf>,
    archive_file: Option<PathBuf>,
    chain_id: Option<String>,
) -> anyhow::Result<PathBuf> {
    let out = match (home.as_ref(), archive_file.as_ref()) {
        (None, Some(x)) => x.to_owned(),
        (Some(x), None) => {
            let mut buf = x.to_owned();
            buf.push(chain_id.unwrap_or("penumbra-1".to_owned()));
            buf.push(REINDEXER_FILE_NAME);
            buf
        }
        (None, None) => default_reindexer_archive_filepath(
            chain_id.unwrap_or("penumbra-1".to_owned()).as_str(),
        )?,
        // If both were declared, prefer the explicit archive-file path.
        (Some(_), Some(x)) => x.clone(),
    };
    Ok(out)
}

/// The name of the reindexer archive file.
pub const REINDEXER_FILE_NAME: &str = "reindexer-archive.sqlite";
