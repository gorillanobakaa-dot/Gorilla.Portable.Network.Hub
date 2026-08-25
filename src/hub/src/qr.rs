//! A QR code for joining the wifi, drawn in the terminal.
//!
//! A bonus lane and never the main one. Plenty of the phones this is built for
//! have a cracked or dead camera, or a camera app that wants an account before
//! it will scan anything, so the network name and password stay written out in
//! full underneath. A code that saves thirty seconds for some of the room must
//! not become the only way in for the rest of it.
//!
//! No dependency, for the reason nothing else here has one: a QR crate pulls
//! in a build tree, and this binary is under a megabyte because somebody has
//! to download it over a connection measured in single digits of KB per second.
//!
//! Deliberately narrow. Byte mode, error correction level L, versions 1 to 5,
//! which between them cover any wifi credential a person would type and share
//! one useful property: at level L those five versions are all a SINGLE error
//! correction block, so none of the block interleaving exists here at all.
//! A wifi payload is around forty bytes and version 3 holds fifty-five.

/// A finished code: `size` by `size` modules, dark where true.
pub struct Code {
    pub size: usize,
    dark: Vec<bool>,
}

impl Code {
    pub fn dark(&self, row: usize, col: usize) -> bool {
        row < self.size && col < self.size && self.dark[row * self.size + col]
    }
}

/// Data codewords per version at level L. All single-block.
const DATA_CODEWORDS: [usize; 5] = [19, 34, 55, 80, 108];
/// Error correction codewords per version at level L.
const EC_CODEWORDS: [usize; 5] = [7, 10, 15, 20, 26];

/// The string a phone's camera turns into "join this network?".
///
/// Not our invention and not negotiable: this is the de facto format every
/// phone camera understands, so the punctuation is load bearing. A colon or a
/// semicolon inside a network name or a password has to be escaped, or it ends
/// the field early and the phone is offered half a password.
pub fn wifi_payload(ssid: &str, password: &str) -> String {
    format!("WIFI:T:WPA;S:{};P:{};;", escape(ssid), escape(password))
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | ';' | ',' | ':' | '"') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The whole thing: credentials in, code out. None when it will not fit, which
/// at a 105-byte ceiling means a password nobody is typing off a board anyway.
pub fn wifi_join(ssid: &str, password: &str) -> Option<Code> {
    encode(wifi_payload(ssid, password).as_bytes())
}

// ------------------------------------------------------------- GF(256)

/// Multiply in the field the QR standard uses: GF(256) modulo
/// x^8 + x^4 + x^3 + x^2 + 1, which is 0x11D.
///
/// No lookup tables. The largest code here needs 108 by 26 multiplications,
/// which is nothing, and a table would be two more arrays to get wrong.
fn gmul(mut a: u8, mut b: u8) -> u8 {
    let mut p = 0u8;
    for _ in 0..8 {
        if b & 1 != 0 {
            p ^= a;
        }
        let overflow = a & 0x80 != 0;
        a <<= 1;
        if overflow {
            a ^= 0x1d; // the low byte of 0x11D, the high bit having shifted out
        }
        b >>= 1;
    }
    p
}

/// The generator polynomial for `n` error correction codewords: the product of
/// (x - 2^i) for i in 0..n. Highest degree first.
fn generator(n: usize) -> Vec<u8> {
    let mut g = vec![1u8];
    let mut root = 1u8;
    for _ in 0..n {
        let mut next = vec![0u8; g.len() + 1];
        for (i, &c) in g.iter().enumerate() {
            next[i] ^= c;
            next[i + 1] ^= gmul(c, root);
        }
        g = next;
        root = gmul(root, 2);
    }
    g
}

/// Polynomial long division; the remainder is the error correction block.
fn ec_codewords(data: &[u8], n: usize) -> Vec<u8> {
    let g = generator(n);
    let mut rem = vec![0u8; data.len() + n];
    rem[..data.len()].copy_from_slice(data);
    for i in 0..data.len() {
        let lead = rem[i];
        if lead != 0 {
            for (j, &gc) in g.iter().enumerate() {
                rem[i + j] ^= gmul(gc, lead);
            }
        }
    }
    rem[data.len()..].to_vec()
}

