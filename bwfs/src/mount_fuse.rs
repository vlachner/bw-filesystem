use std::{
    ffi::OsStr,
    fs::OpenOptions,
    io::{Read, Seek, SeekFrom, Write},
    sync::Mutex,
    time::SystemTime,
};

use std::os::unix::ffi::OsStrExt;

use fuser::*;
use libc::ENOENT;

use bwfs::fs_layout::*;

// Tiempo de vida para atributos de archivos en caché
const TTL: std::time::Duration = std::time::Duration::from_secs(1);

// Verifica si el bit en la posición idx está activo en el bitmap
pub fn test_bit(bm: &[u8], idx: u64) -> bool {
    let b = (idx / 8) as usize;
    let i = (idx % 8) as u8;
    bm[b] & (1 << i) != 0
}

// Establece el bit en la posición idx en el bitmap
pub fn set_bit(bm: &mut [u8], idx: u64) {
    let b = (idx / 8) as usize;
    let i = (idx % 8) as u8;
    bm[b] |= 1 << i;
}

// Limpia el bit en la posición idx en el bitmap
pub fn clear_bit(bm: &mut [u8], idx: u64) {
    let b = (idx / 8) as usize;
    let i = (idx % 8) as u8;
    bm[b] &= !(1 << i);
}

// Escribe una entrada de directorio en el inodo de directorio especificado
fn write_dir_entry(fs: &mut FilesystemState, dir: u64, entry: DirEntry) {
    let block_size = fs.superblock.block_size;
    let entry_size = std::mem::size_of::<DirEntry>();

    for i in 0..12 {
        let blk = fs.inodes[dir as usize].direct[i];

        let blk = if blk == 0 {
            let new = fs.alloc_block();
            fs.inodes[dir as usize].direct[i] = new;
            new
        } else {
            blk
        };

        let off = fs.superblock.data_area_start + blk * block_size;
        fs.file.seek(SeekFrom::Start(off)).unwrap();

        let mut buf = vec![0; block_size as usize];
        fs.file.read_exact(&mut buf).unwrap_or(());

        for (idx, chunk) in buf.chunks_exact(entry_size).enumerate() {
            let d: DirEntry = unsafe { std::ptr::read(chunk.as_ptr() as *const _) };

            if d.inode == 0 {
                let abs = off + idx as u64 * entry_size as u64;
                fs.file.seek(SeekFrom::Start(abs)).unwrap();
                fs.file.write_all(&to_bytes(&entry)).unwrap();

                fs.inodes[dir as usize].size += entry_size as u64;
                fs.persist_inode(dir);

                return;
            }
        }
    }

    panic!("Directory is full");
}

// Elimina una entrada de directorio por nombre y retorna su número de inodo
fn remove_dir_entry(fs: &mut FilesystemState, dir: u64, name: &str) -> u64 {
    let inode = &mut fs.inodes[dir as usize];

    for &blk in inode.direct.iter() {
        if blk == 0 {
            continue;
        }
        let off = block_offset(&fs.superblock, blk);
        fs.file.seek(SeekFrom::Start(off)).unwrap();

        let mut buf = vec![0; fs.superblock.block_size as usize];
        fs.file.read_exact(&mut buf).unwrap();

        let entries = buf.chunks_exact(std::mem::size_of::<DirEntry>());
        for (idx, e) in entries.enumerate() {
            let mut d: DirEntry = unsafe { std::ptr::read(e.as_ptr() as *const _) };
            if d.name_len > 0 && &d.name[..d.name_len as usize] == name.as_bytes() {
                let ino = d.inode;
                d.inode = 0;
                let offset = off + idx as u64 * std::mem::size_of::<DirEntry>() as u64;
                fs.file.seek(SeekFrom::Start(offset)).unwrap();
                fs.file.write_all(&to_bytes(&d)).unwrap();
                fs.persist_inode(dir);
                return ino;
            }
        }
    }

    panic!("File not found");
}

// Estado del sistema de archivos que mantiene el superbloque, bitmaps e inodos en memoria
pub struct FilesystemState {
    pub file: std::fs::File,
    pub superblock: Superblock,
    pub inode_bitmap: Vec<u8>,
    pub block_bitmap: Vec<u8>,
    pub inodes: Vec<Inode>,
}

