#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]
#![recursion_limit = "512"]

/// RP2350 binary info entries for picotool / bootloader.
#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    embassy_rp::binary_info::rp_program_name!(c"micro-bacnet-bridge"),
    embassy_rp::binary_info::rp_program_description!(
        c"BACnet MS/TP to BACnet/IP bridge (Icomb Place)"
    ),
    embassy_rp::binary_info::rp_cargo_version!(),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

mod bacnet_ip;
mod bridge;
mod config;
mod core1;
mod dns;
mod http;
mod ipc;
mod mdns;
mod mqtt;
mod ntp;
mod ota;
mod platform;
mod snmp;
mod syslog;
mod web_assets;

use defmt::info;
use embassy_executor::Spawner;
use embassy_net::{Config as NetConfig, Stack, StackResources};
use embassy_net_wiznet::chip::W5500;
use embassy_rp::bind_interrupts;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::spi::{Config as SpiConfig, Spi};
use embassy_rp::trng::Trng;
use embassy_time::Timer;
use embedded_hal_bus::spi::ExclusiveDevice;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct TrngIrqs {
    TRNG_IRQ => embassy_rp::trng::InterruptHandler<embassy_rp::peripherals::TRNG>;
});

// ---------------------------------------------------------------------------
// Static allocations for embassy-net stack resources
// ---------------------------------------------------------------------------

/// Number of sockets the network stack can hold simultaneously.
/// HTTP (4 workers) + mDNS (1) + BACnet/IP (1) + DHCP internal (1) + NTP (1) +
/// SNMP (1) + MQTT/TCP (1) + DNS/UDP (1) + Syslog/UDP (1) = 12
const SOCKET_COUNT: usize = 12;

static STACK_RESOURCES: StaticCell<StackResources<SOCKET_COUNT>> = StaticCell::new();
static WIZNET_STATE: StaticCell<embassy_net_wiznet::State<4, 4>> = StaticCell::new();

