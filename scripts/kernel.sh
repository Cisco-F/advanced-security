#!/bin/bash

set -e

BUILD_DIR=${BUILD_DIR}/kernel
KCONFIG=${ROOT}/scripts/configs/kernel.config
BUILD_FLAGS="-C ${KERNEL_DIR} LLVM=1 ARCH=arm64 O=${BUILD_DIR} KCONFIG_CONFIG=${KCONFIG}"

config() {
  make ${BUILD_FLAGS} menuconfig
}

build() {
  make ${BUILD_FLAGS} -j$(nproc)
  cp ${BUILD_DIR}/arch/arm64/boot/vmlinuz.efi ${BUILD_DIR}/../bootaa64.efi
}

install_modules() {
  make ${BUILD_FLAGS} INSTALL_MOD_PATH="$1" modules_install -j$(nproc)
}

case $1 in
  "config")
    config
    ;;
  "build")
    build
    ;;
  "install")
    shift 1
    install_modules $1
    ;;
  *)
    exit 1
esac

