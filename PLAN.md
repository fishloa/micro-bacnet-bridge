# micro-bacnet-bridge — Implementation Plan

**Approach: Rust (embassy-rs) + C bacnet-stack via FFI**

Rust on Core 0 for networking, HTTP, mDNS, bridge logic, config.
C bacnet-stack on Core 1 for the timing-critical MS/TP state machine.
BACnet/IP datalink bridged through Rust FFI to the C bacnet-stack NPDU/APDU layer.

---

## 1. Architecture Overview

```
┌──────────────────────────── RP2040 ────────────────────────────┐
│                                                                 │
│  Core 0 (Rust / embassy-rs)          Core 1 (C / bacnet-stack) │
│  ┌──────────────────────┐            ┌───────────────────┐     │
│  │  #[embassy_executor]  │  shared    │  core1_entry()    │     │
│  │                       │  memory    │  MS/TP Master FSM │     │
│  │  ┌─────────────────┐ │  ring buf  │  (bacnet-stack)   │     │
│  │  │ embassy-net      │ │◄─────────►│  UART1 + SP3485   │     │
│  │  │ w5500 driver     │ │  +spinlock │  DE/RE control    │     │
│  │  │ ┌─────────────┐ │ │            └───────────────────┘     │
│  │  │ │ BACnet/IP   │ │ │                                      │
│  │  │ │ UDP:47808   │ │ │                                      │
│  │  │ └─────────────┘ │ │                                      │
│  │  │ ┌─────────────┐ │ │                                      │
│  │  │ │ picoserve   │ │ │                                      │
│  │  │ │ HTTP :80    │ │ │                                      │
│  │  │ │ REST + SSE  │ │ │                                      │
│  │  │ └─────────────┘ │ │                                      │
│  │  │ ┌─────────────┐ │ │                                      │
│  │  │ │ mDNS :5353  │ │ │                                      │
│  │  │ │ (multicast) │ │ │                                      │
│  │  │ └─────────────┘ │ │                                      │
│  │  │ DHCP (embassy)  │ │                                      │
│  │  └─────────────────┘ │                                      │
│  │  bridge.rs            │                                      │
│  │  config.rs (flash)    │                                      │
│  │  auth.rs              │                                      │
│  │  bacnet_ffi.rs        │                                      │
│  └──────────────────────┘                                      │
│                                                                 │
│  SPI0 (GPIO16-21) ──► W5500 ──► RJ45 (PoE)                   │
│  UART1 (GPIO4/5, GPIO3 DE) ──► SP3485 ──► RS-485 bus         │
└────────────────────────────────────────────────────────────────┘
```

### Key Decisions

1. **Language split by core.** Core 1 runs C (bacnet-stack MS/TP master FSM) — this
   is timing-critical code with a 20-year-proven implementation. Core 0 runs Rust
   with embassy-rs async runtime for everything else.

2. **Why hybrid, not all-C or all-Rust.**
   - No mature Rust BACnet stack exists (bacnet-rs v0.2 has no MS/TP state machine).
   - picoserve gives us HTTP routing, SSE, JSON, static files for free — replacing
     ~1500 lines of hand-written C HTTP parser.
   - embassy async/await is cleaner than manual C poll loops for concurrent networking.
   - Memory safety on inter-core shared buffers — the #1 C bug source.
   - The C bacnet-stack compiles as a static library and links via `cc` crate.

3. **Inter-core communication.** Shared memory ring buffers in static SRAM, protected
   by RP2040 hardware spinlocks accessed via `critical-section` crate. BACnet PDUs
   flow between cores through these buffers. RP2040 FIFO used for wake signalling.

4. **No RTOS.** Embassy async executor on Core 0 (cooperative, single-threaded).
   Bare-metal loop on Core 1 (C bacnet-stack MS/TP FSM, interrupt-driven UART).

5. **SPI bus.** W5500-EVB-Pico-PoE uses **SPI0** on GPIO16-21. Only accessed from
   Core 0 (Rust). Core 1 never touches SPI.

6. **No dynamic allocation.** Rust side: `#![no_std]`, `heapless` collections,
   static buffers. C side: all static, no malloc after init.

