use fuser::{
    Filesystem, Request,
    ReplyAttr, ReplyCreate, ReplyOpen, ReplyData, ReplyWrite, ReplyEmpty, ReplyEntry,
    ReplyStatfs, ReplyLseek, FileAttr, FileType, MountOption,
};
use libc::{ENOENT, EEXIST, EINVAL};
use std::{
    collections::HashMap,
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};
use image::{GrayImage, ImageBuffer, Luma};
use std::io;

const BLOCK_W: usize = 1000;
const BLOCK_H: usize = 1000;
const BLOCK_BYTES: usize = BLOCK_W * BLOCK_H;
const TTL: Duration = Duration::from_secs(1);

type Inode = u64;
type FH = u64;

#[derive(Clone, Debug)]
struct FileNode {
    ino: Inode,
    name: String,
    is_dir: bool,
    size: u64,
    blocks: Vec<PathBuf>,
    dirty: HashMap<usize, Vec<u8>>,
    perm: u32,
    atime: SystemTime,
    mtime: SystemTime,
    ctime: SystemTime,
    mode: u32,
}

impl FileNode {
    fn new(ino: Inode, name: &str, is_dir: bool, perm: u32) -> Self {
        let now = SystemTime::now();
        Self {
            ino,
            name: name.to_string(),
            is_dir,
            size: if is_dir { 0 } else { 0 },
            blocks: vec![],
            dirty: HashMap::new(),
            perm,
            atime: now,
            mtime: now,
            ctime: now,
            mode: 0,
        }
    }

    fn attr(&self) -> FileAttr {
        FileAttr {
            ino: self.ino,
            size: self.size,
            blocks: ((self.size + (BLOCK_BYTES as u64) - 1) / (BLOCK_BYTES as u64)) as u64,
            atime: self.atime,
            mtime: self.mtime,
            ctime: self.ctime,
            crtime: self.ctime,
            kind: if self.is_dir { FileType::Directory } else { FileType::RegularFile },
            perm: (self.perm & 0o7777) as u16,
            nlink: if self.is_dir { 2 } else { 1 },
            uid: 1000,
            gid: 1000,
            rdev: 0,
            flags: 0,
            blksize: BLOCK_BYTES as u32,
        }
    }
}

struct FilesystemState {
    next_ino: Inode,
    path_map: HashMap<String, Inode>,
    nodes: HashMap<Inode, FileNode>,
    handles: HashMap<FH, (Inode, i32)>,
}

impl FilesystemState {
    fn new(_backing: PathBuf) -> Self {
        let mut st = Self {
            next_ino: 2,
            path_map: HashMap::new(),
            nodes: HashMap::new(),
            handles: HashMap::new(),
        };
        let root = FileNode::new(1, "/", true, 0o755);
        st.path_map.insert("/".to_string(), 1);
        st.nodes.insert(1, root);
        st
    }

    fn alloc_ino(&mut self) -> Inode {
        let ino = self.next_ino;
        self.next_ino += 1;
        ino
    }

    fn make_full(parent: Inode, parent_name: &str, name: &str) -> String {
        if parent == 1 {
            format!("/{}", name)
        } else {
            format!("{}/{}", parent_name, name)
        }
    }
}

struct ImageFS {
    state: Arc<Mutex<FilesystemState>>,
}

impl ImageFS {
    fn new(backing: PathBuf) -> Self {
        Self { state: Arc::new(Mutex::new(FilesystemState::new(backing))) }
    }

    fn load_block_from_path(path: &Path) -> io::Result<Vec<u8>> {
        if !path.exists() {
            return Ok(vec![0u8; BLOCK_BYTES]);
        }
        let bytes = std::fs::read(path)?;
        let img = image::load_from_memory(&bytes).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let gray = img.to_luma8();
        let mut out = vec![0u8; BLOCK_BYTES];
        let w = gray.width() as usize;
        let h = gray.height() as usize;
        for y in 0..BLOCK_H {
            for x in 0..BLOCK_W {
                let idx = y * BLOCK_W + x;
                if x < w && y < h {
                    out[idx] = gray.get_pixel(x as u32, y as u32)[0];
                } else {
                    out[idx] = 0;
                }
            }
        }
        Ok(out)
    }

