use std::{
    ffi::OsStr,
    fs::OpenOptions,
    io::{Read, Seek, SeekFrom, Write},
    sync::Mutex,
    time::SystemTime,
};

use fuser::*;
use libc::ENOENT;

use bwfs::fs_layout::*;

const TTL: std::time::Duration = std::time::Duration::from_secs(1);

pub fn test_bit(bm: &[u8], idx: u64) -> bool {
    let b = (idx / 8) as usize;
    let i = (idx % 8) as u8;
    bm[b] & (1 << i) != 0
}

pub fn set_bit(bm: &mut [u8], idx: u64) {
    let b = (idx / 8) as usize;
    let i = (idx % 8) as u8;
    bm[b] |= 1 << i;
}

pub fn clear_bit(bm: &mut [u8], idx: u64) {
    let b = (idx / 8) as usize;
    let i = (idx % 8) as u8;
    bm[b] &= !(1 << i);
}

fn write_dir_entry(fs: &mut FilesystemState, dir: u64, entry: DirEntry) {
    let block_size = fs.superblock.block_size;
    let entry_size = std::mem::size_of::<DirEntry>();

    for i in 0..12 {
        let blk = fs.inodes[dir as usize].direct[i];

        let blk = if blk == 0 {
            let new = fs.alloc_block();
            fs.inodes[dir as usize].direct[i] = new;
            new
        } else {
            blk
        };

        let off = fs.superblock.data_area_start + blk * block_size;
        fs.file.seek(SeekFrom::Start(off)).unwrap();

        let mut buf = vec![0; block_size as usize];
        fs.file.read_exact(&mut buf).unwrap_or(());

        for (idx, chunk) in buf.chunks_exact(entry_size).enumerate() {
            let d: DirEntry = unsafe { std::ptr::read(chunk.as_ptr() as *const _) };

            if d.inode == 0 {
                let abs = off + idx as u64 * entry_size as u64;
                fs.file.seek(SeekFrom::Start(abs)).unwrap();
                fs.file.write_all(&to_bytes(&entry)).unwrap();

                fs.inodes[dir as usize].size += entry_size as u64;
                fs.persist_inode(dir);

                return;
            }
        }
    }

    panic!("Directory is full");
}

fn remove_dir_entry(fs: &mut FilesystemState, dir: u64, name: &str) -> u64 {
    let inode = &mut fs.inodes[dir as usize];

    for &blk in inode.direct.iter() {
        if blk == 0 { continue; }
        let off = block_offset(&fs.superblock, blk);
        fs.file.seek(SeekFrom::Start(off)).unwrap();

        let mut buf = vec![0; fs.superblock.block_size as usize];
        fs.file.read_exact(&mut buf).unwrap();

        let entries = buf.chunks_exact(std::mem::size_of::<DirEntry>());
        for (idx, e) in entries.enumerate() {
            let mut d: DirEntry = unsafe { std::ptr::read(e.as_ptr() as *const _) };
            if d.name_len > 0 && &d.name[..d.name_len as usize] == name.as_bytes() {
                let ino = d.inode;
                d.inode = 0;
                let offset = off + idx as u64 * std::mem::size_of::<DirEntry>() as u64;
                fs.file.seek(SeekFrom::Start(offset)).unwrap();
                fs.file.write_all(&to_bytes(&d)).unwrap();
                fs.persist_inode(dir);
                return ino;
            }
        }
    }

    panic!("File not found");
}

pub struct FilesystemState {
    pub file: std::fs::File,
    pub superblock: Superblock,
    pub inode_bitmap: Vec<u8>,
    pub block_bitmap: Vec<u8>,
    pub inodes: Vec<Inode>,
}

pub struct BWFS {
    state: Mutex<FilesystemState>,
}

fn now() -> SystemTime {
    SystemTime::now()
}

fn inode_offset(sb: &Superblock, ino: u64) -> u64 {
    sb.inode_table_start + ino * std::mem::size_of::<Inode>() as u64
}

fn block_offset(sb: &Superblock, block: u64) -> u64 {
    sb.data_area_start + block * sb.block_size
}

/* ---------------- DISK IO ---------------- */

impl FilesystemState {
    fn persist_inode(&mut self, ino: u64) {
        let off = inode_offset(&self.superblock, ino);
        self.file.seek(SeekFrom::Start(off)).unwrap();
        self.file.write_all(&to_bytes(&self.inodes[ino as usize])).unwrap();
    }

    fn persist_inode_bitmap(&mut self) {
        self.file.seek(SeekFrom::Start(self.superblock.inode_bitmap_start)).unwrap();
        self.file.write_all(&self.inode_bitmap).unwrap();
    }

