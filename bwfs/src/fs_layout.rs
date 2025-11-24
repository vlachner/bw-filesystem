//! Core on-disk structures used by BWFS.
//!
//! These structures define the binary layout of the filesystem image.
//! They are written directly to disk by `mkfs.bwfs` and later read by
//! the mounter (`mount.bwfs`) and diagnostic tools (e.g. `bwfs-info`).
//!
//! All structs are annotated with `#[repr(C)]` so the compiler guarantees
//! predictable field order and binary compatibility. This is essential
//! in a filesystem: the layout must remain stable and independent of
//! Rust compiler optimizations.

/// Superblock: global header describing the entire filesystem.
///
/// This structure lives at offset 0 of the BWFS image and allows the
/// mounting code to parse the rest of the filesystem layout.
///
/// Fields:
/// - `magic`:    Magic identifier for BWFS (e.g. `"BWFS"`).
/// - `version`:  Filesystem version for future compatibility.
/// - `block_size`: Size of each data block in bytes.
/// - `total_blocks`: How many data blocks exist in the filesystem.
/// - `inode_count`:  Number of reserved inodes in the inode table.
/// - `inode_table_start`: Offset *in bytes* where the inode table begins.
/// - `data_area_start`:   Offset *in bytes* where block storage begins.
///
/// Summary:
///   [0x0000] Superblock (fixed size)
///   [..]     Inode table (inode_count entries)
///   [..]     Data area (blocks)
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Superblock {
    /// Filesystem identifier: always "BWFS".
    pub magic: [u8; 4],

    /// Filesystem version.
    pub version: u32,

    /// Size of each block in bytes (e.g. 125000 for 1000×1000 monochrome).
    pub block_size: u64,

    /// Number of data blocks.
    pub total_blocks: u64,

    /// Number of allocated inode slots in the inode table.
    pub inode_count: u64,

    /// Byte offset to the start of the inode table.
    pub inode_table_start: u64,

    /// Byte offset to the start of the data block area.
    pub data_area_start: u64,
}

/// Inode: metadata structure describing a file or directory.
///
/// Inodes are fixed-size entries in the inode table. They do NOT contain
/// filenames. Directory entries (`DirEntry`) map names to inode numbers.
///
/// Fields:
/// - `mode`: file type + permissions (UNIX-style bitmask).
/// - `_pad`: alignment padding (ensures 64-bit alignment).
/// - `size`: file size in bytes.
/// - `direct`: array of direct block pointers (logical block indices).
///
/// This simplified inode structure omits:
/// - timestamps
/// - extended attributes
/// - indirect/ double-indirect pointers
///
/// It is sufficient for a teaching filesystem and small projects.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Inode {
    /// File type + permissions (UNIX layout).
    /// Examples:
    /// - Directory: 0o040000 | 0o755
    /// - Regular file: 0o100000 | 0o644
    pub mode: u16,

    /// Reserved padding for alignment.
    pub _pad: u16,

    /// Logical file size in bytes.
    pub size: u64,

    /// Direct pointers to data blocks.
    /// `direct[0]` is typically the first block of file data.
    /// Direct pointers simplify implementation by avoiding indirect blocks.
    pub direct: [u64; 12],
}

impl Inode {
    /// Returns an inode initialized with zeros.
    ///
    /// Used by mkfs when creating an empty inode table.
    pub fn empty() -> Self {
        Self {
            mode: 0,
            _pad: 0,
            size: 0,
            direct: [0; 12],
        }
    }
}

/// Convert any `Copy` struct into a raw byte vector.
///
/// This is needed because the filesystem image is written as a
/// binary blob. We must serialize the structs exactly as they appear
/// in memory.
///
/// # Safety
///
/// The function uses `std::ptr::copy_nonoverlapping` to copy the struct’s
/// memory representation into a `Vec<u8>`. Because the struct is annotated
/// with `#[repr(C)]`, its layout is stable and safe to copy byte-for-byte.
///
/// This function does **not** perform any endianness conversion.
/// All fields are written in native little-endian format,
/// matching how most CPUs represent integers.
///
/// For a production filesystem you would consider portable encoding,
/// but for a teaching OS/filesystem, native endian is acceptable.
pub fn to_bytes<T: Copy>(v: &T) -> Vec<u8> {
    let size = std::mem::size_of::<T>();
    let mut buf = vec![0u8; size];
    unsafe {
        std::ptr::copy_nonoverlapping(v as *const T as *const u8, buf.as_mut_ptr(), size);
    }
    buf
}

// ---------------------------------------------------------
// Directory Entry structure
// ---------------------------------------------------------

pub const DIR_TYPE_FILE: u8 = 1;
pub const DIR_TYPE_DIR: u8 = 2;

pub const DIR_NAME_MAX: usize = 60;

/// Directory entry mapping a filename to an inode number.
/// Stored inside directory data blocks.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct DirEntry {
    pub inode: u64,               // inode number
    pub name_len: u8,             // number of bytes used in `name`
    pub file_type: u8,            // DIR_TYPE_FILE or DIR_TYPE_DIR
    pub _pad: [u8; 6],            // alignment padding
    pub name: [u8; DIR_NAME_MAX], // UTF-8 bytes of filename
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
