#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{
    bind_interrupts,
    peripherals,
    sdmmc::{self, DataBlock, Sdmmc},
    time::Hertz,
    usb::{self, Config as UsbConfig, Driver},
    
};
use embassy_usb::Builder;
use embassy_usb::Handler;
use embassy_usb::control::{InResponse, OutResponse, Recipient, Request, RequestType};
use {defmt_rtt as _, panic_probe as _};
use defmt_rtt as _;
use embassy_time::Timer;
use embassy_usb::driver::{EndpointIn, EndpointOut};
use panic_probe as _;

use hasm_openbmc::scsi::*;


static EP_OUT_BUFFER: static_cell::StaticCell<[u8; 256]> = static_cell::StaticCell::new();
static CONFIG_DESC: static_cell::StaticCell<[u8; 256]> = static_cell::StaticCell::new();
static BOS_DESC: static_cell::StaticCell<[u8; 256]> = static_cell::StaticCell::new();
static CTRL_BUF: static_cell::StaticCell<[u8; 64]> = static_cell::StaticCell::new();
static MSC_HANDLER: static_cell::StaticCell<MscHandler> = static_cell::StaticCell::new();
// 16 扇区 × 512 = 8192 字节；DataBlock 是 DMA 安全的 4 字节对齐块，放在 BSS 段避免栈溢出
static CACHE_BUF: static_cell::StaticCell<[DataBlock; 16]> = static_cell::StaticCell::new();

/// USB MSC class-specific control request handler.
/// Responds to GET_MAX_LUN (0xFE) and Bulk-Only Reset (0xFF).
/// The Raspberry Pi Boot ROM requires a valid GET_MAX_LUN reply or it
/// immediately abandons enumeration (green LED stuck on).
struct MscHandler {
    iface_num: u8,
}

impl Handler for MscHandler {
    fn control_in<'a>(&'a mut self, req: Request, buf: &'a mut [u8]) -> Option<InResponse<'a>> {
        if req.request_type == RequestType::Class
            && req.recipient == Recipient::Interface
            && req.request == 0xFE  // GET_MAX_LUN
            && req.index == self.iface_num as u16
        {
            info!("GET_MAX_LUN -> 0");
            buf[0] = 0x00; // one LUN (index 0)
            Some(InResponse::Accepted(&buf[..1]))
        } else {
            None
        }
    }

    fn control_out(&mut self, req: Request, _data: &[u8]) -> Option<OutResponse> {
        if req.request_type == RequestType::Class
            && req.recipient == Recipient::Interface
            && req.request == 0xFF  // Bulk-Only Mass Storage Reset
            && req.index == self.iface_num as u16
        {
            info!("BULK_ONLY_RESET");
            Some(OutResponse::Accepted)
        } else {
            None
        }
    }
}


/// TF 卡读取封装
///
/// 用法示例：
/// ```no_run
/// let mut tf = TfCard::new(sdmmc);
/// let mut buf = DataBlock([0u8; 512]);
/// tf.read(0, &mut buf).await.unwrap();
/// ```
pub struct TfCard<'d> {
    sdmmc: Sdmmc<'d, peripherals::SDIO>,
}

impl<'d> TfCard<'d> {
    pub fn new(sdmmc: Sdmmc<'d, peripherals::SDIO>) -> Self {
        Self { sdmmc }
    }

    /// 读取指定扇区（512 字节）到 buf
    ///
    /// # 参数
    /// - `sector`: 扇区编号，从 0 开始
    /// - `buf`: 目标缓冲区，大小固定为 512 字节
    pub async fn read(
        &mut self,
        sector: u32,
        buf: &mut DataBlock,
    ) -> Result<(), sdmmc::Error> {
        self.sdmmc.read_block(sector, buf).await
    }

    /// 读取指定扇区到用户提供的 `[u8; 512]` 切片
    ///
    /// 内部使用 `DataBlock`，读取完成后将数据复制到 `out`。
    pub async fn read_into(
        &mut self,
        sector: u32,
        out: &mut [u8; 512],
    ) -> Result<(), sdmmc::Error> {
        let mut block = DataBlock([0u8; 512]);
        self.sdmmc.read_block(sector, &mut block).await?;
        out.copy_from_slice(&block.0);
        Ok(())
    }
}

bind_interrupts!(struct Irqs {
    SDIO => sdmmc::InterruptHandler<peripherals::SDIO>;
});

bind_interrupts!(struct UsbIrqs {
    OTG_FS => usb::InterruptHandler<peripherals::USB_OTG_FS>;
});

