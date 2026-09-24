//! Just enough cryptography to lock the private record, with no crates.
//!
//! WHAT AND WHY. A child's disclosure written in a plain file on a shared
//! laptop can put that child in danger if the wrong person opens it; it may
//! also be needed later as evidence. The owner's decision (2026-09-24): keep
//! every word, locked with a password only the trusted adult knows, and
//! optionally a second adult's. That needs three standard pieces:
//!
//!   ChaCha20 (RFC 8439)        the cipher: turns the words into noise
//!   HMAC-SHA256 (RFC 2104)     the seal: any change to the file is detected
//!   PBKDF2-HMAC-SHA256 (RFC 8018) a password into a key, slowly, so guessing
//!                              passwords one after another is expensive
//!
//! Put together as encrypt-then-MAC with separate keys for each job. The
//! project's rule is one binary and no dependencies, so these are written out
//! here; each is checked against its RFC's published test values, and those
//! values were checked independently against Python's `cryptography` 50.0.1
//! and `hashlib` on 2026-09-24.
//!
//! WHAT THIS DOES NOT DO. It does not protect words while they cross the wifi
//! (the page is plain http; anyone with the wifi password and the right tools
//! could read them in flight), and it does not stop someone deleting the file.
//! It keeps a stolen or borrowed laptop from reading what was said.

use crate::sha256::Sha256;

/// HMAC-SHA256.
pub fn hmac(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&crate::sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..64 {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = Sha256::new();
    inner.update(&ipad);
    inner.update(msg);
    let ih = inner.finish();
    let mut outer = Sha256::new();
    outer.update(&opad);
    outer.update(&ih);
    outer.finish()
}

/// PBKDF2-HMAC-SHA256, one 32-byte block (all this needs).
pub fn pbkdf2(password: &[u8], salt: &[u8], iterations: u32) -> [u8; 32] {
    let mut s = salt.to_vec();
    s.extend_from_slice(&1u32.to_be_bytes());
    let mut u = hmac(password, &s);
    let mut t = u;
    for _ in 1..iterations {
        u = hmac(password, &u);
        for i in 0..32 {
            t[i] ^= u[i];
        }
    }
    t
}

fn quarter(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]);
    s[d] = (s[d] ^ s[a]).rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] = (s[b] ^ s[c]).rotate_left(7);
}

fn block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let mut s = [0u32; 16];
    s[0] = 0x6170_7865;
    s[1] = 0x3320_646e;
    s[2] = 0x7962_2d32;
    s[3] = 0x6b20_6574;
    for i in 0..8 {
        s[4 + i] = u32::from_le_bytes(key[i * 4..i * 4 + 4].try_into().unwrap());
    }
    s[12] = counter;
    for i in 0..3 {
        s[13 + i] = u32::from_le_bytes(nonce[i * 4..i * 4 + 4].try_into().unwrap());
    }
    let init = s;
    for _ in 0..10 {
        quarter(&mut s, 0, 4, 8, 12);
        quarter(&mut s, 1, 5, 9, 13);
        quarter(&mut s, 2, 6, 10, 14);
        quarter(&mut s, 3, 7, 11, 15);
        quarter(&mut s, 0, 5, 10, 15);
        quarter(&mut s, 1, 6, 11, 12);
        quarter(&mut s, 2, 7, 8, 13);
        quarter(&mut s, 3, 4, 9, 14);
    }
    let mut out = [0u8; 64];
    for i in 0..16 {
        out[i * 4..i * 4 + 4].copy_from_slice(&s[i].wrapping_add(init[i]).to_le_bytes());
    }
    out
}

/// ChaCha20: the same call encrypts and decrypts.
pub fn chacha20(key: &[u8; 32], counter: u32, nonce: &[u8; 12], data: &mut [u8]) {
    for (i, chunk) in data.chunks_mut(64).enumerate() {
        let ks = block(key, counter.wrapping_add(i as u32), nonce);
        for (b, k) in chunk.iter_mut().zip(ks.iter()) {
            *b ^= k;
        }
    }
}