// ---------------------------------------------------------------------------
// Embassy entry point (Core 0)
// ---------------------------------------------------------------------------

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // Copy .time_critical section from flash to SRAM before anything else.
    // These symbols are defined by the linker script.
    extern "C" {
        static mut __stime_critical: u8;
        static __etime_critical: u8;
        static __sitime_critical: u8; // source address in flash (LMA)
    }
    unsafe {
        let dst = core::ptr::addr_of_mut!(__stime_critical);
        let src = core::ptr::addr_of!(__sitime_critical);
        let len = core::ptr::addr_of!(__etime_critical) as usize - dst as usize;
        if len > 0 {
            core::ptr::copy_nonoverlapping(src as *const u8, dst, len);
        }
    }

    info!(
        "micro-bacnet-bridge starting (Icomb Place firmware v{})",
        env!("FIRMWARE_VERSION")
    );

    // Use crystal oscillator (12MHz XOSC → PLL → 150MHz system clock).
    // Default::default() uses ROSC (~6MHz, unpredictable) which breaks
    // UART baud rate calculation (C code assumes SYS_CLK_HZ = 150MHz).
    let mut config = embassy_rp::config::Config::default();
    config.clocks = embassy_rp::clocks::ClockConfig::crystal(12_000_000);
    let p = embassy_rp::init(config);

    // ---- GPIO: LED heartbeat ----
    let mut led = Output::new(p.PIN_25, Level::Low);

    // ---- Hardware TRNG (RP2350) ----
    let mut trng = Trng::new(p.TRNG, TrngIrqs, embassy_rp::trng::Config::default());

    // ---- Flash + config (before W5500, we need the MAC address) ----
    let flash =
        embassy_rp::flash::Flash::<_, embassy_rp::flash::Async, { platform::FLASH_SIZE }>::new(
            p.FLASH, p.DMA_CH2,
        );
    let mut cfg_mgr = config::ConfigManager::new(flash);
    let mut bridge_config = cfg_mgr.load();

    // MAC address: stored in a dedicated identity flash sector that survives
    // all reflashes (OTA and BOOTSEL). Generated from TRNG on first boot.
    let mac_addr = match cfg_mgr.load_mac() {
        Some(mac) if mac[1..] != [0, 0, 0, 0, 0] => mac,
        _ => {
            let seed = trng.blocking_next_u64();
            let mac = [
                0x02, // locally administered, unicast
                (seed >> 8) as u8,
                (seed >> 16) as u8,
                (seed >> 24) as u8,
                (seed >> 32) as u8,
                (seed >> 40) as u8,
            ];
            cfg_mgr.save_mac(&mac);
            info!(
                "first boot: generated MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
            );
            mac
        }
    };

    // If hostname is factory default, make it unique with last 3 MAC bytes
    if bridge_config.hostname.as_str() == "bacnet-bridge" {
        let mut unique: heapless::String<32> = heapless::String::new();
        let _ = core::fmt::write(
            &mut unique,
            format_args!(
                "bacnet-bridge-{:02x}{:02x}{:02x}",
                mac_addr[3], mac_addr[4], mac_addr[5]
            ),
        );
        bridge_config.hostname = unique;
    }

    info!(
        "config: device_id={}, hostname={}",
        bridge_config.bacnet.device_id,
        bridge_config.hostname.as_str()
    );

    // Publish MAC address so the mDNS task can include it in TXT records without
    // holding a mutex.  Store as two AtomicU32: HI = bytes 0–1, LO = bytes 2–5.
    {
        let hi: u32 = ((mac_addr[0] as u32) << 8) | (mac_addr[1] as u32);
        let lo: u32 = ((mac_addr[2] as u32) << 24)
            | ((mac_addr[3] as u32) << 16)
            | ((mac_addr[4] as u32) << 8)
            | (mac_addr[5] as u32);
        http::MAC_ADDR_HI.store(hi, core::sync::atomic::Ordering::Relaxed);
        http::MAC_ADDR_LO.store(lo, core::sync::atomic::Ordering::Relaxed);
    }

    // Hand flash to OTA subsystem
    {
        let mut flash_guard = ota::FLASH.lock().await;
        *flash_guard = Some(cfg_mgr.into_flash());
    }

    // ---- Diagnostic: blink LED twice to show we reached SPI init ----
    led.set_high();
    Timer::after_millis(200).await;
    led.set_low();
    Timer::after_millis(200).await;
    led.set_high();
    Timer::after_millis(200).await;
    led.set_low();
    Timer::after_millis(200).await;

    // ---- SPI0 for W5500 ----
    let mut spi_cfg = SpiConfig::default();
    spi_cfg.frequency = 33_000_000;

    let spi_bus = Spi::new(
        p.SPI0, p.PIN_18, p.PIN_19, p.PIN_16, p.DMA_CH0, p.DMA_CH1, spi_cfg,
    );

    let cs = Output::new(p.PIN_17, Level::High);
    let spi_dev = ExclusiveDevice::new(spi_bus, cs, embassy_time::Delay).unwrap();

    let w5500_int = embassy_rp::gpio::Input::new(p.PIN_21, embassy_rp::gpio::Pull::Up);
    let mut w5500_rst = Output::new(p.PIN_20, Level::High);

    // Reset the W5500 cleanly — pulse RST low for 10ms then wait 500ms
    w5500_rst.set_low();
    Timer::after_millis(10).await;
    w5500_rst.set_high();
    Timer::after_millis(500).await;

    let wiznet_state = WIZNET_STATE.init(embassy_net_wiznet::State::new());

    let (w5500_device, w5500_runner) = embassy_net_wiznet::new::<4, 4, W5500, _, _, _>(
        mac_addr,
        wiznet_state,
        spi_dev,
        w5500_int,
        w5500_rst,
    )
    .await
    .expect("W5500 init failed");

    spawner.spawn(w5500_task(w5500_runner)).unwrap();

    // Store loaded config in global for HTTP/mDNS tasks
    {
        let mut cfg = http::CONFIG.lock().await;
        *cfg = Some(bridge_config.clone());
    }

    // ---- embassy-net stack ----
    // Use DHCP if configured, otherwise static IP
    let net_config = if bridge_config.network.dhcp {
        NetConfig::dhcpv4(Default::default())
    } else {
        let ip = bridge_config.network.ip;
        let subnet = bridge_config.network.subnet;
        let gw = bridge_config.network.gateway;
        let prefix_len = subnet_mask_to_prefix(subnet);
        NetConfig::ipv4_static(embassy_net::StaticConfigV4 {
            address: embassy_net::Ipv4Cidr::new(
                embassy_net::Ipv4Address::new(ip[0], ip[1], ip[2], ip[3]),
                prefix_len,
            ),
            dns_servers: heapless::Vec::new(),
            gateway: Some(embassy_net::Ipv4Address::new(gw[0], gw[1], gw[2], gw[3])),
        })
    };

    // Generate a random seed from the hardware TRNG for the network stack
    let random_seed = trng.blocking_next_u64();

    // STACK_RESOURCES.init() returns &'static mut StackResources, so the
    // Stack and Runner returned by embassy_net::new() are already Stack<'static>
    // and Runner<'static, _> — no unsafe transmute needed.
    let stack_resources: &'static mut _ = STACK_RESOURCES.init(StackResources::new());
    let (stack, stack_runner): (Stack<'static>, _) =
        embassy_net::new(w5500_device, net_config, stack_resources, random_seed);

    spawner.spawn(net_task(stack_runner)).unwrap();
    spawner.spawn(http::http_task(stack, spawner)).unwrap();
    spawner.spawn(mdns::mdns_task(stack)).unwrap();
    spawner.spawn(bacnet_ip::bacnet_ip_task(stack)).unwrap();
    spawner.spawn(ntp::ntp_task(stack)).unwrap();
    spawner.spawn(snmp::snmp_task(stack)).unwrap();
    spawner.spawn(mqtt::mqtt_task(stack)).unwrap();

    // ---- Core 1: MS/TP master (C) ----
    core1::launch_core1(
        p.CORE1,
        bridge_config.bacnet.mstp_baud,
        bridge_config.bacnet.mstp_mac,
        bridge_config.bacnet.max_master,
    );

    // LED is controlled by Core 1 (C code) — flashes on Who-Is broadcast.
    // Core 0 just keeps it on as a "running" indicator, Core 1 toggles it.
    info!("startup complete");
    led.set_high();
    // Main task has nothing else to do — sleep forever.
    loop {
        Timer::after_millis(60_000).await;
    }
}

