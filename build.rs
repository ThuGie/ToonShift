fn main() {
    println!("cargo:rerun-if-env-changed=TOONSHIFT_COMMIT");
    let commit = std::env::var("TOONSHIFT_COMMIT")
        .ok()
        .filter(|value| value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()));
    println!(
        "cargo:rustc-env=TOONSHIFT_COMMIT={}",
        commit.as_deref().unwrap_or("local")
    );
}