// ------------------------------------------------------------- bitstream

struct Bits {
    bytes: Vec<u8>,
    len: usize,
}

impl Bits {
    fn new() -> Bits {
        Bits { bytes: Vec::new(), len: 0 }
    }
    fn push(&mut self, value: u16, width: usize) {
        for i in (0..width).rev() {
            if self.len % 8 == 0 {
                self.bytes.push(0);
            }
            if value >> i & 1 == 1 {
                let last = self.bytes.len() - 1;
                self.bytes[last] |= 0x80 >> (self.len % 8);
            }
            self.len += 1;
        }
    }
}

/// Encode a payload, choosing the smallest version that holds it.
pub fn encode(payload: &[u8]) -> Option<Code> {
    // Four bits of mode plus eight of length is twelve bits of header, so the
    // usable byte count is the data capacity less two whole codewords.
    let version = (1..=5).find(|v| payload.len() + 2 <= DATA_CODEWORDS[v - 1])?;
    let capacity = DATA_CODEWORDS[version - 1];
    let ec_len = EC_CODEWORDS[version - 1];

    let mut bits = Bits::new();
    bits.push(0b0100, 4); // byte mode
    bits.push(payload.len() as u16, 8); // 8-bit count, correct for versions 1 to 9
    for &b in payload {
        bits.push(b as u16, 8);
    }
    // Terminator: up to four zero bits, fewer if the data ends flush.
    let spare = capacity * 8 - bits.len;
    bits.push(0, spare.min(4));
    // Then out to a whole byte.
    if bits.len % 8 != 0 {
        bits.push(0, 8 - bits.len % 8);
    }
    let mut data = bits.bytes;
    // The two pad bytes the standard names, alternating, until full.
    for i in 0..(capacity - data.len()) {
        data.push(if i % 2 == 0 { 0xEC } else { 0x11 });
    }
    let ec = ec_codewords(&data, ec_len);
    // Single block, so the "interleaving" is a concatenation.
    let mut codewords = data;
    codewords.extend_from_slice(&ec);

    let mut grid = Grid::new(version);
    grid.place_function_patterns();
    grid.place_data(&codewords);

    // Every mask is legal; the standard picks the one scoring lowest against
    // four penalty rules, which is what stops a code coming out with great
    // blank fields or false finder patterns in it. Scanners read the mask
    // number out of the format information, so this is about how easily a
    // camera locks on, not about correctness.
    let mut best: Option<(u32, Grid)> = None;
    for mask in 0..8u8 {
        let mut candidate = grid.clone();
        candidate.apply_mask(mask);
        candidate.place_format(mask);
        let score = candidate.penalty();
        if best.as_ref().map(|(s, _)| score < *s).unwrap_or(true) {
            best = Some((score, candidate));
        }
    }
    let (_, chosen) = best?;
    Some(Code { size: chosen.size, dark: chosen.dark })
}

// ------------------------------------------------------------- the grid

#[derive(Clone)]
struct Grid {
    version: usize,
    size: usize,
    dark: Vec<bool>,
    /// True where a function pattern lives: never carries data, never masked.
    fixed: Vec<bool>,
}

impl Grid {
    fn new(version: usize) -> Grid {
        let size = 17 + 4 * version;
        Grid {
            version,
            size,
            dark: vec![false; size * size],
            fixed: vec![false; size * size],
        }
    }

    fn set(&mut self, row: usize, col: usize, dark: bool, fixed: bool) {
        let i = row * self.size + col;
        self.dark[i] = dark;
        if fixed {
            self.fixed[i] = true;
        }
    }

    fn is_dark(&self, row: usize, col: usize) -> bool {
        self.dark[row * self.size + col]
    }

