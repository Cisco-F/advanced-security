#!/bin/bash

# Build helper for Raspberry Pi 4 UEFI firmware.
#
# HASM-OpenBMC can boot the controlled Raspberry Pi from a USB image. This script
# prepares a UEFI firmware payload suitable for that image, including Secure Boot
# defaults and network boot features used during advanced-security experiments.
#
# Expected environment:
# - FIRMWARE_DIR points at the directory containing edk2, edk2-platforms, and
#   edk2-non-osi.
# - BUILD_DIR points at the shared build output root.
# - aarch64-linux-gnu-* tools are available in PATH.
#
# The script has two phases:
# - `prepare` builds EDK2 BaseTools and downloads certificate material;
# - `build` runs the RPi4 DSC build and copies RPI_EFI.fd to the output root.
#
# With no argument it prepares only when keys are missing, then builds.

set -e
BASE_DIR="${FIRMWARE_DIR}/edk2"
BUILD_DIR="${BUILD_DIR}/firmware"
PROJECT_URL=https://github.com/pftf/RPi4

export WORKSPACE=${BUILD_DIR}
export PACKAGES_PATH="${FIRMWARE_DIR}/edk2:${FIRMWARE_DIR}/edk2-platforms:${FIRMWARE_DIR}/edk2-non-osi"
export BUILD_FLAGS="-D SECURE_BOOT_ENABLE=TRUE -D INCLUDE_TFTP_COMMAND=TRUE -D NETWORK_ISCSI_ENABLE=TRUE -D SMC_PCI_SUPPORT=1"
export TLS_DISABLE_FLAGS="-D NETWORK_TLS_ENABLE=FALSE -D NETWORK_ALLOW_HTTP_CONNECTIONS=TRUE"
export GCC5_AARCH64_PREFIX="aarch64-linux-gnu-"
export DEFAULT_KEYS="-D DEFAULT_KEYS=TRUE -D PK_DEFAULT_FILE=$WORKSPACE/keys/pk.cer -D KEK_DEFAULT_FILE1=$WORKSPACE/keys/ms_kek1.cer -D KEK_DEFAULT_FILE2=$WORKSPACE/keys/ms_kek2.cer -D DB_DEFAULT_FILE1=$WORKSPACE/keys/ms_db1.cer -D DB_DEFAULT_FILE2=$WORKSPACE/keys/ms_db2.cer -D DB_DEFAULT_FILE3=$WORKSPACE/keys/ms_db3.cer -D DB_DEFAULT_FILE4=$WORKSPACE/keys/ms_db4.cer -D DBX_DEFAULT_FILE1=$WORKSPACE/keys/arm64_dbx.bin"

# Feature flag intent:
# - Secure Boot is enabled so the image can exercise signed-boot paths.
# - TFTP and iSCSI commands are included for network-boot experiments.
# - TLS is disabled only because the isolated lab flow may use local HTTP
#   services; it should be reconsidered before moving outside the bench setup.
# - DEFAULT_KEYS points EDK2 at deterministic certificate inputs generated or
#   downloaded during `prepare_env`.

# Prepare EDK2 tooling and Secure Boot key inputs.
prepare_env() {
  mkdir -p "$BUILD_DIR"
  # BaseTools are host-side utilities required by the EDK2 build command.
  make -C "$BASE_DIR"/BaseTools -j"$(nproc)"
  pushd "$WORKSPACE" &> /dev/null
  mkdir keys
  # Generate a local Platform Key and download Microsoft/UEFI public databases
  # so the firmware can start with a realistic Secure Boot variable set.
  openssl req -new -x509 -newkey rsa:2048 -subj "/CN=Raspberry Pi Platform Key/" -keyout /dev/null -outform DER -out keys/pk.cer -days 7300 -nodes -sha256
  curl -L https://go.microsoft.com/fwlink/?LinkId=321185 -o keys/ms_kek1.cer
  curl -L https://go.microsoft.com/fwlink/?linkid=2239775 -o keys/ms_kek2.cer
  curl -L https://go.microsoft.com/fwlink/?linkid=321192 -o keys/ms_db1.cer
  curl -L https://go.microsoft.com/fwlink/?linkid=321194 -o keys/ms_db2.cer
  curl -L https://go.microsoft.com/fwlink/?linkid=2239776 -o keys/ms_db3.cer
  curl -L https://go.microsoft.com/fwlink/?linkid=2239872 -o keys/ms_db4.cer
  curl -L https://uefi.org/sites/default/files/resources/dbxupdate_arm64.bin -o keys/arm64_dbx.bin
  popd &> /dev/null
}

# Build the RPi4 UEFI firmware with the selected feature flags.
build_firmware() {
  pushd "$BUILD_DIR" &> /dev/null
  # edksetup.sh exports build-system variables used by the `build` command.
  source "$BASE_DIR"/edksetup.sh
  # HTTP is allowed here because the lab boot flow may fetch artifacts from a
  # controlled network service. Do not reuse these flags for production firmware
  # without revisiting the security model.
  build -a AARCH64 -t GCC5 -b RELEASE -p "$FIRMWARE_DIR"/edk2-platforms/Platform/RaspberryPi/RPi4/RPi4.dsc --pcd gEfiMdeModulePkgTokenSpaceGuid.PcdFirmwareVendor=L"$PROJECT_URL" --pcd gEfiMdeModulePkgTokenSpaceGuid.PcdFirmwareVersionString=L"UEFI Firmware" $DEFAULT_KEYS $BUILD_FLAGS $TLS_DISABLE_FLAGS
  cp Build/RPi4/RELEASE_GCC5/FV/RPI_EFI.fd ../
  popd &> /dev/null
}

if [[ $# == 0 ]]; then
  # Default path is idempotent enough for day-to-day rebuilds: keep existing keys
  # and only rebuild the firmware payload.
  if [[ ! -d $WORKSPACE/keys ]]; then
    prepare_env
  fi
  build_firmware
  exit 0
fi

case $1 in
  prepare)
    # Explicit environment/key preparation.
    prepare_env
    ;;
  build)
    # Explicit firmware rebuild using already-prepared tools and keys.
    build_firmware
    ;;
  *)
    exit 1
    ;;
esac
