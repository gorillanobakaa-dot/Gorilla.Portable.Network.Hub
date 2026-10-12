//! Which wifi radios this laptop has, what each one REALLY is, and which one
//! carries the net.
//!
//! WHY THIS EXISTS. On 2026-10-12 the net ran all afternoon on a radio
//! Windows called "802.11n USB Wireless LAN Card". It is an ALFA AWUS036NH:
//! a Ralink RT3070 behind a high-power amplifier and an external antenna,
//! bought for range, plugged in because the laptop's own Intel card had been
//! switched off. Nothing on any screen said so, and a model reading the
//! adapter list called it a cheap dongle. Windows names a USB radio after the
//! generic driver; the device says who made it in two places nobody reads:
//! the USB vendor/product id and the first three bytes of the MAC address.
//! This reads both, the same way `Documents\Scripts\WifiRadios` does, and
//! the two share their tables.
//!
//! WHICH RADIO CARRIES THE NET. Windows' Mobile Hotspot makes "Microsoft
//! Wi-Fi Direct Virtual Adapter" devices and offers no choice of radio. In
//! Plug and Play each virtual adapter is the child of exactly one physical
//! radio: that parent is the carrier. Measured on this laptop: both virtual
//! adapters hung off USB 148F:3070, the ALFA, while the AX201 was disabled.
//! Windows picks the radio itself; the owner's way of choosing the long-range
//! one was to switch the built-in card off, and the tool's `prefer` does the
//! same deliberately. The hub reports; it does not switch cards off.
//!
//! An unknown radio is reported as unknown with its ids, never guessed.

use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Radio {
    /// Windows' name: "Wi-Fi 2".
    pub name: String,
    /// What Windows calls it: the driver's words.
    pub windows_says: String,
    /// Up, Disconnected, Disabled, Not Present.
    pub status: String,
    pub mac: String,
    pub pnp: String,
    /// "USB" or "PCI".
    pub bus: String,
    /// "148F:3070" or "8086:51F0".
    pub hw_id: String,
    /// From the MAC's first three bytes.
    pub maker: String,
    pub chipset: String,
    pub bands: String,
    pub streams: String,
    /// The product when maker and chipset together name one.
    pub product: String,
    /// "long range", "built-in", "" when unknown.
    pub kind: String,
    /// A Wi-Fi Direct virtual adapter (the hotspot) is this radio's child.
    pub carries_hotspot: bool,
}

impl Radio {
    /// "ALFA AWUS036NH (Ralink RT3070)", "Intel Wi-Fi 6 AX201", or Windows' words.
    pub fn identity(&self) -> String {
        if !self.product.is_empty() && self.product != self.chipset {
            format!("{} ({})", self.product, self.chipset)
        } else if !self.product.is_empty() {
            self.product.clone()
        } else if !self.chipset.is_empty() && !self.maker.is_empty() {
            format!("{} radio, {}", self.maker, self.chipset)
        } else if !self.chipset.is_empty() {
            self.chipset.clone()
        } else {
            self.windows_says.clone()
        }
    }

    /// One line for a screen: identity, kind, bands.
    pub fn line(&self) -> String {
        let mut s = self.identity();
        if !self.kind.is_empty() {
            s.push_str(&format!(", {}", self.kind));
        }
        if !self.bands.is_empty() {
            s.push_str(&format!(", {}", self.bands));
        }
        s
    }
}

/// USB vendor:product -> (chipset, bands, streams).
const USB_CHIPS: &[(&str, &str, &str, &str)] = &[
    ("148F:3070", "Ralink RT3070", "2.4 GHz only", "1x1"),
    ("148F:3072", "Ralink RT3072", "2.4 GHz only", "2x2"),
    ("148F:5370", "Ralink RT5370", "2.4 GHz only", "1x1"),
    ("148F:5372", "Ralink RT5372", "2.4 GHz only", "2x2"),
    ("148F:3572", "Ralink RT3572", "2.4 and 5 GHz", "2x2"),
    ("148F:7601", "MediaTek MT7601U", "2.4 GHz only", "1x1"),
    ("0E8D:7610", "MediaTek MT7610U", "2.4 and 5 GHz", "1x1"),
    ("0E8D:7612", "MediaTek MT7612U", "2.4 and 5 GHz", "2x2"),
    ("0CF3:9271", "Atheros AR9271", "2.4 GHz only", "1x1"),
    ("0BDA:8187", "Realtek RTL8187", "2.4 GHz only", "1x1"),
    ("0BDA:8176", "Realtek RTL8188CUS", "2.4 GHz only", "1x1"),
    ("0BDA:8178", "Realtek RTL8192CU", "2.4 GHz only", "2x2"),
    ("0BDA:8812", "Realtek RTL8812AU", "2.4 and 5 GHz", "2x2"),
    ("0BDA:8813", "Realtek RTL8814AU", "2.4 and 5 GHz", "4x4"),
    ("0BDA:B812", "Realtek RTL8812BU", "2.4 and 5 GHz", "2x2"),
    ("0BDA:C811", "Realtek RTL8821CU", "2.4 and 5 GHz", "1x1"),
    ("0BDA:F192", "Realtek RTL8188FTV", "2.4 GHz only", "1x1"),
    ("2357:010C", "Realtek RTL8192EU", "2.4 GHz only", "2x2"),
    ("2357:0115", "Realtek RTL8812AU", "2.4 and 5 GHz", "2x2"),
];

