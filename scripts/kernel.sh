#!/bin/bash

# Build helper for the Raspberry Pi kernel used by the BMC boot image.
#
# The STM32 BMC serves the Pi a disk image over USB MSC. This script builds the
# ARM64 kernel artifact that can be placed into that image as `bootaa64.efi`,
# matching the UEFI boot flow prepared by `firmware.sh`.
#
# Expected environment:
# - ROOT points at this repository root;
# - KERNEL_DIR points at a Linux kernel source tree;
# - BUILD_DIR points at the shared build output root.
#
# The checked-in kernel.config is used as KCONFIG_CONFIG so rebuilds are
# reproducible across machines.

set -e

BUILD_DIR=${BUILD_DIR}/kernel
KCONFIG=${ROOT}/scripts/configs/kernel.config
BUILD_FLAGS="-C ${KERNEL_DIR} LLVM=1 ARCH=arm64 O=${BUILD_DIR} KCONFIG_CONFIG=${KCONFIG}"

# Why LLVM=1:
# The ARM64 kernel tree supports Clang/LLVM builds well, and using one toolchain
# family keeps host setup simpler for reproducible lab images.
#
# Why O=${BUILD_DIR}:
# Out-of-tree builds keep downloaded or vendor kernel sources clean. The build
# directory can be removed and regenerated without touching KERNEL_DIR.
#
# Why copy to bootaa64.efi:
# Raspberry Pi UEFI looks for the standard ARM64 EFI boot filename on the boot
# volume. The kernel's EFI-stub image can satisfy that role directly.

# Open menuconfig against the repository-owned config file.
config() {
  make ${BUILD_FLAGS} menuconfig
}

# Build the ARM64 EFI-stub kernel and copy it to the boot filename expected by
# the Raspberry Pi UEFI flow.
build() {
  make ${BUILD_FLAGS} -j$(nproc)
  cp ${BUILD_DIR}/arch/arm64/boot/vmlinuz.efi ${BUILD_DIR}/../bootaa64.efi
}

# Install modules into a caller-provided rootfs staging directory.
install_modules() {
  make ${BUILD_FLAGS} INSTALL_MOD_PATH="$1" modules_install -j$(nproc)
}

# Keep the interface tiny because this script is usually called from a larger
# image-build pipeline.
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