7. **Design system.** Frontend uses Verdant UI from `https://icomb.place/design-system/`
   via CDN `<link>` tags. All UI styled with `vui-*` classes.

---

## 2. Technology Stack

### Rust (Core 0)

| Crate | Version | Purpose |
|-------|---------|---------|
| `embassy-rp` | latest | RP2040 HAL: SPI, UART, GPIO, Flash, multicore, timers |
| `embassy-executor` | latest | Async task executor |
| `embassy-net` | latest | TCP/IP stack (smoltcp-based) |
| `embassy-net-wiznet` | latest | W5500 MACRAW driver for embassy-net |
| `embassy-time` | latest | Async delays, timeouts (1μs resolution) |
| `embassy-sync` | latest | Async channels, mutexes, signals |
| `picoserve` | latest | HTTP server: routing, JSON, SSE, static files |
| `serde` + `serde_json_core` | latest | JSON serialization (no_std, no alloc) |
| `heapless` | 0.8 | Fixed-size Vec, String, HashMap |
| `critical-section` | latest | Multicore-safe critical sections |
| `static_cell` | latest | Safe static initialization |
| `embedded-io-async` | latest | Async I/O traits |

### C (Core 1 + FFI library)

| Library | Purpose |
|---------|---------|
| bacnet-stack (submodule) | MS/TP FSM, NPDU/APDU codec, BACnet services, object model |
| Pico SDK (minimal) | UART1 driver, GPIO, hardware timer (for Core 1 only) |

### Frontend

| Tool | Purpose |
|------|---------|
| SvelteKit + bun | Static site build |
| Verdant UI (CDN) | Design system — colours, components, glass effects |
| rust-embed or include_bytes! | Embed gzip'd assets into firmware binary |

---

## 3. Module Dependency Graph

```
firmware/
├── Cargo.toml                  ← workspace root
├── build.rs                    ← cc crate: compile bacnet-stack → libbacknet.a
├── .cargo/config.toml          ← target = thumbv6m-none-eabi, linker config
├── src/
│   ├── main.rs                 ← embassy entry, Core 0 init, spawn tasks
│   ├── bacnet_ffi.rs           ← unsafe FFI bindings to C bacnet-stack
│   ├── bridge.rs               ← PDU routing: MS/TP ↔ BACnet/IP
│   ├── bacnet_ip.rs            ← BACnet/IP task (embassy-net UDP socket)
│   ├── http.rs                 ← picoserve router: REST API + static assets
│   ├── sse.rs                  ← SSE endpoint for live point updates
│   ├── mdns.rs                 ← mDNS responder (embassy-net UDP multicast)
│   ├── config.rs               ← Flash persistence (embassy-rp Flash)
│   ├── auth.rs                 ← Session cookies, bcrypt, user roles
│   ├── web_assets.rs           ← include_bytes! of gzip'd SvelteKit build
│   ├── ipc.rs                  ← Inter-core ring buffer + spinlock API
│   └── core1.rs                ← Core 1 launch: calls into C bacnet-stack
├── csrc/
│   ├── core1_entry.c           ← C entry point for Core 1
│   ├── mstp_port.c             ← UART1 init, DE/RE pin, ISR for MS/TP
│   ├── bacnet_port.c           ← bacnet-stack platform hooks (timers, etc.)
│   └── ipc_c.c                 ← C side of shared ring buffer API
├── lib/
│   └── bacnet-stack/           ← git submodule
├── frontend/                   ← SvelteKit app (builds to static assets)
│   ├── src/routes/
│   ├── src/lib/
│   ├── package.json
│   └── bun.lockb
└── tools/
    └── embed_assets.py         ← gzip SvelteKit build → Rust include_bytes
```

### Build Flow

```
bun run build (frontend/)
    ↓
embed_assets.py → firmware/assets/*.gz
    ↓
cargo build --release --target thumbv6m-none-eabi
    ├── build.rs: cc crate compiles csrc/*.c + bacnet-stack → libbacknet.a
    ├── rustc compiles src/*.rs, links libbacknet.a
    └── produces ELF
    ↓
elf2uf2-rs → micro-bacnet-bridge.uf2
```

---

## 4. W5500 Socket Allocation

