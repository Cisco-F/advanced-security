#!/bin/bash

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

prepare_env() {
  mkdir -p "$BUILD_DIR"
  make -C "$BASE_DIR"/BaseTools -j"$(nproc)"
  pushd "$WORKSPACE" &> /dev/null
  mkdir keys
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

build_firmware() {
  pushd "$BUILD_DIR" &> /dev/null
  source "$BASE_DIR"/edksetup.sh
  build -a AARCH64 -t GCC5 -b RELEASE -p "$FIRMWARE_DIR"/edk2-platforms/Platform/RaspberryPi/RPi4/RPi4.dsc --pcd gEfiMdeModulePkgTokenSpaceGuid.PcdFirmwareVendor=L"$PROJECT_URL" --pcd gEfiMdeModulePkgTokenSpaceGuid.PcdFirmwareVersionString=L"UEFI Firmware" $DEFAULT_KEYS $BUILD_FLAGS $TLS_DISABLE_FLAGS
  cp Build/RPi4/RELEASE_GCC5/FV/RPI_EFI.fd ../
  popd &> /dev/null
}

if [[ $# == 0 ]]; then
  if [[ ! -d $WORKSPACE/keys ]]; then
    prepare_env
  fi
  build_firmware
  exit 0
fi

case $1 in
  prepare)
    prepare_env
    ;;
  build)
    build_firmware
    ;;
  *)
    exit 1
    ;;
esac