/// PCI vendor:device -> (chipset, bands, streams): the built-in card.
const PCI_CHIPS: &[(&str, &str, &str, &str)] = &[
    ("8086:51F0", "Intel Wi-Fi 6 AX201", "2.4 and 5 GHz", "2x2"),
    ("8086:A0F0", "Intel Wi-Fi 6 AX201", "2.4 and 5 GHz", "2x2"),
    ("8086:2723", "Intel Wi-Fi 6 AX200", "2.4 and 5 GHz", "2x2"),
    ("8086:2725", "Intel Wi-Fi 6E AX210", "2.4, 5 and 6 GHz", "2x2"),
    ("8086:7AF0", "Intel Wi-Fi 6E AX211", "2.4, 5 and 6 GHz", "2x2"),
    ("8086:51F1", "Intel Wi-Fi 6E AX211", "2.4, 5 and 6 GHz", "2x2"),
    ("8086:24FD", "Intel Wireless-AC 8265", "2.4 and 5 GHz", "2x2"),
    ("8086:095A", "Intel Wireless-AC 7265", "2.4 and 5 GHz", "2x2"),
    ("168C:0032", "Qualcomm Atheros AR9485", "2.4 GHz only", "1x1"),
    ("10EC:8821", "Realtek RTL8821AE", "2.4 and 5 GHz", "1x1"),
    ("10EC:C821", "Realtek RTL8821CE", "2.4 and 5 GHz", "1x1"),
    ("10EC:B822", "Realtek RTL8822BE", "2.4 and 5 GHz", "2x2"),
    ("14C3:7961", "MediaTek MT7921", "2.4, 5 and 6 GHz", "2x2"),
];

/// The MAC's first three bytes: who made the radio.
const OUI: &[(&str, &str)] = &[
    ("00:C0:CA", "ALFA Network"),
    ("00:0C:43", "Ralink"),
    ("00:E0:4C", "Realtek"),
    ("50:C7:BF", "TP-Link"),
    ("C4:E9:84", "TP-Link"),
    ("E8:DE:27", "TP-Link"),
    ("30:B5:C2", "TP-Link"),
    ("00:1F:1F", "Edimax"),
    ("80:1F:02", "Edimax"),
    ("9C:EF:D5", "Panda Wireless"),
    ("E8:4E:06", "Edup"),
];

/// Maker + chipset -> (product, kind). Only pairs that name one product.
const PRODUCTS: &[(&str, &str, &str, &str)] = &[
    ("ALFA Network", "Ralink RT3070", "ALFA AWUS036NH", "long range"),
    ("ALFA Network", "Atheros AR9271", "ALFA AWUS036NHA", "long range"),
    ("ALFA Network", "Realtek RTL8187", "ALFA AWUS036H", "long range"),
    ("ALFA Network", "Realtek RTL8812AU", "ALFA AWUS036ACH", "long range, dual band"),
    ("ALFA Network", "MediaTek MT7612U", "ALFA AWUS036ACM", "dual band"),
    ("ALFA Network", "Realtek RTL8814AU", "ALFA AWUS1900", "long range, dual band"),
    ("Panda Wireless", "Ralink RT5372", "Panda PAU06", "small"),
];

fn hw_id_of(pnp: &str) -> (String, String) {
    let up = pnp.to_ascii_uppercase();
    for (bus, v, p) in [("USB", "VID_", "&PID_"), ("PCI", "VEN_", "&DEV_")] {
        if let Some(i) = up.find(v) {
            let vendor = &up[i + 4..(i + 8).min(up.len())];
            if let Some(j) = up.find(p) {
                let dev = &up[j + 5..(j + 9).min(up.len())];
                if vendor.len() == 4 && dev.len() == 4 {
                    return (bus.to_string(), format!("{vendor}:{dev}"));
                }
            }
        }
    }
    (String::new(), String::new())
}

fn maker_of(mac: &str) -> String {
    let m = mac.to_ascii_uppercase().replace('-', ":");
    let oui = m.get(..8).unwrap_or("");
    OUI.iter().find(|(o, _)| *o == oui).map(|(_, n)| n.to_string()).unwrap_or_default()
}