// Implementación del sistema de archivos BWFS con estado protegido por mutex
pub struct BWFS {
    state: Mutex<FilesystemState>,
}

// Retorna la hora actual del sistema
fn now() -> SystemTime {
    SystemTime::now()
}

// Calcula el offset en disco del inodo especificado
fn inode_offset(sb: &Superblock, ino: u64) -> u64 {
    sb.inode_table_start + ino * std::mem::size_of::<Inode>() as u64
}

// Calcula el offset en disco del bloque de datos especificado
fn block_offset(sb: &Superblock, block: u64) -> u64 {
    sb.data_area_start + block * sb.block_size
}

/* ---------------- DISK IO ---------------- */

impl FilesystemState {
    // Persiste un inodo en disco
    fn persist_inode(&mut self, ino: u64) {
        let off = inode_offset(&self.superblock, ino);
        self.file.seek(SeekFrom::Start(off)).unwrap();
        self.file
            .write_all(&to_bytes(&self.inodes[ino as usize]))
            .unwrap();
    }

    // Persiste el bitmap de inodos en disco
    fn persist_inode_bitmap(&mut self) {
        self.file
            .seek(SeekFrom::Start(self.superblock.inode_bitmap_start))
            .unwrap();
        self.file.write_all(&self.inode_bitmap).unwrap();
    }

    // Persiste el bitmap de bloques en disco
    fn persist_block_bitmap(&mut self) {
        self.file
            .seek(SeekFrom::Start(self.superblock.block_bitmap_start))
            .unwrap();
        self.file.write_all(&self.block_bitmap).unwrap();
    }

    // Asigna un inodo libre y lo marca como usado
    fn alloc_inode(&mut self) -> u64 {
        for i in 0..self.superblock.inode_count {
            if !test_bit(&self.inode_bitmap, i) {
                set_bit(&mut self.inode_bitmap, i);
                self.persist_inode_bitmap();
                return i;
            }
        }
        panic!("No free inodes");
    }

    // Asigna un bloque libre y lo marca como usado (bloque 0 está reservado)
    fn alloc_block(&mut self) -> u64 {
        for i in 1..self.superblock.total_blocks {
            if !test_bit(&self.block_bitmap, i) {
                set_bit(&mut self.block_bitmap, i);
                self.persist_block_bitmap();
                return i;
            }
        }
        panic!("Disk full");
    }

    // Libera un inodo y lo marca como disponible
    fn free_inode(&mut self, ino: u64) {
        clear_bit(&mut self.inode_bitmap, ino);
        self.persist_inode_bitmap();
    }

    // Libera un bloque y lo marca como disponible
    fn free_block(&mut self, blk: u64) {
        clear_bit(&mut self.block_bitmap, blk);
        self.persist_block_bitmap();
    }
}

/* ---------------- MOUNT ---------------- */

impl BWFS {
    // Monta una imagen de sistema de archivos BWFS desde disco
    pub fn mount(image: &str) -> Self {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(image)
            .unwrap();

        let mut sb = std::mem::MaybeUninit::<Superblock>::uninit();
        unsafe {
            let p = sb.as_mut_ptr() as *mut u8;
            file.read_exact(std::slice::from_raw_parts_mut(
                p,
                std::mem::size_of::<Superblock>(),
            ))
            .unwrap();
        }
        let sb = unsafe { sb.assume_init() };

        let ib = ((sb.inode_count + 7) / 8) as usize;
        let bb = ((sb.total_blocks + 7) / 8) as usize;

        file.seek(SeekFrom::Start(sb.inode_bitmap_start)).unwrap();
        let mut inode_bitmap = vec![0; ib];
        file.read_exact(&mut inode_bitmap).unwrap();

        file.seek(SeekFrom::Start(sb.block_bitmap_start)).unwrap();
        let mut block_bitmap = vec![0; bb];
        file.read_exact(&mut block_bitmap).unwrap();

        let mut inodes = vec![Inode::empty(); sb.inode_count as usize];
        file.seek(SeekFrom::Start(sb.inode_table_start)).unwrap();

        for i in 0..sb.inode_count {
            let mut buf = [0u8; std::mem::size_of::<Inode>()];
            file.read_exact(&mut buf).unwrap();
            unsafe {
                inodes[i as usize] = std::ptr::read(buf.as_ptr() as *const _);
            }
        }

        BWFS {
            state: Mutex::new(FilesystemState {
                file,
                superblock: sb,
                inode_bitmap,
                block_bitmap,
                inodes,
            }),
        }
    }

