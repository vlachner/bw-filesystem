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

// Marca como ocupado un índice dentro de un bitmap (inodos o bloques)
fn set_bit(bm: &mut [u8], idx: u64) {
    let b = (idx / 8) as usize;
    let i = (idx % 8) as u8;
    bm[b] |= 1 << i;
}

// Función principal: crea la imagen del sistema de archivos BWFS desde un archivo de configuración
fn main() {
    let args = Cli::parse();

   // Carga la configuración del sistema de archivos desde el archivo indicado
    let cfg = config::load_config(&args.config);

   // Crea el directorio de salida si no existe
    create_dir_all(&cfg.data_dir).expect("cannot create data_dir");

   // Construye la ruta completa del archivo de imagen
    let image_path = format!("{}/{}.img", cfg.data_dir, cfg.image_prefix);
    let path = Path::new(&image_path);

   // Calcula el tamaño necesario para los bitmaps de inodos y bloques en bytes
    let inode_bitmap_bytes = (cfg.inode_count + 7) / 8;
    let block_bitmap_bytes = (cfg.total_blocks + 7) / 8;

   // Función auxiliar para alinear valores al siguiente múltiplo de 4 KiB
    let align4k = |x: u64| (x + 4095) & !4095;

   // Calcula el layout del bitmap de inodos dentro de la imagen
    let inode_bitmap_start = 4096;
    let inode_bitmap_end = inode_bitmap_start + align4k(inode_bitmap_bytes);

   // Calcula el layout del bitmap de bloques dentro de la imagen
    let block_bitmap_start = inode_bitmap_end;
    let block_bitmap_end = block_bitmap_start + align4k(block_bitmap_bytes);

   // Determina el offset inicial de la tabla de inodos
    let inode_size = std::mem::size_of::<fs_layout::Inode>() as u64;
    let inode_table_start = block_bitmap_end;
    let inode_table_size = cfg.inode_count * inode_size;

   // Calcula el inicio del área de datos del sistema de archivos
    let data_area_start = inode_table_start + inode_table_size;
    let total_size = data_area_start + cfg.total_blocks * cfg.block_size;

   // Define el inodo y bloque iniciales reservados para el directorio raíz
    let root_inode_index: u64 = 1;
    let root_block_index: u64 = 1;

   // Crea el archivo de imagen y trunca cualquier contenido previo
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .expect("cannot create image");

   // Establece el tamaño final del archivo de imagen
    file.set_len(total_size).unwrap();

   // Construye la estructura del superblock con la información del FS
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

   // Escribe el superblock al inicio de la imagen
    file.seek(SeekFrom::Start(0)).unwrap();
    file.write_all(&fs_layout::to_bytes(&sb)).unwrap();

   // Inicializa los bitmaps de inodos y bloques en memoria
    let mut inode_bitmap = vec![0u8; inode_bitmap_bytes as usize];
    let mut block_bitmap = vec![0u8; block_bitmap_bytes as usize];

   // Marca como usados el inodo raíz y su bloque de datos
    set_bit(&mut inode_bitmap, root_inode_index);
    set_bit(&mut block_bitmap, root_block_index);

   // Escribe el bitmap de inodos en la imagen
    file.seek(SeekFrom::Start(inode_bitmap_start)).unwrap();
    file.write_all(&inode_bitmap).unwrap();

   // Escribe el bitmap de bloques en la imagen
    file.seek(SeekFrom::Start(block_bitmap_start)).unwrap();
    file.write_all(&block_bitmap).unwrap();

   // Genera un inodo vacío para inicializar toda la tabla
    let empty_inode = fs_layout::Inode::empty();
    let inode_bytes = fs_layout::to_bytes(&empty_inode);

   // Inicializa todos los inodos como vacíos
    file.seek(SeekFrom::Start(inode_table_start)).unwrap();
    for _ in 0..cfg.inode_count {
        file.write_all(&inode_bytes).unwrap();
    }

   // Obtiene el tamaño de una entrada de directorio
    let dir_entry_size = std::mem::size_of::<fs_layout::DirEntry>() as u64;

   // Construye el inodo correspondiente al directorio raíz
    let mut root_inode = fs_layout::Inode::empty();
    root_inode.mode = 0o040755;
    root_inode.size = 2 * dir_entry_size;
    root_inode.direct[0] = root_block_index;

   // Calcula el offset del inodo raíz dentro de la tabla
    let root_inode_offset = inode_table_start + root_inode_index * inode_size;

   // Escribe el inodo raíz en disco
    file.seek(SeekFrom::Start(root_inode_offset)).unwrap();
    file.write_all(&fs_layout::to_bytes(&root_inode)).unwrap();

   // Calcula el offset del bloque de datos del directorio raíz
    let dir_block_offset = data_area_start + root_block_index * cfg.block_size;

   // Posiciona el cursor en el inicio del bloque raíz
    file.seek(SeekFrom::Start(dir_block_offset)).unwrap();

   // Construye las entradas "." y ".." del directorio raíz
    let dot = fs_layout::DirEntry::new(root_inode_index, ".", true);
    let dotdot = fs_layout::DirEntry::new(root_inode_index, "..", true);

   // Escribe las entradas de directorio en el bloque raíz
    file.write_all(&fs_layout::to_bytes(&dot)).unwrap();
    file.write_all(&fs_layout::to_bytes(&dotdot)).unwrap();

   // Rellena el resto del bloque con ceros si las entradas no ocupan todo el bloque
    let used_bytes = 2 * dir_entry_size;
    if used_bytes < cfg.block_size {
        let padding = vec![0u8; (cfg.block_size - used_bytes) as usize];
        file.write_all(&padding).unwrap();
    }

   // Muestra la ubicación de la imagen generada
    println!("BWFS image created at {}", image_path);
    println!("To mount: mount_bwfs -c {} <mountpoint>", args.config);
}
