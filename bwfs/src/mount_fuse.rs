//! FUSE implementation for BWFS.
//!
//! This module implements the filesystem operations required by FUSE
//! to mount and interact with a BWFS image.

use fuser::{
    FileAttr, FileType, Filesystem, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    Request,
};
use libc::{ENOENT, ENOTDIR};
use std::ffi::OsStr;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::time::{Duration, UNIX_EPOCH};

use bwfs::fs_layout::{DirEntry, Inode, Superblock, DIR_TYPE_DIR};

const TTL: Duration = Duration::from_secs(1);

/// Main FUSE filesystem structure
pub struct BwfsFilesystem {
    file: File,
    superblock: Superblock,
}

impl BwfsFilesystem {
    /// Create a new BWFS filesystem from an image file
    pub fn new(image_path: &str) -> Self {
        let mut file = File::open(image_path).expect("Cannot open image file");

        // Read superblock
        let superblock = read_struct::<Superblock>(&mut file, 0);

        // Verify magic
        if &superblock.magic != b"BWFS" {
            panic!("Invalid BWFS magic number");
        }

        println!("BWFS Superblock loaded:");
        println!("  Version: {}", superblock.version);
        println!("  Block size: {} bytes", superblock.block_size);
        println!("  Total blocks: {}", superblock.total_blocks);
        println!("  Inode count: {}", superblock.inode_count);

        Self { file, superblock }
    }

    /// Read an inode by its number
    fn read_inode(&mut self, ino: u64) -> Result<Inode, i32> {
        if ino >= self.superblock.inode_count {
            return Err(ENOENT);
        }

        let inode_size = std::mem::size_of::<Inode>() as u64;
        let offset = self.superblock.inode_table_start + ino * inode_size;

        Ok(read_struct(&mut self.file, offset))
    }

    /// Convert inode to FUSE FileAttr
    fn inode_to_attr(&self, ino: u64, inode: &Inode) -> FileAttr {
        let kind = if inode.mode & 0o040000 != 0 {
            FileType::Directory
        } else {
            FileType::RegularFile
        };

        FileAttr {
            ino,
            size: inode.size,
            blocks: (inode.size + 511) / 512,
            atime: UNIX_EPOCH,
            mtime: UNIX_EPOCH,
            ctime: UNIX_EPOCH,
            crtime: UNIX_EPOCH,
            kind,
            perm: (inode.mode & 0o7777) as u16,
            nlink: 1,
            uid: 0,
            gid: 0,
            rdev: 0,
            blksize: self.superblock.block_size as u32,
            flags: 0,
        }
    }

    /// Read directory entries from a directory inode
    fn read_dir_entries(&mut self, inode: &Inode) -> Result<Vec<DirEntry>, i32> {
        if inode.mode & 0o040000 == 0 {
            return Err(ENOTDIR);
        }

        // Get first data block
        let block_idx = inode.direct[0];
        let block_offset = self.superblock.data_area_start 
            + block_idx * self.superblock.block_size;

        // Read directory entries
        let entry_size = std::mem::size_of::<DirEntry>() as u64;
        let max_entries = self.superblock.block_size / entry_size;

        let mut entries = Vec::new();

        for i in 0..max_entries {
            let offset = block_offset + i * entry_size;
            let entry: DirEntry = read_struct(&mut self.file, offset);

            // Stop at first empty entry
            if entry.inode == 0 {
                break;
            }

            entries.push(entry);
        }

        Ok(entries)
    }

    /// Read data from a file
    fn read_file_data(&mut self, inode: &Inode, offset: u64, size: u32) -> Result<Vec<u8>, i32> {
        // Calculate which block we need
        let block_size = self.superblock.block_size;
        let block_idx = offset / block_size;
        let block_offset = offset % block_size;

        if block_idx >= inode.direct.len() as u64 {
            return Ok(Vec::new());
        }

        let data_block = inode.direct[block_idx as usize];
        if data_block == 0 {
            // Sparse file - return zeros
            return Ok(vec![0u8; size as usize]);
        }

        // Calculate absolute offset in image
        let abs_offset = self.superblock.data_area_start 
            + data_block * block_size 
            + block_offset;

        // Read data
        let to_read = size.min((block_size - block_offset) as u32) as usize;
        let mut buffer = vec![0u8; to_read];

        self.file.seek(SeekFrom::Start(abs_offset))
            .map_err(|_| libc::EIO)?;
        self.file.read_exact(&mut buffer)
            .map_err(|_| libc::EIO)?;

        Ok(buffer)
    }
}

impl Filesystem for BwfsFilesystem {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        let name_str = match name.to_str() {
            Some(s) => s,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        // Read parent directory
        let parent_inode = match self.read_inode(parent) {
            Ok(inode) => inode,
            Err(e) => {
                reply.error(e);
                return;
            }
        };

        // Get directory entries
        let entries = match self.read_dir_entries(&parent_inode) {
            Ok(e) => e,
            Err(e) => {
                reply.error(e);
                return;
            }
        };

        // Find matching entry
        for entry in entries {
            let entry_name = std::str::from_utf8(&entry.name[..entry.name_len as usize])
                .unwrap_or("");

            if entry_name == name_str {
                // Found it! Read the inode and return attributes
                match self.read_inode(entry.inode) {
                    Ok(inode) => {
                        let attr = self.inode_to_attr(entry.inode, &inode);
                        reply.entry(&TTL, &attr, 0);
                        return;
                    }
                    Err(e) => {
                        reply.error(e);
                        return;
                    }
                }
            }
        }

        reply.error(ENOENT);
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        match self.read_inode(ino) {
            Ok(inode) => {
                let attr = self.inode_to_attr(ino, &inode);
                reply.attr(&TTL, &attr);
            }
            Err(e) => reply.error(e),
        }
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyData,
    ) {
        let inode = match self.read_inode(ino) {
            Ok(inode) => inode,
            Err(e) => {
                reply.error(e);
                return;
            }
        };

        // Check if it's a directory
        if inode.mode & 0o040000 != 0 {
            reply.error(libc::EISDIR);
            return;
        }

        // Check bounds
        if offset < 0 || offset as u64 >= inode.size {
            reply.data(&[]);
            return;
        }

        // Read data
        match self.read_file_data(&inode, offset as u64, size) {
            Ok(data) => reply.data(&data),
            Err(e) => reply.error(e),
        }
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        let inode = match self.read_inode(ino) {
            Ok(inode) => inode,
            Err(e) => {
                reply.error(e);
                return;
            }
        };

        let entries = match self.read_dir_entries(&inode) {
            Ok(e) => e,
            Err(e) => {
                reply.error(e);
                return;
            }
        };

        for (i, entry) in entries.iter().enumerate().skip(offset as usize) {
            let name = std::str::from_utf8(&entry.name[..entry.name_len as usize])
                .unwrap_or("<invalid>");

            let file_type = if entry.file_type == DIR_TYPE_DIR {
                FileType::Directory
            } else {
                FileType::RegularFile
            };

            // Add entry to reply
            let full = reply.add(entry.inode, (i + 1) as i64, file_type, name);
            
            if full {
                break;
            }
        }

        reply.ok();
    }
}

/// Helper function to read a struct from file
fn read_struct<T: Copy>(file: &mut File, offset: u64) -> T {
    let mut buf = vec![0u8; std::mem::size_of::<T>()];
    file.seek(SeekFrom::Start(offset)).expect("seek failed");
    file.read_exact(&mut buf).expect("read failed");

    unsafe { std::ptr::read(buf.as_ptr() as *const T) }
}