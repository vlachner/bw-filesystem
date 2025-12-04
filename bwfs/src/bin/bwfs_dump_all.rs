use bwfs::fs_layout::*;
use clap::Parser;
use image::{GrayImage, Luma};
use std::{
    collections::HashMap,
    fs::{create_dir_all, File},
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const IMG_W: u32 = 1000;
const IMG_H: u32 = 1000;
const PIXELS: usize = (IMG_W * IMG_H) as usize;

#[derive(Parser)]
struct Cli {
    #[arg(short, long)]
    image: String,

    #[arg(short, long)]
    out: String,
}

// Lee el superblock desde el inicio del archivo del sistema de archivos
fn read_superblock(file: &mut File) -> Superblock {
    let mut buf = [0u8; std::mem::size_of::<Superblock>()];
    file.seek(SeekFrom::Start(0)).unwrap();
    file.read_exact(&mut buf).unwrap();
    unsafe { std::ptr::read(buf.as_ptr() as *const Superblock) }
}

// Lee la tabla completa de inodos desde disco y la devuelve como un vector
fn read_inode_table(file: &mut File, sb: &Superblock) -> Vec<Inode> {
    let mut v = Vec::new();
    let inode_size = std::mem::size_of::<Inode>();

    file.seek(SeekFrom::Start(sb.inode_table_start)).unwrap();
    for _ in 0..sb.inode_count {
        let mut buf = vec![0u8; inode_size];
        file.read_exact(&mut buf).unwrap();
        let inode: Inode = unsafe { std::ptr::read(buf.as_ptr() as *const _) };
        v.push(inode);
    }
    v
}

// Lee todas las entradas de directorio de un inodo de tipo directorio
fn read_directory_entries(file: &mut File, sb: &Superblock, inode: &Inode) -> Vec<DirEntry> {
    let mut out = Vec::new();
    let block_size = sb.block_size as usize;
    let entry_size = std::mem::size_of::<DirEntry>();

    for blk in inode.direct {
        if blk == 0 {
            continue;
        }
        let offset = sb.data_area_start + blk * sb.block_size;
        let mut buf = vec![0u8; block_size];
        file.seek(SeekFrom::Start(offset)).unwrap();
        file.read_exact(&mut buf).unwrap();

        for chunk in buf.chunks_exact(entry_size) {
            let d: DirEntry = unsafe { std::ptr::read(chunk.as_ptr() as *const _) };
            if d.inode != 0 && d.name_len > 0 {
                out.push(d);
            }
        }
    }

    out
}

// Construye un mapa que asocia número de inodo con nombre de archivo a partir del directorio raíz
fn build_inode_to_name_map(
    file: &mut File,
    sb: &Superblock,
    inodes: &[Inode],
) -> HashMap<u64, String> {
    let mut map = HashMap::new();

    let root = &inodes[1];
    let entries = read_directory_entries(file, sb, root);

    for d in entries {
        let name = std::str::from_utf8(&d.name[..d.name_len as usize])
            .unwrap()
            .to_string();
        map.insert(d.inode, name);
    }

    map
}

// Extrae un bloque de datos y lo guarda como imagen PNG en escala de grises
fn dump_png(
    file: &mut File,
    sb: &Superblock,
    inode_num: u64,
    block_index: usize,
    block_id: u64,
    name: &str,
    out_dir: &str,
) {
    let block_size = sb.block_size as usize;
    let disk_offset = sb.data_area_start + block_id * sb.block_size;

    let mut raw = vec![0u8; block_size];
    file.seek(SeekFrom::Start(disk_offset)).unwrap();
    file.read_exact(&mut raw).unwrap();

    let safe = name.replace("/", "_");

    let png_path = format!(
        "{}/inode_{}_blk_{}_{}.png",
        out_dir, inode_num, block_index, safe
    );

    let mut img = GrayImage::new(IMG_W, IMG_H);

    for i in 0..PIXELS {
        let val = if i < raw.len() { raw[i] } else { 0 };
        let x = (i as u32) % IMG_W;
        let y = (i as u32) / IMG_W;
        img.put_pixel(x, y, Luma([val]));
    }

    img.save(&png_path).unwrap();
    println!("dumped {}", png_path);
}

// Orquesta la lectura del sistema de archivos y el volcado de bloques a imágenes PNG
fn main() {
    let args = Cli::parse();
    let image_path = args.image;
    let out_dir = args.out;

    create_dir_all(&out_dir).unwrap();

    let p = Path::new(&image_path);
    if !p.exists() {
        eprintln!("image not found: {}", image_path);
        return;
    }

    let mut file = File::open(&image_path).unwrap();

    let sb = read_superblock(&mut file);
    let inodes = read_inode_table(&mut file, &sb);

    println!("loaded BWFS image: block size = {}", sb.block_size);

    let name_map = build_inode_to_name_map(&mut file, &sb, &inodes);

    for (ino, inode) in inodes.iter().enumerate() {
        if inode.mode == 0 {
            continue;
        }

        let name = name_map
            .get(&(ino as u64))
            .cloned()
            .unwrap_or_else(|| "anon".into());

        for (i, blk) in inode.direct.iter().enumerate() {
            if *blk == 0 {
                continue;
            }
            dump_png(&mut file, &sb, ino as u64, i, *blk, &name, &out_dir);
        }
    }

    println!("done → {}", out_dir);
}
