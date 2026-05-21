//! SCSI and USB MSC constants used by the transparent command set.
//!
//! This file intentionally keeps wire values close to their symbolic names. The
//! command handlers build response buffers by hand, so documenting constants at
//! the source helps reviewers map magic bytes back to the USB MSC/SCSI specs.
//!
//! Only the subset needed for read-only boot media is currently consumed by the
//! dispatcher. Additional enum variants are kept as references for future command
//! support and for clearer INQUIRY response construction.

#[derive(Debug, Clone, Copy)]
/// Status byte placed in the USB MSC Command Status Wrapper.
pub enum ScsiStatus {
    /// Command completed successfully.
    ScsiSuccess = 0,
    /// Command failed; host may request sense data.
    ScsiFail = 1,
}

/// REQUEST SENSE response-code values.
pub enum ScsiSenseRespCode {
    /// Error information describes the current command.
    Current = 0x70,
    /// Error information describes a deferred condition.
    Deferred = 0x71,
}

#[derive(Debug, Clone, Copy)]
/// Common SCSI sense-key values.
pub enum ScsiErrorType {
    /// No specific sense information.
    ScsiGood = 0x00,
    /// Device is not ready to complete the command.
    ScsiNotReady = 0x02,
    /// Medium read/write problem.
    ScsiMediumError = 0x03,
    /// Unsupported or malformed command.
    ScsiInvalidCommand = 0x05,
}

/// Peripheral Device Type
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PeripheralType {
    /// Block-addressable disk-like device.
    DirectAccess = 0x00,      // Direct-access disk device.
    /// Tape-like sequential device.
    SequentialAccess = 0x01,   // Sequential-access tape-like device.
    Printer = 0x02,
    Processor = 0x03,
    /// Medium that can be written once.
    WriteOnce = 0x04,          // Write-once device.
    CdRom = 0x05,
    Scanner = 0x06,
    OpticalMemory = 0x07,
    MediumChanger = 0x08,
    Communications = 0x09,
    StorageArray = 0x0C,
    EnclosureServices = 0x0D,
    SimplifiedDirectAccess = 0x0E,
    OpticalCardReader = 0x0F,
    BridgeController = 0x10,
    ObjectStorage = 0x11,
    AutomationDrive = 0x12,
    SecurityManager = 0x13,
    Unknown = 0x1F,
}

/// Medium type flags
#[derive(Debug, Clone, Copy)]
pub struct MediumFlags {
    // Reserved placeholder for future MODE SENSE support. The current firmware
    // advertises write protection with a compact fixed response instead.
    // pub removable: bool,
    // pub medium_present: bool,
}

/// SCSI version
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScsiVersion {
    /// No standards claim.
    NoStandard = 0x00,
    /// Original SCSI standard.
    SCSI1 = 0x01,
    /// SCSI-2, enough for the simple USB MSC boot profile.
    SCSI2 = 0x02,
    SPC = 0x03,      // SCSI Primary Commands
    SPC2 = 0x04,
    SPC3 = 0x05,
    SPC4 = 0x06,
    SPC5 = 0x07,
}

/// Response data format
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScsiResponseFormat {
    /// Standard INQUIRY data formatted as SPC-2.
    SPC2 = 0x02,    // SCSI Primary Commands-2 format.
    SPC3 = 0x03,
    SPC4 = 0x04,
}

// ============= MSC consts =============
/// Little-endian `USBC` signature expected at the start of every CBW.
pub const CBW_SIGNATURE: u32 = 0x4342_5355; // "USBC"
/// Little-endian `USBS` signature placed at the start of every CSW.
pub const CSW_SIGNATURE: u32 = 0x5342_5355; // "USBS"
/// 8-byte vendor field returned by SCSI INQUIRY.
pub const MSC_VENDOR_NAME: &[u8] = b"RustBMC ";
/// 16-byte product field returned by SCSI INQUIRY.
pub const MSC_PRODUCT_NAME: &[u8] = b"STM32 VirtualUSB";
/// 4-byte product revision returned by SCSI INQUIRY.
pub const MSC_PRODUCT_REVISION: &[u8] = b"1.0 ";
/// Logical block size exposed to the USB host.
pub const SECTOR_SIZE: u32 = 512;
/// Advertised virtual disk capacity.
pub const DISK_SIZE: u32 = 256 * 1024 * 1024; // 256MB USB Disk
/// Number of 512-byte sectors in the virtual disk.
pub const SECTOR_COUNT: u32 = DISK_SIZE / SECTOR_SIZE;

// ============= SCSI cmd code =============
/// Probe whether the logical unit is ready.
pub const SCSI_TEST_UNIT_READY: u8 = 0x00;
/// Ask for sense data after a failed command.
pub const SCSI_REQUEST_SENSE: u8 = 0x03;
/// Ask for vendor/product/device identity.
pub const SCSI_INQUIRY: u8 = 0x12;
/// Ask for mode parameters; used here to advertise write protection.
pub const SCSI_MODE_SENSE_6: u8 = 0x1A;
/// Ask for supported medium capacities.
pub const SCSI_READ_FORMAT_CAPACITIES: u8 = 0x23;
/// Ask for last LBA and block size.
pub const SCSI_READ_CAPACITY_10: u8 = 0x25;
/// Read a run of logical blocks.
pub const SCSI_READ_10: u8 = 0x28;
/// Write command opcode retained for future mutable-media support.
pub const SCSI_WRITE_10: u8 = 0x2A;
