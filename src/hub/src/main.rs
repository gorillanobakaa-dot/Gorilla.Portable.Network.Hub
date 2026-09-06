// Version: 0.1.0 · updated 26-08-24-21-45
//
// Gorilla Portable Network Hub.
//
// A laptop that creates a network where none exists and hands a folder to every
// device in the room. Built for classrooms with no internet, no router and often
// no mains power.
//
// One binary, three subcommands. As three separate executables the same code
// came to 1,233 KB because each carried its own Rust runtime, panic machinery
// and SHA-256. Merged, they share one copy. On the connections this is for,
// bytes are minutes of somebody's life.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

mod sha256;
mod term;
mod dns;
mod net;
mod cable;
mod dhcp;
mod tune;
mod page;
mod qr;
mod zip;
mod tui;
mod fetch;
mod serve;
mod sums;

const USAGE: &str = "\
Gorilla Portable Network Hub

  hub                       open the screen, which is the way in for most people

  hub serve <folder>        hand out the files in a folder
  hub cable <folder>        hand a folder down a cable to one other computer
  hub cable-get             find the computer on the other end and take it all
  hub get   <url>           download a file, resuming if interrupted
  hub sums  <file>          fingerprint each piece, using every core
  hub doctor                say what this computer can and cannot do
  hub fix-firewall          let this computer be reached from the network

  hub <command> --help      detail for one command

Everything works with no internet. Making a wifi network needs administrator
rights; joining one that already exists needs nothing at all. A cable between
two computers needs neither, and is the fastest way to move a lot at once.";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // No arguments opens the screen rather than printing usage and stopping.
    //
    // This is the whole difference between a tool for the person who wrote it
    // and a tool for a teacher. On Windows the program is a file on a memory
    // stick that gets double-clicked: that passes no arguments, and a usage
    // message in a console window that closes again is indistinguishable from
    // the program being broken.
    if args.len() < 2 {
        tui::run();
        return;
    }
    // The subcommand is dropped and argv[0] kept, so each module sees exactly
    // the argument shape it saw when it was its own program.
    let mut rest: Vec<String> = vec![args[0].clone()];
    rest.extend_from_slice(&args[2..]);

    match args[1].as_str() {
        "serve" => serve::run(rest),
        "cable" | "cable-send" => cable_send(rest),
        "cable-get" | "cable-receive" => cable_get(rest),
        "fix-firewall" => tune::fix_firewall(),
        "get" | "fetch" => fetch::run(rest),
        "sums" => sums::run(rest),
        "screen" | "tui" => tui::run(),
        "doctor" => doctor(),
        "-h" | "--help" | "help" => println!("{USAGE}"),
        "-V" | "--version" | "version" => println!("hub {}", env!("CARGO_PKG_VERSION")),
        other => {
            eprintln!("Not a command: {other}\n");
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}

/// What this computer can do, in one screen, for when it will not work and
/// there is nobody nearby to ask.
///
/// Every line is something that has actually gone wrong: a wifi card that can
/// join a network but not create one, a terminal reporting no size, a machine
/// with no route off itself. Guessing at those over a bad phone line is
/// hopeless; reading them out is not.
fn doctor() {
    println!("hub {}", env!("CARGO_PKG_VERSION"));
    println!("  built for      {}", std::env::consts::OS);
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0);
    println!("  processors     {threads}");
    println!("  devices at once (default)  {}", serve::default_helpers());
    let (rows, cols) = term::size();
    println!("  window         {rows} rows by {cols} columns");
    match net::wifi_interface() {
        Some(i) => println!("  wifi adapter   {i}"),
        None => println!("  wifi adapter   none found, so this computer cannot make a network"),
    }
    match net::default_gateway() {
        // On Linux this is read from the kernel's routing table. Everywhere
        // else it is ".1 of whatever subnet we are on", which is right for
        // every hotspot and most home routers and wrong for a network with a
        // mask wider than a /24. Said out loud rather than presented as fact.
        Some(g) if cfg!(target_os = "linux") => println!("  gateway        {g}"),
        Some(g) => println!("  gateway        {g} (a guess, not read from the system)"),
        None => println!("  gateway        none, this computer has no route off itself"),
    }
    let addrs = net::local_addresses();
    if addrs.is_empty() {
        println!("  addresses      none, so nobody can reach this computer");
    }
    for a in addrs {
        println!("  address        {a}");
    }
    // Only when this machine looks like it IS the hotspot. Listing "everyone on
    // the network" from inside an office would print the whole building.
    let hotspot_here = net::local_addresses().into_iter().find(|a| {
        let o = a.octets();
        (o[0] == 10 && o[1] == 42) || (o[0] == 192 && o[1] == 168 && (o[2] == 137 || o[2] == 43))
    });
    if let Some(ours) = hotspot_here {
        let joined = net::joined_devices(ours);
        println!("  on your network {} device(s)", joined.len());
        for j in &joined {
            // Lease name if this is running as root, otherwise ask the network,
            // which is what the screen does and what needs no privileges.
            let named = j.name.clone().or_else(|| {
                dns::reverse_lookup(j.ip, ours, std::time::Duration::from_millis(500))
                    .map(|f| dns::short_name(&f))
            });
            match named {
                Some(n) => println!("    {:<16} {n}", j.ip.to_string()),
                None => println!("    {:<16} (no name; it did not give one)", j.ip.to_string()),
            }
        }
        if joined.is_empty() {
            println!("    nothing has joined yet");
        }
    }
    // The cable half, which is new in 0.9.0 and is the half that fails
    // silently: a firewall drops the packets and everything still looks fine.
    let w = tune::wire();
    if w.measured {
        println!("  cable          {} at {} Mbps, {}", w.adapter, w.megabits, w.cable);
    } else {
        println!("  cable          speed not readable, so 1 Gbps is assumed");
    }
    println!("  sending pieces of {} KB at a time", w.chunk / 1024);
    match tune::reachable() {
        Some(true) => println!("  reachable      yes, the firewall lets others in"),
        Some(false) => println!("  reachable      NO. Run: hub fix-firewall"),
        None => println!("  reachable      cannot tell on this system; try it and see"),
    }
    let addrs_now = net::local_addresses();
    if dhcp::safe_to_offer(&addrs_now, net::default_gateway()) {
        println!("  cable addresses  this looks like a bare cable, so addresses can be given out");
    } else {
        println!("  cable addresses  not offered here, which is correct on a network with a router");
    }

    println!("  password made  {}", if net::suggest_password().is_empty() {
        "no, this computer has no random source"
    } else {
        "yes"
    });
}

