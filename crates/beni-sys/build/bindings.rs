// The constants the bindings a build uses declare, read by the build
// script's configuration checks.
//
// A build script is outside `cargo test`'s reach, so the read lives
// here and both `build.rs` and the library's test build include it.

/// The value of the `u32` constant `name` that `bindings` declares, read
/// from the `pub const name: u32 = value;` line bindgen writes for it.
pub(crate) fn declared_u32(bindings: &str, name: &str) -> Option<u32> {
    bindings.lines().find_map(|line| {
        line.trim_start()
            .strip_prefix("pub const ")?
            .strip_prefix(name)?
            .strip_prefix(':')?
            .split_once('=')?
            .1
            .trim()
            .strip_suffix(';')?
            .parse::<u32>()
            .ok()
    })
}

/// A bindings file holding `contents`, named after `case`, which each of
/// the build script's tests gives uniquely so concurrent tests cannot
/// collide.
#[cfg(test)]
pub(crate) fn bindings(case: &str, contents: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("beni-sys-bindings-{}-{}", std::process::id(), case));
    std::fs::create_dir_all(&dir).expect("the case directory is creatable");
    let path = dir.join("bindings.rs");
    std::fs::write(&path, contents).expect("the bindings are writable");
    path
}
