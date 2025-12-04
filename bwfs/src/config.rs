/// Cargador de configuración para BWFS.
/// Este módulo carga y valida el archivo `config.ini` usado por `mkfs.bwfs`.
/// La configuración controla los parámetros de diseño del sistema de archivos, configuración de red para modo distribuido y rutas de almacenamiento.
/// Todos los campos son obligatorios excepto `network.peers`, que puede estar vacío.

use configparser::ini::Ini;
/// Contiene todos los parámetros de configuración requeridos por mkfs.bwfs.
/// Cada campo corresponde directamente a una clave dentro de `config.ini`, agrupadas en las secciones `[filesystem]`, `[network]` y `[storage]`.
/// `mkfs.bwfs` usa estos valores para: determinar qué tan grande debe ser la imagen del sistema de archivos, asignar áreas de inodos y datos, embeber metadatos (nombre, fingerprint) en el superbloque, preparar metadatos de red para nodos BWFS distribuidos, determinar dónde guardar los archivos `.img` generados.

pub struct BwfsConfig {
    /// Nombre legible del sistema de archivos.
    pub name: String,

    /// Tamaño de un bloque en bytes. Ejemplo: para un bloque monocromático de 1000x1000 - 125000 bytes.
    pub block_size: u64,

    /// Número de bloques de datos a crear en el sistema de archivos. Tamaño total del FS = superbloque + tabla de inodos + block_size * total_blocks.
    pub total_blocks: u64,

    /// Número de inodos reservados en la tabla de inodos.
    pub inode_count: u64,

    /// Dirección en la que este nodo escuchará comandos BWFS distribuidos.
    pub listen_addr: String,

    /// Puerto para el listener..
    pub listen_port: u16,
    
    /// Lista opcional de peers participando en modo BWFS distribuido. Ejemplo: ["10.0.0.1:9000", "10.0.0.2:9000"]
    pub peers: Vec<String>,

    /// Directorio donde se almacenará la imagen del sistema de archivos.
    pub data_dir: String,

    /// Prefijo usado al nombrar archivos de imagen. Ejemplo: "bwfs_block" → "bwfs_block.img"
    pub image_prefix: String,

    /// Fingerprint del sistema de archivos almacenado en el superbloque. Usado posteriormente por el montador para identificar el FS.
    pub fingerprint: String,
}

/// Carga y parsea la configuración BWFS desde `config.ini`.
/// Carga el archivo INI, extrae claves de las secciones `[filesystem]`, `[network]` y `[storage]`, convierte campos numéricos a `u64` o `u16`, valida que los campos requeridos existan y divide `network.peers` en una lista.
/// Esta función hará `panic!()` con un mensaje descriptivo si: falta un campo requerido, un campo numérico no puede ser parseado, o el archivo de configuración no puede ser cargado.
/// Esto es aceptable porque `mkfs.bwfs` debe fallar rápidamente ante una mala configuración.
pub fn load_config(path: &str) -> BwfsConfig {
    let mut ini = Ini::new();
    ini.load(path).expect("Could not load config.ini");

    /// Sección [filesystem]
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

    /// Sección [network]
    let listen_addr = ini
        .get("network", "listen_addr")
        .expect("missing network.listen_addr");

    let listen_port = ini
        .getuint("network", "listen_port")
        .expect("missing network.listen_port")
        .expect("invalid network.listen_port") as u16;

    /// `peers` es opcional: string vacío → vector vacío
    let peers_raw = ini.get("network", "peers").unwrap_or_default();
    let peers = parse_list(&peers_raw);

    /// Sección [storage]
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

/// Parsea una lista separada por comas como: `"node1:9000, node2:9000"` en: `["node1:9000", "node2:9000"]`
fn parse_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}
