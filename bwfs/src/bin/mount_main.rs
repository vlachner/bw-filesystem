//! Entry point for the `mount.bwfs` tool.

use clap::Parser;
use bwfs::config;
use std::path::Path;

#[path = "../mount_fuse.rs"]
mod fuse_impl;

#[derive(Parser)]
struct Cli {
    #[arg(short, long)]
    config: String,

    #[arg(value_name = "MOUNTPOINT")]
    mountpoint: String,

    #[arg(short, long)]
    foreground: bool,
}

fn main() {
    let args = Cli::parse();
    let cfg = config::load_config(&args.config);
    let image_path = format!("{}/{}.img", cfg.data_dir, cfg.image_prefix);
    
    if !Path::new(&image_path).exists() {
        eprintln!("Error: Filesystem image not found at {}", image_path);
        eprintln!("Please run mkfs_bwfs first to create the filesystem.");
        std::process::exit(1);
    }

    if !Path::new(&args.mountpoint).exists() {
        eprintln!("Error: Mount point {} does not exist", args.mountpoint);
        eprintln!("Please create it first: mkdir -p {}", args.mountpoint);
        std::process::exit(1);
    }

    println!("Mounting BWFS filesystem...");
    println!("  Image: {}", image_path);
    println!("  Mount point: {}", args.mountpoint);

    // Opciones mínimas - sin AllowOther
    let options = vec![
        fuser::MountOption::FSName("bwfs".to_string()),
        fuser::MountOption::RO,
    ];

    let fs = fuse_impl::BwfsFilesystem::new(&image_path);

    println!("Mounting... (Press Ctrl+C to unmount)");
    
    match fuser::mount2(fs, &args.mountpoint, &options) {
        Ok(()) => {
            println!("Filesystem unmounted successfully");
        }
        Err(e) => {
            eprintln!("Failed to mount filesystem: {}", e);
            std::process::exit(1);
        }
    }
}
