//! mkfs module: responsible for creating a brand-new BWFS filesystem image.
//!
//! This file performs the full formatting:
//!   1. Load config.ini
//!   2. Compute filesystem layout (superblock → inode table → data blocks)
//!   3. Allocate .img file of correct final size
//!   4. Write superblock
//!   5. Initialize inode table with empty inodes
//!   6. Create root inode (inode 0)
//!   7. Write root directory block (entries "." and "..")
//!
//! After this step, the filesystem image is a valid BWFS filesystem.
//! It can be inspected using bwfs-info, and later mounted via FUSE.

use std::fs::{create_dir_all, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use crate::config::load_config;
use crate::fs_layout::{to_bytes, DirEntry, Inode, Superblock};

/// Main entry point for mkfs.bwfs
///
/// # Parameters
/// `config_path` — path to the INI configuration file.
///
/// This function *fails fast* when configuration or disk operations are invalid.
/// For filesystem tools, this is acceptable and expected.
pub fn run_mkfs(config_path: &str) {
    // ---------------------------------------------------------
    // 1) Load configuration
    // ---------------------------------------------------------
    let cfg = load_config(config_path);

    // ---------------------------------------------------------
    // 2) Ensure output directory exists
    // ---------------------------------------------------------
    create_dir_all(&cfg.data_dir).expect("cannot create data_dir");

    // Build final path: <data_dir>/<image_prefix>.img
    let image_path = format!("{}/{}.img", cfg.data_dir, cfg.image_prefix);
    let path = Path::new(&image_path);

    // ---------------------------------------------------------
    // 3) Compute filesystem layout in bytes
    // ---------------------------------------------------------
    let inode_size = std::mem::size_of::<Inode>() as u64;
    let inode_table_size = cfg.inode_count * inode_size;

    // Superblock fixed at 4096 bytes (4 KiB alignment)
    let inode_table_start = 4096;

    // Data blocks follow immediately after inode table
    let data_area_start = inode_table_start + inode_table_size;

    // Full image size = superblock + inode table + block storage
    let total_size = data_area_start + cfg.total_blocks * cfg.block_size;

    // ---------------------------------------------------------
    // 4) Create or truncate the filesystem image
    // ---------------------------------------------------------
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .expect("cannot create image");

    file.set_len(total_size).unwrap();

    // ---------------------------------------------------------
    // 5) Write Superblock at offset 0
    // ---------------------------------------------------------
    let sb = Superblock {
        magic: *b"BWFS",
        version: 1,
        block_size: cfg.block_size,
        total_blocks: cfg.total_blocks,
        inode_count: cfg.inode_count,
        inode_table_start,
        data_area_start,
    };

    file.seek(SeekFrom::Start(0)).unwrap();
    file.write_all(&to_bytes(&sb)).unwrap();

    // ---------------------------------------------------------
    // 6) Write empty inode table
    // ---------------------------------------------------------
    let empty_inode = Inode::empty();
    let inode_bytes = to_bytes(&empty_inode);

    file.seek(SeekFrom::Start(inode_table_start)).unwrap();
    for _ in 0..cfg.inode_count {
        file.write_all(&inode_bytes).unwrap();
    }

    // ---------------------------------------------------------
    // 7) Create ROOT inode (inode 0)
    // ---------------------------------------------------------
    //
    // Root inode properties:
    // - directory (0o040000)
    // - permissions (0o755)
    // - size = 1 full block
    // - direct[0] = block 0 (first block of data area)
    //
    let root_inode_offset = inode_table_start + 0 * inode_size;

    let mut root_inode = Inode::empty();
    root_inode.mode = 0o040755; // directory + rwxr-xr-x
    root_inode.size = cfg.block_size; // directory stored in one block
    root_inode.direct[0] = 0; // logical data block index 0

    file.seek(SeekFrom::Start(root_inode_offset)).unwrap();
    file.write_all(&to_bytes(&root_inode)).unwrap();

    // ---------------------------------------------------------
    // 8) Write ROOT directory block
    // ---------------------------------------------------------
    //
    // Block 0 in data area holds entries:
    //   "."  → inode 0
    //   ".." → inode 0  (root parent = itself)
    //
    let dir_block_index: u64 = 0;
    let dir_block_offset = data_area_start + dir_block_index * cfg.block_size;

    let dot = DirEntry::new(0, ".", true);
    let dotdot = DirEntry::new(0, "..", true);

    let dir_entry_size = std::mem::size_of::<DirEntry>();

    file.seek(SeekFrom::Start(dir_block_offset)).unwrap();
    file.write_all(&to_bytes(&dot)).unwrap();
    file.write_all(&to_bytes(&dotdot)).unwrap();

    // Fill rest of directory block with zeros
    let used_bytes = 2 * dir_entry_size as u64;
    if used_bytes < cfg.block_size {
        let padding = vec![0u8; (cfg.block_size - used_bytes) as usize];
        file.write_all(&padding).unwrap();
    }

    // ---------------------------------------------------------
    // Done
    // ---------------------------------------------------------
    println!("BWFS image created at {}", image_path);
}
