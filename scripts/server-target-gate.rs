pub fn main() {
    let target = std::env::var("TARGET").expect("Cargo must provide TARGET");
    assert_eq!(
        target, "x86_64-unknown-linux-gnu",
        "xsos only supports the x86_64-unknown-linux-gnu compilation target"
    );
}