/// Hand a folder down a cable to one other computer.
///
/// This is `serve` with two extra things running beside it, and it is a
/// separate command rather than a flag because the situation is different
/// enough to deserve its own name and its own help. On wifi the network
/// already exists and somebody else runs it. On a cable there is no network
/// until this makes one, and the two jobs a router would normally do have to
/// be done here: saying where this computer is (cable.rs) and giving the other
/// end an address (dhcp.rs).
///
/// Both are started before serving and stopped when serving returns.
fn cable_send(args: Vec<String>) {
    const USAGE: &str = "\
hub cable  -  hand a folder down a cable to one other computer

  hub cable <folder> [options]

  --name <text>         what to call this computer on the other end's screen
  --port <number>       which port to listen on (default 8080)
  --addresses-anyway    give out addresses even though this computer is on
                        another network. Read the warning first: on a network
                        that already has a router this can take it down for
                        everyone on it. Only for a machine you are testing.
  -h, --help            this text

  example:
    hub cable ~/lessons

  Plug an ordinary network cable between the two computers. No router, no
  switch and no internet are needed, and laptops made since about 2005 do not
  need a crossover cable. On the other computer either run `hub cable-get`, or
  open the address this prints in any web browser.

  Give the computers about half a minute after plugging the cable in. Both ends
  have to settle on an address before anything can happen, and Windows and Mac
  take that long to give up waiting for a router that is not there.";

    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return;
    }
    let folder = match args.iter().skip(1).find(|a| !a.starts_with('-')) {
        Some(f) => f.clone(),
        None => {
            eprintln!("Which folder should be handed out?\n");
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };
    let name = flag(&args, "--name").unwrap_or_else(|| {
        std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "this computer".into())
    });
    let port: u16 = flag(&args, "--port").and_then(|v| v.parse().ok()).unwrap_or(8080);

    // Naming the sender switches the served page from the class page to the
    // accept page. Set before serving starts so the first request already
    // gets it.
    crate::page::set_sender(&name);

    let stop = Arc::new(AtomicBool::new(false));
    let short = std::path::Path::new(&folder)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| folder.clone());

    let addresses = net::local_addresses();
    let gateway = net::default_gateway();

    println!("Handing out {short} down the cable.");
    let wire = tune::wire();
    if wire.measured {
        println!("  cable          {} at {} Mbps, {}", wire.adapter, wire.megabits, wire.cable);
    } else {
        println!("  cable          speed not readable, assuming 1 Gbps");
    }

    match cable::start_beacon(port, name, short.clone(), stop.clone()) {
        Ok(_) => println!("  finding        saying where this computer is, once a second"),
        Err(e) => println!("  finding        not announcing ({e}); type the address by hand"),
    }

    let ours = addresses
        .iter()
        .copied()
        .find(|a| a.is_link_local())
        .unwrap_or(std::net::Ipv4Addr::new(169, 254, 1, 1));
    // Names before addresses, because the address lease has to say whether a
    // resolver is running, and it only knows that once one has tried to start.
    //
    // Behind the SAME guard as the address server, and that is not caution for
    // its own sake. This resolver answers every name with our own address,
    // which is right on a cable with two machines and nothing else, and on a
    // real network is a machine claiming to be every host on the internet.
    let anyway = args.iter().any(|a| a == "--addresses-anyway");
    if anyway && !dhcp::safe_to_offer(&addresses, gateway) {
        println!();
        println!("  !! This computer is on another network as well as the cable, and you");
        println!("  !! have asked for addresses to be given out regardless. If that other");
        println!("  !! network has a router, this can take it down for everyone on it.");
        println!();
    }
    let bare_cable = anyway || dhcp::safe_to_offer(&addresses, gateway);
    let naming = bare_cable && dns::start(ours, stop.clone()).is_ok();
    match dhcp::start(ours, &addresses, gateway, naming, anyway, stop.clone()) {
        Ok(_) => println!("  addresses      giving the other computer an address if it asks"),
        Err(e) => println!("  addresses      {e}"),
    }
    if naming {
        println!("  names          they can type  gorilla/  instead of an address");
        println!("                 and their computer should offer to open this page itself");
    } else {
        // Not a warning. On Windows this is nearly always available; on Linux
        // and macOS port 53 needs root, and the transfer works without it.
        println!("  names          not answering names, so the address has to be typed");
    }

    // The address is NOT printed here.
    //
    // It cannot be known yet: whether the page ends up on port 80 or on 8080
    // depends on a bind that has not happened, and serve() prints the right
    // one a moment later. Printing a guess here produced two addresses one
    // above the other, differing by a port, one of them wrong, on the single
    // line a person is meant to read out to somebody else.
    if !addresses.iter().any(|a| a.is_link_local()) {
        println!("  open this      no cable address yet. Wait half a minute and look again.");
    }
    println!();

    // serve does the rest, and blocks. It is given the argument shape it
    // expects rather than this command's, because it parses its own flags.
    let serve_args = vec![args[0].clone(), folder, "--port".into(), port.to_string()];
    serve::run(serve_args);
    stop.store(true, Ordering::Relaxed);
}

