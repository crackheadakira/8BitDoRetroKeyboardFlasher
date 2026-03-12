use hidapi::HidApi;

use crate::flash::FlashSession;

mod flash;
mod packet;

const VENDOR_ID: u16 = 0x2dc8;
const PRODUCT_ID: u16 = 0x5200;

const USAGE_PAGE: u16 = 0x008c;
const USAGE: u16 = 0x0001;

const PACKET_SIZE: usize = 33;
const PAYLOAD_SIZE: usize = 16;

fn main() {
    println!("=== 8BitDo Retro Keyboard Flash Tool ===");
    let handshake = std::env::args().any(|a| a == "--handshake");

    let firmware = std::fs::read("official.dat").expect("Failed to read firmware file");
    println!(
        "Firmware size: {} bytes ({} chunks)",
        firmware.len(),
        firmware.len().div_ceil(PAYLOAD_SIZE)
    );

    if handshake {
        println!("\n*** HANDSHAKE MODE - will abort before flashing ***");
    }

    let api = HidApi::new().expect("Failed to create HID API");
    let device = api
        .device_list()
        .find(|d| {
            d.vendor_id() == VENDOR_ID
                && d.product_id() == PRODUCT_ID
                && d.usage_page() == USAGE_PAGE
                && d.usage() == USAGE
        })
        .expect("Keyboard not found")
        .open_device(&api)
        .expect("Failed to open device");

    println!("Connected to keyboard\n");
    let mut flash_session = FlashSession::new(device);

    println!("[1/4] Sending handshake...");
    flash_session.handshake().unwrap();

    if handshake {
        println!("\nHandshake succeeded, keyboard is responding correctly.");
        println!("Run without --handshake to actually flash.");
        std::process::exit(0);
    }

    println!("✓ Handshake successful, proceeding with flash...");

    println!("\n[2/4] Sending firmware...");

    flash_session.firmware(firmware).unwrap();

    println!("\n[3/4] Sending commit packet...");

    flash_session.commit().unwrap();

    println!("\n[4/4] Sending reboot packet...");

    flash_session.reboot().unwrap();

    println!("\n=== Flash complete ===");
}