With embassy-net-wiznet, the W5500 operates in MACRAW mode (single socket for
all Ethernet frames). embassy-net's smoltcp stack handles TCP/UDP multiplexing
in software. This **eliminates the 8-socket hardware limit** — we get unlimited
logical sockets limited only by RAM for socket buffers.

| Logical socket | Protocol | Purpose | Buffer |
|----------------|----------|---------|--------|
| BACnet/IP | UDP :47808 | BACnet/IP PDUs | 2 KB RX, 2 KB TX |
| mDNS | UDP :5353 | Multicast responder | 1 KB RX, 1 KB TX |
| HTTP 1 | TCP :80 | picoserve connection | 4 KB RX, 4 KB TX |
| HTTP 2 | TCP :80 | picoserve connection | 4 KB RX, 4 KB TX |
| HTTP 3 | TCP :80 | picoserve connection | 4 KB RX, 4 KB TX |
| SSE 1 | TCP :80 | Persistent SSE stream | 1 KB RX, 2 KB TX |
| SSE 2 | TCP :80 | Persistent SSE stream | 1 KB RX, 2 KB TX |
| DHCP | UDP :68 | embassy-net internal | shared |
| **Total** | | | **~32 KB** |

**Key advantage over all-C approach:** No socket juggling. embassy-net handles
connection multiplexing over the single MACRAW socket. DHCP is handled internally
by embassy-net. More concurrent connections possible.

---

## 5. Memory Allocation Map

### Flash (2048 KB)

| Region | Offset | Size | Contents |
|--------|--------|------|----------|
| Boot2 | 0x000 | 256 B | RP2040 second-stage bootloader |
| Firmware | 0x100 | ≤ 1.5 MB | XIP: Rust binary + linked C bacnet-stack |
| Web assets | embedded | ≤ 400 KB | Gzip'd SvelteKit build via include_bytes! |
| Config | 0x1FF000 | 4 KB | Last sector: persistent config struct |

**Estimated firmware size:** 250-400 KB (Rust + C + web assets, with LTO).
Still well within 1.5 MB even accounting for Rust's larger binaries.

### SRAM (264 KB = 4×64 KB + 2×4 KB scratch)

| Component | Size | Notes |
|-----------|------|-------|
| Core 0 stack | 8 KB | Embassy executor + async tasks |
| Core 1 stack | 4 KB | Scratch bank Y (C MS/TP loop) |
| embassy-net smoltcp buffers | ~16 KB | TCP/UDP socket buffers (see table above) |
| BACnet objects table | ~16 KB | `heapless::Vec<BacnetPoint, 256>` |
| Inter-core ring buffers (×2) | 16 KB | 8 KB each direction, spinlock-protected |
| C bacnet-stack static buffers | ~8 KB | MS/TP frames, APDU encode/decode |
| picoserve buffers | ~4 KB | HTTP request/response (per-connection) |
| JSON serialization buffer | 4 KB | `heapless::String<4096>` |
| mDNS packet buffer | 1 KB | Single query/response |
| Config struct (RAM copy) | 4 KB | Deserialized from flash on boot |
| Auth session table | 1 KB | `heapless::Vec<Session, 8>` |
| COV subscription table | 4 KB | `heapless::Vec<CovSub, 32>` |
| **Subtotal** | **~90 KB** | |
| **Headroom** | **~174 KB** | Exceeds 64 KB requirement |

---

## 6. FFI Boundary Design

The C bacnet-stack runs on Core 1. Rust on Core 0 communicates via a well-defined
shared memory interface. **No FFI function calls cross cores at runtime** — only
data flows through ring buffers.

### Shared Data Structures (defined in both Rust and C)

```rust
// ipc.rs — Rust side
#[repr(C)]
pub struct BacnetPdu {
    pub source_net: u16,       // source network number
    pub source_mac: [u8; 7],   // source MAC address
    pub source_mac_len: u8,
    pub dest_net: u16,         // destination network number
    pub dest_mac: [u8; 7],     // destination MAC address
    pub dest_mac_len: u8,
    pub pdu_type: u8,          // APDU type
    pub data_len: u16,
    pub data: [u8; 480],       // max APDU size for MS/TP
}

// Ring buffer: Core 1 → Core 0 (MS/TP received PDUs)
// Ring buffer: Core 0 → Core 1 (PDUs to send on MS/TP)
```

