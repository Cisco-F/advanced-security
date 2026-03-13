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

pub async fn dump_system_info(_system_id: &str) -> &'static str {
    r##"{
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
}