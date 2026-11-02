use std::{env, fs, io::Write, path::Path, process};

fn main() {
    let exe = env::current_exe().unwrap();
    let role = exe.file_stem().unwrap().to_str().unwrap();
    let args: Vec<_> = env::args().skip(1).collect();
    let action = args.first().map(String::as_str).unwrap_or("");
    let mut log = fs::OpenOptions::new().create(true).append(true).open(env::var_os("FIXTURE_LOG").unwrap()).unwrap();
    writeln!(log, "{role}\t{}", args.join("\t")).unwrap();
    if args.iter().any(|s| s == "--version") {
        if env::var("FIXTURE_UNUSABLE").ok().as_deref() == Some(role) {
            eprintln!("fixture loader failed");
            process::exit(7);
        }
        println!("fixture 1.0");
        return;
    }
    if role == "cargo" && action == "pkgid" {
        match env::var("FIXTURE_PKGID").as_deref() {
            Ok("failed") => { eprintln!("fixture package missing"); process::exit(7); }
            Ok("non-utf8") => { std::io::stdout().write_all(&[0xff]).unwrap(); }
            _ => println!("path+file:///fixture#robostickies@0.1.0"),
        }
        return;
    }
    if role == "cargo" && action == "build" {
        println!("{{\"reason\":\"compiler-message\",\"message\":{{\"rendered\":\"fixture compiler diagnostic\\n\"}}}}");
        let mode = env::var("FIXTURE_BUILD").unwrap_or_default();
        if mode == "malformed" { println!("broken json"); return; }
        if mode == "failed" { process::exit(7); }
        if mode == "missing" { return; }
        let artifact = env::current_dir().unwrap().join("artifacts with spaces/profile_notes");
        for (package, name, kind, executable) in [
            ("another-package", "profile_notes", "example", format!("{:?}", artifact)),
            ("path+file:///fixture#robostickies@0.1.0", "different", "example", format!("{:?}", artifact)),
            ("path+file:///fixture#robostickies@0.1.0", "profile_notes", "bin", format!("{:?}", artifact)),
            ("path+file:///fixture#robostickies@0.1.0", "profile_notes", "example", "null".into()),
            ("path+file:///fixture#robostickies@0.1.0", "profile_notes", "example", format!("{:?}", artifact)),
        ] {
            println!("{{\"reason\":\"compiler-artifact\",\"package_id\":{package:?},\"target\":{{\"name\":{name:?},\"kind\":[{kind:?}]}},\"executable\":{executable}}}");
        }
        return;
    }
    let report = if role == "cargo" && args.iter().any(|a| a == "--output-dir") {
        let index = args.iter().position(|a| a == "--output-dir").unwrap();
        Some(Path::new(&args[index + 1]).join(if action == "llvm-cov" { "html/index.html" } else { "tarpaulin-report.html" }))
    } else if role == "samply" && action == "record" {
        let index = args.iter().position(|a| a == "--output").unwrap();
        Some(Path::new(&args[index + 1]).to_owned())
    } else { None };
    if env::var_os("FIXTURE_NO_REPORT").is_none() {
        if let Some(report) = report {
            fs::create_dir_all(report.parent().unwrap()).unwrap();
            fs::write(report, "fresh fixture report").unwrap();
        }
    }
    if env::var("FIXTURE_FAIL").ok().as_deref() == Some(action)
        || env::var("FIXTURE_FAIL").ok().as_deref() == Some(role) {
        eprintln!("fixture execution failed");
        process::exit(7);
    }
}