### What the C code does (Core 1 only)

- Runs MS/TP master state machine (token passing, frame TX/RX)
- Decodes received MS/TP frames → extracts NPDU → places in ring buffer
- Reads ring buffer for outbound PDUs → encodes MS/TP frame → transmits
- Handles UART1 ISR for byte-level MS/TP timing
- Calls `multicore_lockout_victim_init()` at startup

### What the Rust code does (Core 0 only)

- BACnet/IP: receives UDP PDUs, extracts NPDU, routes to bridge logic
- Bridge: reads PDUs from Core 1 ring buffer, forwards to BACnet/IP (and vice versa)
- APDU decode/encode for REST API integration (read/write points)
- All HTTP, SSE, mDNS, DHCP, config, auth

### Build-time FFI (build.rs)

```rust
// build.rs
fn main() {
    cc::Build::new()
        .compiler("arm-none-eabi-gcc")
        .target("thumbv6m-none-eabi")
        .files(&[
            "csrc/core1_entry.c",
            "csrc/mstp_port.c",
            "csrc/bacnet_port.c",
            "csrc/ipc_c.c",
        ])
        // bacnet-stack sources (selected subset)
        .file("lib/bacnet-stack/src/bacnet/datalink/mstp.c")
        .file("lib/bacnet-stack/src/bacnet/datalink/crc.c")
        .file("lib/bacnet-stack/src/bacnet/npdu.c")
        // ... other needed bacnet-stack files
        .include("lib/bacnet-stack/src")
        .include("csrc")
        .flag("-mcpu=cortex-m0plus")
        .flag("-mthumb")
        .flag("-Os")
        .compile("bacnet");
}
```

---

## 7. Build Order (Implementation Phases)

