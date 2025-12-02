use clap::Parser;
use bwfs::{config, fs_layout};
use std::fs::{create_dir_all, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Parser)]
struct Cli {
    #[arg(short, long)]
    config: String,
}

fn main() {
    let args = Cli::parse();

    let cfg = config::load_config(&args.config);

    create_dir_all(&cfg.data_dir).expect("cannot create data_dir");

    let image_path = format!("{}/{}.img", cfg.data_dir, cfg.image_prefix);
    let path = Path::new(&image_path);
    
    let inode_bitmap_bytes = (cfg.inode_count + 7) / 8;
    let block_bitmap_bytes = (cfg.total_blocks + 7) / 8;

    let align4k = |x: u64| (x + 4095) & !4095;

    let inode_bitmap_start = 4096;
    let inode_bitmap_end = inode_bitmap_start + align4k(inode_bitmap_bytes);

    let block_bitmap_start = inode_bitmap_end;
    let block_bitmap_end = block_bitmap_start + align4k(block_bitmap_bytes);

    let inode_size = std::mem::size_of::<fs_layout::Inode>() as u64;
    let inode_table_start = block_bitmap_end;
    let inode_table_size = cfg.inode_count * inode_size;

    let data_area_start = inode_table_start + inode_table_size;
    let total_size = data_area_start + cfg.total_blocks * cfg.block_size;

    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .expect("cannot create image");

    file.set_len(total_size).unwrap();

    let sb = fs_layout::Superblock {
        magic: *b"BWFS",
        version: 1,
        block_size: cfg.block_size,
        total_blocks: cfg.total_blocks,
        inode_count: cfg.inode_count,
        inode_table_start,
        data_area_start,
        inode_bitmap_start,
        block_bitmap_start,
    };

    file.seek(SeekFrom::Start(0)).unwrap();
    file.write_all(&fs_layout::to_bytes(&sb)).unwrap();

    let empty_inode = fs_layout::Inode::empty();
    let inode_bytes = fs_layout::to_bytes(&empty_inode);

    file.seek(SeekFrom::Start(inode_table_start)).unwrap();
    for _ in 0..cfg.inode_count {
        file.write_all(&inode_bytes).unwrap();
    }

    let root_inode_offset = inode_table_start;

    let mut root_inode = fs_layout::Inode::empty();
    root_inode.mode = 0o040755;
    root_inode.size = cfg.block_size;
    root_inode.direct[0] = 0;

    file.seek(SeekFrom::Start(root_inode_offset)).unwrap();
    file.write_all(&fs_layout::to_bytes(&root_inode)).unwrap();

    let dir_block_offset = data_area_start;

    let dot = fs_layout::DirEntry::new(0, ".", true);
    let dotdot = fs_layout::DirEntry::new(0, "..", true);

    let dir_entry_size = std::mem::size_of::<fs_layout::DirEntry>();

    file.seek(SeekFrom::Start(dir_block_offset)).unwrap();
    file.write_all(&fs_layout::to_bytes(&dot)).unwrap();
    file.write_all(&fs_layout::to_bytes(&dotdot)).unwrap();

    let used_bytes = 2 * dir_entry_size as u64;
    if used_bytes < cfg.block_size {
        let padding = vec![0u8; (cfg.block_size - used_bytes) as usize];
        file.write_all(&padding).unwrap();
    }

    println!("BWFS image created at {}", image_path);
    println!("To mount: mount_bwfs -c {} <mountpoint>", args.config);
}