    fn place_function_patterns(&mut self) {
        let size = self.size;
        // Three finders with their separators. Walking one module OUTSIDE the
        // 7x7 in every direction lays the light separator down in the same
        // pass, which is why the loop starts at -1.
        for (r0, c0) in [(0isize, 0isize), (0, size as isize - 7), (size as isize - 7, 0)] {
            for dr in -1isize..=7 {
                for dc in -1isize..=7 {
                    let (r, c) = (r0 + dr, c0 + dc);
                    if r < 0 || c < 0 || r >= size as isize || c >= size as isize {
                        continue;
                    }
                    let inside = (0..7).contains(&dr) && (0..7).contains(&dc);
                    let ring = dr == 0 || dr == 6 || dc == 0 || dc == 6;
                    let core = (2..=4).contains(&dr) && (2..=4).contains(&dc);
                    self.set(r as usize, c as usize, inside && (ring || core), true);
                }
            }
        }
        // Timing: the alternating line that tells a scanner the module pitch.
        for i in 8..size - 8 {
            let on = i % 2 == 0;
            self.set(6, i, on, true);
            self.set(i, 6, on, true);
        }
        // The one module that is always dark.
        self.set(4 * self.version + 9, 8, true, true);
        // Alignment. Versions 2 to 5 have exactly one, opposite the top-left
        // finder; version 1 has none. The other three centre pairs would land
        // on the finders.
        if self.version >= 2 {
            let centre = (4 * self.version + 10) as isize;
            for dr in -2isize..=2 {
                for dc in -2isize..=2 {
                    let ring = dr.abs() == 2 || dc.abs() == 2 || (dr == 0 && dc == 0);
                    self.set((centre + dr) as usize, (centre + dc) as usize, ring, true);
                }
            }
        }
        // Reserve the format information, written once the mask is known.
        for i in 0..9 {
            if !self.fixed[8 * self.size + i] {
                self.set(8, i, false, true);
            }
            if !self.fixed[i * self.size + 8] {
                self.set(i, 8, false, true);
            }
        }
        // Not symmetrical, and the asymmetry matters. The second copy of the
        // format field is seven modules up the left edge and eight along the
        // bottom-right, fifteen in total. Reserving eight on BOTH sides walks
        // one module too far up the column and lands on the always-dark
        // module, blanking it. Nothing downstream notices: the code still
        // round trips, because that module carries no data. It was the
        // structural test, checking coordinates written out by hand, that
        // caught it.
        for i in 0..8 {
            self.set(8, size - 1 - i, false, true);
        }
        for i in 0..7 {
            self.set(size - 1 - i, 8, false, true);
        }
    }

    /// The zigzag: two-module columns walked from the right edge, alternating
    /// up and down, skipping the vertical timing column.
    fn place_data(&mut self, codewords: &[u8]) {
        let size = self.size;
        let mut bit = 0usize;
        let mut col = size - 1;
        let mut upward = true;
        loop {
            if col == 6 {
                // The timing column is not part of any data column pair.
                col -= 1;
            }
            for i in 0..size {
                let row = if upward { size - 1 - i } else { i };
                for c in [col, col - 1] {
                    if self.fixed[row * size + c] {
                        continue;
                    }
                    let on = bit < codewords.len() * 8
                        && codewords[bit / 8] >> (7 - bit % 8) & 1 == 1;
                    self.dark[row * size + c] = on;
                    bit += 1;
                }
            }
            upward = !upward;
            if col < 2 {
                break;
            }
            col -= 2;
        }
    }

    fn apply_mask(&mut self, mask: u8) {
        for row in 0..self.size {
            for col in 0..self.size {
                if self.fixed[row * self.size + col] {
                    continue;
                }
                if mask_at(mask, row, col) {
                    self.dark[row * self.size + col] ^= true;
                }
            }
        }
    }

    fn place_format(&mut self, mask: u8) {
        let bits = format_bits(mask);
        let size = self.size;
        for i in 0..15 {
            let on = bits >> i & 1 == 1;
            // Copy one, wrapped around the top-left finder.
            let (r, c) = match i {
                0..=5 => (8, i),
                6 => (8, 7),
                7 => (8, 8),
                8 => (7, 8),
                _ => (14 - i, 8),
            };
            self.set(r, c, on, true);
            // Copy two, split between the other two finders, so a code with a
            // damaged corner still reports its own mask.
            let (r, c) = if i < 7 { (size - 1 - i, 8) } else { (8, size - 15 + i) };
            self.set(r, c, on, true);
        }
    }