    fn save_block_to_path(path: &Path, buf: &[u8]) -> io::Result<()> {
        assert_eq!(buf.len(), BLOCK_BYTES);
        let mut imgbuf: GrayImage = ImageBuffer::new(BLOCK_W as u32, BLOCK_H as u32);
        for y in 0..BLOCK_H {
            for x in 0..BLOCK_W {
                let value = buf[y * BLOCK_W + x];
                imgbuf.put_pixel(x as u32, y as u32, Luma([value]));
            }
        }
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        imgbuf.save(path).map_err(|e| io::Error::new(io::ErrorKind::Other, e))
    }

    pub fn ensure_blocks_for_size(node: &mut FileNode, new_size: u64) {
        let needed_blocks =
            ((new_size + BLOCK_BYTES as u64 - 1) / BLOCK_BYTES as u64) as usize;

        while node.blocks.len() < needed_blocks {
            let new_block = Self::alloc_block_path();
            node.blocks.push(new_block.into());
        }
    }

    pub fn alloc_block_path() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);

        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        format!("block_{id}.png")
    }
}

impl Filesystem for ImageFS {
    fn getattr(&mut self, _req: &Request<'_>, ino: Inode, _fh: Option<u64>, reply: ReplyAttr) {
        let st = self.state.lock().unwrap();
        match st.nodes.get(&ino) {
            Some(node) => reply.attr(&TTL, &node.attr()),
            None => reply.error(ENOENT),
        }
    }

    fn setattr(
        &mut self,
        _req: &fuser::Request<'_>,
        ino: u64,
        mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        _size: Option<u64>,
        atime: Option<fuser::TimeOrNow>,
        mtime: Option<fuser::TimeOrNow>,
        _ctime: Option<std::time::SystemTime>,
        _fh: Option<u64>,
        _crtime: Option<std::time::SystemTime>,
        _chgtime: Option<std::time::SystemTime>,
        _bkuptime: Option<std::time::SystemTime>,
        _flags: Option<u32>,
        reply: fuser::ReplyAttr,
    ) {
        let mut st = self.state.lock().unwrap();

        let node = match st.nodes.get_mut(&ino) {
            Some(n) => n,
            None => { reply.error(libc::ENOENT); return; }
        };

        let now = std::time::SystemTime::now();

        if let Some(fuser::TimeOrNow::Now) | None = atime {
            node.atime = now;
        }
        if let Some(fuser::TimeOrNow::SpecificTime(t)) = atime {
            node.atime = t;
        }

        if let Some(fuser::TimeOrNow::Now) | None = mtime {
            node.mtime = now;
        }
        if let Some(fuser::TimeOrNow::SpecificTime(t)) = mtime {
            node.mtime = t;
        }

        if let Some(new_mode) = mode {
            node.mode = new_mode;
        }

        reply.attr(&std::time::Duration::from_secs(1), &node.attr());
    }

