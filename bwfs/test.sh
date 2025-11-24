#!/bin/bash
# Script de prueba para BWFS

set -e

echo "=========================================="
echo "BWFS Test Script"
echo "=========================================="
echo ""

# Variables
CONFIG="config.ini"
DATA_DIR="/tmp/bwfs_data"
MOUNTPOINT="/tmp/bwfs_mount"

# Cleanup anterior
echo "[1/6] Limpiando datos anteriores..."
rm -rf "$DATA_DIR" "$MOUNTPOINT"
mkdir -p "$DATA_DIR" "$MOUNTPOINT"
echo "✓ Directorio limpio"
echo ""

# Compilar
echo "[2/6] Compilando BWFS..."
cargo build --release
echo "✓ Compilación exitosa"
echo ""

# Crear filesystem
echo "[3/6] Creando filesystem..."
./target/release/mkfs_bwfs -c "$CONFIG"
echo "✓ Filesystem creado"
echo ""

# Inspeccionar con bwfs-info
echo "[4/6] Inspeccionando filesystem..."
./target/release/bwfs_info "$DATA_DIR/bwfs_image.img"
echo ""

# Montar
echo "[5/6] Montando filesystem..."
echo "Ejecutando: mount_bwfs -c $CONFIG $MOUNTPOINT"
echo "Presiona Ctrl+C para desmontar"
echo ""

# Montar en foreground
./target/release/mount_bwfs -c "$CONFIG" -f "$MOUNTPOINT"

# Esta línea solo se ejecuta después de desmontar
echo ""
echo "[6/6] Filesystem desmontado"
echo ""
echo "=========================================="
echo "Prueba completada"
echo "=========================================="