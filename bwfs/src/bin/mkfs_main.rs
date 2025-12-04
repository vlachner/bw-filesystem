use bwfs::{config, fs_layout};
use clap::Parser;
use std::fs::{create_dir_all, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Parser)]
struct Cli {
    #[arg(short, long)]
    config: String,
}

// local helper; same logic as in mount_fuse.rs
fn set_bit(bm: &mut [u8], idx: u64) {
    let b = (idx / 8) as usize;
    let i = (idx % 8) as u8;
    bm[b] |= 1 << i;
}

fn main() {
    let args = Cli::parse();

    let cfg = config::load_config(&args.config);

    create_dir_all(&cfg.data_dir).expect("cannot create data_dir");

    let image_path = format!("{}/{}.img", cfg.data_dir, cfg.image_prefix);
    let path = Path::new(&image_path);

    /* ---------------------------------------------------------
       BITMAP + TABLE LAYOUT
    --------------------------------------------------------- */
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

    // we will use inode 1 as root, data block 1 as its directory block
    let root_inode_index: u64 = 1;
    let root_block_index: u64 = 1;

    /* ---------------------------------------------------------
       CREATE IMAGE
    --------------------------------------------------------- */
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .expect("cannot create image");

    file.set_len(total_size).unwrap();

    /* ---------------------------------------------------------
       WRITE SUPERBLOCK
    --------------------------------------------------------- */
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

    /* ---------------------------------------------------------
       INITIALIZE AND WRITE BITMAPS
    --------------------------------------------------------- */
    let mut inode_bitmap = vec![0u8; inode_bitmap_bytes as usize];
    let mut block_bitmap = vec![0u8; block_bitmap_bytes as usize];

    // mark root inode and its data block as allocated
    set_bit(&mut inode_bitmap, root_inode_index);
    set_bit(&mut block_bitmap, root_block_index);

    // write inode bitmap
    file.seek(SeekFrom::Start(inode_bitmap_start)).unwrap();
    file.write_all(&inode_bitmap).unwrap();

    // write block bitmap
    file.seek(SeekFrom::Start(block_bitmap_start)).unwrap();
    file.write_all(&block_bitmap).unwrap();

    /* ---------------------------------------------------------
       CLEAR INODE TABLE
    --------------------------------------------------------- */
    let empty_inode = fs_layout::Inode::empty();
    let inode_bytes = fs_layout::to_bytes(&empty_inode);

    file.seek(SeekFrom::Start(inode_table_start)).unwrap();
    for _ in 0..cfg.inode_count {
        file.write_all(&inode_bytes).unwrap();
    }

    /* ---------------------------------------------------------
       ROOT DIRECTORY (inode 1, block index 1)
    --------------------------------------------------------- */

    // entries "." and ".."
    let dir_entry_size = std::mem::size_of::<fs_layout::DirEntry>() as u64;

    let mut root_inode = fs_layout::Inode::empty();
    root_inode.mode = 0o040755; // directory
    root_inode.size = 2 * dir_entry_size; // exactly "." and ".."
    root_inode.direct[0] = root_block_index; // first data block used

    // Write root inode at inode #1
    let root_inode_offset = inode_table_start + root_inode_index * inode_size;

    file.seek(SeekFrom::Start(root_inode_offset)).unwrap();
    file.write_all(&fs_layout::to_bytes(&root_inode)).unwrap();

    /* ---------------------------------------------------------
       WRITE "." and ".." INTO ROOT DIR BLOCK
    --------------------------------------------------------- */
    let dir_block_offset = data_area_start + root_block_index * cfg.block_size;

    file.seek(SeekFrom::Start(dir_block_offset)).unwrap();

    let dot = fs_layout::DirEntry::new(root_inode_index, ".", true);
    let dotdot = fs_layout::DirEntry::new(root_inode_index, "..", true);

    file.write_all(&fs_layout::to_bytes(&dot)).unwrap();
    file.write_all(&fs_layout::to_bytes(&dotdot)).unwrap();

    // pad rest of the block
    let used_bytes = 2 * dir_entry_size;
    if used_bytes < cfg.block_size {
        let padding = vec![0u8; (cfg.block_size - used_bytes) as usize];
        file.write_all(&padding).unwrap();
    }

    /* ---------------------------------------------------------
       DONE
    --------------------------------------------------------- */
    println!("BWFS image created at {}", image_path);
    println!("To mount: mount_bwfs -c {} <mountpoint>", args.config);
}
