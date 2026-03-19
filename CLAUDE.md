You are building production firmware for a BACnet MS/TP ↔ BACnet/IP bridge running on a
WIZnet W5500-EVB-Pico2 (RP2350A + W5500 hardwired TCP/IP). The project is at
https://github.com/fishloa/micro-bacnet-bridge. Begin by reading the entire repo, then
plan before writing any code.

## Hardware

### Target board: WIZnet W5500-EVB-Pico2
- RP2350A dual-core Cortex-M33 @ 150MHz, 520KB SRAM, 4MB flash
- W5500 hardwired TCP/IP via SPI0 (GPIO16–21, internal)
- SP3485 RS-485 transceiver on UART1 (GPIO4=TX, GPIO5=RX, GPIO3=DE/RE direction)
- WIZPoE-P1 PoE module, 802.3af, powers board via RJ45
- No OS. **Hybrid Rust + C firmware:**
  - Core 0: Rust (embassy-rs) — networking, HTTP, BACnet/IP, mDNS, DHCP, bridge logic
  - Core 1: C (bacnet-stack) — MS/TP master state machine (timing critical)

### Debug probe: WIZnet W5500-EVB-Pico-PoE (RP2040)
- Flashed with debugprobe firmware, connected to Mac via USB
- SWD wiring: probe GPIO2→target SWCLK pad, probe GPIO3→target SWDIO pad, GND→GND
- Optional serial passthrough: probe GPIO5 (RX)←target GPIO0 (TX), probe GPIO4 (TX)→target GPIO1 (RX)
- Use `probe-rs run --chip RP2350` or `probe-rs attach --chip RP2350` for flash/debug

## Toolchain & build environment

All build tools run on the CI runner (Ubuntu), never on the device:
- Rust (thumbv8m.main-none-eabihf target) + embassy-rs: Core 0 firmware
- arm-none-eabi-gcc: compiles C bacnet-stack (linked into Rust binary via `cc` crate)
- Cargo: builds Rust + links C static library → ELF → uf2 via elf2uf2-rs
- Bun + SvelteKit: compiles frontend to static HTML/CSS/JS (use bun everywhere
  npm would otherwise be used — bun install, bun run build, bunx svelte-kit etc)
- Python: embeds gzip'd frontend assets for Rust `include_bytes!`
- bacpypes3 (Python): runs simulated BACnet devices for integration testing
The RP2350A runs only the compiled firmware binary. No runtime interpreters.

### Key Rust crates
- `embassy-rp` — RP2350A HAL (SPI, UART, GPIO, Flash, multicore, timers)
- `embassy-net` + `embassy-net-wiznet` — TCP/IP stack + W5500 MACRAW driver
- `picoserve` — HTTP server with routing, JSON, SSE, static file serving (no_std)
- `serde` + `serde_json_core` — JSON serialization (no_std, no alloc)
- `heapless` — fixed-size collections (no alloc)

## Vendor identity

- Vendor name: Icomb Place
- Vendor string used in: BACnet vendor identifier field, mDNS TXT records,
  HTTP Server header, OpenAPI info block, README, all user-facing strings
- BACnet vendor ID: use 0xFFFF (unregistered) unless/until a real ID is assigned

## Functional requirements

### 1. Networking
- DHCP on boot, fallback to stored static IP if DHCP fails within 10s
- Respond to ICMP ping
- Static IP, subnet, gateway, DNS configurable via admin UI and persisted to flash

### 2. mDNS / Bonjour device discovery
- Implement mDNS responder in C using W5500 UDP multicast (224.0.0.251:5353)
- Advertise hostname as {device-name}.local (configurable, default: bacnet-bridge.local)
- Advertise two mDNS service records:
  - _http._tcp.local — port 80, for admin UI discovery
  - _bacnet._udp.local — port 47808, for BACnet/IP discovery
- TXT records on _bacnet service:
  - deviceId={bacnet device ID}
  - vendor=Icomb Place
  - version={firmware version}
- Respond to mDNS queries for own hostname and service types
- Re-announce on IP change (DHCP renew or static IP change)
- DNS-SD: respond to _services._dns-sd._udp.local PTR queries
- Implement minimal mDNS in firmware/src/mdns.c — no third-party library required,
  the protocol is simple enough (DNS packet format over multicast UDP)

### 3. BACnet bridge (bidirectional, all object types)
- Core 1 runs MS/TP master state machine
  - Configurable MAC address (0–127), baud rate (9600/19200/38400/76800)
  - Auto-discovers all devices on the serial bus via Who-Is on startup
  - Forwards all confirmed/unconfirmed services between MS/TP and BACnet/IP
  - Handles COV subscriptions transparently (translate subscriber addresses)
  - Supports: ReadProperty, WriteProperty, ReadPropertyMultiple, SubscribeCOV,
    SubscribeCOVProperty, Who-Is, I-Am, Who-Has, I-Have, TimeSynchronization
- BACnet/IP on UDP port 47808
  - Exposes bridge as a BACnet device (configurable Device ID, device name)
  - BBMD support (configurable BDT, optional registration as foreign device)

