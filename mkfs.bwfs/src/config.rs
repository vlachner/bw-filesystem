//! Configuration loader for BWFS.
//!
//! This module loads and validates the `config.ini` file used by `mkfs.bwfs`.
//! The configuration controls filesystem layout parameters,
//! networking settings for distributed mode, and storage paths.
//!
//! The format expected is:
//!
//! ```ini
//! [filesystem]
//! name = my_bwfs
//! block_size = 125000
//! total_blocks = 200
//! inode_count = 1000
//!
//! [network]
//! listen_addr = 127.0.0.1
//! listen_port = 8080
//! peers = server1:9000, server2:9000
//!
//! [storage]
//! data_dir = /tmp/bwfs_data
//! image_prefix = bwfs_block
//! fingerprint = BWFS_2024_V1
//! ```
//!
//! All fields are mandatory except `network.peers`, which can be empty.

use configparser::ini::Ini;

/// Holds all configuration parameters required by mkfs.bwfs.
///
/// Each field corresponds directly to a key inside the `config.ini`,
/// grouped across the `[filesystem]`, `[network]`, and `[storage]`
/// sections.
///
/// `mkfs.bwfs` uses these values to:
/// - determine how large the filesystem image should be
/// - allocate inode and data areas
/// - embed metadata (name, fingerprint) into the superblock
/// - prepare networking metadata for distributed BWFS nodes
/// - determine where to save the generated `.img` file(s)
pub struct BwfsConfig {
    /// Human-readable name of the filesystem.
    pub name: String,

    /// Size of one block in bytes.
    /// Example: for a 1000x1000 monochrome block - 125000 bytes.
    pub block_size: u64,

    /// Number of data blocks to create in the filesystem.
    /// Total FS size = superblock + inode table + block_size * total_blocks.
    pub total_blocks: u64,

    /// Number of inodes reserved in the inode table.
    pub inode_count: u64,

    /// Address on which this node will listen for distributed BWFS commands.
    pub listen_addr: String,

    /// Port for the listener.
    pub listen_port: u16,

    /// Optional list of peers participating in distributed BWFS mode.
    /// Example: ["10.0.0.1:9000", "10.0.0.2:9000"]
    pub peers: Vec<String>,

    /// Directory where the filesystem image will be stored.
    pub data_dir: String,

    /// Prefix used when naming image files.
    /// Example: "bwfs_block" → "bwfs_block.img"
    pub image_prefix: String,

    /// Filesystem fingerprint stored in the superblock.
    /// Used later by the mounter to identify the FS.
    pub fingerprint: String,
}

/// Load and parse the BWFS configuration from `config.ini`.
///
/// # Behavior
///
/// - Loads the INI file.
/// - Extracts keys from the `[filesystem]`, `[network]`, and `[storage]` sections.
/// - Converts numeric fields to `u64` or `u16`.
/// - Validates that required fields exist.
/// - Splits `network.peers` into a list.
///
/// # Panics
///
/// This function will `panic!()` with a descriptive message if:
///
/// - a required field is missing
/// - a numeric field cannot be parsed
/// - the configuration file cannot be loaded
///
/// This is acceptable because `mkfs.bwfs` should fail fast on bad configuration.
pub fn load_config(path: &str) -> BwfsConfig {
    let mut ini = Ini::new();
    ini.load(path).expect("Could not load config.ini");

    // -------------------------
    // [filesystem] section
    // -------------------------
    let name = ini
        .get("filesystem", "name")
        .expect("missing filesystem.name");

    let block_size = ini
        .getuint("filesystem", "block_size")
        .expect("missing filesystem.block_size")
        .expect("invalid filesystem.block_size") as u64;

    let total_blocks = ini
        .getuint("filesystem", "total_blocks")
        .expect("missing filesystem.total_blocks")
        .expect("invalid filesystem.total_blocks") as u64;

    let inode_count = ini
        .getuint("filesystem", "inode_count")
        .expect("missing filesystem.inode_count")
        .expect("invalid filesystem.inode_count") as u64;

    // -------------------------
    // [network] section
    // -------------------------
    let listen_addr = ini
        .get("network", "listen_addr")
        .expect("missing network.listen_addr");

    let listen_port = ini
        .getuint("network", "listen_port")
        .expect("missing network.listen_port")
        .expect("invalid network.listen_port") as u16;

    // `peers` is optional: empty string → empty vector
    let peers_raw = ini.get("network", "peers").unwrap_or_default();
    let peers = parse_list(&peers_raw);

    // -------------------------
    // [storage] section
    // -------------------------
    let data_dir = ini
        .get("storage", "data_dir")
        .expect("missing storage.data_dir");

    let image_prefix = ini
        .get("storage", "image_prefix")
        .expect("missing storage.image_prefix");

    let fingerprint = ini
        .get("storage", "fingerprint")
        .expect("missing storage.fingerprint");

    BwfsConfig {
        name,
        block_size,
        total_blocks,
        inode_count,
        listen_addr,
        listen_port,
        peers,
        data_dir,
        image_prefix,
        fingerprint,
    }
}

/// Parse a comma-separated list such as:
///
/// `"node1:9000, node2:9000"`
///
/// into:
///
/// `["node1:9000", "node2:9000"]`
fn parse_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}
