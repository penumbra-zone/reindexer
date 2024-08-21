use anyhow::anyhow;
use std::path::PathBuf;

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

/// The name of the reindexer archive file.
pub const REINDEXER_FILE_NAME: &'static str = "reindexer_archive.bin";