    fn lookup(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        reply: ReplyEntry,
    ) {
        let st = self.state.lock().unwrap();

        let parent_node = match st.nodes.get(&parent) {
            Some(n) if n.is_dir => n,
            _ => {
                reply.error(ENOENT);
                return;
            }
        };

        let name_str = name.to_string_lossy();
        let full = FilesystemState::make_full(parent, &parent_node.name, &name_str);

        let ino = match st.path_map.get(&full) {
            Some(&i) => i,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        let node = match st.nodes.get(&ino) {
            Some(n) => n,
            None => {
                reply.error(ENOENT);
                return;
            }
        };

        reply.entry(&TTL, &node.attr(), 0);
    }

    fn create(
        &mut self,
        _req: &Request<'_>,
        parent: Inode,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        flags: i32,
        reply: ReplyCreate,
    ) {
        let mut st = self.state.lock().unwrap();
        let parent_node = match st.nodes.get(&parent) {
            Some(n) if n.is_dir => n.clone(),
            _ => { reply.error(ENOENT); return; }
        };
        let name_str = name.to_string_lossy();
        let full = FilesystemState::make_full(parent, &parent_node.name, &name_str);
        if st.path_map.contains_key(&full) {
            reply.error(EEXIST);
            return;
        }
        let ino = st.alloc_ino();
        let mut node = FileNode::new(ino, &full, false, 0o644);
        node.size = 0;
        st.path_map.insert(full.clone(), ino);
        st.nodes.insert(ino, node);
        // create a simple fh
        let fh = ino; // simple mapping
        st.handles.insert(fh, (ino, flags));
        let created = st.nodes.get(&ino).unwrap().clone();
        reply.created(&TTL, &created.attr(), 0, fh, flags as u32);
    }

    fn open(&mut self, _req: &Request<'_>, ino: Inode, flags: i32, reply: ReplyOpen) {
        let mut st = self.state.lock().unwrap();
        if !st.nodes.contains_key(&ino) {
            reply.error(ENOENT);
            return;
        }
        let fh = ino + 1000;
        st.handles.insert(fh, (ino, flags));
        reply.opened(fh, flags as u32);
    }

    fn read(
        &mut self,
        _req: &Request<'_>,
        ino: Inode,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyData,
    ) {
        let mut st = self.state.lock().unwrap();
        let node = match st.nodes.get_mut(&ino) {
            Some(n) => n,
            None => { reply.error(ENOENT); return; }
        };

        let off = offset as u64;
        if off >= node.size {
            reply.data(&[]);
            return;
        }
        let end = std::cmp::min(node.size, off + size as u64);
        let mut out: Vec<u8> = Vec::with_capacity((end - off) as usize);

        let mut pos = off;
        while pos < end {
            let block_idx = (pos / (BLOCK_BYTES as u64)) as usize;
            let block_off = (pos % (BLOCK_BYTES as u64)) as usize;
            let to_read = std::cmp::min(end - pos, (BLOCK_BYTES - block_off) as u64) as usize;

            if block_idx >= node.blocks.len() {
                out.extend(std::iter::repeat(0u8).take(to_read));
            } else {
                if let Some(buf) = node.dirty.get(&block_idx) {
                    out.extend_from_slice(&buf[block_off..block_off + to_read]);
                } else {
                    match ImageFS::load_block_from_path(&node.blocks[block_idx]) {
                        Ok(buf) => out.extend_from_slice(&buf[block_off..block_off + to_read]),
                        Err(_) => out.extend(std::iter::repeat(0u8).take(to_read)),
                    }
                }
            }
            pos += to_read as u64;
        }

        node.atime = SystemTime::now();
        reply.data(&out);
    }

    fn write(
        &mut self,
        _req: &Request<'_>,
        ino: Inode,
        _fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: ReplyWrite,
    ) {
        let mut st = self.state.lock().unwrap();
        let node = match st.nodes.get_mut(&ino) {
            Some(n) => n,
            None => { reply.error(ENOENT); return; }
        };

        let mut pos = offset as u64;
        let mut written = 0usize;
        let total = data.len();

        let final_size = std::cmp::max(node.size, pos + total as u64);
        ImageFS::ensure_blocks_for_size(node, final_size);

        while written < total {
            let block_idx = (pos / (BLOCK_BYTES as u64)) as usize;
            let block_off = (pos % (BLOCK_BYTES as u64)) as usize;
            let to_write = std::cmp::min(total - written, BLOCK_BYTES - block_off);

            let buf = node.dirty.entry(block_idx).or_insert_with(|| {
                ImageFS::load_block_from_path(&node.blocks[block_idx]).unwrap_or_else(|_| vec![0u8; BLOCK_BYTES])
            });

            buf[block_off..block_off + to_write].copy_from_slice(&data[written..written + to_write]);

            written += to_write;
            pos += to_write as u64;
        }

        node.size = std::cmp::max(node.size, offset as u64 + written as u64);
        node.mtime = SystemTime::now();
        reply.written(written as u32);
    }

    fn rename(
        &mut self,
        _req: &Request<'_>,
        parent: Inode,
        name: &OsStr,
        newparent: Inode,
        newname: &OsStr,
        _flags: u32,
        reply: ReplyEmpty,
    ) {
        let mut st = self.state.lock().unwrap();
        let parent_node = match st.nodes.get(&parent) {
            Some(n) => n.clone(),
            None => { reply.error(ENOENT); return; }
        };
        let new_parent_node = match st.nodes.get(&newparent) {
            Some(n) => n.clone(),
            None => { reply.error(ENOENT); return; }
        };
        let old_full = FilesystemState::make_full(parent, &parent_node.name, &name.to_string_lossy());
        let new_full = FilesystemState::make_full(newparent, &new_parent_node.name, &newname.to_string_lossy());
        let ino = match st.path_map.remove(&old_full) {
            Some(i) => i,
            None => { reply.error(ENOENT); return; }
        };
        st.path_map.insert(new_full.clone(), ino);
        if let Some(node) = st.nodes.get_mut(&ino) {
            node.name = new_full;
            node.mtime = SystemTime::now();
        }
        reply.ok();
    }

    fn mkdir(
        &mut self,
        _req: &Request<'_>,
        parent: Inode,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        let mut st = self.state.lock().unwrap();
        let parent_node = match st.nodes.get(&parent) {
            Some(n) if n.is_dir => n.clone(),
            _ => { reply.error(ENOENT); return; }
        };
        let name_s = name.to_string_lossy();
        let full = FilesystemState::make_full(parent, &parent_node.name, &name_s);
        if st.path_map.contains_key(&full) {
            reply.error(EEXIST);
            return;
        }
        let ino = st.alloc_ino();
        let node = FileNode::new(ino, &full, true, mode);
        st.path_map.insert(full.clone(), ino);
        st.nodes.insert(ino, node);
        let n = st.nodes.get(&ino).unwrap().clone();
        reply.entry(&TTL, &n.attr(), 0);
    }

    fn statfs(&mut self, _req: &Request<'_>, _ino: Inode, reply: ReplyStatfs) {
        let st = self.state.lock().unwrap();
        let blocks = 1_000_000u64;
        reply.statfs(
            blocks,
            blocks / 2,
            blocks / 2,
            st.nodes.len() as u64,
            0,
            BLOCK_BYTES as u32,
            255,
            0,
        );
    }

    fn fsync(&mut self, _req: &Request<'_>, ino: Inode, _fh: u64, _datasync: bool, reply: ReplyEmpty) {
        let mut st = self.state.lock().unwrap();
        let node = match st.nodes.get_mut(&ino) {
            Some(n) => n,
            None => { reply.error(ENOENT); return; }
        };
        for (&idx, buf) in node.dirty.iter() {
            if idx >= node.blocks.len() { continue; }
            let path = node.blocks[idx].clone();
            if let Err(e) = ImageFS::save_block_to_path(&path, buf) {
                eprintln!("fsync save error: {:?}", e);
                reply.error(libc::EIO);
                return;
            }
        }
        node.dirty.clear();
        node.mtime = SystemTime::now();
        reply.ok();
    }

    fn access(&mut self, _req: &Request<'_>, ino: Inode, _mask: i32, reply: ReplyEmpty) {
        let st = self.state.lock().unwrap();
        if st.nodes.contains_key(&ino) {
            reply.ok();
        } else {
            reply.error(ENOENT);
        }
    }

    fn unlink(&mut self, _req: &Request<'_>, parent: Inode, name: &OsStr, reply: ReplyEmpty) {
        let mut st = self.state.lock().unwrap();
        let parent_node = match st.nodes.get(&parent) {
            Some(n) => n.clone(),
            None => { reply.error(ENOENT); return; }
        };
        let full = FilesystemState::make_full(parent, &parent_node.name, &name.to_string_lossy());
        let ino = match st.path_map.remove(&full) {
            Some(i) => i,
            None => { reply.error(ENOENT); return; }
        };
        if let Some(node) = st.nodes.remove(&ino) {
            for p in node.blocks {
                let _ = std::fs::remove_file(p);
            }
        }
        reply.ok();
    }

    fn flush(&mut self, _req: &Request<'_>, ino: Inode, _fh: u64, _lock_owner: u64, reply: ReplyEmpty) {
        self.fsync(_req, ino, 0, false, reply);
    }

    fn lseek(&mut self, _req: &Request<'_>, ino: Inode, _fh: u64, offset: i64, whence: i32, reply: ReplyLseek) {
        let st = self.state.lock().unwrap();
        let node = match st.nodes.get(&ino) {
            Some(n) => n.clone(),
            None => { reply.error(ENOENT); return; }
        };
        let newoff = match whence {
            libc::SEEK_SET => offset,
            libc::SEEK_CUR => offset,
            libc::SEEK_END => node.size as i64 + offset,
            _ => { reply.error(EINVAL); return; }
        };
        if newoff < 0 { reply.error(EINVAL); return; }
        reply.offset(newoff);
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} <mountpoint> <backing_dir>", args[0]);
        std::process::exit(1);
    }
    let mountpoint = &args[1];
    let backing = PathBuf::from(&args[2]);
    std::fs::create_dir_all(&backing).expect("create backing dir");

    let fs = ImageFS::new(backing);

    fuser::mount2(
        fs,
        mountpoint,
        &[
            MountOption::FSName("imgfs".to_string()),
            MountOption::AutoUnmount,
            MountOption::RW,
        ],
    ).expect("mount failed");
}