pub(crate) fn identify(mut r: Radio) -> Radio {
    let (bus, id) = hw_id_of(&r.pnp);
    r.bus = bus;
    r.hw_id = id;
    r.maker = maker_of(&r.mac);
    let table: &[(&str, &str, &str, &str)] = match r.bus.as_str() {
        "USB" => USB_CHIPS,
        "PCI" => PCI_CHIPS,
        _ => &[],
    };
    if let Some((_, chip, bands, streams)) = table.iter().find(|(h, ..)| *h == r.hw_id) {
        r.chipset = chip.to_string();
        r.bands = bands.to_string();
        r.streams = streams.to_string();
    }
    if r.bus == "PCI" && !r.chipset.is_empty() {
        r.product = r.chipset.clone();
        r.kind = "built-in".into();
        if r.maker.is_empty() {
            r.maker = r.chipset.split(' ').next().unwrap_or("").to_string();
        }
    }
    if let Some((_, _, product, kind)) = PRODUCTS.iter().find(|(m, c, ..)| *m == r.maker && *c == r.chipset) {
        r.product = product.to_string();
        r.kind = kind.to_string();
    }
    r
}

/// Physical radios from the adapter list (the tool's JSON shape: Name,
/// Description, Status, Mac, PnP, Parent), each Wi-Fi Direct virtual adapter
/// folded under its parent.
pub(crate) fn build(adapters: &crate::json::J) -> Vec<Radio> {
    let mut physical: Vec<Radio> = Vec::new();
    let mut virtual_parents: Vec<String> = Vec::new();
    for a in adapters.arr() {
        let pnp = a.get("PnP").str().to_string();
        let desc = a.get("Description").str().to_string();
        if pnp.to_ascii_lowercase().contains("vwifimp") || desc.contains("Virtual") {
            virtual_parents.push(a.get("Parent").str().to_ascii_uppercase());
            continue;
        }
        physical.push(identify(Radio {
            name: a.get("Name").str().to_string(),
            windows_says: desc,
            status: a.get("Status").str().to_string(),
            mac: a.get("Mac").str().to_string(),
            pnp,
            ..Default::default()
        }));
    }
    for r in &mut physical {
        if virtual_parents.iter().any(|p| *p == r.pnp.to_ascii_uppercase()) {
            r.carries_hotspot = true;
        }
    }
    physical
}

const PS_ADAPTERS: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
$out = @()
foreach ($a in Get-NetAdapter -IncludeHidden) {
  if ($a.InterfaceDescription -notmatch 'Wi-Fi|Wireless|802\.11|WLAN') { continue }
  $o = [ordered]@{ Name=$a.Name; Description=$a.InterfaceDescription; Status=[string]$a.Status; Mac=$a.MacAddress; PnP=$a.PnPDeviceID; Parent='' }
  if ($a.PnPDeviceID) { $o.Parent = (Get-PnpDeviceProperty -InstanceId $a.PnPDeviceID -KeyName DEVPKEY_Device_Parent).Data }
  $out += [pscustomobject]$o
}
if ($out.Count -eq 1) { '[' + ($out | ConvertTo-Json -Depth 3) + ']' } else { $out | ConvertTo-Json -Depth 3 }
"#;

/// The radios, read from Windows now. Slow (a PowerShell start): the screens
/// use `snapshot`, which refreshes in the background.
#[cfg(windows)]
pub fn read() -> Vec<Radio> {
    let out = crate::net::command("powershell")
        .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", PS_ADAPTERS])
        .stdin(std::process::Stdio::null())
        .output();
    let Ok(out) = out else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stdout);
    crate::json::parse(text.trim()).map(|j| build(&j)).unwrap_or_default()
}

#[cfg(not(windows))]
pub fn read() -> Vec<Radio> {
    Vec::new()
}

struct Cache {
    at: Option<Instant>,
    radios: Vec<Radio>,
    refreshing: bool,
}

static CACHE: Mutex<Cache> = Mutex::new(Cache { at: None, radios: Vec::new(), refreshing: false });
const FRESH_FOR: Duration = Duration::from_secs(20);

/// The radios as last read, refreshed on a thread when older than 20 s. The
/// first call returns nothing and starts the read; a screen drawn every frame
/// never waits on PowerShell.
pub fn snapshot() -> Vec<Radio> {
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let stale = c.at.map(|t| t.elapsed() > FRESH_FOR).unwrap_or(true);
    if stale && !c.refreshing && cfg!(windows) {
        c.refreshing = true;
        drop(c);
        std::thread::Builder::new()
            .name("radios".into())
            .spawn(|| {
                let radios = read();
                let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
                c.radios = radios;
                c.at = Some(Instant::now());
                c.refreshing = false;
            })
            .ok();
        return CACHE.lock().unwrap_or_else(|e| e.into_inner()).radios.clone();
    }
    c.radios.clone()
}

