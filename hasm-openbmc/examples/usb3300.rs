#![no_std]
#![no_main]

use defmt::*;
use defmt_rtt as _;
use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::usb::{self, Config as UsbConfig, Driver};
use embassy_stm32::{bind_interrupts, peripherals};
use embassy_time::Timer;
use embassy_usb::Builder;
use embassy_usb::Handler;
use embassy_usb::control::{InResponse, OutResponse, Recipient, Request, RequestType};
use embassy_usb::driver::{Endpoint, EndpointIn, EndpointOut};
use panic_probe as _;

use hasm_openbmc::scsi::cmd::BOOT_SECTOR;
use hasm_openbmc::scsi::fake_fs::*;
use hasm_openbmc::scsi::*;

static EP_OUT_BUFFER: static_cell::StaticCell<[u8; 1024]> = static_cell::StaticCell::new();
static CONFIG_DESC: static_cell::StaticCell<[u8; 256]> = static_cell::StaticCell::new();
static BOS_DESC: static_cell::StaticCell<[u8; 256]> = static_cell::StaticCell::new();
static CTRL_BUF: static_cell::StaticCell<[u8; 64]> = static_cell::StaticCell::new();
static USB_STATE_HANDLER: static_cell::StaticCell<UsbStateHandler> = static_cell::StaticCell::new();
static MSC_HANDLER: static_cell::StaticCell<MscHandler> = static_cell::StaticCell::new();

struct UsbStateHandler;

// STM32F407 OTG_HS 基地址 (RM0090 Table 1)
const OTG_HS_BASE: u32 = 0x4004_0000;

/// 安全地读一个 OTG_HS 寄存器（只读，不修改任何状态）
#[inline]
fn otg_read(offset: u32) -> u32 {
    unsafe { core::ptr::read_volatile((OTG_HS_BASE + offset) as *const u32) }
}

impl Handler for UsbStateHandler {
    fn enabled(&mut self, enabled: bool) {
        info!("USB enabled={}", enabled);
        if enabled {
            // ─── 此处时钟已由 Bus::init() 使能，寄存器读值有效 ───
            let cid      = otg_read(0x003C); // Core ID
            let gusbcfg  = otg_read(0x000C); // USB config
            let gccfg    = otg_read(0x0038); // General core config
            let gotgctl  = otg_read(0x0000); // OTG control
            let gintsts  = otg_read(0x0014); // Interrupt status
            info!("  CID     = 0x{:08X}  (期望 0x00001[012]00)", cid);
            info!("  GUSBCFG = 0x{:08X}  bit30(FDMOD)={} bit6(PHYSEL)={}",
                gusbcfg,
                (gusbcfg >> 30) & 1,
                (gusbcfg >> 6) & 1);
            info!("  GCCFG   = 0x{:08X}  bit16(PWRDWN)={} bit21(NOVBUSSENS)={}",
                gccfg,
                (gccfg >> 16) & 1,
                (gccfg >> 21) & 1);
            info!("  GOTGCTL = 0x{:08X}  bit6(BVALOEN)={} bit7(BVALOVAL)={}",
                gotgctl,
                (gotgctl >> 6) & 1,
                (gotgctl >> 7) & 1);
            info!("  GINTSTS = 0x{:08X}", gintsts);

            // ─────────────────────────────────────────────────────────────
            // embassy-stm32 0.4.0 ULPI 路径（config_v1, CID=0x1100）不设置
            // GOTGCTL.BVALOEN/BVALOVAL → OTG 状态机认为 B-session 无效 →
            // 不发 ULPI「Enable FS Transceiver」命令 → USB3300 不拉高 D+
            // → Windows 永远看不到设备。
            //
            // ⚠ 已知硬件限制：USB3300 ULPI 接口工作于 60 MHz，杜邦线
            //   无法可靠传输该频率信号。即使 BVALOEN 修复后，ULPI 命令
            //   仍可能因信号完整性差而丢失，导致连接看似成功（USBRST/<10ms
            //   后触发，属内部 OTG 状态机误触，非 Windows 发起）但枚举
            //   始终失败。两个独立来源(rumena.cn + CSDN u010396127)均明确
            //   报告：STM32+USB3300 杜邦线连接无法枚举。需要 PCB 才能
            //   可靠工作。
            // ─────────────────────────────────────────────────────────────
            if (gotgctl >> 6) & 1 == 0 {
                info!("  BVALOEN=0 → 强制 BVALOEN=1 BVALOVAL=1 + 软重连");
                unsafe {
                    // 步骤 1: 覆盖 B-session valid
                    let gotgctl_ptr = (OTG_HS_BASE + 0x0000) as *mut u32;
                    let v = core::ptr::read_volatile(gotgctl_ptr);
                    core::ptr::write_volatile(gotgctl_ptr, v | (1 << 6) | (1 << 7));

                    // 步骤 2: 软断连 — DCTL.SDIS(bit1)=1
                    // DCTL 在 OTG_HS 设备寄存器区 offset 0x0804
                    let dctl_ptr = (OTG_HS_BASE + 0x0804) as *mut u32;
                    let dv = core::ptr::read_volatile(dctl_ptr);
                    core::ptr::write_volatile(dctl_ptr, dv | (1 << 1)); // SDIS=1

                    // ~2ms 延迟 @168MHz (cortex_m::asm::delay 每次约 2 cycles)
                    cortex_m::asm::delay(336_000);

                    // 步骤 3: 软重连 — SDIS=0，OTG SM 以 BVALOVAL=1 重新上线
                    core::ptr::write_volatile(dctl_ptr,
                        core::ptr::read_volatile(dctl_ptr) & !(1u32 << 1)); // SDIS=0

                    // 步骤 4: 回读验证（确认 DCTL 写入有效，排除总线异常）
                    let dctl_verify = core::ptr::read_volatile(dctl_ptr);
                    let gotgctl_verify = core::ptr::read_volatile(gotgctl_ptr);
                    info!("  验证: DCTL=0x{:08X} (期望 bit1=0/SDIS已清), GOTGCTL=0x{:08X} (期望 bit6/7=1)",
                        dctl_verify, gotgctl_verify);
                }
                info!("  软重连完成，等待主机 USB-Reset 枚举");
                info!("  [诊断] 若 USB bus reset 在 <50ms 内触发 = 杜邦线ULPI毛刺引发OTG内部误触，非 Windows 发起");
            } else {
                info!("  BVALOEN 已置位，OTG B-session 正常");
            }
        }
    }

