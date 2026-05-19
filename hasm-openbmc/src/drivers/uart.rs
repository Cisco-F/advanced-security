use embassy_stm32::{
    Peri, bind_interrupts,
    peripherals::{PA10, PA9, USART1},
    usart::{self, BufferedUart, Config as UartConfig},
};
use static_cell::StaticCell;

static UART_TX_BUF: StaticCell<[u8; 256]> = StaticCell::new();
static UART_RX_BUF: StaticCell<[u8; 1024]> = StaticCell::new();

bind_interrupts!(pub struct UartIrqs {
    USART1 => usart::BufferedInterruptHandler<USART1>;
});

/// Initialize USART1 as a buffered UART (PA10=RX, PA9=TX).
pub fn uart_init(
    usart: Peri<'static, USART1>,
    rx: Peri<'static, PA10>,
    tx: Peri<'static, PA9>,
    baudrate: u32,
) -> BufferedUart<'static> {
    let mut cfg = UartConfig::default();
    cfg.baudrate = baudrate;
    BufferedUart::new(
        usart,
        rx,
        tx,
        UART_TX_BUF.init([0; 256]),
        UART_RX_BUF.init([0; 1024]),
        UartIrqs,
        cfg,
    )
    .unwrap()
}