    fn persist_block_bitmap(&mut self) {
        self.file.seek(SeekFrom::Start(self.superblock.block_bitmap_start)).unwrap();
        self.file.write_all(&self.block_bitmap).unwrap();
    }

    fn alloc_inode(&mut self) -> u64 {
        for i in 0..self.superblock.inode_count {
            if !test_bit(&self.inode_bitmap, i) {
                set_bit(&mut self.inode_bitmap, i);
                self.persist_inode_bitmap();
                return i;
            }
        }
        panic!("No free inodes");
    }

    fn alloc_block(&mut self) -> u64 {
        for i in 0..self.superblock.total_blocks {
            if !test_bit(&self.block_bitmap, i) {
                set_bit(&mut self.block_bitmap, i);
                self.persist_block_bitmap();
                return i;
            }
        }
        panic!("Disk full");
    }

    fn free_inode(&mut self, ino: u64) {
        clear_bit(&mut self.inode_bitmap, ino);
        self.persist_inode_bitmap();
    }

    fn free_block(&mut self, blk: u64) {
        clear_bit(&mut self.block_bitmap, blk);
        self.persist_block_bitmap();
    }
}

/* ---------------- MOUNT ---------------- */

impl BWFS {
    pub fn mount(image: &str) -> Self {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(image)
            .unwrap();

        let mut sb = std::mem::MaybeUninit::<Superblock>::uninit();
        unsafe {
            let p = sb.as_mut_ptr() as *mut u8;
            file.read_exact(std::slice::from_raw_parts_mut(p, std::mem::size_of::<Superblock>())).unwrap();
        }
        let sb = unsafe { sb.assume_init() };

        let ib = ((sb.inode_count + 7) / 8) as usize;
        let bb = ((sb.total_blocks + 7) / 8) as usize;

        file.seek(SeekFrom::Start(sb.inode_bitmap_start)).unwrap();
        let mut inode_bitmap = vec![0; ib];
        file.read_exact(&mut inode_bitmap).unwrap();

        file.seek(SeekFrom::Start(sb.block_bitmap_start)).unwrap();
        let mut block_bitmap = vec![0; bb];
        file.read_exact(&mut block_bitmap).unwrap();

        let mut inodes = vec![Inode::empty(); sb.inode_count as usize];
        file.seek(SeekFrom::Start(sb.inode_table_start)).unwrap();

        for i in 0..sb.inode_count {
            let mut buf = [0u8; std::mem::size_of::<Inode>()];
            file.read_exact(&mut buf).unwrap();
            unsafe {
                inodes[i as usize] = std::ptr::read(buf.as_ptr() as *const _);
            }
        }

        BWFS {
            state: Mutex::new(FilesystemState {
                file,
                superblock: sb,
                inode_bitmap,
                block_bitmap,
                inodes,
            })
        }
    }

    fn getattr_inode(ino: u64, inode: &Inode) -> FileAttr {
        FileAttr {
            ino,
            size: inode.size,
            blocks: 1,
            atime: now(),
            mtime: now(),
            ctime: now(),
            crtime: now(),
            kind: if inode.mode & 0o040000 != 0 { FileType::Directory } else { FileType::RegularFile },
            perm: (inode.mode & 0o7777) as u16,
            nlink: 1,
            uid: 1000,
            gid: 1000,
            rdev: 0,
            flags: 0,
            blksize: 512,
        }
    }
}

/* ---------------- FUSE OPS ---------------- */

impl Filesystem for BWFS {
    fn getattr(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyAttr) {
        let st = self.state.lock().unwrap();
        if ino >= st.inodes.len() as u64 { return reply.error(ENOENT); }
        reply.attr(&TTL, &BWFS::getattr_inode(ino, &st.inodes[ino as usize]));
    }

    fn mkdir(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        reply: ReplyEntry
    ) {
        let mut st = self.state.lock().unwrap();
        let ino = st.alloc_inode();

        let mut inode = Inode::empty();
        inode.mode = (mode | 0o040000) as u16;
        st.inodes[ino as usize] = inode;
        st.persist_inode(ino);

        let entry = DirEntry::new(ino, name.to_str().unwrap(), true);
        write_dir_entry(&mut st, parent, entry);
        reply.entry(&TTL, &BWFS::getattr_inode(ino, &inode), 0);
    }

