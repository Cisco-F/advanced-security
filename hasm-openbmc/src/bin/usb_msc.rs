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
                    response.residue = dtl.saturating_sub(offset as u32);
                    write_ok = false;
                    break;
                }
                offset += chunk_size;
            }
            
            if write_ok && (send_len as u32) < dtl {
                response.residue = dtl - (send_len as u32);
            }
        } else if dtl > 0 {
            // 如果主机想往 U 盘【写入】数据（WRITE_10等），
            // 当前我们因为是虚拟空白盘，直接忽略写入数据就行，或者清空调缓冲（不造成卡顿）
            response.residue = dtl;
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