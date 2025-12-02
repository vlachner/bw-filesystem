#[repr(C)]
#[derive(Copy, Clone)]
pub struct Superblock {
    pub magic: [u8; 4],
    pub version: u32,

    pub block_size: u64,
    pub total_blocks: u64,
    pub inode_count: u64,

    pub inode_bitmap_start: u64,
    pub block_bitmap_start: u64,
    pub inode_table_start: u64,
    pub data_area_start: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct Inode {
    pub mode: u16,
    pub _pad: u16,
    pub size: u64,
    pub direct: [u64; 12],
}

impl Inode {
    pub fn empty() -> Self {
        Self {
            mode: 0,
            _pad: 0,
            size: 0,
            direct: [0; 12],
        }
    }
}

pub fn to_bytes<T: Copy>(v: &T) -> Vec<u8> {
    let size = std::mem::size_of::<T>();
    let mut buf = vec![0u8; size];
    unsafe {
        std::ptr::copy_nonoverlapping(v as *const T as *const u8, buf.as_mut_ptr(), size);
    }
    buf
}

pub const DIR_TYPE_FILE: u8 = 1;
pub const DIR_TYPE_DIR: u8 = 2;
pub const DIR_NAME_MAX: usize = 60;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct DirEntry {
    pub inode: u64,
    pub name_len: u8,
    pub file_type: u8,
    pub _pad: [u8; 6],
    pub name: [u8; DIR_NAME_MAX],
}

impl DirEntry {
    pub fn empty() -> Self {
        Self {
            inode: 0,
            name_len: 0,
            file_type: 0,
            _pad: [0; 6],
            name: [0; DIR_NAME_MAX],
        }
    }

    pub fn new(inode: u64, name_str: &str, is_dir: bool) -> Self {
        let mut e = DirEntry::empty();
        let bytes = name_str.as_bytes();
        let len = bytes.len().min(DIR_NAME_MAX);

        e.inode = inode;
        e.file_type = if is_dir { DIR_TYPE_DIR } else { DIR_TYPE_FILE };
        e.name_len = len as u8;
        e.name[..len].copy_from_slice(&bytes[..len]);

        e
    }
}
