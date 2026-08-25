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
mod sha256;
mod term;
mod dns;
mod net;
mod page;
mod qr;
mod tui;
mod fetch;
mod serve;
mod sums;

const USAGE: &str = "\
Gorilla Portable Network Hub

  hub                       open the screen, which is the way in for most people

  hub serve <folder>        hand out the files in a folder
  hub get   <url>           download a file, resuming if interrupted
  hub sums  <file>          fingerprint each piece, using every core
  hub doctor                say what this computer can and cannot do

  hub <command> --help      detail for one command

Everything works with no internet. Making a wifi network needs administrator
rights; joining one that already exists needs nothing at all.";

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
    println!("  password made  {}", if net::suggest_password().is_empty() {
        "no, this computer has no random source"
    } else {
        "yes"
    });
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
