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
use embassy_usb::Builder;
use embassy_usb::driver::{EndpointIn, EndpointOut};
use panic_probe as _;

use hasm_openbmc::scsi::cmd::BOOT_SECTOR;
use hasm_openbmc::scsi::fake_fs::*;
use hasm_openbmc::scsi::*;

static EP_OUT_BUFFER: static_cell::StaticCell<[u8; 256]> = static_cell::StaticCell::new();
static CONFIG_DESC: static_cell::StaticCell<[u8; 256]> = static_cell::StaticCell::new();
static BOS_DESC: static_cell::StaticCell<[u8; 256]> = static_cell::StaticCell::new();
static CTRL_BUF: static_cell::StaticCell<[u8; 64]> = static_cell::StaticCell::new();

bind_interrupts!(struct Irqs {
    OTG_FS => usb::InterruptHandler<peripherals::USB_OTG_FS>;
});

#[embassy_executor::task]
async fn usb_task(mut usb: embassy_usb::UsbDevice<'static, Driver<'static, peripherals::USB_OTG_FS>>) {
    usb.run().await;
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = hasm_openbmc::hal::init::sys_init();
    info!("✓ Clock init");

    // USB 配置
    let ep_out_buffer = EP_OUT_BUFFER.init([0; 256]);
    let mut usb_cfg = UsbConfig::default();
    usb_cfg.vbus_detection = false;
    let driver = Driver::new_fs(
        p.USB_OTG_FS, 
        Irqs, 
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
    let mut alt_setting = interface.alt_setting(0x08, 0x06, 0x50, None);
    let mut ep_out = alt_setting.endpoint_bulk_out(None, 64);
    let mut ep_in = alt_setting.endpoint_bulk_in(None, 64);
    drop(function);

    let usb = builder.build();
    unwrap!(spawner.spawn(usb_task(usb)));

    info!("✓ USB MSC device ready!");

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
        let mut response = handle_scsi_cmd(cmd, &mut data_buf);

        if (flags & 0x80) != 0 && dtl > 0 && response.resp_len > 0 {
            // 需要发送的总长度
            let send_len = core::cmp::min(response.resp_len, dtl as usize);
            let mut offset = 0;
            let mut write_ok = true;

            while offset < send_len {
                let chunk_size = core::cmp::min(send_len - offset, 64);

                let lba = u32::from_be_bytes([cmd[2], cmd[3], cmd[4], cmd[5]]);
                let chunk_data = if cmd[0] == SCSI_READ_10 {
                    let abs_offset = (lba * SECTOR_SIZE) + (offset as u32);
                    let cur_sector = abs_offset / SECTOR_SIZE;
                    let sector_offset = (abs_offset % SECTOR_SIZE) as usize;  

                    match cur_sector {
                        // LBA 0: 引导扇区 (存放容量、架构信息)
                        0 => &BOOT_SECTOR[sector_offset..sector_offset + chunk_size],
                        
                        // LBA 1 和 257: 分别是 FAT1 和 FAT2 表的开头。
                        // 我们的 FAT_SECTOR 里写明了 "2号簇是不再延续的最后一个簇(0xFFFF)"
                        1 | 257 => &FAT_SECTOR[sector_offset .. sector_offset + chunk_size],
                        
                        // LBA 513: FAT16 把根目录推到了这里！(存放 HELLO.TXT 的文件名和属性)
                        513 => &ROOT_DIR_SECTOR[sector_offset .. sector_offset + chunk_size],
                        
                        // LBA 545: 第 2 号簇在这个新磁盘中的绝对存放位置！(存放真实文本)
                        545 => &HELLO_DATA_SECTOR[sector_offset .. sector_offset + chunk_size],
                        
                        // 其他所有没用到的空间，全给 0，让操作系统觉得这是个干净的空盘
                        _ => {
                            static ZERO_BUF: [u8; 512] = [0; 512];
                            &ZERO_BUF[sector_offset .. sector_offset + chunk_size]
                        }
                    } 
                } else {
                    &data_buf[offset .. offset + chunk_size]
                };

                if let Err(e) = ep_in.write(chunk_data).await {
                    warn!("MSC IN write chunk error: {:?}", e);
                    response.residue = dtl.saturating_sub(offset as u32);
                    write_ok = false;
                    break;
                }
                offset += chunk_size;
            }

            if write_ok && (send_len as u32) < dtl {
                // 如果我们发送的数据比主机预期的还少，说明主机多余的数据我们无法处理了，直接把剩余的都标记为未处理（residue）
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