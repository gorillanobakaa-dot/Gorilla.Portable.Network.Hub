//! Just enough JSON to read what a hub's class net answers (room.rs writes
//! it). The window joining another hub's chat (tui/member.rs) is the reader.
//! Hand-written, as room.rs's writer is: the shapes are small and ours.

#[derive(Clone, Debug, PartialEq)]
pub enum J {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    pub fn get(&self, key: &str) -> &J {
        match self {
            J::Obj(kv) => kv.iter().find(|(k, _)| k == key).map(|(_, v)| v).unwrap_or(&J::Null),
            _ => &J::Null,
        }
    }
    pub fn str(&self) -> &str {
        match self {
            J::Str(s) => s,
            _ => "",
        }
    }
    pub fn num(&self) -> u64 {
        match self {
            J::Num(n) if *n >= 0.0 => *n as u64,
            _ => 0,
        }
    }
    pub fn bool(&self) -> bool {
        matches!(self, J::Bool(true))
    }
    pub fn arr(&self) -> &[J] {
        match self {
            J::Arr(a) => a,
            _ => &[],
        }
    }
    pub fn entries(&self) -> &[(String, J)] {
        match self {
            J::Obj(kv) => kv,
            _ => &[],
        }
    }
}

pub fn parse(s: &str) -> Option<J> {
    let mut p = P { b: s.as_bytes(), i: 0 };
    let v = p.value()?;
    p.ws();
    (p.i == p.b.len()).then_some(v)
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: u8) -> Option<()> {
        self.ws();
        (self.b.get(self.i) == Some(&c)).then(|| self.i += 1)
    }
    fn lit(&mut self, word: &str, v: J) -> Option<J> {
        self.b[self.i..].starts_with(word.as_bytes()).then(|| {
            self.i += word.len();
            v
        })
    }
    fn value(&mut self) -> Option<J> {
        self.ws();
        match *self.b.get(self.i)? {
            b'{' => {
                self.i += 1;
                let mut kv = Vec::new();
                if self.eat(b'}').is_some() {
                    return Some(J::Obj(kv));
                }
                loop {
                    self.ws();
                    let k = self.string()?;
                    self.eat(b':')?;
                    let v = self.value()?;
                    kv.push((k, v));
                    if self.eat(b',').is_some() {
                        continue;
                    }
                    self.eat(b'}')?;
                    return Some(J::Obj(kv));
                }
            }
            b'[' => {
                self.i += 1;
                let mut a = Vec::new();
                if self.eat(b']').is_some() {
                    return Some(J::Arr(a));
                }
                loop {
                    a.push(self.value()?);
                    if self.eat(b',').is_some() {
                        continue;
                    }
                    self.eat(b']')?;
                    return Some(J::Arr(a));
                }
            }
            b'"' => self.string().map(J::Str),
            b't' => self.lit("true", J::Bool(true)),
            b'f' => self.lit("false", J::Bool(false)),
            b'n' => self.lit("null", J::Null),
            _ => {
                let start = self.i;
                while self.i < self.b.len() && matches!(self.b[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.i += 1;
                }
                std::str::from_utf8(&self.b[start..self.i]).ok()?.parse().ok().map(J::Num)
            }
        }
    }
    fn hex4(&mut self) -> Option<u32> {
        let h = std::str::from_utf8(self.b.get(self.i..self.i + 4)?).ok()?;
        self.i += 4;
        u32::from_str_radix(h, 16).ok()
    }
    fn string(&mut self) -> Option<String> {
        if self.b.get(self.i) != Some(&b'"') {
            return None;
        }
        self.i += 1;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let c = *self.b.get(self.i)?;
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let e = *self.b.get(self.i)?;
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'u' => {
                            let hi = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&hi) && self.b.get(self.i..self.i + 2) == Some(b"\\u") {
                                self.i += 2;
                                let lo = self.hex4()?;
                                0x10000 + ((hi - 0xD800) << 10) + (lo.wrapping_sub(0xDC00) & 0x3FF)
                            } else {
                                hi
                            };
                            char::from_u32(code).unwrap_or('\u{fffd}')
                        }
                        _ => return None,
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                _ => out.push(c),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What room.rs writes, read back: the round trip the joining window
    /// depends on, including the escapes that keep a message out of a
    /// script tag.
    #[test]
    fn reads_what_the_room_writes() {
        let s = format!(
            "{{\"v\":7,\"me\":{{\"key\":\"ip:1\",\"op\":false}},\"text\":{},\"lines\":[{{\"id\":2,\"kind\":\"text\"}}],\"check\":null}}",
            crate::room::json_str("</script> \"hi\" \u{270b} \u{1f600}")
        );
        let j = parse(&s).expect("parses");
        assert_eq!(j.get("v").num(), 7);
        assert_eq!(j.get("me").get("key").str(), "ip:1");
        assert!(!j.get("me").get("op").bool());
        assert_eq!(j.get("text").str(), "</script> \"hi\" \u{270b} \u{1f600}");
        assert_eq!(j.get("lines").arr()[0].get("id").num(), 2);
        assert_eq!(j.get("check"), &J::Null);
        assert!(parse("{\"a\":").is_none());
    }
}