    fn create(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate
    )
 {
        let mut st = self.state.lock().unwrap();
        let ino = st.alloc_inode();

        let mut inode = Inode::empty();
        inode.mode = (mode | 0o100000) as u16;
        st.inodes[ino as usize] = inode;
        st.persist_inode(ino);

        let entry = DirEntry::new(ino, name.to_str().unwrap(), false);
        write_dir_entry(&mut st, parent, entry);
        reply.created(&TTL, &BWFS::getattr_inode(ino, &inode), 0, 0, 0);
    }

    fn read(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyData
    ) {
        let mut st = self.state.lock().unwrap();
        let inode = &st.inodes[ino as usize];
        let mut buf = vec![0; size as usize];
        let mut pos = offset as usize;

        let mut copied = 0;
        for blk in inode.direct {
            if blk == 0 { continue; }
            let base = block_offset(&st.superblock, blk);
            st.file.seek(SeekFrom::Start(base)).unwrap();
            let mut block = vec![0; st.superblock.block_size as usize];
            st.file.read_exact(&mut block).unwrap();

            let take = std::cmp::min(size as usize - copied, block.len());
            if pos < block.len() {
                buf[copied..copied+take].copy_from_slice(&block[pos..pos+take]);
                copied += take;
            }
            pos = pos.saturating_sub(block.len());
            if copied >= size as usize { break; }
        }

        reply.data(&buf[..copied]);
    }

    fn write(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyWrite
    ) {
        let mut st = self.state.lock().unwrap();

        let block_size = st.superblock.block_size;

        let mut remaining = data;
        let mut file_offset = offset as u64;

        for i in 0..12 {
            if remaining.is_empty() { break; }

            let blk = {
                let blk = st.inodes[ino as usize].direct[i];
                if blk == 0 {
                    let new = st.alloc_block();
                    st.inodes[ino as usize].direct[i] = new;
                    new
                } else {
                    blk
                }
            };

            let blk_off = st.superblock.data_area_start + blk * block_size;

            let write_at = file_offset.min(block_size);
            let writable = (block_size - write_at).min(remaining.len() as u64) as usize;

            st.file.seek(SeekFrom::Start(blk_off + write_at)).unwrap();
            st.file.write_all(&remaining[..writable]).unwrap();

            remaining = &remaining[writable..];
            file_offset = file_offset.saturating_sub(block_size);
        }

        let new_size = (offset as u64 + data.len() as u64).max(st.inodes[ino as usize].size);
        st.inodes[ino as usize].size = new_size;
        st.persist_inode(ino);

        reply.written(data.len() as u32);
    }

    fn unlink(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let mut st = self.state.lock().unwrap();
        let ino = remove_dir_entry(&mut st, parent, name.to_str().unwrap());
        let inode = &st.inodes[ino as usize];

        for b in inode.direct {
            if b != 0 { st.free_block(b); }
        }

        st.free_inode(ino);
        reply.ok();
    }

    fn statfs(&mut self, _req: &Request, _ino: u64, reply: ReplyStatfs) {
        let st = self.state.lock().unwrap();

        let free_blocks: u64 = st.block_bitmap.iter().map(|b| b.count_zeros() as u64).sum();
        let free_inodes: u64 = st.inode_bitmap.iter().map(|b| b.count_zeros() as u64).sum();

        reply.statfs(
            st.superblock.total_blocks,
            free_blocks,
            free_blocks,
            st.superblock.inode_count,
            free_inodes,
            st.superblock.block_size as u32,
            255,
            0,
        );
    }

    fn rename(
        &mut self,
        _req: &Request<'_>,
        p1: u64,
        name: &OsStr,
        p2: u64,
        new: &OsStr,
        _flags: u32,
        reply: ReplyEmpty
    ) {
        let mut st = self.state.lock().unwrap();
        let ino = remove_dir_entry(&mut st, p1, name.to_str().unwrap());
        write_dir_entry(&mut st, p2, DirEntry::new(ino, new.to_str().unwrap(), false));
        reply.ok();
    }

    fn access(&mut self, _req: &Request<'_>, _: u64, _: i32, reply: ReplyEmpty) { reply.ok() }
    fn flush(&mut self, _req: &Request<'_>, _: u64, _: u64, _: u64, reply: ReplyEmpty) { reply.ok() }
    fn fsync(&mut self, _req: &Request<'_>, _: u64, _: u64, _: bool, reply: ReplyEmpty) { reply.ok() }
    fn lseek(&mut self, _req: &Request<'_>, _: u64, _: u64, _: i64, _: i32, reply: ReplyLseek) { reply.error(libc::ENOSYS) }
    fn open(&mut self, _req: &Request<'_>, _: u64, _: i32, reply: ReplyOpen) { reply.opened(0, 0) }
}