/// Find the computer on the other end of the cable and take everything it has.
///
/// The point of this command is that nothing has to be typed. The other end is
/// saying where it is once a second; this listens, and then does what the
/// receive screen does, without anybody having to read an IP address off one
/// screen and type it into another.
fn cable_get(args: Vec<String>) {
    const USAGE: &str = "\
hub cable-get  -  take everything from the computer on the other end of a cable

  hub cable-get [options]

  --into <folder>       where to put what arrives (default: the current folder)
  --wait <seconds>      how long to listen before giving up (default 60)
  -h, --help            this text

  Plug an ordinary network cable between the two computers and run
  `hub cable <folder>` on the other one. Nothing needs to be typed here: the
  other computer says where it is, and this finds it.";

    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return;
    }
    let into = flag(&args, "--into").unwrap_or_else(|| ".".into());
    let wait: u64 = flag(&args, "--wait").and_then(|v| v.parse().ok()).unwrap_or(60);

    println!("Listening for the other computer. Up to {wait} seconds.");
    let peer = match cable::find_peer(Duration::from_secs(wait)) {
        Ok(Some(p)) => p,
        Ok(None) => {
            eprintln!("Nothing answered in {wait} seconds.");
            eprintln!();
            eprintln!("Things to check, in the order worth checking them:");
            eprintln!("  the cable is in both computers and the little light is on");
            eprintln!("  `hub cable <folder>` is running on the other computer");
            eprintln!("  both computers have been plugged in for half a minute");
            eprintln!("  the firewall here is not blocking it: run `hub fix-firewall`");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Cannot listen for the other computer: {e}");
            eprintln!("Something else may already be using port {}.", cable::BEACON_PORT);
            std::process::exit(1);
        }
    };

    println!("Found {} at {}, handing out {}.", peer.name, peer.ip, peer.folder);
    let files = match net::list_files(peer.ip, peer.port) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Found the computer but cannot read its list of files: {e}");
            std::process::exit(1);
        }
    };
    if files.is_empty() {
        println!("It is not handing out any files yet.");
        return;
    }
    if let Err(e) = std::fs::create_dir_all(&into) {
        eprintln!("Cannot make the folder {into}: {e}");
        std::process::exit(1);
    }

    println!("{} file(s) to fetch into {into}.", files.len());
    let mut failed = Vec::new();
    for (i, f) in files.iter().enumerate() {
        // The slash matters. url_path() escapes the name and does not add
        // one, so without it the host and the filename run together into
        // "http://169.254.1.1:8080lesson.pdf", which is not an address.
        let url = format!("http://{}:{}/{}", peer.ip, peer.port, fetch::url_path(&f.name));
        let out = std::path::Path::new(&into).join(&f.name);
        if let Some(parent) = out.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        println!("  [{}/{}] {}", i + 1, files.len(), f.name);
        if let Err(e) = fetch::download(&url, &out.to_string_lossy(), 4, false) {
            eprintln!("        did not arrive: {e}");
            failed.push(f.name.clone());
        }
    }
    println!();
    if failed.is_empty() {
        println!("All {} file(s) arrived.", files.len());
    } else {
        println!("{} of {} did not arrive:", failed.len(), files.len());
        for f in &failed {
            println!("  {f}");
        }
        println!("Run the same command again. What already arrived is not fetched twice.");
        std::process::exit(1);
    }
}

