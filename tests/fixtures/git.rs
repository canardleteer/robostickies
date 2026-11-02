use std::{
    env,
    io::{self, Read},
    process,
};

fn main() {
    let args: Vec<_> = env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check-ignore") => {
            let mut input = Vec::new();
            io::stdin().read_to_end(&mut input).unwrap();
            assert!(input.is_empty());
            println!("GIT_PROBE_OUTPUT_MUST_BE_CAPTURED");
        }
        Some("ls-files") if !args.iter().any(|arg| arg == "--error-unmatch") => {}
        _ => process::exit(1),
    }
}
