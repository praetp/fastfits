use std::process::Command;

fn main() {
    // Build date (UTC, YYYY-MM-DD)
    let date = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|epoch| epoch.parse::<i64>().ok())
        .map(|_secs| {
            // Convert seconds-since-epoch to a date string via the `date` command
            // (keeps the build script dependency-free).
            let out = Command::new("date")
                .args(["-u", "-d", &format!("@{_secs}"), "+%Y-%m-%d"])
                .output();
            out.ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_owned())
        })
        .flatten()
        .unwrap_or_else(|| {
            // Fall back to running `date` with current time
            Command::new("date")
                .args(["-u", "+%Y-%m-%d"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_owned())
                .unwrap_or_else(|| "unknown".to_owned())
        });
    println!("cargo:rustc-env=FASTFITS_BUILD_DATE={date}");

    // rustc version
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let version = Command::new(&rustc)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=FASTFITS_RUSTC_VERSION={version}");

    // Re-run only when build.rs itself changes (not on every source edit)
    println!("cargo:rerun-if-changed=build.rs");
}
