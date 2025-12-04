use bwfs::config;
use clap::Parser;
use std::path::Path;

#[path = "../mount_fuse.rs"]
mod fuse_impl;

#[derive(Parser)]
struct Cli {
    #[arg(short, long)]
    config: String,

    #[arg(value_name = "MOUNTPOINT")]
    mountpoint: String,

    #[arg(short, long)]
    foreground: bool,
}

// Función principal: valida parámetros, carga configuración y monta el sistema de archivos vía FUSE
fn main() {
    let args = Cli::parse();

    // Carga la configuración del sistema de archivos desde el archivo indicado
    let cfg = config::load_config(&args.config);

    // Construye la ruta completa del archivo de imagen del sistema de archivos
    let image_path = format!("{}/{}.img", cfg.data_dir, cfg.image_prefix);

    // Verifica que la imagen del sistema de archivos exista
    if !Path::new(&image_path).exists() {
        eprintln!("Error: Filesystem image not found at {}", image_path);
        eprintln!("Please run mkfs_bwfs first to create the filesystem.");
        std::process::exit(1);
    }

    // Verifica que el punto de montaje exista
    if !Path::new(&args.mountpoint).exists() {
        eprintln!("Error: Mount point {} does not exist", args.mountpoint);
        eprintln!("Please create it first: mkdir -p {}", args.mountpoint);
        std::process::exit(1);
    }

    // Muestra información básica antes de montar el FS
    println!("Mounting BWFS filesystem...");
    println!("  Image: {}", image_path);
    println!("  Mount point: {}", args.mountpoint);

    // Define las opciones de montaje para FUSE
    let options = vec![
        fuser::MountOption::FSName("bwfs".to_string()),
    ];

    // Inicializa la estructura FUSE del sistema de archivos usando la imagen
    let fs = fuse_impl::BWFS::mount(&image_path);

    // Indica que el sistema está listo para ser montado
    println!("Mounting... (Press Ctrl+C to unmount)");

    // Ejecuta el montaje con FUSE y maneja errores
    match fuser::mount2(fs, &args.mountpoint, &options) {
        Ok(()) => {
            println!("Filesystem unmounted successfully");
        }
        Err(e) => {
            eprintln!("Failed to mount filesystem: {}", e);
            std::process::exit(1);
        }
    }
}
