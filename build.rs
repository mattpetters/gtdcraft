// Stamps the version into the program and embeds Info.plist (__TEXT,__info_plist) so macOS sees GTDCraft's Calendar
// usage description even outside GTDCraft.app (`cargo run`). Two numbers: the marketing
// version (CFBundleShortVersionString, semver from Cargo.toml, the one place it lives) and the build version
// (CFBundleVersion, the commit count), plus the commit's short SHA.
fn git(args: &[&str]) -> String {
    std::process::Command::new("git").args(args).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

/// Replaces `<key>{key}</key><string>…</string>`'s value.
fn stamp(plist: &str, key: &str, value: &str) -> String {
    let k = format!("<key>{key}</key><string>");
    let at = plist.find(&k).unwrap_or_else(|| panic!("Info.plist has {key}")) + k.len();
    let end = at + plist[at..].find('<').unwrap();
    format!("{}{value}{}", &plist[..at], &plist[end..])
}

fn main() {
    println!("cargo:rerun-if-changed=Info.plist");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
    let sha = git(&["rev-parse", "--short", "HEAD"]);
    let build = Some(git(&["rev-list", "--count", "HEAD"])).filter(|n| !n.is_empty()).unwrap_or_else(|| "0".into());
    println!("cargo:rustc-env=GTDCRAFT_SHA={sha}");
    println!("cargo:rustc-env=GTDCRAFT_BUILD={build}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("ios") {
        live_activity();
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        let plist = std::fs::read_to_string(std::path::Path::new(&dir).join("Info.plist")).unwrap();
        let plist = stamp(&stamp(&plist, "CFBundleShortVersionString", &std::env::var("CARGO_PKG_VERSION").unwrap()), "CFBundleVersion", &build);
        let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("Info.plist");
        std::fs::write(&out, plist).unwrap();
        println!("cargo:rustc-link-arg-bin=gtdcraft=-Wl,-sectcreate,__TEXT,__info_plist,{}", out.display());
    }
}

/// iPhone/iPad: the Focus Live Activity is ActivityKit (Swift only). Compile ios/LiveActivity + ios/Shared into a static
/// library and link it and the Swift runtime into gtdcraft, which calls `gtd_live_start` / `gtd_live_end`.
fn live_activity() {
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let sim = std::env::var("TARGET").unwrap_or_default().ends_with("-sim");
    let sdk_name = if sim { "iphonesimulator" } else { "iphoneos" };
    let run = |args: &[&str]| -> String {
        let o = std::process::Command::new("xcrun").args(args).output().expect("xcrun");
        assert!(o.status.success(), "xcrun {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    };
    let sdk = run(&["--sdk", sdk_name, "--show-sdk-path"]);
    let swiftc = run(&["--sdk", sdk_name, "-f", "swiftc"]);
    let sources = [dir.join("ios/LiveActivity/GTDLive.swift"), dir.join("ios/Shared/GTDFocusAttributes.swift")];
    for s in &sources {
        println!("cargo:rerun-if-changed={}", s.display());
    }
    let lib = out.join("libgtdlive.a");
    let target = if sim { "arm64-apple-ios17.0-simulator" } else { "arm64-apple-ios17.0" };
    let status = std::process::Command::new(&swiftc)
        .args(["-emit-library", "-static", "-parse-as-library", "-O", "-module-name", "GTDLive", "-target", target, "-sdk", &sdk, "-o"])
        .arg(&lib)
        .args(&sources)
        .status()
        .expect("swiftc");
    assert!(status.success(), "swiftc failed for the Live Activity library");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=gtdlive");
    // the Swift runtime (in the OS since iOS 12.2) and its compatibility shims from the toolchain
    let toolchain_lib = std::path::Path::new(&swiftc).parent().unwrap().join("../lib/swift").join(sdk_name);
    println!("cargo:rustc-link-search=native={}/usr/lib/swift", sdk);
    println!("cargo:rustc-link-search=native={}", toolchain_lib.display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    println!("cargo:rustc-link-lib=framework=ActivityKit");
    println!("cargo:rustc-link-lib=framework=WidgetKit");
}
