// Representa el superbloque del sistema de archivos con metadatos globales
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Superblock {
    // Número mágico para identificar el sistema de archivos (4 bytes)
    pub magic: [u8; 4],
    // Versión del sistema de archivos
    pub version: u32,
    // Tamaño de cada bloque en bytes
    pub block_size: u64,
    // Número total de bloques en el sistema de archivos
    pub total_blocks: u64,
    // Número total de inodos disponibles
    pub inode_count: u64,
    // Bloque donde inicia el bitmap de inodos
    pub inode_bitmap_start: u64,
    // Bloque donde inicia el bitmap de bloques
    pub block_bitmap_start: u64,
    // Bloque donde inicia la tabla de inodos
    pub inode_table_start: u64,
    // Bloque donde inicia el área de datos
    pub data_area_start: u64,
}

// Representa un inodo que almacena metadatos de archivos y directorios
#[repr(C)]
#[derive(Copy, Clone)]
pub struct Inode {
    // Permisos y tipo de archivo
    pub mode: u16,
    // Padding para alineación
    pub _pad: u16,
    // Tamaño del archivo en bytes
    pub size: u64,
    // Arreglo de punteros directos a bloques de datos (12 bloques)
    pub direct: [u64; 12],
}

impl Inode {
    // Crea un inodo vacío con todos los campos inicializados en cero
    pub fn empty() -> Self {
        Self {
            mode: 0,
            _pad: 0,
            size: 0,
            direct: [0; 12],
        }
    }
}

// Convierte cualquier estructura copiable a un vector de bytes
pub fn to_bytes<T: Copy>(v: &T) -> Vec<u8> {
    let size = std::mem::size_of::<T>();
    let mut buf = vec![0u8; size];
    unsafe {
        std::ptr::copy_nonoverlapping(v as *const T as *const u8, buf.as_mut_ptr(), size);
    }
    buf
}

// Constante que identifica una entrada de directorio como archivo regular
pub const DIR_TYPE_FILE: u8 = 1;
// Constante que identifica una entrada de directorio como directorio
pub const DIR_TYPE_DIR: u8 = 2;
// Longitud máxima permitida para nombres de archivos/directorios
pub const DIR_NAME_MAX: usize = 60;

// Representa una entrada de directorio que asocia un nombre con un inodo
#[repr(C)]
#[derive(Copy, Clone)]
pub struct DirEntry {
    // Número de inodo al que apunta esta entrada
    pub inode: u64,
    // Longitud del nombre del archivo/directorio
    pub name_len: u8,
    // Tipo de entrada (archivo o directorio)
    pub file_type: u8,
    // Padding para alineación
    pub _pad: [u8; 6],
    // Nombre del archivo/directorio (máximo 60 caracteres)
    pub name: [u8; DIR_NAME_MAX],
}

impl DirEntry {
    // Crea una entrada de directorio vacía con todos los campos en cero
    pub fn empty() -> Self {
        Self {
            inode: 0,
            name_len: 0,
            file_type: 0,
            _pad: [0; 6],
            name: [0; DIR_NAME_MAX],
        }
    }
    
    // Crea una nueva entrada de directorio con el inodo, nombre y tipo especificados
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
