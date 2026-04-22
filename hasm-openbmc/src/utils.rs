use embassy_net::Ipv4Address;

pub fn itoa(mut n: u16, buf: &mut [u8]) -> &[u8] {
    if n == 0 { return b"0"; }
    let mut i = 0;
    while n > 0 && i < buf.len() {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    let slice = &mut buf[..i];
    slice.reverse();
    slice
}

pub fn u32_to_ascii(mut n: u32, buf: &mut [u8]) -> &[u8] {
    if n == 0 { return b"0"; }
    let mut i = 0;
    while n > 0 && i < buf.len() {
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        i += 1;
    }
    buf[..i].reverse();
    &buf[..i]
}

pub fn format_ip(ip: Ipv4Address, out: &mut [u8]) -> &[u8] {
    // write dotted quad into out, return slice
    let octets = ip.octets();
    let mut idx = 0usize;
    for (i, &o) in octets.iter().enumerate() {
        // write decimal
        let mut tmp = [0u8; 3];
        let mut n = o as u16;
        if n == 0 {
            out[idx] = b'0';
            idx += 1;
        } else {
            let mut t = 0usize;
            while n > 0 {
                tmp[t] = b'0' + (n % 10) as u8;
                n /= 10;
                t += 1;
            }
            for k in 0..t { out[idx + k] = tmp[t - 1 - k]; }
            idx += t;
        }
        if i != 3 {
            out[idx] = b'.';
            idx += 1;
        }
    }
    &out[..idx]
}

pub fn dump_system_info(_system_id: &str, power_on: bool) -> &'static str {
    if power_on {
        SYSTEM_INFO_ON
    } else {
        SYSTEM_INFO_OFF
    }
}

static SYSTEM_INFO_ON: &str = r##"{
        "@odata.type": "#ComputerSystem.v1_15_0.ComputerSystem",
        "@odata.id": "/redfish/v1/Systems/1",
        "Id": "1",
        "Name": "Main System",
        "PowerState": "On",
        "Actions": {
            "#ComputerSystem.Reset": {
                "target": "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset",
                "ResetType@Redfish.AllowableValues": [
                    "On",
                    "ForceOff",
                ]
            }
        }
    }"##
;

static SYSTEM_INFO_OFF: &str = r##"{
        "@odata.type": "#ComputerSystem.v1_15_0.ComputerSystem",
        "@odata.id": "/redfish/v1/Systems/1",
        "Id": "1",
        "Name": "Main System",
        "PowerState": "Off",
        "Actions": {
            "#ComputerSystem.Reset": {
                "target": "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset",
                "ResetType@Redfish.AllowableValues": [
                    "On",
                    "ForceOff",
                ]
            }
        }
    }"##
;