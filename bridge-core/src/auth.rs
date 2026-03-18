//! Authentication and authorisation helpers.
//!
//! Provides password hashing (SHA-256 with a stored salt), bearer-token
//! verification (SHA-256 of the plaintext token), and role-based permission
//! checks.
//!
//! # Password storage
//!
//! This firmware targets a `no_std` environment. A full bcrypt implementation
//! in `no_std` would require significant dependencies. We therefore use a
//! compact scheme:
//!
//! ```text
//! stored_hash = SHA256(salt || password_utf8)
//! ```
//!
//! The 32-byte salt is stored in the first 32 bytes of the 64-byte
//! `password_hash` field of [`crate::config::UserConfig`].  The next 32 bytes
//! store the SHA-256 digest.  This approach:
//! - Requires only the `sha2` crate (already in the dependency tree via
//!   `embedded-tls`).
//! - Is immune to rainbow-table attacks (unique salt per user).
//! - Has zero dynamic allocation.
//!
//! For API bearer tokens the scheme is simpler: `token_hash = SHA256(token_bytes)`.
//! No salt is needed because the tokens are long random strings with very high
//! entropy.
//!
//! # Roles and permissions
//!
//! Three roles are defined (lowest → highest privilege):
//!
//! | Role       | Capabilities |
//! |------------|-------------|
//! | `Viewer`   | Read dashboard, read config, read API |
//! | `Operator` | Viewer + write points, edit per-point config |
//! | `Admin`    | Operator + edit system config, manage users, manage TLS, OTA |

use crate::config::{TokenConfig, UserRole};

// ---------------------------------------------------------------------------
// SHA-256 implementation (no_std, no alloc)
// ---------------------------------------------------------------------------
//
// Rather than pulling in a heavy external crate just for SHA-256, we inline a
// minimal, self-contained SHA-256 implementation that works in no_std with no
// dynamic allocation.  This is ~80 lines of straightforward code and has been
// verified against the NIST test vectors in the unit tests below.
//
// Reference: FIPS PUB 180-4.

const SHA256_INITIAL: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

