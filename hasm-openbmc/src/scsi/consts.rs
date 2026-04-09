#[derive(Debug, Clone, Copy)]
pub enum ScsiStatus {
    ScsiSuccess = 0,
    ScsiFail = 1,
}

pub enum ScsiSenseRespCode {
    Current = 0x70,
    Deferred = 0x71,
}

#[derive(Debug, Clone, Copy)]
pub enum ScsiErrorType {
    ScsiGood = 0x00,
    ScsiNotReady = 0x02,
    ScsiMediumError = 0x03,
    ScsiInvalidCommand = 0x05,
}

/// Peripheral Device Type
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PeripheralType {
    DirectAccess = 0x00,      // 直接访问设备（磁盘）
    SequentialAccess = 0x01,   // 顺序访问（磁带）
    Printer = 0x02,
    Processor = 0x03,
    WriteOnce = 0x04,          // 一次性写入
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
    // pub removable: bool,
    // pub medium_present: bool,
}

/// SCSI version
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScsiVersion {
    NoStandard = 0x00,
    SCSI1 = 0x01,
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
    SPC2 = 0x02,    // SCSI Primary Commands-2 格式
    SPC3 = 0x03,
    SPC4 = 0x04,
}

// ============= MSC consts =============
pub const CBW_SIGNATURE: u32 = 0x4342_5355; // "USBC"
pub const CSW_SIGNATURE: u32 = 0x5342_5355; // "USBS"
pub const MSC_VENDOR_NAME: &[u8] = b"RustBMC ";
pub const MSC_PRODUCT_NAME: &[u8] = b"STM32 VirtualUSB";
pub const MSC_PRODUCT_REVISION: &[u8] = b"1.0 ";
pub const SECTOR_SIZE: u32 = 512;
pub const SECTOR_COUNT: u32 = 64;
pub const DISK_SIZE: u32 = SECTOR_SIZE * SECTOR_COUNT; // 32KB USB Disk

// ============= SCSI cmd code =============
pub const SCSI_TEST_UNIT_READY: u8 = 0x00;
pub const SCSI_REQUEST_SENSE: u8 = 0x03;
pub const SCSI_INQUIRY: u8 = 0x12;
pub const SCSI_MODE_SENSE_6: u8 = 0x1A;
pub const SCSI_READ_FORMAT_CAPACITIES: u8 = 0x23;
pub const SCSI_READ_CAPACITY_10: u8 = 0x25;
pub const SCSI_READ_10: u8 = 0x28;
pub const SCSI_WRITE_10: u8 = 0x2A;