    fn reset(&mut self) {
        // OTG_HS 设备状态寄存器 DSTS (offset 0x808)
        let dsts    = otg_read(0x0808);
        let dcfg    = otg_read(0x0800); // DCFG.DSPD bits[1:0]
        let doepctl = otg_read(0x0B00); // DOEPCTL0: EP0 OUT control
        let enumspd = (dsts >> 1) & 0b11;
        let speed_str = match enumspd {
            0b00 => "High-Speed",
            0b01 => "FS-ext (ULPI 12Mbps)",     // ExternalFullSpeed 期望值
            0b10 => "Low-Speed",
            0b11 => "FS-int (内部PHY 12Mbps)",
            _ => "Unknown",
        };
        info!("USB bus reset! DSTS=0x{:08X}({}) DCFG=0x{:08X} DOEPCTL0=0x{:08X}",
            dsts, speed_str, dcfg, doepctl);
        // 关键判断：
        //   DSTS.ENUMSPD=0b01 + DOEPCTL0.EPENA(bit31)=1 → EP0 已激活，
        //     若之后 addressed() 不打印 → Windows 发送了请求但设备未响应
        //     (ULPI 数据传输丢包, 杜邦线问题)
        //   DSTS.ENUMSPD=0b?? + 此日志 < 50ms 触发 → 伪Reset(杜邦线毛刺)，
        //     Windows 根本没看到 D+，仍需 PCB
        if (doepctl >> 31) & 1 == 1 {
            info!("  EP0已激活: 等待 Windows GET_DESCRIPTOR. 若 addressed() 不出现 → ULPI数据丢包");
        } else {
            info!("  EP0 未激活 (DOEPCTL0.EPENA=0)! 枚举前提条件缺失，很可能是杜邦线伪Reset");
        }
    }

    fn addressed(&mut self, addr: u8) {
        info!("USB addressed: {}", addr);
    }

    fn configured(&mut self, configured: bool) {
        info!("USB configured={}", configured);
    }

    fn suspended(&mut self, suspended: bool) {
        // Suspend/resume can flap during unstable link; keep this at debug level.
        debug!("USB suspended={}", suspended);
    }
}