/// Read a `--flag value` pair out of the arguments.
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

#[cfg(test)]
mod packaging_tests {
    /// The version in every packaging recipe has to match the crate's.
    ///
    /// A stale version in a packaging file is not a loud failure. `makepkg`
    /// succeeds, `dpkg-deb` succeeds, the package installs, and the only sign
    /// is a version string nobody reads. The sibling project let its PKGBUILD
    /// drift twenty-six releases behind while its README pointed Arch users
    /// straight at it, so following the project's own instructions built
    /// something from the previous month.
    ///
    /// This test is the only reason those numbers can be trusted.
    #[test]
    fn every_packaging_recipe_names_this_version() {
        let version = env!("CARGO_PKG_VERSION");
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

        let pkgbuild = std::fs::read_to_string(format!("{root}/packaging/PKGBUILD"))
            .expect("packaging/PKGBUILD must exist");
        let pkgver = pkgbuild
            .lines()
            .find_map(|l| l.strip_prefix("pkgver="))
            .expect("PKGBUILD must set pkgver");
        assert_eq!(pkgver, version, "packaging/PKGBUILD pkgver has drifted");

        let deb = std::fs::read_to_string(format!("{root}/packaging/build-deb.sh"))
            .expect("packaging/build-deb.sh must exist");
        assert!(
            deb.contains(&format!("VERSION=${{VERSION:-{version}}}")),
            "packaging/build-deb.sh default version has drifted from {version}"
        );

        // The guide tells people which file to download by name. A version in
        // prose drifts exactly as quietly as one in a recipe, and it is the
        // line a person types.
        let howto = std::fs::read_to_string(format!("{root}/docs/HOW-TO.md"))
            .expect("docs/HOW-TO.md must exist");
        assert!(
            howto.contains(&format!("hub {version}")),
            "docs/HOW-TO.md does not tell people to expect version {version}"
        );
    }
}