    // Convierte un inodo a atributos de archivo FUSE
    fn getattr_inode(ino: u64, inode: &Inode) -> FileAttr {
        FileAttr {
            ino,
            size: inode.size,
            blocks: 1,
            atime: now(),
            mtime: now(),
            ctime: now(),
            crtime: now(),
            kind: if inode.mode & 0o040000 != 0 {
                FileType::Directory
            } else {
                FileType::RegularFile
            },
            nlink: 1,
            perm: 0o777,
            uid: 0,
            gid: 0,
            rdev: 0,
            flags: 0,
            blksize: 512,
        }
    }
}

/* ---------------- FUSE OPS ---------------- */

impl Filesystem for BWFS {
    // Obtiene los atributos de un archivo o directorio
    fn getattr(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyAttr) {
        let st = self.state.lock().unwrap();

        let inode = &st.inodes[ino as usize];
        println!(
            "getattr(ino = {}): mode={:o}, size={}, direct={:?}",
            ino, inode.mode, inode.size, inode.direct,
        );

        if ino >= st.inodes.len() as u64 {
            return reply.error(ENOENT);
        }

        reply.attr(&TTL, &BWFS::getattr_inode(ino, inode));
    }

    // Crea un nodo de archivo (archivo regular)
    fn mknod(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        mode: u32,
        rdev: u32,
        umask: u32,
        reply: ReplyEntry,
    ) {
        println!(
            "mknod(parent = {}, name = {:?}, mode = {:o}, rdev = {}, umask = {})",
            parent, name, mode, rdev, umask
        );

        let mut st = self.state.lock().unwrap();

        if parent as usize >= st.inodes.len() {
            return reply.error(libc::ENOENT);
        }

        let ino = st.alloc_inode();

        let mut inode = Inode::empty();
        inode.mode = (mode | 0o100000) as u16;
        inode.size = 0;

        st.inodes[ino as usize] = inode;
        st.persist_inode(ino);

        let entry = DirEntry::new(ino, name.to_str().unwrap(), false);
        write_dir_entry(&mut st, parent, entry);

        let attr = BWFS::getattr_inode(ino, &st.inodes[ino as usize]);
        reply.entry(&TTL, &attr, 0);
    }

    // Crea un nuevo directorio
    fn mkdir(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        let mut st = self.state.lock().unwrap();
        let ino = st.alloc_inode();

        let mut inode = Inode::empty();
        inode.mode = (mode | 0o040000) as u16;
        st.inodes[ino as usize] = inode;
        st.persist_inode(ino);

        let entry = DirEntry::new(ino, name.to_str().unwrap(), true);
        write_dir_entry(&mut st, parent, entry);
        reply.entry(&TTL, &BWFS::getattr_inode(ino, &inode), 0);
    }

    // Lee el contenido de un directorio
    fn readdir(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        println!("readdir(ino = {}, offset = {})", ino, offset);

        let mut st = self.state.lock().unwrap();

        if ino as usize >= st.inodes.len() {
            return reply.error(libc::ENOENT);
        }

        let inode = &st.inodes[ino as usize];

        if inode.mode & 0o040000 == 0 {
            return reply.error(libc::ENOTDIR);
        }

        // Emite "."
        if offset == 0 {
            if reply.add(ino, 1, FileType::Directory, ".") {
                return;
            }
        }

        // Emite ".."
        if offset <= 1 {
            let parent = if ino == 1 { 1 } else { 1 };
            if reply.add(parent, 2, FileType::Directory, "..") {
                return;
            }
        }

        // Carga el bloque del directorio
        let blk = inode.direct[0];
        if blk == 0 {
            return reply.ok();
        }

        let block_off = block_offset(&st.superblock, blk);
        let mut buf = vec![0u8; st.superblock.block_size as usize];

        st.file.seek(SeekFrom::Start(block_off)).unwrap();
        st.file.read_exact(&mut buf).unwrap();

        let entry_size = std::mem::size_of::<DirEntry>();

        let mut idx = 2; // después de "." y ".."

        for chunk in buf.chunks_exact(entry_size) {
            let d: DirEntry = unsafe { std::ptr::read(chunk.as_ptr() as *const _) };

            if d.inode == 0 {
                break; // detiene en la primera ranura libre
            }

            let name = std::str::from_utf8(&d.name[..d.name_len as usize]).unwrap();
            let child = &st.inodes[d.inode as usize];

            let ftyp = if child.mode & 0o040000 != 0 {
                FileType::Directory
            } else {
                FileType::RegularFile
            };

            if idx >= offset {
                if reply.add(d.inode, idx as i64 + 1, ftyp, name) {
                    return;
                }
            }

            idx += 1;
        }

        reply.ok();
    }