### 4. Config persistence
- Struct stored in last flash sector using Pico SDK hardware/flash API
- Magic number for validity check, versioned schema
- Fields: IP config, hostname, BACnet device ID/name, MS/TP MAC/baud, admin
  credentials hash, user list, point mappings/aliases/ignored flags

### 5. HTTP admin server (port 80)
- Served from flash as C arrays (no SD card, no filesystem)
- Frontend: SvelteKit, compiled to static assets via bun, embedded in firmware
  as C arrays via embed_assets.py (xxd or similar)
- **Design system: Verdant UI** — the Icomb Place design system at
  https://icomb.place/design-system/. The SvelteKit app.html must link the
  CDN-hosted stylesheets so all colours, typography, glass effects, and
  component classes come from the shared design system:
  ```html
  <link rel="stylesheet" href="https://icomb.place/design-system/verdant-tokens.css">
  <link rel="stylesheet" href="https://icomb.place/design-system/verdant-base.css">
  ```
  Use `vui-*` CSS classes (vui-btn, vui-card, vui-glass, vui-badge, vui-input,
  vui-alert, vui-dropdown, vui-section-header, vui-checkbox, etc.) and `--vui-*`
  CSS custom properties for all styling. The design system source lives at
  `../icomb-place-design-system/`. See global CLAUDE.md for styling rules.
- Auth: session cookie, bcrypt-hashed passwords, configurable users with roles
  (admin: full access, viewer: read-only)
- Password reset: if no users configured, allow setup on first access

### 5a. Network config page
- Set static IP / subnet / gateway / DNS or enable DHCP
- Set device hostname (updates mDNS advertisement immediately)
- Save to flash, reboot to apply

### 5b. BACnet points dashboard (main panel)
- Realtime display matching the layout in the reference screenshot:
  - Left sidebar: device list with device ID, name, network address
  - Right panel: point list for selected device showing:
    name, object type badge (AI/AO/AV/BI/BO/BV/MSI/MSO/MSV/NC/TL etc),
    description, present value, units, writable indicator
  - Filter bar (supports regex), tabs: Analog / Binary / Input / Output / Control loop
  - Visible objects count, selected count
  - Write button per writable point, opens inline editor
- SSE endpoint: /api/events streams JSON point value updates at configurable poll
  interval (default 1s), only changed values
- On device selection: fetch full point list, then subscribe to SSE for live values

### 5c. BACnet point configuration
- Per-point settings: display name override, description, unit label, ignore flag
- Persisted to flash

### 5d. Bridge / device config
- BACnet device ID, device name, vendor (Icomb Place), MS/TP MAC, MS/TP baud,
  max master
- BBMD table editor
- Hostname (synced to mDNS)
- All saved to flash

### 6. REST/OpenAPI interface
- Base path: /api/v1
- OpenAPI 3.1 spec served at /api/openapi.json, info.contact.name = "Icomb Place"
- Swagger UI served at /api/docs
- Endpoints:
  - GET  /devices              — list discovered BACnet devices
  - GET  /devices/{id}/points  — list all points for a device
  - GET  /devices/{id}/points/{obj} — read a point
  - PUT  /devices/{id}/points/{obj} — write a point (auth required)
  - GET  /config/network       — get network config
  - PUT  /config/network       — set network config
  - GET  /config/bacnet        — get BACnet config
  - PUT  /config/bacnet        — set BACnet config
  - GET  /config/mdns          — get mDNS/hostname config
  - PUT  /config/mdns          — set hostname, toggle mDNS on/off
  - GET  /config/points        — get all point mappings
  - PUT  /config/points/{obj}  — update point mapping
  - POST /auth/login           — get session token
  - POST /auth/logout
  - GET  /users                — list users (admin only)
  - POST /users                — create user (admin only)
  - DELETE /users/{id}         — delete user (admin only)
  - GET  /system/status        — uptime, IP, DHCP state, MS/TP stats, mDNS hostname
  - POST /system/reboot        — reboot device

## Repository structure

