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
mod net;
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
    println!("  password made  {}", if net::suggest_password().is_empty() {
        "no, this computer has no random source"
    } else {
        "yes"
    });
}
