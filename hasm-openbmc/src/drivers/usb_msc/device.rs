//! USB Mass Storage Class device construction.
//!
//! This module owns the USB descriptors, control-request handler, and bulk IN/OUT
//! endpoints used by the SCSI transport layer. The exported device follows the
//! Bulk-Only Transport profile:
//! - interface class 0x08: Mass Storage;
//! - subclass 0x06: SCSI transparent command set;
//! - protocol 0x50: bulk-only transport.
//!
//! The firmware exposes exactly one logical unit. The host sends Command Block
//! Wrappers (CBW) on bulk OUT, receives command data on bulk IN, and finally
//! receives a Command Status Wrapper (CSW) on bulk IN.
//!
//! All descriptor/control buffers are static. Embassy's USB builder borrows them
//! for the full lifetime of the device, so stack allocation would be invalid.
//! The endpoint packet size is 64 bytes because this is USB full speed.

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
    /// Bytes that were still unsent when the endpoint failed.
    pub residue: u32,             // Number of bytes not sent
    /// USB endpoint error returned by Embassy.
    pub usb_error: EndpointError, // The underlying USB error
}

/// Handles class-specific control requests for the MSC interface.
struct MscHandler {
    /// Interface number assigned by the USB builder.
    iface_num: u8,
}

impl Handler for MscHandler {
    fn control_in<'a>(&'a mut self, req: Request, buf: &'a mut [u8]) -> Option<InResponse<'a>> {
        // GET_MAX_LUN returns the highest supported logical-unit index. A value
        // of zero means "one LUN, numbered 0".
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
        // Bulk-Only Reset asks the device to clear transport state. The command
        // loop is stateless between CBWs, so acknowledging the request is enough
        // for the current implementation.
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

/// Owns the USB MSC endpoints and the built USB device.
///
/// The `usb_device` field is taken by `usb_device_task`, while `ep_in` and
/// `ep_out` remain with the SCSI command loop. Splitting the ownership this way
/// matches Embassy's model: the device runner handles bus/control traffic, and
/// endpoint futures handle bulk payloads.
pub struct MSCDev<D: embassy_usb::driver::Driver<'static>> {
    pub ep_in: Option<D::EndpointIn>,
    pub ep_out: Option<D::EndpointOut>,
    pub usb_device: Option<UsbDevice<'static, D>>,
}

impl MSCDev<Driver<'static, USB_OTG_FS>> {
    /// Create an empty MSC wrapper before USB peripherals are available.
    pub fn init() -> Self {
        Self {
            ep_in: None,
            ep_out: None,
            usb_device: None,
        }
    }

    /// Build descriptors, allocate endpoints, and construct the USB device.
    pub fn new(
        &mut self,
        usb_otg_fs: Peri<'static, USB_OTG_FS>,
        dp: Peri<'static, PA12>,
        dm: Peri<'static, PA11>,
    ) {
        // VBUS detection is disabled because this board is commonly powered and
        // debugged from a lab setup where the USB cable may not provide reliable
        // VBUS sensing to the MCU pin.
        let ep_out_buffer = EP_OUT_BUFFER.init([0; 256]);
        let mut usb_cfg = Config::default();
        usb_cfg.vbus_detection = false;
        let driver = Driver::new_fs(usb_otg_fs, UsbIrqs, dp, dm, ep_out_buffer, usb_cfg);

        let config_desc = CONFIG_DESC.init([0; 256]);
        let bos_desc = BOS_DESC.init([0; 256]);
        let ctrl_buf = CTRL_BUF.init([0; 64]);

        // Demo VID/PID values. For a production device these must be replaced
        // with assigned identifiers.
        let mut cfg = embassy_usb::Config::new(0xc0de, 0xcafe);
        cfg.manufacturer = Some("MyBMC");
        cfg.product = Some("STM32F407 USB MSC");
        cfg.serial_number = Some("F407-MSC-001");
        cfg.max_power = 100;
        cfg.max_packet_size_0 = 64;

        let mut builder = Builder::new(driver, cfg, config_desc, bos_desc, &mut [], ctrl_buf);

        // MSC interface descriptors: Class=0x08, Subclass=0x06, Protocol=0x50
        // Endpoint direction is from the USB host's perspective:
        // OUT carries CBW and write payloads from host to device;
        // IN carries read payloads and CSW status back to host.
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
/// Minimal byte-stream interface needed by the SCSI command handlers.
///
/// Keeping this as a trait lets command parsing be tested or reused with other
/// sinks, while the production implementation writes to USB bulk endpoints.
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
                return Err(e);
            }
        };

        if n < 31 {
            // A valid CBW is exactly 31 bytes. Short reads indicate a transport
            // error or host reset, so the command loop should drop this packet.
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
            // Full-speed bulk endpoints max out at 64 bytes per transaction.
            // Larger SCSI payloads are fragmented here so command handlers can
            // pass whole sector buffers without knowing USB packet limits.
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