    // Crea y abre un archivo
    fn create(
        &mut self,
        _req: &Request<'_>,
        parent: u64,
        name: &OsStr,
        mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        let mut st = self.state.lock().unwrap();
        let ino = st.alloc_inode();

        let mut inode = Inode::empty();
        inode.mode = (mode | 0o100000) as u16;
        st.inodes[ino as usize] = inode;
        st.persist_inode(ino);

        let entry = DirEntry::new(ino, name.to_str().unwrap(), false);
        write_dir_entry(&mut st, parent, entry);
        reply.created(&TTL, &BWFS::getattr_inode(ino, &inode), 0, 0, 0);
    }

    // Lee datos de un archivo
    fn read(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyData,
    ) {
        let mut st = self.state.lock().unwrap();

        let inode = &st.inodes[ino as usize];
        let direct_blocks = inode.direct; // copia la lista de bloques para evitar conflicto de préstamo

        let block_size = st.superblock.block_size as usize;
        let mut buf = vec![0u8; size as usize];

        let mut remaining = size as usize;
        let mut global_off = offset as usize;
        let mut copied = 0usize;

        // Itera usando la lista copiada de bloques
        for (block_i, blk) in direct_blocks.iter().enumerate() {
            if *blk == 0 {
                continue;
            }

            let block_start = block_i * block_size;
            let block_end = block_start + block_size;

            if global_off >= block_end {
                continue;
            }

            let blk_off = global_off.saturating_sub(block_start);

            let disk_offset =
                st.superblock.data_area_start + (*blk as u64) * st.superblock.block_size;
            st.file.seek(SeekFrom::Start(disk_offset)).unwrap();

            let mut block = vec![0u8; block_size];
            st.file.read_exact(&mut block).unwrap();

            let available = block_size - blk_off;
            let take = available.min(remaining);

            buf[copied..copied + take].copy_from_slice(&block[blk_off..blk_off + take]);

            copied += take;
            remaining -= take;
            global_off += take;

            if remaining == 0 {
                break;
            }
        }

        reply.data(&buf[..copied]);
    }

    // Escribe datos en un archivo
    fn write(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyWrite,
    ) {
        let mut st = self.state.lock().unwrap();
        let block_size = st.superblock.block_size as u64;

        let mut written = 0usize;
        let mut off = offset as u64;

        while written < data.len() {
            let blk_index = (off / block_size) as usize;
            if blk_index >= 12 {
                break;
            }

            if st.inodes[ino as usize].direct[blk_index] == 0 {
                st.inodes[ino as usize].direct[blk_index] = st.alloc_block();
            }

            let blk = st.inodes[ino as usize].direct[blk_index];
            let blk_offset = st.superblock.data_area_start + blk * block_size;

            let inside = (off % block_size) as usize;
            let space = block_size as usize - inside;
            let chunk = space.min(data.len() - written);

            st.file
                .seek(SeekFrom::Start(blk_offset + inside as u64))
                .unwrap();
            st.file.write_all(&data[written..written + chunk]).unwrap();

            off += chunk as u64;
            written += chunk;
        }

        let inode = &mut st.inodes[ino as usize];
        inode.size = inode.size.max(offset as u64 + data.len() as u64);
        st.persist_inode(ino);

        reply.written(written as u32);
    }

    // Elimina un archivo del sistema de archivos
    fn unlink(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEmpty) {
        let mut st = self.state.lock().unwrap();
        let ino = remove_dir_entry(&mut st, parent, name.to_str().unwrap());
        let inode = &st.inodes[ino as usize];

        for b in inode.direct {
            if b != 0 {
                st.free_block(b);
            }
        }

        st.free_inode(ino);
        reply.ok();
    }