### Phase 0: Project Scaffolding
- [x] CLAUDE.md specification
- [x] PLAN.md (this document)
- [ ] `Cargo.toml` — embassy dependencies, cc build dep, target config
- [ ] `.cargo/config.toml` — thumbv6m-none-eabi target, elf2uf2-rs runner
- [ ] `build.rs` — minimal cc crate setup (compile empty C file to verify toolchain)
- [ ] `.gitignore` — target/, node_modules/, *.uf2, assets/*.gz
- [ ] Directory structure: `src/`, `csrc/`, `lib/`, `frontend/`, `tools/`
- [ ] Add git submodule: bacnet-stack
- [ ] Verify: `cargo build --release` produces .uf2 from empty `main.rs`
- [ ] Verify: C compilation via build.rs works with arm-none-eabi-gcc

### Phase 1: W5500 Networking + DHCP
- [ ] `src/main.rs` — embassy init, SPI0, W5500 driver, embassy-net stack
- [ ] DHCP via embassy-net (built-in, automatic)
- [ ] `src/config.rs` — Flash read/write via embassy-rp, static IP fallback
- [ ] Verify: board gets IP via DHCP, responds to ping
- [ ] **Test:** Unit test config struct serialization (host-compiled)

### Phase 2: HTTP Server + Static Assets
- [ ] `src/http.rs` — picoserve router with basic routes
- [ ] `src/web_assets.rs` — include_bytes! for gzip'd frontend
- [ ] `tools/embed_assets.py` — gzip SvelteKit build → assets/ dir
- [ ] `frontend/` — scaffold SvelteKit project with bun, Verdant UI CDN links
- [ ] Minimal `+page.svelte` — "BACnet Bridge" landing with Verdant UI styling
- [ ] Build pipeline: `bun run build` → `embed_assets.py` → `cargo build`
- [ ] Verify: browse to device IP, see styled page served by picoserve
- [ ] **Test:** Integration test HTTP responses

### Phase 3: mDNS Responder
- [ ] `src/mdns.rs` — embassy-net UDP task on multicast 224.0.0.251:5353
- [ ] DNS packet encode/decode (minimal, ~200 lines)
- [ ] Respond to A queries for `{hostname}.local`
- [ ] Advertise `_http._tcp.local` and `_bacnet._udp.local` services
- [ ] DNS-SD meta-query support (`_services._dns-sd._udp.local`)
- [ ] Re-announce on IP change
- [ ] Verify: `dns-sd -B _http._tcp` discovers device
- [ ] **Test:** Unit test DNS packet encode/decode

### Phase 4: MS/TP State Machine (Core 1, C)
- [ ] `csrc/core1_entry.c` — C entry point, bacnet-stack MS/TP init
- [ ] `csrc/mstp_port.c` — UART1 init, DE/RE GPIO3 toggle, byte-level ISR
- [ ] `csrc/ipc_c.c` — C side of shared ring buffer (read/write PDUs)
- [ ] `src/ipc.rs` — Rust side of ring buffer, spinlock via critical-section
- [ ] `src/core1.rs` — `multicore::spawn_core1()`, calls C `core1_entry()`
- [ ] `build.rs` — compile bacnet-stack MS/TP sources + port files
- [ ] `multicore_lockout_victim_init()` called from C on Core 1
- [ ] Verify: MS/TP token passing on RS-485 bus
- [ ] **Test:** Unit test MS/TP frame encode/decode (C, host-compiled)

### Phase 5: BACnet/IP + Bridge
- [ ] `src/bacnet_ip.rs` — embassy-net UDP socket on port 47808, BVLC handling
- [ ] `src/bacnet_ffi.rs` — FFI to bacnet-stack NPDU/APDU encode/decode
- [ ] `src/bridge.rs` — route PDUs: MS/TP ring buffer ↔ BACnet/IP UDP
- [ ] Device object: bridge's own Device ID, name, vendor (Icomb Place)
- [ ] Who-Is / I-Am forwarding (both directions)
- [ ] ReadProperty / WriteProperty forwarding
- [ ] ReadPropertyMultiple forwarding
- [ ] COV subscription management + notification forwarding
- [ ] Auto-discovery: Who-Is on MS/TP bus at startup, build device table
- [ ] BBMD support (configurable BDT, optional foreign device registration)
- [ ] Verify: BACnet client (YABE) sees MS/TP devices through the bridge
- [ ] **Test:** Integration tests with bacpypes3 simulator

### Phase 6: REST API + Auth
- [ ] `src/auth.rs` — bcrypt hashing (embedded bcrypt crate), session cookies
- [ ] Extend `src/http.rs` picoserve router with all /api/v1/* endpoints
- [ ] `src/sse.rs` — SSE endpoint for live point value streaming
- [ ] First-access setup flow (no users → create admin)
- [ ] OpenAPI spec at `/api/openapi.json`
- [ ] Verify: curl can list devices, read/write points, manage users
- [ ] **Test:** Unit test auth; integration test REST endpoints

### Phase 7: Frontend Dashboard
- [ ] `frontend/src/lib/api.ts` — typed API client
- [ ] `frontend/src/lib/DeviceList.svelte` — device sidebar with vui-card
- [ ] `frontend/src/lib/PointsPanel.svelte` — points table with vui-badge, filters
- [ ] SSE integration (`/api/events`) for live value updates
- [ ] Inline write editor for writable points
- [ ] `frontend/src/routes/config/+page.svelte` — network, BACnet, mDNS config
- [ ] `frontend/src/routes/users/+page.svelte` — user management (admin only)
- [ ] Verify: full dashboard workflow end-to-end

### Phase 8: CI/CD + Release
- [ ] `.github/workflows/build.yml` — install Rust + arm-none-eabi-gcc + bun,
      build frontend, embed assets, cargo build, run tests
- [ ] `.github/workflows/release.yml` — tag-triggered, attach .uf2 to release
- [ ] `.github/workflows/pages.yml` — Redoc API docs → GitHub Pages
- [ ] `docs/openapi.yaml` — complete OpenAPI 3.1 spec
- [ ] `README.md` — hardware setup, wiring, build, flash, first-boot, mDNS

---

## 8. Risk Register

### High Risk

| Risk | Impact | Mitigation |
|------|--------|------------|
| **C FFI + embassy on RP2040 is uncommon** | Few examples of mixed Rust/C on RP2040 with embassy. Build system integration (cc crate + arm-none-eabi-gcc + Pico SDK headers for Core 1) could have unexpected issues. | Phase 0 validates this immediately. Compile minimal C + Rust, verify linking. Use `tana/pico-std-rust` as reference. Fallback: all-C if FFI proves unworkable. |
| **Interrupt conflicts between embassy and C** | embassy-rp auto-binds interrupt handlers. C bacnet-stack on Core 1 needs UART1 ISR. Could get duplicate symbol errors. | Use `bind_interrupts!` macro explicitly. Core 1 C code manages its own interrupts independently (RP2040 interrupts are per-core). Test in Phase 4. |
| **MS/TP timing on Core 1** | At 76800 baud, token response must be < 15ms. XIP cache misses on C code could add jitter. | Link timing-critical C functions to RAM section (`__not_in_flash`). Profile with logic analyser. |
| **Flash write halts both cores** | Config save blocks Core 1's MS/TP FSM for ~50-100ms. | Use embassy-rp `Flash` with `multicore_lockout`. Only on explicit user save. Device re-acquires MS/TP token after. |

### Medium Risk

| Risk | Impact | Mitigation |
|------|--------|------------|
| **Rust binary size** | Monomorphization + embassy runtime could push firmware past 400 KB. | Enable LTO (`lto = "fat"`), `opt-level = "z"`, `codegen-units = 1`. Monitor size from Phase 0. Budget is 1.5 MB — substantial headroom. |
| **embassy-net multicast for mDNS** | embassy-net (smoltcp) multicast support may require manual IGMP join or W5500 register twiddling. | Test in Phase 3. W5500 MACRAW mode passes all frames to smoltcp; multicast filtering happens in software. Should work. |
| **bacnet-stack subset selection** | Need to identify exactly which .c files from bacnet-stack to compile. Too many = bloat, too few = missing symbols. | Start minimal (mstp.c, crc.c, npdu.c). Add files as linker errors reveal missing symbols. |
| **Two compilers in CI** | CI needs both `rustup` (thumbv6m target) and `arm-none-eabi-gcc`. | Both available via apt/GitHub Actions. Well-documented. |

### Low Risk

| Risk | Impact | Mitigation |
|------|--------|------------|
| **Flash endurance** | 100K cycles per sector. | Write only on explicit save. 1/day = 273 years. |
| **picoserve missing PUT/DELETE** | Spec needs PUT for config/points. picoserve may only support GET/POST/HEAD. | Check picoserve source. If missing, add custom method support or use POST for mutations. |
| **Bun lockfile in CI** | Binary lockfile needs matching bun version. | Pin version in `oven-sh/setup-bun@v1`. |

---

## 9. Advantages Over All-C Approach

| Aspect | All-C | Rust + C FFI (this plan) |
|--------|-------|--------------------------|
| HTTP server | ~1500 lines hand-written parser | picoserve: routing, SSE, JSON, static files — zero custom HTTP code |
| Socket management | 8 hardware sockets, manual juggling | embassy-net MACRAW: unlimited logical sockets |
| Concurrency | Manual poll loop, callback state machines | async/await tasks, compiler-enforced |
| Memory safety | Manual buffer management | Compile-time guarantees on Rust side |
| JSON handling | Hand-written or jsmn | serde + serde_json_core (derive macros) |
| BACnet stack | Native C, zero friction | Same C code, thin FFI boundary |
| Build system | CMake (well-known) | Cargo + cc crate (less common for embedded, but works) |
| Risk | Low | Medium (FFI integration) |

---

## 10. Open Questions (Need Answer Before Coding)

1. **SPI0 vs SPI1:** WIZnet docs say W5500-EVB-Pico uses SPI0 (GPIO16-21). The
   CLAUDE.md spec says SPI1. Which is the actual board?

2. **BACnet vendor ID:** Using 0xFFFF for now. ASHRAE registration planned?

3. **Max MS/TP devices:** How many devices on the RS-485 bus? Current plan: 32
   devices × 256 points. Affects `heapless` collection sizes.

4. **Reference screenshot:** Spec mentions a dashboard layout reference. Not in repo.

5. **BBMD:** Needed for v1, or simple BACnet/IP (single subnet) sufficient?

6. **picoserve HTTP methods:** Need to verify PUT/DELETE support. If missing,
   are POST-based mutations acceptable for the REST API?
