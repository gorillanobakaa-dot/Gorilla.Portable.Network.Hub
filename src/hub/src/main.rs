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
mod fetch;
mod serve;
mod sums;

const USAGE: &str = "\
Gorilla Portable Network Hub

  hub serve <folder>        hand out the files in a folder
  hub get   <url>           download a file, resuming if interrupted
  hub sums  <file>          checksum each piece, using every core

  hub <command> --help      detail for one command

Everything works with no internet. The machine handing files out needs
administrator rights once; the machines receiving them need nothing at all.";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        println!("{USAGE}");
        std::process::exit(2);
    }
    // The subcommand is dropped and argv[0] kept, so each module sees exactly
    // the argument shape it saw when it was its own program.
    let mut rest: Vec<String> = vec![args[0].clone()];
    rest.extend_from_slice(&args[2..]);

    match args[1].as_str() {
        "serve" => serve::run(rest),
        "get" | "fetch" => fetch::run(rest),
        "sums" => sums::run(rest),
        "-h" | "--help" | "help" => println!("{USAGE}"),
        "-V" | "--version" | "version" => println!("hub {}", env!("CARGO_PKG_VERSION")),
        other => {
            eprintln!("Not a command: {other}\n");
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    }
}