    // Retorna estadísticas del sistema de archivos
    fn statfs(&mut self, _req: &Request, _ino: u64, reply: ReplyStatfs) {
        let st = self.state.lock().unwrap();

        let free_blocks: u64 = st.block_bitmap.iter().map(|b| b.count_zeros() as u64).sum();
        let free_inodes: u64 = st.inode_bitmap.iter().map(|b| b.count_zeros() as u64).sum();

        reply.statfs(
            st.superblock.total_blocks,
            free_blocks,
            free_blocks,
            st.superblock.inode_count,
            free_inodes,
            st.superblock.block_size as u32,
            255,
            0,
        );
    }

    // Renombra o mueve un archivo o directorio
    fn rename(
        &mut self,
        _req: &Request<'_>,
        p1: u64,
        name: &OsStr,
        p2: u64,
        new: &OsStr,
        _flags: u32,
        reply: ReplyEmpty,
    ) {
        let mut st = self.state.lock().unwrap();
        let ino = remove_dir_entry(&mut st, p1, name.to_str().unwrap());
        write_dir_entry(
            &mut st,
            p2,
            DirEntry::new(ino, new.to_str().unwrap(), false),
        );
        reply.ok();
    }

    // Busca un archivo o directorio por nombre en el directorio padre
    fn lookup(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEntry) {
        println!("lookup(parent = {}, name = {:?})", parent, name);

        let mut st = self.state.lock().unwrap();
        let name_bytes = name.as_bytes();
        let entry_size = std::mem::size_of::<DirEntry>();

        // Valida el inodo padre
        if parent as usize >= st.inodes.len() {
            return reply.error(libc::ENOENT);
        }
        let parent_inode = &st.inodes[parent as usize];

        // "." → el padre mismo
        if name_bytes == b"." {
            let attr = BWFS::getattr_inode(parent, parent_inode);
            return reply.entry(&TTL, &attr, 0);
        }

        // ".." → padre de raíz es raíz
        if name_bytes == b".." && parent == 1 {
            let inode = &st.inodes[1];
            let attr = BWFS::getattr_inode(1, inode);
            return reply.entry(&TTL, &attr, 0);
        }

        // Debe ser directorio
        if parent_inode.mode & 0o040000 == 0 {
            return reply.error(libc::ENOTDIR);
        }

        // Carga el bloque del directorio
        let blk = parent_inode.direct[0];
        if blk == 0 {
            return reply.error(libc::ENOENT);
        }

        let block_off = block_offset(&st.superblock, blk);
        let mut buf = vec![0u8; st.superblock.block_size as usize];
        st.file.seek(SeekFrom::Start(block_off)).unwrap();
        st.file.read_exact(&mut buf).unwrap();

        // Escanea entradas reales del directorio y detiene en la primera entrada libre
        for chunk in buf.chunks_exact(entry_size) {
            let d: DirEntry = unsafe { std::ptr::read(chunk.as_ptr() as *const _) };

            if d.inode == 0 {
                break; // resto del bloque es relleno
            }

            let dname = &d.name[..d.name_len as usize];

            if dname == name_bytes {
                let ino = d.inode;
                let inode = &st.inodes[ino as usize];
                let attr = BWFS::getattr_inode(ino, inode);
                return reply.entry(&TTL, &attr, 0);
            }
        }

        reply.error(libc::ENOENT);
    }

    // Verifica permisos de acceso a un archivo
    fn access(&mut self, _req: &Request<'_>, _: u64, _: i32, reply: ReplyEmpty) {
        reply.ok()
    }
    // Vacía el buffer de escritura de un archivo
    fn flush(&mut self, _req: &Request<'_>, _: u64, _: u64, _: u64, reply: ReplyEmpty) {
        reply.ok()
    }
    // Sincroniza los datos del archivo con el disco
    fn fsync(&mut self, _req: &Request<'_>, _: u64, _: u64, _: bool, reply: ReplyEmpty) {
        reply.ok()
    }
    // Cambia la posición de lectura/escritura en un archivo
    fn lseek(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        _fh: u64,
        offset: i64,
        _whence: i32,
        reply: ReplyLseek,
    ) {
        reply.offset(offset);
    }
    // Abre un archivo para lectura o escritura
    fn open(&mut self, _req: &Request<'_>, _: u64, _: i32, reply: ReplyOpen) {
        reply.opened(0, 0)
    }
}
