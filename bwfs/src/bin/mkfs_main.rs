//! Entry point for the `mkfs.bwfs` tool.

use clap::Parser;
use bwfs::{config, fs_layout};
use std::fs::{create_dir_all, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

/// Command-line interface for the mkfs.bwfs tool.
#[derive(Parser)]
struct Cli {
    /// Path to the configuration file
    #[arg(short, long)]
    config: String,
}

fn main() {
    let args = Cli::parse();

    // Load configuration
    let cfg = config::load_config(&args.config);

    // Ensure output directory exists
    create_dir_all(&cfg.data_dir).expect("cannot create data_dir");

    // Build final path
    let image_path = format!("{}/{}.img", cfg.data_dir, cfg.image_prefix);
    let path = Path::new(&image_path);

    // Compute filesystem layout
    let inode_size = std::mem::size_of::<fs_layout::Inode>() as u64;
    let inode_table_size = cfg.inode_count * inode_size;
    let inode_table_start = 4096;
    let data_area_start = inode_table_start + inode_table_size;
    let total_size = data_area_start + cfg.total_blocks * cfg.block_size;

    // Create filesystem image
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .expect("cannot create image");

    file.set_len(total_size).unwrap();

    // Write Superblock
    let sb = fs_layout::Superblock {
        magic: *b"BWFS",
        version: 1,
        block_size: cfg.block_size,
        total_blocks: cfg.total_blocks,
        inode_count: cfg.inode_count,
        inode_table_start,
        data_area_start,
    };

    file.seek(SeekFrom::Start(0)).unwrap();
    file.write_all(&fs_layout::to_bytes(&sb)).unwrap();

    // Write empty inode table
    let empty_inode = fs_layout::Inode::empty();
    let inode_bytes = fs_layout::to_bytes(&empty_inode);

    file.seek(SeekFrom::Start(inode_table_start)).unwrap();
    for _ in 0..cfg.inode_count {
        file.write_all(&inode_bytes).unwrap();
    }

    // Create ROOT inode (inode 0)
    let root_inode_offset = inode_table_start;

    let mut root_inode = fs_layout::Inode::empty();
    root_inode.mode = 0o040755;
    root_inode.size = cfg.block_size;
    root_inode.direct[0] = 0;

    file.seek(SeekFrom::Start(root_inode_offset)).unwrap();
    file.write_all(&fs_layout::to_bytes(&root_inode)).unwrap();

    // Write ROOT directory block
    let dir_block_offset = data_area_start;

    let dot = fs_layout::DirEntry::new(0, ".", true);
    let dotdot = fs_layout::DirEntry::new(0, "..", true);

    let dir_entry_size = std::mem::size_of::<fs_layout::DirEntry>();

    file.seek(SeekFrom::Start(dir_block_offset)).unwrap();
    file.write_all(&fs_layout::to_bytes(&dot)).unwrap();
    file.write_all(&fs_layout::to_bytes(&dotdot)).unwrap();

    // Fill rest with zeros
    let used_bytes = 2 * dir_entry_size as u64;
    if used_bytes < cfg.block_size {
        let padding = vec![0u8; (cfg.block_size - used_bytes) as usize];
        file.write_all(&padding).unwrap();
    }

    println!("BWFS image created at {}", image_path);
    println!("To mount: mount_bwfs -c {} <mountpoint>", args.config);
}