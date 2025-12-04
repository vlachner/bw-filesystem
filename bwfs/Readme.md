# BWFS

Sistema de archivos con bloques almacenados como imágenes (1000×1000)

Este documento explica cómo reproducir la demo completa del sistema de archivos BWFS, desde la compilación hasta la visualización de los bloques como imágenes PNG.

1. Compilar el proyecto

```
cd bwfs
cargo build --release
```

Esto genera los binarios en:

```
./target/release/
```

2. Crear la imagen del sistema de archivos, en el `config.ini` especificas el `data_dir`

```
./target/release/mkfs_bwfs -c config.ini
```

Este comando:
lee parámetros desde config.ini
calcula el layout interno (superbloque, bitmaps, tabla de inodos, área de datos)
genera la imagen del filesystem en:

`~/bwfs_storage/bwfs_image.img`

3. Verificar la imagen creada

```
   ./target/release/bwfs_info ~/bwfs_storage/bwfs_image.img
```

Muestra

Tamaño de bloque (1000×1000 → 125 000 bytes)

Número de bloques

Inicio del área de datos

Inodo raíz y contenido inicial

Sirve como verificación inicial antes de montar.

4. Montar BWFS con FUSE

Asegurar directorio de montaje:

```
mkdir -p ~/bwfs_mount
```

Montar:

```
./target/release/mount_bwfs -c config.ini -f ~/bwfs_mount
```

Verificar:

```
mount | grep bwfs
```

Mientras está montado, todas las operaciones de archivos pasan por las funciones FUSE implementadas en BWFS.

5. Crear directorios y archivos dentro de BWFS

```
   mkdir ~/bwfs_mount/test
   echo "hello world" > ~/bwfs_mount/test/hello.txt

    mkdir ~/bwfs_mount/test2
    echo "hello world 2" > ~/bwfs_mount/test2/hello2.txt
```

Estos comandos actúan sobre BWFS, no sobre el filesystem del sistema operativo.

6. Pruebas con archivos grandes

```
   100 KB
   head -c 100000 /dev/urandom > ~/bwfs_mount/hello100k.bin

200 KB
head -c 200000 /dev/urandom > ~/bwfs_mount/hello200k.bin

500 KB
head -c 500000 /dev/urandom > ~/bwfs_mount/hello500k.bin

800 KB
head -c 800000 /dev/urandom > ~/bwfs_mount/hello800k.bin

2 MB
head -c 2000000 /dev/urandom > ~/bwfs_mount/hello2M.bin
```

BWFS dividirá cada archivo en bloques según sea necesario.

7. Desmontar el filesystem

```
   sudo umount -f ~/bwfs_mount
```

8. Helper para visualizar los bloques como imágenes PNG

Crear carpeta de salida:

```
mkdir -p ~/bwfs_dump
```

Ejecutar el dumper:

```
cargo run --release --bin bwfs_dump_all -- \
 --image ~/bwfs_storage/bwfs_image.img \
 --out ~/bwfs_dump
```

````
<nombre*archivo>\_blk*<n>.png
```

Permite visualizar el contenido físico de cada bloque en la imagen BWFS.
````