#[rustfmt::skip]
const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
    0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
    0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
    0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
    0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
    0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn sha256_block(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 64];
    for i in 0..16 {
        w[i] = u32::from_be_bytes([
            block[i * 4],
            block[i * 4 + 1],
            block[i * 4 + 2],
            block[i * 4 + 3],
        ]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(SHA256_K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }
    state[0] = state[0].wrapping_add(a);
    state[1] = state[1].wrapping_add(b);
    state[2] = state[2].wrapping_add(c);
    state[3] = state[3].wrapping_add(d);
    state[4] = state[4].wrapping_add(e);
    state[5] = state[5].wrapping_add(f);
    state[6] = state[6].wrapping_add(g);
    state[7] = state[7].wrapping_add(h);
}

/// Compute SHA-256 of `data` and write the 32-byte digest into `out`.
///
/// This function works in `no_std` without any heap allocation.
/// It processes the input in 64-byte blocks using the standard SHA-256
/// padding scheme (FIPS 180-4 §5.1.1).
pub fn sha256(data: &[u8], out: &mut [u8; 32]) {
    let mut state = SHA256_INITIAL;
    let bit_len = (data.len() as u64) * 8;

    // Process complete 64-byte blocks.
    let mut offset = 0usize;
    while offset + 64 <= data.len() {
        let mut block = [0u8; 64];
        block.copy_from_slice(&data[offset..offset + 64]);
        sha256_block(&mut state, &block);
        offset += 64;
    }

    // Build the final padded block(s).
    let remainder = data.len() - offset;
    let mut last = [0u8; 128];
    last[..remainder].copy_from_slice(&data[offset..]);
    last[remainder] = 0x80; // append bit '1'

    // The length field (8 bytes, big-endian) must fit in the last block.
    // If there's no room in the current block, emit two padded blocks.
    let len_offset = if remainder < 56 { 56 } else { 120 };
    last[len_offset..len_offset + 8].copy_from_slice(&bit_len.to_be_bytes());

    let mut block = [0u8; 64];
    block.copy_from_slice(&last[..64]);
    sha256_block(&mut state, &block);
    if remainder >= 56 {
        block.copy_from_slice(&last[64..128]);
        sha256_block(&mut state, &block);
    }

    // Serialise the state to `out` (big-endian u32 words).
    for (i, word) in state.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Hash a bearer token with SHA-256.
///
/// The input is the raw token bytes (UTF-8 string bytes in practice).
/// The output is a 32-byte digest suitable for storing in [`TokenConfig::token_hash`].
pub fn hash_token(token: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    sha256(token, &mut out);
    out
}

/// Hash a password with SHA-256 using the provided 32-byte salt.
///
/// Computes `SHA256(salt || password_utf8)` and writes the result into `digest`.
/// The caller is responsible for generating a random `salt` (e.g. from the
/// RP2040 ROSC entropy source) and storing it together with `digest` in the
/// 64-byte `password_hash` field: bytes `[0..32]` = salt, bytes `[32..64]` = digest.
pub fn hash_password(password: &str, salt: &[u8; 32], digest: &mut [u8; 32]) {
    // Build a stack buffer: salt (32 bytes) || password (max 64 bytes) = max 96 bytes.
    // Passwords longer than 64 bytes are silently truncated — a generous limit for
    // an embedded device.
    let pw = password.as_bytes();
    let pw_len = pw.len().min(64);

    let mut buf = [0u8; 96];
    buf[..32].copy_from_slice(salt);
    buf[32..32 + pw_len].copy_from_slice(&pw[..pw_len]);

    sha256(&buf[..32 + pw_len], digest);
}

/// Verify a plaintext password against stored salt and hash fields.
///
/// - `salt` — the 32-byte per-user salt stored in [`UserConfig::password_salt`].
/// - `stored_digest` — the 32-byte SHA-256 digest stored in [`UserConfig::password_hash`].
///
/// An all-zeros `stored_digest` means the account is not configured (returns `false`).
/// Returns `true` if `SHA256(salt || password_utf8) == stored_digest`.
pub fn verify_password(password: &str, salt: &[u8; 32], stored_digest: &[u8; 32]) -> bool {
    // All-zeros stored hash → account not configured (reject).
    if *stored_digest == [0u8; 32] {
        return false;
    }
    let mut computed = [0u8; 32];
    hash_password(password, salt, &mut computed);
    // Constant-time comparison to prevent timing attacks.
    constant_time_eq(&computed, stored_digest)
}

/// Constant-time comparison of two 32-byte slices.
///
/// Returns `true` only if every byte is equal.  The result is computed without
/// short-circuiting so the execution time does not reveal how many bytes match.
fn constant_time_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

/// Search `tokens` for one whose `token_hash` matches `SHA256(token_bytes)`.
///
/// Returns the [`UserRole`] of the matching token, or `None` if no match is
/// found.  The comparison is performed with constant-time equality.
pub fn find_token_role(token: &str, tokens: &[TokenConfig]) -> Option<UserRole> {
    let computed = hash_token(token.as_bytes());
    for t in tokens {
        if constant_time_eq(&computed, &t.token_hash) {
            return Some(t.role);
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Permission system
// ---------------------------------------------------------------------------

/// An action that can be guarded by role-based access control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Permission {
    /// View the live dashboard (present values, device list).
    ViewDashboard,
    /// View configuration pages and REST config endpoints.
    ViewConfig,
    /// Write a point value (REST PUT or dashboard write button).
    WritePoints,
    /// Edit per-point display names, units, mode, and processors.
    EditPointConfig,
    /// Edit system-wide config: network, BACnet, mDNS, NTP, syslog, MQTT, SNMP.
    EditSystemConfig,
    /// Create, delete, or change user accounts and API tokens.
    ManageUsers,
    /// Configure TLS certificates and enable/disable HTTPS.
    ManageTls,
    /// Trigger or configure OTA firmware updates.
    ManageFirmware,
    /// Export the full configuration as JSON.
    ExportConfig,
}

/// Check whether `role` has permission to perform `action`.
///
/// Permission matrix:
///
/// | Permission        | Viewer | Operator | Admin |
/// |-------------------|--------|----------|-------|
/// | ViewDashboard     | ✓      | ✓        | ✓     |
/// | ViewConfig        | ✓      | ✓        | ✓     |
/// | WritePoints       |        | ✓        | ✓     |
/// | EditPointConfig   |        | ✓        | ✓     |
/// | EditSystemConfig  |        |          | ✓     |
/// | ManageUsers       |        |          | ✓     |
/// | ManageTls         |        |          | ✓     |
/// | ManageFirmware    |        |          | ✓     |
/// | ExportConfig      |        |          | ✓     |
pub fn has_permission(role: &UserRole, action: Permission) -> bool {
    match action {
        // All authenticated users can view the dashboard and config.
        Permission::ViewDashboard | Permission::ViewConfig => true,

        // Operator and Admin can write point values and edit point-level config.
        Permission::WritePoints | Permission::EditPointConfig => match role {
            UserRole::Operator | UserRole::Admin => true,
            UserRole::Viewer => false,
        },

        // Only Admin can touch system-wide settings, users, TLS, OTA, and exports.
        Permission::EditSystemConfig
        | Permission::ManageUsers
        | Permission::ManageTls
        | Permission::ManageFirmware
        | Permission::ExportConfig => matches!(role, UserRole::Admin),
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{TokenConfig, UserRole};
    use heapless::String;

    // -----------------------------------------------------------------------
    // SHA-256 correctness (NIST FIPS 180-4 test vectors)
    // -----------------------------------------------------------------------

    /// Helper: hex-decode a 64-char string into a [u8; 32].
    fn hex32(s: &str) -> [u8; 32] {
        let mut out = [0u8; 32];
        let bytes = s.as_bytes();
        for i in 0..32 {
            let hi = hex_nibble(bytes[i * 2]);
            let lo = hex_nibble(bytes[i * 2 + 1]);
            out[i] = (hi << 4) | lo;
        }
        out
    }

    fn hex_nibble(b: u8) -> u8 {
        match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => 0,
        }
    }

    #[test]
    fn sha256_empty_string() {
        // SHA256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        let mut out = [0u8; 32];
        sha256(b"", &mut out);
        let expected = hex32("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(out, expected, "SHA256(\"\") mismatch");
    }

    #[test]
    fn sha256_abc() {
        // NIST FIPS 180-4 / RFC 6234 test vector for SHA-256("abc"):
        // ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        let mut out = [0u8; 32];
        sha256(b"abc", &mut out);
        let expected_bytes: [u8; 32] = [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ];
        assert_eq!(out, expected_bytes, "SHA256(\"abc\") mismatch");
    }

    #[test]
    fn sha256_hello() {
        // SHA256("hello") — verified with openssl dgst -sha256
        // = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        let mut out = [0u8; 32];
        sha256(b"hello", &mut out);
        let expected_bytes: [u8; 32] = [
            0x2c, 0xf2, 0x4d, 0xba, 0x5f, 0xb0, 0xa3, 0x0e, 0x26, 0xe8, 0x3b, 0x2a, 0xc5, 0xb9,
            0xe2, 0x9e, 0x1b, 0x16, 0x1e, 0x5c, 0x1f, 0xa7, 0x42, 0x5e, 0x73, 0x04, 0x33, 0x62,
            0x93, 0x8b, 0x98, 0x24,
        ];
        assert_eq!(out, expected_bytes, "SHA256(\"hello\") mismatch");
    }

    #[test]
    fn sha256_55_bytes() {
        // 55 bytes of 'a' — fits in a single 64-byte padded block (length field in bytes 55..63)
        let input = [b'a'; 55];
        let mut out = [0u8; 32];
        sha256(&input, &mut out);
        // Verify it doesn't panic and produces a non-zero output.
        assert_ne!(out, [0u8; 32]);
    }

    #[test]
    fn sha256_64_bytes() {
        // 64 bytes — exactly one block; the length field spills into a second padded block.
        let input = [b'b'; 64];
        let mut out = [0u8; 32];
        sha256(&input, &mut out);
        assert_ne!(out, [0u8; 32]);
    }

    #[test]
    fn sha256_128_bytes() {
        // 128 bytes — exactly two full blocks.
        let input = [b'c'; 128];
        let mut out = [0u8; 32];
        sha256(&input, &mut out);
        assert_ne!(out, [0u8; 32]);
    }

    #[test]
    fn sha256_deterministic() {
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        sha256(b"same input", &mut a);
        sha256(b"same input", &mut b);
        assert_eq!(a, b);
    }

    #[test]
    fn sha256_different_inputs_differ() {
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        sha256(b"input-a", &mut a);
        sha256(b"input-b", &mut b);
        assert_ne!(a, b);
    }

    // -----------------------------------------------------------------------
    // hash_token
    // -----------------------------------------------------------------------

    #[test]
    fn hash_token_is_sha256() {
        let token = "my-secret-token";
        let result = hash_token(token.as_bytes());
        let mut expected = [0u8; 32];
        sha256(token.as_bytes(), &mut expected);
        assert_eq!(result, expected);
    }

    #[test]
    fn hash_token_deterministic() {
        let a = hash_token(b"abcdef");
        let b = hash_token(b"abcdef");
        assert_eq!(a, b);
    }

    #[test]
    fn hash_token_different_tokens_differ() {
        let a = hash_token(b"token-one");
        let b = hash_token(b"token-two");
        assert_ne!(a, b);
    }

    // -----------------------------------------------------------------------
    // hash_password / verify_password
    // -----------------------------------------------------------------------

    #[test]
    fn hash_password_deterministic() {
        let salt = [0x42u8; 32];
        let mut d1 = [0u8; 32];
        let mut d2 = [0u8; 32];
        hash_password("secret", &salt, &mut d1);
        hash_password("secret", &salt, &mut d2);
        assert_eq!(d1, d2);
    }

    #[test]
    fn hash_password_salt_changes_output() {
        let salt1 = [0x01u8; 32];
        let salt2 = [0x02u8; 32];
        let mut d1 = [0u8; 32];
        let mut d2 = [0u8; 32];
        hash_password("same", &salt1, &mut d1);
        hash_password("same", &salt2, &mut d2);
        assert_ne!(d1, d2);
    }

    #[test]
    fn hash_password_different_passwords_differ() {
        let salt = [0x77u8; 32];
        let mut d1 = [0u8; 32];
        let mut d2 = [0u8; 32];
        hash_password("password1", &salt, &mut d1);
        hash_password("password2", &salt, &mut d2);
        assert_ne!(d1, d2);
    }

    #[test]
    fn verify_password_correct() {
        let salt = [0xAAu8; 32];
        let mut digest = [0u8; 32];
        hash_password("correct-horse", &salt, &mut digest);
        assert!(verify_password("correct-horse", &salt, &digest));
    }

    #[test]
    fn verify_password_wrong_password_fails() {
        let salt = [0xBBu8; 32];
        let mut digest = [0u8; 32];
        hash_password("correct", &salt, &mut digest);
        assert!(!verify_password("wrong", &salt, &digest));
    }

    #[test]
    fn verify_password_empty_stored_hash_fails() {
        // All-zeros digest means the account is not configured.
        let salt = [0u8; 32];
        let digest = [0u8; 32];
        assert!(!verify_password("anything", &salt, &digest));
    }

    #[test]
    fn verify_password_empty_password() {
        // An empty password can still be hashed and verified.
        let salt = [0x11u8; 32];
        let mut digest = [0u8; 32];
        hash_password("", &salt, &mut digest);
        assert!(verify_password("", &salt, &digest));
        assert!(!verify_password("not-empty", &salt, &digest));
    }

    #[test]
    fn verify_password_roundtrip() {
        let passwords = ["hunter2", "correct horse battery staple", "1234", "ä"];
        for pw in &passwords {
            let salt = [0x5Au8; 32];
            let mut digest = [0u8; 32];
            hash_password(pw, &salt, &mut digest);
            assert!(
                verify_password(pw, &salt, &digest),
                "roundtrip failed for {}",
                pw
            );
            assert!(
                !verify_password("wrong", &salt, &digest),
                "false positive for {}",
                pw
            );
        }
    }

    // -----------------------------------------------------------------------
    // find_token_role
    // -----------------------------------------------------------------------

    fn make_token(name: &str, token: &str, role: UserRole) -> TokenConfig {
        let mut n = String::<32>::new();
        let _ = n.push_str(name);
        let mut created_by = String::<16>::new();
        let _ = created_by.push_str("admin");
        TokenConfig {
            name: n,
            token_hash: hash_token(token.as_bytes()),
            role,
            created_by,
        }
    }

    #[test]
    fn find_token_role_found() {
        let tokens = [
            make_token("viewer-tok", "tok-viewer-123", UserRole::Viewer),
            make_token("admin-tok", "tok-admin-456", UserRole::Admin),
        ];
        assert_eq!(
            find_token_role("tok-viewer-123", &tokens),
            Some(UserRole::Viewer)
        );
        assert_eq!(
            find_token_role("tok-admin-456", &tokens),
            Some(UserRole::Admin)
        );
    }

    #[test]
    fn find_token_role_not_found() {
        let tokens = [make_token("tok", "secret", UserRole::Operator)];
        assert_eq!(find_token_role("wrong-token", &tokens), None);
    }

    #[test]
    fn find_token_role_empty_list() {
        let tokens: [TokenConfig; 0] = [];
        assert_eq!(find_token_role("anything", &tokens), None);
    }

    #[test]
    fn find_token_role_operator() {
        let tokens = [make_token("op", "op-token-xyz", UserRole::Operator)];
        assert_eq!(
            find_token_role("op-token-xyz", &tokens),
            Some(UserRole::Operator)
        );
    }

    #[test]
    fn find_token_role_returns_first_match() {
        // Duplicate hashes shouldn't appear in practice, but if they do
        // the first match is returned.
        let tokens = [
            make_token("a", "dup-token", UserRole::Viewer),
            make_token("b", "dup-token", UserRole::Admin),
        ];
        assert_eq!(
            find_token_role("dup-token", &tokens),
            Some(UserRole::Viewer)
        );
    }

    // -----------------------------------------------------------------------
    // has_permission
    // -----------------------------------------------------------------------

    #[test]
    fn viewer_can_view_dashboard_and_config() {
        let role = UserRole::Viewer;
        assert!(has_permission(&role, Permission::ViewDashboard));
        assert!(has_permission(&role, Permission::ViewConfig));
    }

    #[test]
    fn viewer_cannot_write_or_edit() {
        let role = UserRole::Viewer;
        assert!(!has_permission(&role, Permission::WritePoints));
        assert!(!has_permission(&role, Permission::EditPointConfig));
        assert!(!has_permission(&role, Permission::EditSystemConfig));
        assert!(!has_permission(&role, Permission::ManageUsers));
        assert!(!has_permission(&role, Permission::ManageTls));
        assert!(!has_permission(&role, Permission::ManageFirmware));
        assert!(!has_permission(&role, Permission::ExportConfig));
    }

    #[test]
    fn operator_can_write_points_and_edit_point_config() {
        let role = UserRole::Operator;
        assert!(has_permission(&role, Permission::ViewDashboard));
        assert!(has_permission(&role, Permission::ViewConfig));
        assert!(has_permission(&role, Permission::WritePoints));
        assert!(has_permission(&role, Permission::EditPointConfig));
    }

    #[test]
    fn operator_cannot_manage_system_or_users() {
        let role = UserRole::Operator;
        assert!(!has_permission(&role, Permission::EditSystemConfig));
        assert!(!has_permission(&role, Permission::ManageUsers));
        assert!(!has_permission(&role, Permission::ManageTls));
        assert!(!has_permission(&role, Permission::ManageFirmware));
        assert!(!has_permission(&role, Permission::ExportConfig));
    }

    #[test]
    fn admin_has_all_permissions() {
        let role = UserRole::Admin;
        let all = [
            Permission::ViewDashboard,
            Permission::ViewConfig,
            Permission::WritePoints,
            Permission::EditPointConfig,
            Permission::EditSystemConfig,
            Permission::ManageUsers,
            Permission::ManageTls,
            Permission::ManageFirmware,
            Permission::ExportConfig,
        ];
        for perm in &all {
            assert!(
                has_permission(&role, *perm),
                "Admin should have permission {:?}",
                perm
            );
        }
    }

    #[test]
    fn permission_matrix_complete() {
        // Every permission × every role should return a definite value (no panic).
        let roles = [UserRole::Viewer, UserRole::Operator, UserRole::Admin];
        let perms = [
            Permission::ViewDashboard,
            Permission::ViewConfig,
            Permission::WritePoints,
            Permission::EditPointConfig,
            Permission::EditSystemConfig,
            Permission::ManageUsers,
            Permission::ManageTls,
            Permission::ManageFirmware,
            Permission::ExportConfig,
        ];
        for role in &roles {
            for perm in &perms {
                let _ = has_permission(role, *perm);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Privilege escalation invariants
    // -----------------------------------------------------------------------

    #[test]
    fn privilege_ordering_holds() {
        // For every permission: if Viewer has it, Operator must also have it.
        let perms = [
            Permission::ViewDashboard,
            Permission::ViewConfig,
            Permission::WritePoints,
            Permission::EditPointConfig,
            Permission::EditSystemConfig,
            Permission::ManageUsers,
            Permission::ManageTls,
            Permission::ManageFirmware,
            Permission::ExportConfig,
        ];
        for perm in &perms {
            let viewer = has_permission(&UserRole::Viewer, *perm);
            let operator = has_permission(&UserRole::Operator, *perm);
            let admin = has_permission(&UserRole::Admin, *perm);
            // Operator >= Viewer, Admin >= Operator.
            assert!(
                operator >= viewer,
                "Operator must have at least Viewer's permission for {:?}",
                perm
            );
            assert!(
                admin >= operator,
                "Admin must have at least Operator's permission for {:?}",
                perm
            );
        }
    }
}
