#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_stm32::{
    Config,
    bind_interrupts,
    peripherals,
    sdmmc::{self, DataBlock, Sdmmc},
    time::Hertz,
};
use embassy_stm32::rcc::*;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    SDIO => sdmmc::InterruptHandler<peripherals::SDIO>;
});

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

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    // 时钟配置（与 bmc.rs 保持一致，系统时钟 168 MHz）
    let mut config = Config::default();
    {
        config.rcc.hse = Some(Hse {
            freq: Hertz(25_000_000),
            mode: HseMode::Oscillator,
        });
        config.rcc.pll_src = PllSource::HSE;
        config.rcc.pll = Some(Pll {
            prediv: PllPreDiv::DIV25,
            mul: PllMul::MUL336,
            divp: Some(PllPDiv::DIV2), // 168 MHz
            divq: Some(PllQDiv::DIV7),
            divr: None,
        });
        config.rcc.sys = Sysclk::PLL1_P;
        config.rcc.ahb_pre = AHBPrescaler::DIV1;
        config.rcc.apb1_pre = APBPrescaler::DIV4;
        config.rcc.apb2_pre = APBPrescaler::DIV2;
    }
    let p = embassy_stm32::init(config);

    info!("TF Card Init...");

    // SDIO 4-bit 模式初始化
    // STM32F407ZG 标准 SDIO 引脚：
    //   PC12 -> SDIO_CK  (时钟)
    //   PD2  -> SDIO_CMD (命令)
    //   PC8  -> SDIO_D0
    //   PC9  -> SDIO_D1
    //   PC10 -> SDIO_D2
    //   PC11 -> SDIO_D3
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
    match tf.sdmmc.init_sd_card(Hertz(400_000)).await {
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

    // 读取第 0 扇区（MBR / Boot Sector）
    let mut buf = DataBlock([0u8; 512]);
    match tf.read(0, &mut buf).await {
        Ok(_) => {
            info!("Sector 0 read OK");
            // 按 16 字节一行打印
            for (i, chunk) in buf.0.chunks(16).enumerate() {
                info!(
                    "{:03x}: {:02x} {:02x} {:02x} {:02x}  {:02x} {:02x} {:02x} {:02x}  \
                     {:02x} {:02x} {:02x} {:02x}  {:02x} {:02x} {:02x} {:02x}",
                    i * 16,
                    chunk[0],  chunk[1],  chunk[2],  chunk[3],
                    chunk[4],  chunk[5],  chunk[6],  chunk[7],
                    chunk[8],  chunk[9],  chunk[10], chunk[11],
                    chunk[12], chunk[13], chunk[14], chunk[15],
                );
            }
        }
        Err(e) => {
            error!("Sector 0 read failed: {:?}", e);
        }
    }
}
