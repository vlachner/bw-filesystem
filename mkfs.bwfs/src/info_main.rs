//! CLI entry point for `bwfs-info`
//!
//! Usage:
//!     bwfs_info <image_file>

mod fs_layout;
mod info;

use clap::Parser;

/// Simple inspection tool for BWFS images
#[derive(Parser)]
struct Cli {
    /// Path to the .img file
    image: String,
}

fn main() {
    let args = Cli::parse();
    info::print_fs_info(&args.image);
}
