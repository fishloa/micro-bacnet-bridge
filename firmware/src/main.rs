#![no_std]
#![no_main]

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
mod snmp;
mod sse;
mod syslog;
mod web_assets;

use defmt::info;
use embassy_executor::Spawner;
use embassy_net::{Config as NetConfig, Stack, StackResources};
use embassy_net_wiznet::chip::W5500;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::spi::{Config as SpiConfig, Spi};
use embassy_time::Timer;
use embedded_hal_bus::spi::ExclusiveDevice;
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

// ---------------------------------------------------------------------------
// Static allocations for embassy-net stack resources
// ---------------------------------------------------------------------------

/// Number of sockets the network stack can hold simultaneously.
/// HTTP (1) + mDNS (1) + BACnet/IP (1) + DHCP internal (1) + NTP (1) +
/// SNMP (1) + MQTT/TCP (1) + DNS/UDP (1) + Syslog/UDP (1) + spare (1) = 10
const SOCKET_COUNT: usize = 10;

static STACK_RESOURCES: StaticCell<StackResources<SOCKET_COUNT>> = StaticCell::new();
static WIZNET_STATE: StaticCell<embassy_net_wiznet::State<4, 4>> = StaticCell::new();

// ---------------------------------------------------------------------------
// Embassy entry point (Core 0)
// ---------------------------------------------------------------------------

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    info!(
        "micro-bacnet-bridge starting (Icomb Place firmware v{})",
        env!("FIRMWARE_VERSION")
    );

    let p = embassy_rp::init(Default::default());

    // ---- GPIO: LED heartbeat ----
    let mut led = Output::new(p.PIN_25, Level::Low);

    // ---- Flash + config (before W5500, we need the MAC address) ----
    let flash =
        embassy_rp::flash::Flash::<_, embassy_rp::flash::Async, { config::FLASH_SIZE }>::new(
            p.FLASH, p.DMA_CH2,
        );
    let mut cfg_mgr = config::ConfigManager::new(flash);
    let mut bridge_config = cfg_mgr.load();

    // MAC address: persisted in config. Generated from ROSC entropy on first boot.
    let mac_addr = if bridge_config.mac_addr == [0u8; 6] {
        let seed = rosc_random_seed();
        let mac = [
            0x02, // locally administered, unicast
            (seed >> 8) as u8,
            (seed >> 16) as u8,
            (seed >> 24) as u8,
            (seed >> 32) as u8,
            (seed >> 40) as u8,
        ];
        bridge_config.mac_addr = mac;
        // Persist so MAC is stable across reboots
        cfg_mgr.save(&bridge_config);
        info!(
            "first boot: generated MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        );
        mac
    } else {
        bridge_config.mac_addr
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

    // Hand flash to OTA subsystem
    {
        let mut flash_guard = ota::FLASH.lock().await;
        *flash_guard = Some(cfg_mgr.into_flash());
    }

    // ---- SPI0 for W5500 ----
    let mut spi_cfg = SpiConfig::default();
    spi_cfg.frequency = 40_000_000;

    let spi_bus = Spi::new(
        p.SPI0, p.PIN_18, p.PIN_19, p.PIN_16, p.DMA_CH0, p.DMA_CH1, spi_cfg,
    );

    let cs = Output::new(p.PIN_17, Level::High);
    let spi_dev = ExclusiveDevice::new_no_delay(spi_bus, cs).unwrap();

    let w5500_int = embassy_rp::gpio::Input::new(p.PIN_21, embassy_rp::gpio::Pull::Up);
    let w5500_rst = Output::new(p.PIN_20, Level::High);

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

    // Generate a random seed from the ROSC frequency counter
    let random_seed = rosc_random_seed();

    // STACK_RESOURCES.init() returns &'static mut StackResources, so the
    // Stack and Runner returned by embassy_net::new() are already Stack<'static>
    // and Runner<'static, _> — no unsafe transmute needed.
    let stack_resources: &'static mut _ = STACK_RESOURCES.init(StackResources::new());
    let (stack, stack_runner): (Stack<'static>, _) =
        embassy_net::new(w5500_device, net_config, stack_resources, random_seed);

    spawner.spawn(net_task(stack_runner)).unwrap();
    spawner.spawn(http::http_task(stack)).unwrap();
    spawner.spawn(mdns::mdns_task(stack)).unwrap();
    spawner.spawn(bacnet_ip::bacnet_ip_task(stack)).unwrap();
    spawner.spawn(ntp::ntp_task(stack)).unwrap();
    spawner.spawn(snmp::snmp_task(stack)).unwrap();
    spawner.spawn(mqtt::mqtt_task(stack)).unwrap();

    // ---- Core 1: MS/TP master (C) ----
    core1::launch_core1(p.CORE1);

    // ---- LED heartbeat ----
    info!("startup complete; heartbeat running");
    loop {
        led.set_high();
        Timer::after_millis(100).await;
        led.set_low();
        Timer::after_millis(900).await;
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
            embedded_hal_bus::spi::NoDelay,
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

/// Derive a random u64 seed from chip-specific entropy sources.
///
/// This is not cryptographically strong but provides enough entropy for
/// the network stack's ephemeral port selection.
fn rosc_random_seed() -> u64 {
    #[cfg(feature = "board-pico")]
    {
        // RP2040: ROSC base = 0x40060000; RANDOMBIT register offset = 0x1C.
        // Bit 31 of the RANDOMBIT register is the random bit.
        const RANDOMBIT_ADDR: *const u32 = 0x4006_001C as *const u32;
        let mut seed: u64 = 0;
        for i in 0..64u64 {
            let val = unsafe { core::ptr::read_volatile(RANDOMBIT_ADDR) };
            let bit = ((val >> 31) & 1) as u64;
            seed |= bit << i;
            for _ in 0..16 {
                cortex_m::asm::nop();
            }
        }
        seed ^ 0xDEAD_BEEF_CAFE_1234
    }

    #[cfg(feature = "board-pico2")]
    {
        // RP2350: TRNG base = 0x4012_0000; RNG_DATA register offset = 0x204.
        // The RP2350 has a dedicated hardware TRNG (Arm TrustZone RNG IP).
        // Reading RNG_DATA yields 32 bits of entropy per read.
        const RNG_DATA_ADDR: *const u32 = 0x4012_0204 as *const u32;
        let lo = unsafe { core::ptr::read_volatile(RNG_DATA_ADDR) } as u64;
        let hi = unsafe { core::ptr::read_volatile(RNG_DATA_ADDR) } as u64;
        (hi << 32) | lo
    }
}

/// Convert a subnet mask (e.g. [255,255,255,0]) to a CIDR prefix length.
fn subnet_mask_to_prefix(mask: [u8; 4]) -> u8 {
    let raw = u32::from_be_bytes(mask);
    raw.leading_ones() as u8
}
