//! CLI. FROZEN — do not edit as part of autoresearch.
//!
//!   polymul eval    score against the fixed fixture corpus

use std::process::exit;

use polymul::harness::eval;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
    }
    match args[1].as_str() {
        "eval" => exit(eval::run()),
        _ => usage(),
    }
}

fn usage() -> ! {
    eprintln!("usage:\n  polymul eval");
    exit(2);
}
