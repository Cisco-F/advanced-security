use defmt::{info, warn};
use embassy_stm32::peripherals::{PA11, PA12, USB_OTG_FS};
use embassy_stm32::usb::{Config, Driver};
use embassy_stm32::{Peri, bind_interrupts, peripherals, usb};
use embassy_usb::driver::{EndpointError, EndpointIn, EndpointOut};
use embassy_usb::{Builder, Handler, UsbDevice};
use embassy_usb::control::{InResponse, OutResponse, Recipient, Request, RequestType};
use panic_probe as _;

static EP_OUT_BUFFER: static_cell::StaticCell<[u8; 256]> = static_cell::StaticCell::new();
static CONFIG_DESC: static_cell::StaticCell<[u8; 256]> = static_cell::StaticCell::new();
static BOS_DESC: static_cell::StaticCell<[u8; 256]> = static_cell::StaticCell::new();
static CTRL_BUF: static_cell::StaticCell<[u8; 64]> = static_cell::StaticCell::new();
static MSC_HANDLER: static_cell::StaticCell<MscHandler> = static_cell::StaticCell::new();

bind_interrupts!(struct UsbIrqs {
    OTG_FS => usb::InterruptHandler<peripherals::USB_OTG_FS>;
});

pub struct SendToHostError {
    pub residue: u32,             // Number of bytes not sent
    pub usb_error: EndpointError, // The underlying USB error
}

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

pub struct MSCDev<D: embassy_usb::driver::Driver<'static>> {
    pub ep_in: Option<D::EndpointIn>,
    pub ep_out: Option<D::EndpointOut>,
    pub usb_device: Option<UsbDevice<'static, D>>,
}

impl MSCDev<Driver<'static, USB_OTG_FS>> {
    pub fn init() -> Self {
        Self {
            ep_in: None,
            ep_out: None,
            usb_device: None,
        }
    }

    pub fn new(
        &mut self,
        usb_otg_fs: Peri<'static, USB_OTG_FS>,
        dp: Peri<'static, PA12>,
        dm: Peri<'static, PA11>,
    ) {
        let ep_out_buffer = EP_OUT_BUFFER.init([0; 256]);
        let mut usb_cfg = Config::default();
        usb_cfg.vbus_detection = false;
        let driver = Driver::new_fs(usb_otg_fs, UsbIrqs, dp, dm, ep_out_buffer, usb_cfg);

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
        let ep_out = alt_setting.endpoint_bulk_out(None, 64);
        let ep_in = alt_setting.endpoint_bulk_in(None, 64);
        let iface_num = interface.interface_number().0;
        drop(function);

        let msc_handler = MSC_HANDLER.init(MscHandler { iface_num });
        builder.handler(msc_handler);
        
        let usb_device = builder.build();

        self.ep_in = Some(ep_in);
        self.ep_out = Some(ep_out);
        self.usb_device = Some(usb_device);
    }
}

#[allow(async_fn_in_trait)]
pub trait ScsiDataSink {
    async fn read(&mut self, buf: &mut [u8]) -> Result<(), EndpointError>;
    async fn write(&mut self, buf: &[u8]) -> Result<(), SendToHostError>;
}

impl ScsiDataSink for MSCDev<Driver<'static, USB_OTG_FS>> {
    async fn read(&mut self, buf: &mut [u8]) -> Result<(), EndpointError> {
        let ep_out = match self.ep_out.as_mut() {
            Some(e) => e,
            None => return Err(EndpointError::Disabled),
        };

        let n = match ep_out.read(buf).await {
            Ok(n) => n,
            Err(e) => {
                warn!("MSC OUT read error: {:?}", e);
                return Err(EndpointError::Disabled);
            }
        };

        if n < 31 {
            warn!("Received short CBW: {} bytes", n);
            return Err(EndpointError::Disabled);
        }

        Ok(())
    }

    async fn write(&mut self, buf: &[u8]) -> Result<(), SendToHostError> {
        let ep_in = match self.ep_in.as_mut() {
            Some(e) => e,
            None => {
                return Err(SendToHostError {
                    residue: buf.len() as u32,
                    usb_error: EndpointError::Disabled,
                });
            }
        };

        let mut offset = 0usize;

        while offset < buf.len() {
            let chunk_size = core::cmp::min(64, buf.len() - offset);
            let chunk_data = &buf[offset..offset + chunk_size];

            match ep_in.write(chunk_data).await {
                Ok(()) => {
                    offset += chunk_size;
                }
                Err(e) => {
                    warn!("USB IN write error: {:?}", e);
                    let residue = (buf.len() - offset) as u32;
                    return Err(SendToHostError {
                        residue,
                        usb_error: e,
                    });
                }
            }
        }

        Ok(())
    }
}
