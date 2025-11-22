//! bwfs-info: inspection utility for BWFS filesystem images.
//!
//! This module provides helper functions to read:
//!   - the Superblock
//!   - the root inode
//!   - the root directory entries
//!
//! The goal is to diagnose and verify mkfs outputs without using hexdump.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use crate::fs_layout::{DirEntry, Inode, Superblock};

/// Read a struct from disk given a type T and file offset.
///
/// # Safety
/// We rely on the fact that all on-disk structs use `repr(C)`
/// and are packed exactly as stored.
fn read_struct<T: Copy>(file: &mut File, offset: u64) -> T {
    let mut buf = vec![0u8; std::mem::size_of::<T>()];
    file.seek(SeekFrom::Start(offset)).expect("seek failed");
    file.read_exact(&mut buf).expect("read failed");

    unsafe { std::ptr::read(buf.as_ptr() as *const T) }
}

/// Reads `n` directory entries starting at a given offset.
/// Only used for root directory debugging.
fn read_dir_entry(file: &mut File, offset: u64) -> DirEntry {
    let mut buf = vec![0u8; std::mem::size_of::<DirEntry>()];
    file.seek(SeekFrom::Start(offset)).unwrap();
    file.read_exact(&mut buf).unwrap();

    unsafe { std::ptr::read(buf.as_ptr() as *const DirEntry) }
}

/// Print a human-friendly summary of a BWFS filesystem image.
pub fn print_fs_info(path: &str) {
    let mut file = File::open(path).expect("cannot open image");

    // ---------------------------------------------------------
    // Read SUPERBLOCK
    // ---------------------------------------------------------
    let sb: Superblock = read_struct(&mut file, 0);

    println!("====== BWFS SUPERBLOCK ======");
    println!(
        "Magic:           {:?}",
        std::str::from_utf8(&sb.magic).unwrap_or("???")
    );
    println!("Version:         {}", sb.version);
    println!("Block size:      {} bytes", sb.block_size);
    println!("Total blocks:    {}", sb.total_blocks);
    println!("Inode count:     {}", sb.inode_count);
    println!("Inode table @    {} bytes", sb.inode_table_start);
    println!("Data area @      {} bytes", sb.data_area_start);

    // ---------------------------------------------------------
    // Read ROOT INODE (inode 0)
    // ---------------------------------------------------------
    let inode_size = std::mem::size_of::<Inode>() as u64;
    let root_inode_offset = sb.inode_table_start;

    let root: Inode = read_struct(&mut file, root_inode_offset);

    println!("\n====== ROOT INODE (/) ======");
    println!("Mode:            0o{:o}", root.mode);
    println!("Size:            {}", root.size);
    println!("Direct block[0]: {}", root.direct[0]);

    // ---------------------------------------------------------
    // Read ROOT DIRECTORY BLOCK
    // ---------------------------------------------------------
    let dir_block_idx = root.direct[0];
    let dir_block_offset = sb.data_area_start + dir_block_idx * sb.block_size;

    let entry_size = std::mem::size_of::<DirEntry>() as u64;

    let dot: DirEntry = read_dir_entry(&mut file, dir_block_offset);
    let dotdot: DirEntry = read_dir_entry(&mut file, dir_block_offset + entry_size);

    println!("\n====== ROOT DIRECTORY CONTENT ======");
    print_dir_entry(&dot);
    print_dir_entry(&dotdot);
}

/// Print a single DirEntry in readable form.
fn print_dir_entry(e: &DirEntry) {
    let name = std::str::from_utf8(&e.name[..e.name_len as usize]).unwrap_or("<invalid>");
    let kind = match e.file_type {
        1 => "file",
        2 => "dir",
        _ => "unknown",
    };
    println!("- inode {} : {} ({})", e.inode, name, kind);
}