    /// The four penalty rules, lower being better.
    fn penalty(&self) -> u32 {
        let n = self.size;
        let mut score = 0u32;
        // Rule 1: runs of five or more of one colour, in rows and in columns.
        for i in 0..n {
            for horizontal in [true, false] {
                let mut run = 1u32;
                for j in 1..n {
                    let (a, b) = if horizontal {
                        (self.is_dark(i, j), self.is_dark(i, j - 1))
                    } else {
                        (self.is_dark(j, i), self.is_dark(j - 1, i))
                    };
                    if a == b {
                        run += 1;
                    } else {
                        if run >= 5 {
                            score += 3 + (run - 5);
                        }
                        run = 1;
                    }
                }
                if run >= 5 {
                    score += 3 + (run - 5);
                }
            }
        }
        // Rule 2: every 2x2 block of one colour.
        for r in 0..n - 1 {
            for c in 0..n - 1 {
                let v = self.is_dark(r, c);
                if v == self.is_dark(r, c + 1) && v == self.is_dark(r + 1, c) && v == self.is_dark(r + 1, c + 1) {
                    score += 3;
                }
            }
        }
        // Rule 3: the finder-like sequence with four light modules beside it,
        // which is what sends a scanner hunting for a corner that is not there.
        const A: [bool; 11] = [true, false, true, true, true, false, true, false, false, false, false];
        const B: [bool; 11] = [false, false, false, false, true, false, true, true, true, false, true];
        for i in 0..n {
            for j in 0..n.saturating_sub(10) {
                let row: Vec<bool> = (0..11).map(|k| self.is_dark(i, j + k)).collect();
                if row == A || row == B {
                    score += 40;
                }
                let col: Vec<bool> = (0..11).map(|k| self.is_dark(j + k, i)).collect();
                if col == A || col == B {
                    score += 40;
                }
            }
        }
        // Rule 4: how far the dark proportion strays from half.
        let dark = self.dark.iter().filter(|d| **d).count();
        let percent = dark * 100 / (n * n);
        let deviation = (percent as i32 - 50).abs() / 5;
        score += deviation as u32 * 10;
        score
    }
}

fn mask_at(mask: u8, row: usize, col: usize) -> bool {
    let (r, c) = (row, col);
    match mask {
        0 => (r + c) % 2 == 0,
        1 => r % 2 == 0,
        2 => c % 3 == 0,
        3 => (r + c) % 3 == 0,
        4 => (r / 2 + c / 3) % 2 == 0,
        5 => (r * c) % 2 + (r * c) % 3 == 0,
        6 => ((r * c) % 2 + (r * c) % 3) % 2 == 0,
        _ => ((r + c) % 2 + (r * c) % 3) % 2 == 0,
    }
}

/// Fifteen bits: two of error correction level, three of mask, ten of BCH, the
/// whole thing XORed with a fixed pattern so that all zeroes is not a valid
/// format.
fn format_bits(mask: u8) -> u16 {
    const LEVEL_L: u16 = 0b01;
    let value = LEVEL_L << 3 | mask as u16;
    let mut rem = value << 10;
    // Divide by the BCH generator until what is left fits in ten bits.
    while 16 - rem.leading_zeros() >= 11 {
        rem ^= 0b10100110111 << (16 - rem.leading_zeros() - 11);
    }
    (value << 10 | rem) ^ 0b101010000010010
}

// ------------------------------------------------------------- rendering