// ---------------------------------------------------------------------------
// Background tasks
// ---------------------------------------------------------------------------

#[embassy_executor::task]
async fn w5500_task(
    runner: embassy_net_wiznet::Runner<
        'static,
        W5500,
        ExclusiveDevice<
            Spi<'static, embassy_rp::peripherals::SPI0, embassy_rp::spi::Async>,
            Output<'static>,
            embassy_time::Delay,
        >,
        embassy_rp::gpio::Input<'static>,
        Output<'static>,
    >,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(
    mut runner: embassy_net::Runner<'static, embassy_net_wiznet::Device<'static>>,
) -> ! {
    runner.run().await
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert a subnet mask (e.g. [255,255,255,0]) to a CIDR prefix length.
fn subnet_mask_to_prefix(mask: [u8; 4]) -> u8 {
    let raw = u32::from_be_bytes(mask);
    raw.leading_ones() as u8
}

/// Copy firmware from staging to slot 0, then reboot.
///
/// This function runs from SRAM (.time_critical) with all interrupts disabled.
/// It reads each sector from the staging area via XIP, then uses embassy-rp's
/// Reboot into the OTA staging area using `REBOOT_TYPE_FLASH_UPDATE`.
///
/// The RP2350 bootrom natively supports booting from a different flash
/// region via the `flash_update_boot_window_base` parameter. No manual
/// copy from staging to slot 0 is needed — the bootrom remaps the
/// staging area to appear at the firmware's link address (0x10000000).
///
/// This function NEVER RETURNS.
pub fn ota_copy_and_reboot() -> ! {
    // REBOOT_TYPE_FLASH_UPDATE = 0x4, NO_RETURN_ON_SUCCESS = 0x100
    // p0 = XIP address of the staging area
    let staging_xip = 0x10000000u32 + platform::STAGING_OFFSET;
    info!("ota: rebooting into staging at {:#x}", staging_xip);
    embassy_rp::rom_data::reboot(0x104, 500, staging_xip, 0);
    loop {
        cortex_m::asm::wfi();
    }
}

/// Trigger a full system reset via the RP2350 ROM reboot function.
///
/// Uses the bootrom's `reboot()` API which properly resets all peripherals
/// and re-enters the boot sequence, re-validating the flash image.
/// This is the correct reboot path after OTA firmware updates.
pub fn system_reset() -> ! {
    // REBOOT_TYPE_FLASH_UPDATE = 0x4, NO_RETURN_ON_SUCCESS = 0x100
    // Tells the bootrom that flash has been updated and to re-validate the image.
    embassy_rp::rom_data::reboot(0x104, 500, 0, 0);

    // Should never reach here.
    loop {
        cortex_m::asm::wfi();
    }
}
