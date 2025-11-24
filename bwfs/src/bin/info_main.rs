//! CLI entry point for `bwfs-info`

use clap::Parser;
use bwfs::fs_layout;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

/// Simple inspection tool for BWFS images
#[derive(Parser)]
struct Cli {
    /// Path to the .img file
    image: String,
}

fn main() {
    let args = Cli::parse();
    print_fs_info(&args.image);
}

fn read_struct<T: Copy>(file: &mut File, offset: u64) -> T {
    let mut buf = vec![0u8; std::mem::size_of::<T>()];
    file.seek(SeekFrom::Start(offset)).expect("seek failed");
    file.read_exact(&mut buf).expect("read failed");
    unsafe { std::ptr::read(buf.as_ptr() as *const T) }
}

fn print_fs_info(path: &str) {
    let mut file = File::open(path).expect("cannot open image");

    // Read SUPERBLOCK
    let sb: fs_layout::Superblock = read_struct(&mut file, 0);

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

    // Read ROOT INODE
    let root: fs_layout::Inode = read_struct(&mut file, sb.inode_table_start);

    println!("\n====== ROOT INODE (/) ======");
    println!("Mode:            0o{:o}", root.mode);
    println!("Size:            {}", root.size);
    println!("Direct block[0]: {}", root.direct[0]);

    // Read ROOT DIRECTORY
    let dir_block_offset = sb.data_area_start + root.direct[0] * sb.block_size;
    let entry_size = std::mem::size_of::<fs_layout::DirEntry>() as u64;

    let dot: fs_layout::DirEntry = read_struct(&mut file, dir_block_offset);
    let dotdot: fs_layout::DirEntry = read_struct(&mut file, dir_block_offset + entry_size);

    println!("\n====== ROOT DIRECTORY CONTENT ======");
    print_dir_entry(&dot);
    print_dir_entry(&dotdot);
}

fn print_dir_entry(e: &fs_layout::DirEntry) {
    let name = std::str::from_utf8(&e.name[..e.name_len as usize]).unwrap_or("<invalid>");
    let kind = match e.file_type {
        1 => "file",
        2 => "dir",
        _ => "unknown",
    };
    println!("- inode {} : {} ({})", e.inode, name, kind);
}