/// Two module rows per character row, using the upper half block.
///
/// A terminal cell is about twice as tall as it is wide, so one module per cell
/// would come out as a rectangle twice as tall as it is wide and some scanners
/// refuse it. The upper half block paints the top half in the foreground colour
/// and the bottom half in the background colour, which puts two module rows in
/// one cell and leaves the code close to square.
///
/// The colours are stated outright rather than left to the terminal's own. Half
/// the terminals in the world have a dark background, and a code drawn in the
/// default colours on one of those is inverted: black where it should be white.
/// Most phone cameras will not read that.
pub fn render(code: &Code, quiet: usize) -> Vec<String> {
    const DARK_FG: &str = "\x1b[38;5;0m";
    const LIGHT_FG: &str = "\x1b[38;5;15m";
    const DARK_BG: &str = "\x1b[48;5;0m";
    const LIGHT_BG: &str = "\x1b[48;5;15m";
    let span = code.size + quiet * 2;
    let mut out = Vec::new();
    let mut row = 0;
    while row < span {
        let mut line = String::with_capacity(span * 12);
        for col in 0..span {
            let upper = module(code, row, col, quiet);
            // An odd span leaves the last cell's lower half with no module
            // behind it. It has to be light: a dark strip along the bottom eats
            // into the quiet zone.
            let lower = if row + 1 < span { module(code, row + 1, col, quiet) } else { false };
            line.push_str(if upper { DARK_FG } else { LIGHT_FG });
            line.push_str(if lower { DARK_BG } else { LIGHT_BG });
            line.push('\u{2580}');
        }
        line.push_str("\x1b[0m");
        out.push(line);
        row += 2;
    }
    out
}

fn module(code: &Code, row: usize, col: usize, quiet: usize) -> bool {
    if row < quiet || col < quiet {
        return false;
    }
    code.dark(row - quiet, col - quiet)
}