/// The radio carrying the net, if Windows has made the hotspot's adapter,
/// else the radio that is up.
pub fn carrier() -> Option<Radio> {
    let rs = snapshot();
    rs.iter().find(|r| r.carries_hotspot).or_else(|| rs.iter().find(|r| r.status == "Up")).cloned()
}

/// One line for the status bar: "net on ALFA AWUS036NH (Ralink RT3070), long range, 2.4 GHz only".
pub fn carrier_line() -> Option<String> {
    let r = carrier()?;
    Some(format!("{} {}", if r.carries_hotspot { "net on" } else { "wifi:" }, r.line()))
}

/// What to put in front of the card summary on the fix screen: the identity
/// when Windows' name hides it.
pub fn identity_prefix() -> Option<String> {
    let r = carrier()?;
    if r.product.is_empty() && r.chipset.is_empty() {
        return None;
    }
    let mut s = r.line();
    if r.identity() != r.windows_says {
        s.push_str(&format!(" (Windows calls it \"{}\")", r.windows_says));
    }
    s.push_str(". ");
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded 2026-10-12 on this laptop: the ALFA on USB carrying the
    /// hotspot (two Wi-Fi Direct children), the Intel card disabled.
    const ADAPTERS: &str = r#"[
      {"Name":"Local Area Connection* 3","Description":"Microsoft Wi-Fi Direct Virtual Adapter #3","Status":"Disconnected","Mac":"02-C0-CA-58-E0-5F","PnP":"{5d624f94-8850-40c3-a3fa-a4fd2080baf3}\\vwifimp_wfd\\6&1704cc16&0&13","Parent":"USB\\VID_148F&PID_3070\\1.0"},
      {"Name":"Local Area Connection* 4","Description":"Microsoft Wi-Fi Direct Virtual Adapter #4","Status":"Disconnected","Mac":"12-C0-CA-58-E0-5F","PnP":"{5d624f94-8850-40c3-a3fa-a4fd2080baf3}\\vwifimp_wfd\\6&1704cc16&0&14","Parent":"USB\\VID_148F&PID_3070\\1.0"},
      {"Name":"Wi-Fi","Description":"Intel(R) Wi-Fi 6 AX201 160MHz","Status":"Disabled","Mac":"78-AF-08-EB-18-2E","PnP":"PCI\\VEN_8086&DEV_51F0&SUBSYS_00708086&REV_01\\3&11583659&1&A3","Parent":"PCI\\VEN_8086&DEV_51B8"},
      {"Name":"Wi-Fi 2","Description":"802.11n USB Wireless LAN Card","Status":"Up","Mac":"00-C0-CA-58-E0-5F","PnP":"USB\\VID_148F&PID_3070\\1.0","Parent":"USB\\ROOT_HUB30\\4&25b1e1ce&0&0"}
    ]"#;

    #[test]
    fn the_usb_radio_is_named_for_what_it_is_and_the_hotspot_is_traced_to_it() {
        let j = crate::json::parse(ADAPTERS).expect("json");
        let rs = build(&j);
        assert_eq!(rs.len(), 2, "virtual adapters fold under their parent: {rs:?}");
        let alfa = rs.iter().find(|r| r.name == "Wi-Fi 2").unwrap();
        assert_eq!(alfa.identity(), "ALFA AWUS036NH (Ralink RT3070)");
        assert_eq!((alfa.bus.as_str(), alfa.hw_id.as_str(), alfa.maker.as_str()), ("USB", "148F:3070", "ALFA Network"));
        assert_eq!(alfa.line(), "ALFA AWUS036NH (Ralink RT3070), long range, 2.4 GHz only");
        assert!(alfa.carries_hotspot);
        let intel = rs.iter().find(|r| r.name == "Wi-Fi").unwrap();
        assert_eq!(intel.identity(), "Intel Wi-Fi 6 AX201");
        assert_eq!((intel.kind.as_str(), intel.status.as_str()), ("built-in", "Disabled"));
        assert!(!intel.carries_hotspot);
    }

    #[test]
    fn an_unknown_radio_keeps_windows_words_and_its_ids() {
        let r = identify(Radio { windows_says: "Some Wireless Adapter".into(), mac: "AA-BB-CC-00-11-22".into(), pnp: "USB\\VID_1234&PID_ABCD\\1".into(), ..Default::default() });
        assert_eq!(r.identity(), "Some Wireless Adapter");
        assert_eq!(r.hw_id, "1234:ABCD");
        assert!(r.product.is_empty() && r.chipset.is_empty() && r.kind.is_empty());
    }
}