/// Compare without stopping at the first difference.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Lock `plain` with a 32-byte key: nonce (12) | ciphertext | tag (32).
/// None only when the system gave no randomness, which must not be papered
/// over: a repeated nonce under one key gives the words away.
pub fn seal(key: &[u8; 32], plain: &[u8]) -> Option<Vec<u8>> {
    let enc = hmac(key, b"gorilla record: encrypt");
    let mac = hmac(key, b"gorilla record: seal");
    let n = crate::net::random_bytes(12)?;
    let nonce: [u8; 12] = n.as_slice().try_into().ok()?;
    let mut out = nonce.to_vec();
    let mut body = plain.to_vec();
    chacha20(&enc, 1, &nonce, &mut body);
    out.extend_from_slice(&body);
    let tag = hmac(&mac, &out);
    out.extend_from_slice(&tag);
    Some(out)
}

/// Unlock what `seal` made. None if the key is wrong or anything changed.
pub fn open(key: &[u8; 32], sealed: &[u8]) -> Option<Vec<u8>> {
    if sealed.len() < 12 + 32 {
        return None;
    }
    let enc = hmac(key, b"gorilla record: encrypt");
    let mac = hmac(key, b"gorilla record: seal");
    let (head, tag) = sealed.split_at(sealed.len() - 32);
    if !same(&hmac(&mac, head), tag) {
        return None;
    }
    let nonce: [u8; 12] = head[..12].try_into().ok()?;
    let mut body = head[12..].to_vec();
    chacha20(&enc, 1, &nonce, &mut body);
    Some(body)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64(data: &[u8]) -> String {
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(B64[(n >> 18) as usize & 63] as char);
        s.push(B64[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    s
}

pub fn unb64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for ch in s.trim().bytes() {
        if ch == b'=' {
            break;
        }
        let v = B64.iter().position(|&c| c == ch)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    /// RFC 4231, test case 1.
    #[test]
    fn hmac_matches_rfc_4231() {
        assert_eq!(
            hex(&hmac(&[0x0b; 20], b"Hi There")),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    /// The PBKDF2-HMAC-SHA256 values in wide use ("password", "salt"),
    /// confirmed independently with Python's hashlib on 2026-09-24.
    #[test]
    fn pbkdf2_matches_the_published_values() {
        assert_eq!(hex(&pbkdf2(b"password", b"salt", 1)), "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b");
        assert_eq!(hex(&pbkdf2(b"password", b"salt", 2)), "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43");
        assert_eq!(hex(&pbkdf2(b"password", b"salt", 4096)), "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a");
    }

    /// RFC 8439 section 2.4.2, the whole sunscreen text.
    #[test]
    fn chacha20_matches_rfc_8439() {
        let key: [u8; 32] = core::array::from_fn(|i| i as u8);
        let nonce = [0, 0, 0, 0, 0, 0, 0, 0x4a, 0, 0, 0, 0];
        let mut data = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.".to_vec();
        chacha20(&key, 1, &nonce, &mut data);
        assert_eq!(
            hex(&data),
            "6e2e359a2568f98041ba0728dd0d6981e97e7aec1d4360c20a27afccfd9fae0bf91b65c5524733ab8f593dabcd62b3571639d624e65152ab8f530c359f0861d807ca0dbf500d6a6156a38e088a22b65e52bc514d16ccf806818ce91ab77937365af90bbf74a35be6b40b8eedf2785e42874d"
        );
    }

    #[test]
    fn sealed_words_open_with_the_key_and_nothing_else() {
        let key = [7u8; 32];
        let sealed = seal(&key, "a child's words".as_bytes()).unwrap();
        assert!(!sealed.windows(5).any(|w| w == b"child"), "the words must not be readable in the file");
        assert_eq!(open(&key, &sealed).unwrap(), "a child's words".as_bytes());
        assert!(open(&[8u8; 32], &sealed).is_none(), "the wrong key opens nothing");
        let mut tampered = sealed.clone();
        tampered[14] ^= 1;
        assert!(open(&key, &tampered).is_none(), "a changed file is detected");
        assert_ne!(seal(&key, b"same").unwrap(), seal(&key, b"same").unwrap(), "a fresh nonce every time");
    }

    #[test]
    fn base64_round_trips() {
        for n in 0..70u8 {
            let data: Vec<u8> = (0..n).map(|i| i.wrapping_mul(37)).collect();
            assert_eq!(unb64(&b64(&data)).unwrap(), data);
        }
        assert_eq!(b64(b"Man"), "TWFu");
    }
}