/// MSC class control handler for BOT requests required by some hosts.
struct MscHandler {
    iface_num: u8,
}

impl Handler for MscHandler {
    fn control_in<'a>(&'a mut self, req: Request, buf: &'a mut [u8]) -> Option<InResponse<'a>> {
        if req.request_type == RequestType::Class
            && req.recipient == Recipient::Interface
            && req.request == 0xFE
            && req.index == self.iface_num as u16
        {
            buf[0] = 0x00;
            return Some(InResponse::Accepted(&buf[..1]));
        }
        None
    }

    fn control_out(&mut self, req: Request, _data: &[u8]) -> Option<OutResponse> {
        if req.request_type == RequestType::Class
            && req.recipient == Recipient::Interface
            && req.request == 0xFF
            && req.index == self.iface_num as u16
        {
            return Some(OutResponse::Accepted);
        }
        None
    }
}

bind_interrupts!(struct Irqs {
    OTG_HS => usb::InterruptHandler<peripherals::USB_OTG_HS>;
});

#[embassy_executor::task]
async fn usb_task(mut usb: embassy_usb::UsbDevice<'static, Driver<'static, peripherals::USB_OTG_HS>>) {
    usb.run().await;
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // 诊断开关：先用 ULPI Full-Speed 验证链路稳定性，再切回 High-Speed。
    const USE_HS_ULPI: bool = false;

    let p = embassy_stm32::init(hasm_openbmc::clk_init());
    info!("USB3300 ULPI + MSC init...");

    // USB3300 RESETB 接到 PD3: 低有效复位。
    // RM0090 规定 RESETB 最短拉低 1µs，但 USB3300 经验值建议 ≥50ms，
    // 短脉冲在杜邦线干扰下可能导致 USB3300 进入未定义状态。
    let mut phy_reset = Output::new(p.PD3, Level::High, Speed::Low);
    phy_reset.set_low();
    Timer::after_millis(100).await;  // 100ms 保证 USB3300 可靠复位
    phy_reset.set_high();
    Timer::after_millis(20).await;   // 等待 USB3300 PLL 锁定 (典型 15ms)
    info!("USB3300 reset pulse done on PD3");

    let ep_out_buffer = EP_OUT_BUFFER.init([0; 1024]);
    let mut usb_cfg = UsbConfig::default();
    // 外接 ULPI PHY + 设备模式，很多板子无独立 VBUS sense 脚时建议关闭。
    usb_cfg.vbus_detection = false;    // xcvrdly=true 在 DCFG 中设置 ~400ns 的 UTMI+→ULPI 信号延迟，
    // 改善 ULPI 建立/保持时序裕量（对杜邦线有一定缓解作用）。
    usb_cfg.xcvrdly = true;    let driver = if USE_HS_ULPI {
        info!("ULPI mode: High-Speed");
        Driver::new_hs_ulpi(
            p.USB_OTG_HS,
            Irqs,
            // 你的接线映射: CLK/ DIR/ NXT/ STP
            p.PA5,
            p.PC2,
            p.PC3,
            p.PC0,
            // 你的接线映射: DATA0..DATA7
            p.PA3,
            p.PB0,
            p.PB1,
            p.PB10,
            p.PB11,
            p.PB12,
            p.PB13,
            p.PB5,
            ep_out_buffer,
            usb_cfg,
        )
    } else {
        info!("ULPI mode: Full-Speed (diagnostic)");
        Driver::new_fs_ulpi(
            p.USB_OTG_HS,
            Irqs,
            // 你的接线映射: CLK/ DIR/ NXT/ STP
            p.PA5,
            p.PC2,
            p.PC3,
            p.PC0,
            // 你的接线映射: DATA0..DATA7
            p.PA3,
            p.PB0,
            p.PB1,
            p.PB10,
            p.PB11,
            p.PB12,
            p.PB13,
            p.PB5,
            ep_out_buffer,
            usb_cfg,
        )
    };

    let config_desc = CONFIG_DESC.init([0; 256]);
    let bos_desc = BOS_DESC.init([0; 256]);
    let ctrl_buf = CTRL_BUF.init([0; 64]);

    let mut cfg = embassy_usb::Config::new(0xc0de, 0xcafe);
    cfg.manufacturer = Some("MyBMC");
    cfg.product = Some("STM32F407 USB3300 HS MSC");
    cfg.serial_number = Some("F407-ULPI-001");
    cfg.max_power = 100;
    cfg.max_packet_size_0 = 64;

    let mut builder = Builder::new(driver, cfg, config_desc, bos_desc, &mut [], ctrl_buf);

    // MSC interface descriptors: Class=0x08, Subclass=0x06, Protocol=0x50
    let mut function = builder.function(0x08, 0x06, 0x50);
    let mut interface = function.interface();
    let iface_num = interface.interface_number().0;
    let mut alt_setting = interface.alt_setting(0x08, 0x06, 0x50, None);
    let bulk_mps = if USE_HS_ULPI { 512 } else { 64 };
    let mut ep_out = alt_setting.endpoint_bulk_out(None, bulk_mps);
    let mut ep_in = alt_setting.endpoint_bulk_in(None, bulk_mps);
    drop(function);

    let usb_state_handler = USB_STATE_HANDLER.init(UsbStateHandler);
    builder.handler(usb_state_handler);
    let msc_handler = MSC_HANDLER.init(MscHandler { iface_num });
    builder.handler(msc_handler);

    let usb = builder.build();
    unwrap!(spawner.spawn(usb_task(usb)));

    info!("USB3300 HS MSC ready");

    let mut cbw_buf = [0u8; 31];
    let mut data_buf = [0u8; 4096];

    loop {
        let n = match ep_out.read(&mut cbw_buf).await {
            Ok(n) => n,
            Err(e) => {
                warn!("MSC OUT read error: {:?}, waiting endpoint enable", e);
                ep_out.wait_enabled().await;
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
            let send_len = core::cmp::min(response.resp_len, dtl as usize);
            let mut offset = 0;
            let mut write_ok = true;

            while offset < send_len {
                let chunk_size = core::cmp::min(send_len - offset, bulk_mps as usize);

                let lba = u32::from_be_bytes([cmd[2], cmd[3], cmd[4], cmd[5]]);
                let chunk_data = if cmd[0] == SCSI_READ_10 {
                    let abs_offset = (lba * SECTOR_SIZE) + (offset as u32);
                    let cur_sector = abs_offset / SECTOR_SIZE;
                    let sector_offset = (abs_offset % SECTOR_SIZE) as usize;

                    match cur_sector {
                        0 => &BOOT_SECTOR[sector_offset..sector_offset + chunk_size],
                        1 | 257 => &FAT_SECTOR[sector_offset..sector_offset + chunk_size],
                        513 => &ROOT_DIR_SECTOR[sector_offset..sector_offset + chunk_size],
                        545 => &HELLO_DATA_SECTOR[sector_offset..sector_offset + chunk_size],
                        _ => {
                            static ZERO_BUF: [u8; 512] = [0; 512];
                            &ZERO_BUF[sector_offset..sector_offset + chunk_size]
                        }
                    }
                } else {
                    &data_buf[offset..offset + chunk_size]
                };

                if let Err(e) = ep_in.write(chunk_data).await {
                    warn!("MSC IN write chunk error: {:?}", e);
                    ep_in.wait_enabled().await;
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
            // 主机写入阶段将数据抽干，避免端点堵塞。
            let mut bytes_read = 0;
            let mut dump_buf = [0u8; 64];
            while bytes_read < dtl {
                match ep_out.read(&mut dump_buf).await {
                    Ok(n) => bytes_read += n as u32,
                    Err(e) => {
                        warn!("MSC OUT drain error: {:?}", e);
                        ep_out.wait_enabled().await;
                        break;
                    }
                }
            }

            if cmd[0] == SCSI_WRITE_10 {
                warn!("write protected: command denied");
                response.status = ScsiStatus::ScsiFail;
            }

            response.residue = dtl.saturating_sub(bytes_read);
        }

        let mut csw = [0u8; 13];
        csw[0..4].copy_from_slice(&CSW_SIGNATURE.to_le_bytes());
        csw[4..8].copy_from_slice(&tag.to_le_bytes());
        csw[8..12].copy_from_slice(&response.residue.to_le_bytes());
        csw[12] = response.status as u8;

        if let Err(e) = ep_in.write(&csw).await {
            warn!("MSC CSW write error: {:?}", e);
            ep_in.wait_enabled().await;
        }
    }
}