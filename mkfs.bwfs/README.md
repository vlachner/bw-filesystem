# mkfs_bwfs

`mkfs_bwfs` es el generador del sistema de archivos BWFS.  
Su objetivo es crear la imagen base del FS antes de montarla.

## Archivos

### `Cargo.toml`

Define dependencias (`clap`, `ini`) y configura el binario de Rust.

### `src/main.rs`

Punto de entrada. Procesa los argumentos de línea de comandos y llama al constructor del FS.

### `src/config.rs`

Capa de configuración. Lee el archivo `config.ini` y construye la estructura `BwfsConfig` usada por `mkfs`.

### `src/fs_layout.rs`

Define el formato en disco del sistema de archivos:

- estructura del `Superblock`
- estructura de cada `Inode`
- funciones utilitarias para serializar estos datos a bytes

Estas estructuras determinan cómo se verá la imagen `.img` en disco.

### `src/mkfs.rs`

**Implementa la creación completa del FS**, incluyendo:

1. cargar `config.ini`
2. calcular offsets en disco:
   - superbloque
   - tabla de i-nodos
   - área de datos
3. crear/truncar la imagen final
4. escribir el **superbloque**
5. escribir la **tabla de i-nodos vacíos**
6. inicializar el **i-nodo raíz** (inode 0)
7. escribir el **bloque del directorio raíz** con:
   - `.` → inode 0
   - `..` → inode 0 (el root es su propio padre)

Este módulo deja la imagen totalmente lista para inspección y montaje.

## Uso

```bash
mkfs_bwfs -c config.ini
```

### Comprobar

Cómo inspeccionar la imagen (bwfs-info)

```bash
bwfs_info /tmp/bwfs_data/bwfs_block.img
```

#### Salida esperada

```bash
====== BWFS SUPERBLOCK ======
Magic:           "BWFS"
Version:         1
Block size:      125000 bytes
Total blocks:    200
Inode count:     1000
Inode table @    4096 bytes
Data area @      116096 bytes

====== ROOT INODE (/) ======
Mode:            0o40755
Size:            125000
Direct block[0]: 0

====== ROOT DIRECTORY CONTENT ======
- inode 0 : . (dir)
- inode 0 : .. (dir)
```

Esto confirma que:

- el superbloque es válido
- la tabla de i-nodos está inicializada
- el root inode está correcto
- el directorio raíz fue escrito correctamente
