#![no_std]
#![no_main]

use defmt::*;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32::rcc::{
    AHBPrescaler, APBPrescaler, Hse, HseMode, Pll, PllMul, PllPDiv, PllPreDiv, PllQDiv, PllSource,
    Sysclk,
};
use embassy_stm32::time::Hertz;
use embassy_stm32::usb::{Config as UsbConfig, Driver};
use embassy_stm32::{bind_interrupts, peripherals, usb};
use embassy_time::Timer;
use embassy_usb::{Builder, UsbDevice};
use embassy_usb::driver::{EndpointIn, EndpointOut};
use panic_probe as _;

bind_interrupts!(struct Irqs {
    OTG_FS => usb::InterruptHandler<peripherals::USB_OTG_FS>;
});

macro_rules! make_static {
    ($t:ty,$val:expr) => {{
        static STATIC_CELL: static_cell::StaticCell<$t> = static_cell::StaticCell::new();
        STATIC_CELL.init($val)
    }};
}

// ============= SCSI 命令代码 =============
const SCSI_TEST_UNIT_READY: u8 = 0x00;
const SCSI_REQUEST_SENSE: u8 = 0x03;
const SCSI_INQUIRY: u8 = 0x12;
const SCSI_MODE_SENSE_6: u8 = 0x1A;
const SCSI_READ_FORMAT_CAPACITIES: u8 = 0x23;
const SCSI_READ_CAPACITY_10: u8 = 0x25;
const SCSI_READ_10: u8 = 0x28;

// ============= MSC 常量 =============
const CBW_SIGNATURE: u32 = 0x4342_5355; // "USBC"
const CSW_SIGNATURE: u32 = 0x5342_5355; // "USBS"
const SECTOR_SIZE: u32 = 512;
const VIRTUAL_SECTOR_COUNT: u32 = 8192;

fn handle_scsi_command(
    cmd: &[u8],
    data_buf: &mut [u8],
) -> (u8, u32, usize) {
    if cmd.is_empty() { return (1, 0, 0); }
    match cmd[0] {
        SCSI_TEST_UNIT_READY => {
            (0, 0, 0)
        }
        SCSI_REQUEST_SENSE => {
            let mut sense = [0u8; 18];
            sense[0] = 0x70; sense[2] = 0x00; sense[7] = 10;
            let len = core::cmp::min(sense.len(), data_buf.len());
            data_buf[..len].copy_from_slice(&sense[..len]);
            (0, (sense.len() as u32).saturating_sub(len as u32), len)
        }
        SCSI_INQUIRY => {
            info!("→ INQUIRY");
            let mut resp = [0u8; 36];
            resp[0] = 0x00; resp[1] = 0x80; resp[2] = 0x02; resp[3] = 0x02; resp[4] = 31;
            resp[8..16].copy_from_slice(b"MyBMC   ");
            resp[16..32].copy_from_slice(b"STM32 VirtualUSB"); // 名字可以随便取
            resp[32..36].copy_from_slice(b"1.00");
            
            let len = core::cmp::min(resp.len(), data_buf.len());
            data_buf[..len].copy_from_slice(&resp[..len]);
            (0, (resp.len() as u32).saturating_sub(len as u32), len)
        }
         SCSI_MODE_SENSE_6 => {
            info!("→ MODE_SENSE_6 0x1A");
            let mut resp = [0u8; 4];
            resp[0] = 0x03; // 长度
            resp[1] = 0x00; // 介质类型标准
            resp[2] = 0x00; // 没开启写保护 (如果要伪装成只读光驱，这里改成 0x80)
            resp[3] = 0x00; // 块描述符长度
            
            let len = core::cmp::min(resp.len(), data_buf.len());
            data_buf[..len].copy_from_slice(&resp[..len]);
            (0, (resp.len() as u32).saturating_sub(len as u32), len)
        }
        SCSI_READ_FORMAT_CAPACITIES => {
            info!("→ READ_FORMAT_CAPACITIES 0x23");
            let mut resp = [0u8; 12];
            resp[3] = 0x08; // 列表长度是 8
            resp[4..8].copy_from_slice(&VIRTUAL_SECTOR_COUNT.to_be_bytes()); // 容量
            resp[8] = 0x02; // Formatted Media 表明设备已经可用
            
            let block_len = SECTOR_SIZE.to_be_bytes();
            resp[9] = block_len[1]; resp[10] = block_len[2]; resp[11] = block_len[3];
            
            let len = core::cmp::min(resp.len(), data_buf.len());
            data_buf[..len].copy_from_slice(&resp[..len]);
            (0, (resp.len() as u32).saturating_sub(len as u32), len)
        }
        SCSI_READ_CAPACITY_10 => {
            info!("→ READ_CAPACITY_10 0x25");
            let mut resp = [0u8; 8];
            let last_lba = VIRTUAL_SECTOR_COUNT - 1;
            resp[0..4].copy_from_slice(&last_lba.to_be_bytes());
            resp[4..8].copy_from_slice(&SECTOR_SIZE.to_be_bytes());
            
            let len = core::cmp::min(resp.len(), data_buf.len());
            data_buf[..len].copy_from_slice(&resp[..len]);
            (0, (resp.len() as u32).saturating_sub(len as u32), len)
        }
        SCSI_READ_10 => {
            let lba = u32::from_be_bytes([cmd[2], cmd[3], cmd[4], cmd[5]]);
            let num_blocks = u16::from_be_bytes([cmd[7], cmd[8]]) as u32;
            let total_bytes = (num_blocks * SECTOR_SIZE) as usize;
            
            info!("→ READ_10 LBA={}, Blocks={}", lba, num_blocks);
            
            // 注意：我们直接返回 total_bytes，不再受制于 data_buf 的大小！
            // 因为如果是 READ_10，我们会在 main 循环里特殊处理，不再读取 data_buf！
            (0, 0, total_bytes)
        }
        _ => {
            warn!("Unknown SCSI: 0x{:02x}", cmd[0]);
            (1, 0, 0) // 其他不想处理的指令，直接报错骗过主机
        }
    }
}