#[embassy_executor::task]
async fn usb_task(mut usb: embassy_usb::UsbDevice<'static, Driver<'static, peripherals::USB_OTG_FS>>) {
    usb.run().await;
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_stm32::init(hasm_openbmc::clk_init());

    info!("TF Card Init...");

    let sdmmc = Sdmmc::new_4bit(
        p.SDIO,
        Irqs,
        p.DMA2_CH3,
        p.PC12, // CK
        p.PD2,  // CMD
        p.PC8,  // D0
        p.PC9,  // D1
        p.PC10, // D2
        p.PC11, // D3
        Default::default(),
    );

    // 将 SDIO 包装为 TfCard
    let mut tf = TfCard::new(sdmmc);

    // 初始化 TF 卡（400 kHz 识别频率）
    // 400 kHz 仅用于识别阶段，embassy-stm32 内部处理；此处为数据传输目标频率
    match tf.sdmmc.init_sd_card(Hertz(25_000_000)).await {
        Ok(_) => info!("TF Card init OK"),
        Err(e) => {
            error!("TF Card init failed: {:?}", e);
            return;
        }
    }

    // 打印卡信息
    if let Ok(card) = tf.sdmmc.card() {
        match card {
            sdmmc::SdmmcPeripheral::SdCard(c) => info!("SD Card detected, CSD version: {}", c.csd.version()),
            sdmmc::SdmmcPeripheral::Emmc(_) => info!("eMMC detected"),
        }
    }

    // USB 配置
    let ep_out_buffer = EP_OUT_BUFFER.init([0; 256]);
    let mut usb_cfg = UsbConfig::default();
    usb_cfg.vbus_detection = false;
    let driver = Driver::new_fs(
        p.USB_OTG_FS, 
        UsbIrqs,
        p.PA12, 
        p.PA11, 
        ep_out_buffer, 
        usb_cfg
    );

    let config_desc = CONFIG_DESC.init([0; 256]);
    let bos_desc = BOS_DESC.init([0; 256]);
    let ctrl_buf = CTRL_BUF.init([0; 64]);

    let mut cfg = embassy_usb::Config::new(0xc0de, 0xcafe);
    cfg.manufacturer = Some("MyBMC");
    cfg.product = Some("STM32F407 USB MSC");
    cfg.serial_number = Some("F407-MSC-001");
    cfg.max_power = 100;
    cfg.max_packet_size_0 = 64;

    let mut builder = Builder::new(driver, cfg, config_desc, bos_desc, &mut [], ctrl_buf);

    // MSC interface descriptors: Class=0x08, Subclass=0x06, Protocol=0x50
    let mut function = builder.function(0x08, 0x06, 0x50);
    let mut interface = function.interface();
    let iface_num = interface.interface_number().0;
    let mut alt_setting = interface.alt_setting(0x08, 0x06, 0x50, None);
    let mut ep_out = alt_setting.endpoint_bulk_out(None, 64);
    let mut ep_in = alt_setting.endpoint_bulk_in(None, 64);
    drop(function);

    let msc_handler = MSC_HANDLER.init(MscHandler { iface_num });
    builder.handler(msc_handler);

    let usb = builder.build();
    unwrap!(spawner.spawn(usb_task(usb)));

    info!("✓ USB MSC device ready!");

    let mut cbw_buf = [0u8; 31];

    // SCSI 元数据响应缓冲（INQUIRY/SENSE 等最大 36 字节，64 字节绰绰有余）
    let mut meta_buf = [0u8; 64];

    // READ_10 扇区缓存：16 × 512 = 8192 字节，存在 BSS 段
    const CACHE_SECS: usize = 16;
    let cache_blocks: &mut [DataBlock; CACHE_SECS] =
        CACHE_BUF.init(core::array::from_fn(|_| DataBlock([0u8; 512])));

    loop {
        let n = match ep_out.read(&mut cbw_buf).await {
            Ok(n) => n,
            Err(e) => {
                warn!("MSC OUT read error: {:?}", e);
                Timer::after_millis(10).await;
                continue;
            }
        };

        if n < 31 {
            warn!("Short CBW: {}", n);
            continue;
        }

        let sig = u32::from_le_bytes([cbw_buf[0], cbw_buf[1], cbw_buf[2], cbw_buf[3]]);
        if sig != CBW_SIGNATURE {
            warn!("Bad CBW signature: 0x{:08x}", sig);
            continue;
        }

        let tag = u32::from_le_bytes([cbw_buf[4], cbw_buf[5], cbw_buf[6], cbw_buf[7]]);
        let dtl = u32::from_le_bytes([cbw_buf[8], cbw_buf[9], cbw_buf[10], cbw_buf[11]]);
        let flags = cbw_buf[12];
        let cb_len = core::cmp::min(cbw_buf[14] as usize, 16);
        let cmd = &cbw_buf[15..15 + cb_len];
        let mut response = handle_scsi_cmd(cmd, &mut meta_buf);

        if (flags & 0x80) != 0 && dtl > 0 && response.resp_len > 0 {
            let send_len = core::cmp::min(response.resp_len, dtl as usize);
            let mut offset = 0;
            let mut write_ok = true;

            // 从 CBW 中提取起始 LBA
            let start_lba = u32::from_be_bytes([cmd[2], cmd[3], cmd[4], cmd[5]]);

            let mut cache_start_lba: u32 = u32::MAX;
            let mut cache_valid_secs: usize = 0;

            while offset < send_len {
                let chunk_size = core::cmp::min(send_len - offset, 64);
                let chunk_data: &[u8];

                if cmd[0] == SCSI_READ_10 {
                    let sector_off  = (offset as u32) / SECTOR_SIZE;
                    let byte_in_sec = (offset as u32 % SECTOR_SIZE) as usize;
                    let target_lba  = start_lba + sector_off;

                    // 缓存缺失 → CMD18 multi-block read，一次 DMA 传输 CACHE_SECS 个扇区
                    if target_lba < cache_start_lba || target_lba >= cache_start_lba + cache_valid_secs as u32 {
                        let total_secs = ((send_len as u32) + SECTOR_SIZE - 1) / SECTOR_SIZE;
                        let remaining  = total_secs - sector_off;
                        let to_read    = core::cmp::min(remaining as usize, CACHE_SECS);
                        cache_start_lba  = target_lba;
                        cache_valid_secs = 0;

                        match tf.sdmmc.read_blocks(target_lba, &mut cache_blocks[..to_read]).await {
                            Ok(_) => { cache_valid_secs = to_read; }
                            Err(e) => {
                                error!("TF read_blocks LBA {}: {:?}", target_lba, e);
                                response.status = ScsiStatus::ScsiFail;
                                write_ok = false;
                            }
                        }
                        if !write_ok { break; }
                    }

                    let block_idx = (target_lba - cache_start_lba) as usize;
                    chunk_data = &cache_blocks[block_idx].0[byte_in_sec..byte_in_sec + chunk_size];
                } else {
                    chunk_data = &meta_buf[offset..offset + chunk_size];
                }

                if let Err(e) = ep_in.write(chunk_data).await {
                    warn!("MSC IN write chunk error: {:?}", e);
                    response.residue = dtl.saturating_sub(offset as u32);
                    write_ok = false;
                    break;
                }
                offset += chunk_size;
            }

            if write_ok && (send_len as u32) < dtl {
                response.residue = dtl.saturating_sub(send_len as u32);
            }
                
        } else if dtl > 0 {
            // 主机想往 U盘【写入】数据阶段！
            // 重要：即使我们是虚拟空白盘，也必须把主机发来的数据“抽干”，否则会堵死端点触发 BufferOverflow！
            let mut bytes_read = 0;
            let mut dump_buf = [0u8; 64]; // 数据黑洞（垃圾桶）
            while bytes_read < dtl {
                // 不断从 OUT 端点读取数据，然后直接覆盖丢弃，直到把 dtl 数量的数据全抽干
                match ep_out.read(&mut dump_buf).await {
                    Ok(n) => {
                        bytes_read += n as u32;
                    }
                    Err(e) => {
                        warn!("MSC OUT drain error: {:?}", e);
                        break;
                    }
                }
            }

            if cmd[0] == SCSI_WRITE_10 {
                warn!("write protected: command denied");
                response.status = ScsiStatus::ScsiFail; // 告诉主机：动作失败！
            }
            
            // 告诉状态机，我们已经“妥善处理”（实际是扔了）这部分数据
            response.residue = dtl.saturating_sub(bytes_read);
        }

        let mut csw = [0u8; 13];
        csw[0..4].copy_from_slice(&CSW_SIGNATURE.to_le_bytes());
        csw[4..8].copy_from_slice(&tag.to_le_bytes());
        csw[8..12].copy_from_slice(&response.residue.to_le_bytes());
        csw[12] = response.status as u8;

        if let Err(e) = ep_in.write(&csw).await {
            warn!("MSC CSW write error: {:?}", e);
        }
    }
}