/// How wide and how tall the rendered block will be, so a screen can decide
/// whether it has the room BEFORE it starts drawing.
pub fn rendered_size(code: &Code, quiet: usize) -> (usize, usize) {
    let span = code.size + quiet * 2;
    (span, span.div_ceil(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The format information, against the published table.
    ///
    /// These eight strings are not this code's output written down: they are
    /// the values from the standard, cross-checked on 2026-08-25 against an
    /// independent implementation of the BCH division before either was
    /// trusted. All eight agreed. A wrong format field is the failure that
    /// looks like nothing at all, because the code is drawn perfectly and no
    /// camera can tell which mask to undo.
    #[test]
    fn the_format_field_matches_the_published_table() {
        let published = [
            0b111011111000100u16,
            0b111001011110011,
            0b111110110101010,
            0b111100010011101,
            0b110011000101111,
            0b110001100011000,
            0b110110001000001,
            0b110100101110110,
        ];
        for (mask, want) in published.iter().enumerate() {
            assert_eq!(
                format_bits(mask as u8),
                *want,
                "format bits for level L mask {mask}"
            );
        }
    }

    /// Error correction, checked the way a READER checks it rather than the
    /// way a writer builds it.
    ///
    /// A valid codeword is a multiple of the generator polynomial, which means
    /// every syndrome is zero: sum over j of codeword[j] times alpha to the
    /// power i*(n-1-j), for each i below the number of correction codewords.
    /// That is a different calculation from the long division that produced
    /// them, so agreeing is evidence rather than an echo.
    #[test]
    fn the_error_correction_satisfies_a_readers_check() {
        for &(data_len, ec_len) in &[(19usize, 7usize), (55, 15), (108, 26)] {
            let data: Vec<u8> = (0..data_len).map(|i| (i * 7 + 3) as u8).collect();
            let ec = ec_codewords(&data, ec_len);
            let mut full = data.clone();
            full.extend_from_slice(&ec);
            let n = full.len();
            for i in 0..ec_len {
                let mut syndrome = 0u8;
                for (j, &c) in full.iter().enumerate() {
                    syndrome ^= gmul(c, gpow((i * (n - 1 - j)) % 255));
                }
                assert_eq!(syndrome, 0, "syndrome {i} for a ({data_len},{ec_len}) block");
            }
            // And it must actually be wrong when the data is. A check that
            // passes on damaged input is not a check.
            full[3] ^= 0xff;
            let mut any = 0u8;
            for i in 0..ec_len {
                let mut syndrome = 0u8;
                for (j, &c) in full.iter().enumerate() {
                    syndrome ^= gmul(c, gpow((i * (n - 1 - j)) % 255));
                }
                any |= syndrome;
            }
            assert_ne!(any, 0, "a corrupted ({data_len},{ec_len}) block passed the check");
        }
    }

    fn gpow(mut e: usize) -> u8 {
        let mut v = 1u8;
        while e > 0 {
            v = gmul(v, 2);
            e -= 1;
        }
        v
    }

    /// Read a finished code back and get the original bytes out.
    ///
    /// Honest about its limits: the reader below shares this module's idea of
    /// WHICH modules are function patterns, because an independent copy of that
    /// map would be the same table typed twice. What it does establish on its
    /// own is the whole chain after that: the mask really is the one the format
    /// field advertises, the zigzag really does come back in the order it went
    /// in, and the bytes really are the bytes. The mask, the format field and
    /// the error correction are each pinned by a separate test above.
    ///
    /// The final word on this belongs to a phone camera, and that is where it
    /// went before the feature shipped.
    fn decode(code: &Code, version: usize) -> Vec<u8> {
        let size = code.size;
        let mut skeleton = Grid::new(version);
        skeleton.place_function_patterns();

        // The mask, taken from the code itself rather than remembered.
        let mut raw = 0u16;
        for i in 0..15 {
            let (r, c) = match i {
                0..=5 => (8, i),
                6 => (8, 7),
                7 => (8, 8),
                8 => (7, 8),
                _ => (14 - i, 8),
            };
            if code.dark(r, c) {
                raw |= 1 << i;
            }
        }
        let mut second = 0u16;
        for i in 0..15 {
            let (r, c) = if i < 7 { (size - 1 - i, 8) } else { (8, size - 15 + i) };
            if code.dark(r, c) {
                second |= 1 << i;
            }
        }
        assert_eq!(raw, second, "the two copies of the format field disagree");
        let value = (raw ^ 0b101010000010010) >> 10;
        assert_eq!(value >> 3, 0b01, "error correction level must read back as L");
        let mask = (value & 0b111) as u8;

        // Walk the zigzag once, collecting coordinates, then read them. Built
        // as a list rather than read in place so the ordering is a thing that
        // can be looked at, not a side effect of two nested loops.
        let mut order: Vec<(usize, usize)> = Vec::new();
        let mut col = size - 1;
        let mut upward = true;
        loop {
            if col == 6 {
                col -= 1;
            }
            for i in 0..size {
                let row = if upward { size - 1 - i } else { i };
                for c in [col, col - 1] {
                    if !skeleton.fixed[row * size + c] {
                        order.push((row, c));
                    }
                }
            }
            upward = !upward;
            if col < 2 {
                break;
            }
            col -= 2;
        }

        let mut bytes = Vec::new();
        let mut byte = 0u8;
        for (n, (r, c)) in order.iter().enumerate() {
            let mut bit = code.dark(*r, *c);
            if mask_at(mask, *r, *c) {
                bit = !bit;
            }
            byte = byte << 1 | bit as u8;
            if n % 8 == 7 {
                bytes.push(byte);
                byte = 0;
            }
        }

        assert_eq!(bytes[0] >> 4, 0b0100, "byte mode");
        let len = ((bytes[0] & 0x0f) as usize) << 4 | (bytes[1] >> 4) as usize;
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            let hi = (bytes[1 + i] & 0x0f) << 4;
            let lo = bytes[2 + i] >> 4;
            out.push(hi | lo);
        }
        out
    }

    #[test]
    fn a_wifi_credential_comes_back_out_of_its_own_code() {
        let payload = wifi_payload("Class 7B", "quiet-fox-42");
        assert_eq!(payload, "WIFI:T:WPA;S:Class 7B;P:quiet-fox-42;;");
        let code = wifi_join("Class 7B", "quiet-fox-42").expect("a normal credential must fit");
        assert_eq!(code.size, 29, "37 bytes belongs in version 3");
        let back = decode(&code, 3);
        assert_eq!(String::from_utf8_lossy(&back), payload);
    }

    /// Every version this module claims to support, actually built and read.
    #[test]
    fn all_five_versions_survive_the_round_trip() {
        for (version, size, len) in [(1usize, 21usize, 17usize), (2, 25, 32), (3, 29, 53), (4, 33, 78), (5, 37, 106)] {
            let payload: Vec<u8> = (0..len).map(|i| b'a' + (i % 26) as u8).collect();
            let code = encode(&payload).unwrap_or_else(|| panic!("version {version} payload did not fit"));
            assert_eq!(code.size, size, "wrong version chosen for {len} bytes");
            assert_eq!(decode(&code, version), payload, "version {version} round trip");
        }
    }

    /// A password with a semicolon in it. Unescaped, the phone would read the
    /// password as ending early and offer to join with the wrong key, which
    /// looks like a wrong password rather than a broken code.
    #[test]
    fn punctuation_in_a_password_is_escaped_and_survives() {
        let payload = wifi_payload("Room:5", r"pa;ss\word");
        assert_eq!(payload, r"WIFI:T:WPA;S:Room\:5;P:pa\;ss\\word;;");
        let code = wifi_join("Room:5", r"pa;ss\word").expect("must fit");
        assert_eq!(String::from_utf8_lossy(&decode(&code, 3)), payload);
    }

    /// The three corners a camera looks for, at coordinates written out rather
    /// than taken from the code that placed them.
    #[test]
    fn the_finders_and_timing_are_where_a_camera_expects_them() {
        let code = encode(b"hello").expect("fits in version 1");
        assert_eq!(code.size, 21);
        for (r0, c0) in [(0, 0), (0, 14), (14, 0)] {
            for dr in 0..7 {
                for dc in 0..7 {
                    let ring = dr == 0 || dr == 6 || dc == 0 || dc == 6;
                    let core = (2..=4).contains(&dr) && (2..=4).contains(&dc);
                    assert_eq!(
                        code.dark(r0 + dr, c0 + dc),
                        ring || core,
                        "finder at ({r0},{c0}) is wrong at ({dr},{dc})"
                    );
                }
            }
        }
        // The separator: a light ring immediately outside the top-left finder.
        for i in 0..8 {
            assert!(!code.dark(7, i), "separator row must be light at column {i}");
            assert!(!code.dark(i, 7), "separator column must be light at row {i}");
        }
        // Timing runs dark at even indices along row and column six.
        for i in 8..13 {
            assert_eq!(code.dark(6, i), i % 2 == 0, "row timing at {i}");
            assert_eq!(code.dark(i, 6), i % 2 == 0, "column timing at {i}");
        }
        // The module that is always dark, at 4 times the version plus nine.
        assert!(code.dark(13, 8), "the always-dark module is missing");
    }

    /// A payload past the ceiling is refused rather than truncated. Half a
    /// password in a code that scans perfectly is the worst of both.
    #[test]
    fn an_oversized_payload_is_refused_not_cut_short() {
        assert!(encode(&vec![b'x'; 106]).is_some());
        assert!(encode(&vec![b'x'; 107]).is_none());
    }

    /// The drawing: right shape, and a quiet zone that is genuinely light.
    #[test]
    fn the_drawing_is_the_right_shape_and_states_its_own_colours() {
        let code = encode(b"hello").expect("fits");
        let quiet = 4;
        let lines = render(&code, quiet);
        let (want_cols, want_rows) = rendered_size(&code, quiet);
        assert_eq!(want_cols, 29, "21 modules plus 4 of quiet zone each side");
        assert_eq!(lines.len(), want_rows, "two module rows per character row");
        for line in &lines {
            assert_eq!(
                line.matches('\u{2580}').count(),
                want_cols,
                "every character row is one half block per column"
            );
            // Never the terminal's own colours: on a dark theme the default
            // would draw the code inverted and no camera would read it.
            assert_eq!(line.matches("\x1b[38;5;").count(), want_cols, "a foreground per cell");
            assert_eq!(line.matches("\x1b[48;5;").count(), want_cols, "a background per cell");
        }
        // The top two module rows are quiet zone, so the first drawn row must
        // be light throughout, foreground and background alike.
        assert_eq!(lines[0].matches("\x1b[38;5;15m").count(), want_cols);
        assert_eq!(lines[0].matches("\x1b[48;5;15m").count(), want_cols);
    }
}


