//! Entry point for the `mkfs.bwfs` tool.
//!
//! This binary is responsible for initializing (formatting) a new BWFS filesystem
//! according to the parameters provided in a `config.ini` file.
//!
//! The creation process includes:
//! - Loading and validating configuration values.
//! - Writing the BWFS superblock.
//! - Initializing the inode table.
//! - Creating the root directory inode.
//! - Writing the initial directory block containing "." and "..".
//!
//! This file only handles CLI parsing. The actual filesystem creation
//! logic is implemented in `mkfs.rs`.

mod config;
mod fs_layout;
mod mkfs;

use clap::Parser;

/// Command-line interface for the mkfs.bwfs tool.
///
/// Usage:
///
/// ```bash
/// mkfs_bwfs -c config.ini
/// ```
///
/// Required arguments:
/// - `-c, --config <FILE>`: Path to the `config.ini` file containing
///   filesystem layout and storage parameters.
///
/// Example:
///
/// ```bash
/// mkfs_bwfs --config /etc/bwfs/myfs.ini
/// ```
#[derive(Parser)]
struct Cli {
    /// Path to the configuration file (`.ini`) that defines filesystem parameters.
    #[arg(short, long)]
    config: String,
}

fn main() {
    // Parse command-line arguments (clap handles error messages automatically)
    let args = Cli::parse();

    // Delegate all filesystem creation logic to mkfs::run_mkfs
    // main.rs focused on CLI behavior.
    mkfs::run_mkfs(&args.config);
}