#[embassy_executor::task]
async fn usb_task(mut usb: UsbDevice<'static, Driver<'static, peripherals::USB_OTG_FS>>) {
    usb.run().await;
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // 时钟配置
    let mut config = embassy_stm32::Config::default();
    config.rcc.hse = Some(Hse {
        freq: Hertz(25_000_000),
        mode: HseMode::Oscillator,
    });
    config.rcc.pll_src = PllSource::HSE;
    config.rcc.pll = Some(Pll {
        prediv: PllPreDiv::DIV25,
        mul: PllMul::MUL336,
        divp: Some(PllPDiv::DIV2),
        divq: Some(PllQDiv::DIV7),
        divr: None,
    });
    config.rcc.sys = Sysclk::PLL1_P;
    config.rcc.ahb_pre = AHBPrescaler::DIV1;
    config.rcc.apb1_pre = APBPrescaler::DIV4;
    config.rcc.apb2_pre = APBPrescaler::DIV2;

    let p = embassy_stm32::init(config);
    info!("✓ Clock init");

    // USB 配置
    let ep_out_buffer = make_static!([u8; 256], [0; 256]);
    let mut usb_cfg = UsbConfig::default();
    usb_cfg.vbus_detection = false;
    let driver = Driver::new_fs(p.USB_OTG_FS, Irqs, p.PA12, p.PA11, ep_out_buffer, usb_cfg);

    let config_desc = make_static!([u8; 256], [0; 256]);
    let bos_desc = make_static!([u8; 256], [0; 256]);
    let ctrl_buf = make_static!([u8; 64], [0; 64]);

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
    let mut alt_setting = interface.alt_setting(0x08, 0x06, 0x50, None);
    let mut ep_out = alt_setting.endpoint_bulk_out(None, 64);
    let mut ep_in = alt_setting.endpoint_bulk_in(None, 64);
    drop(function);

    let usb = builder.build();
    unwrap!(spawner.spawn(usb_task(usb)));

    info!("✓ USB MSC device ready!");
    info!("✓ MSC bulk endpoints configured");
    info!("✓ Should enumerate as {}MB USB drive", VIRTUAL_SECTOR_COUNT / 2);

    let mut cbw_buf = [0u8; 31];
    
    // 主缓冲区，4096 字节已经绰绰有余应付 Windows 发起的元数据查阅了！
    let mut data_buf = [0u8; 4096]; 

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

        let (status, mut residue, resp_len) = handle_scsi_command(cmd, &mut data_buf);

        if (flags & 0x80) != 0 && dtl > 0 && resp_len > 0 {
            // 需要发送的总长度
            let send_len = core::cmp::min(resp_len, dtl as usize);
            let mut offset = 0;
            let mut write_ok = true;
            
            // 准备一个 64 字节的水瓢 (全零)
            let zero_chunk = [0u8; 64];
            while offset < send_len {
                let chunk_size = core::cmp::min(send_len - offset, 64);
                
                // 【核心魔法】：如果是读硬盘指令，我们就用全 0 水瓢泼给它！
                // 如果是其他指令(如 INQUIRY 元数据)，才去读 data_buf 里真实的配置数据。
                let chunk_data = if cmd[0] == SCSI_READ_10 {
                    &zero_chunk[..chunk_size]
                } else {
                    &data_buf[offset..offset + chunk_size]
                };
                if let Err(e) = ep_in.write(chunk_data).await {
                    warn!("MSC IN write chunk error: {:?}", e);
                    residue = dtl.saturating_sub(offset as u32);
                    write_ok = false;
                    break;
                }
                offset += chunk_size;
            }
            
            if write_ok && (send_len as u32) < dtl {
                residue = dtl - (send_len as u32);
            }
        } else if dtl > 0 {
            // 如果主机想往 U 盘【写入】数据（WRITE_10等），
            // 当前我们因为是虚拟空白盘，直接忽略写入数据就行，或者清空调缓冲（不造成卡顿）
            residue = dtl;
        }

        let mut csw = [0u8; 13];
        csw[0..4].copy_from_slice(&CSW_SIGNATURE.to_le_bytes());
        csw[4..8].copy_from_slice(&tag.to_le_bytes());
        csw[8..12].copy_from_slice(&residue.to_le_bytes());
        csw[12] = status;

        if let Err(e) = ep_in.write(&csw).await {
            warn!("MSC CSW write error: {:?}", e);
        }
    }
}