```
micro-bacnet-bridge/
├── Cargo.toml                  # Rust workspace root
├── build.rs                    # cc crate: compile bacnet-stack → libbacknet.a
├── .cargo/
│   └── config.toml             # thumbv8m.main-none-eabihf target, elf2uf2-rs runner
├── src/                        # Rust firmware (Core 0)
│   ├── main.rs                 # Embassy entry, spawn async tasks
│   ├── bacnet_ffi.rs           # FFI bindings to C bacnet-stack
│   ├── bridge.rs               # PDU routing: MS/TP ↔ BACnet/IP
│   ├── bacnet_ip.rs            # BACnet/IP (embassy-net UDP)
│   ├── http.rs                 # picoserve router: REST API + static assets
│   ├── sse.rs                  # SSE endpoint for live point updates
│   ├── mdns.rs                 # mDNS responder (embassy-net UDP multicast)
│   ├── config.rs               # Flash persistence (embassy-rp Flash)
│   ├── auth.rs                 # Session cookies, bcrypt, user roles
│   ├── web_assets.rs           # include_bytes! of gzip'd SvelteKit build
│   ├── ipc.rs                  # Inter-core ring buffer + spinlock
│   └── core1.rs                # Core 1 launch: calls C core1_entry()
├── csrc/                       # C firmware (Core 1 MS/TP)
│   ├── core1_entry.c           # C entry point for Core 1
│   ├── mstp_port.c             # UART1 init, DE/RE pin, ISR
│   ├── bacnet_port.c           # bacnet-stack platform hooks
│   └── ipc_c.c                 # C side of shared ring buffer
├── lib/
│   └── bacnet-stack/           # git submodule
├── frontend/                   # SvelteKit admin UI
│   ├── src/
│   │   ├── routes/
│   │   │   ├── +page.svelte
│   │   │   ├── config/+page.svelte
│   │   │   └── users/+page.svelte
│   │   └── lib/
│   │       ├── DeviceList.svelte
│   │       ├── PointsPanel.svelte
│   │       └── api.ts
│   ├── bun.lockb
│   └── package.json
├── tests/
│   ├── unit/                   # Rust unit tests + C unit tests (Unity)
│   ├── integration/            # Python/bacpypes3 tests
│   └── sim/                    # BACnet device simulator for CI
├── tools/
│   └── embed_assets.py         # gzip SvelteKit build → assets/*.gz
├── .github/
│   └── workflows/
│       ├── build.yml
│       ├── release.yml
│       └── pages.yml
├── docs/
│   └── openapi.yaml
└── README.md
```

## CI/CD (GitHub Actions)

All steps run on ubuntu-latest. Use bun for frontend, cargo for firmware.

### build.yml (on every push/PR)
1. Install Rust toolchain + thumbv8m.main-none-eabihf target + elf2uf2-rs
2. Install arm-none-eabi-gcc (for C bacnet-stack compilation via cc crate)
3. Install bun (use oven-sh/setup-bun@v2 action)
4. bun install && bun run build in frontend/ (produces static assets)
5. python embed_assets.py (gzip assets → assets/*.gz for include_bytes!)
6. cargo build --release (target from .cargo/config.toml: thumbv8m.main-none-eabihf)
7. Run Rust unit tests (cargo test -p bridge-core, host target)
8. Run bacpypes3 integration tests against BACnet simulator
9. Upload .elf + .uf2 as build artifacts

### release.yml (on tag v*)
1. Full build as above
2. Create GitHub Release
3. Attach micro-bacnet-bridge-{version}.uf2 to release
4. Generate changelog from commits since last tag

### pages.yml
1. Build Redoc HTML from openapi.yaml
2. Deploy to GitHub Pages at /docs

## Testing

Unit tests (Rust `cargo test` + C/Unity, host-compiled):
- MS/TP frame encode/decode (C/Unity)
- BACnet APDU encode/decode (Rust + C)
- mDNS packet encode/decode (Rust)
- Config struct serialization (Rust)
- Inter-core ring buffer logic (Rust)
- Auth session management (Rust)

Integration tests (Python/bacpypes3, on CI runner):
- Spin up simulated BACnet/IP device with known points
- Verify Who-Is → I-Am response
- Verify ReadProperty round-trip through bridge
- Verify WriteProperty propagation
- Verify COV subscription and notification forwarding
- Verify REST API responses match BACnet state
- Verify SSE stream delivers value changes
- Verify mDNS hostname resolution (using avahi-browse in CI)
- Verify _http._tcp and _bacnet._udp service records advertised correctly

## Code quality

- `cargo clippy` + `cargo fmt` enforced in CI (Rust)
- `clang-format` enforced for C files in csrc/
- No dynamic allocation: Rust `#![no_std]` with `heapless`, C all-static
- Rust public APIs documented with `///` doc comments, C with Doxygen
- README: hardware setup, wiring diagram (RP2350A ↔ SP3485 pins),
  build instructions, flash instructions, first-boot setup,
  mDNS discovery (open browser to http://bacnet-bridge.local),
  API reference link, vendor credit (Icomb Place)

## Constraints

- Total flash budget: firmware ≤ 3MB, web assets ≤ 400KB gzip compressed (4MB flash total)
- RAM budget: leave 128KB headroom, document static allocation map (520KB SRAM total)
- No third-party cloud dependencies — fully local
- All credentials stored as bcrypt hashes, never plaintext
- HTTP only (no TLS) — device is on trusted LAN
- mDNS multicast via embassy-net (smoltcp) over W5500 MACRAW mode —
  multicast filtering in software, no hardware socket limit concern

## Starting point

1. Read the full repo at https://github.com/fishloa/micro-bacnet-bridge
2. Create PLAN.md covering:
   - Architecture decisions and rationale
   - Module dependency graph
   - Memory allocation map
   - Build order (what to implement first to unblock testing)
   - Risk areas (W5500 multicast for mDNS, flash budget for web assets,
     bun lockfile compatibility with CI)
3. Get explicit approval before writing any code
4. Implement in phases, running tests at each phase before proceeding
