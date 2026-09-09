// build/target.rs — which cargo targets the crate builds for.
//
// The one place the supported cross targets are decided.

/// Stop a build for a cross target the crate has no archive story for,
/// naming the target it was asked for. wasm32 brings its own toolchain
/// and the other macOS architecture is built by the host's own
/// compiler; every other cross target has neither.
fn require_supported_target(target: &str, host: &str, is_wasm: bool) {
    if target == host || is_wasm || (is_macos(target) && is_macos(host)) {
        return;
    }

    panic!(
        "beni-sys: unsupported cross-compilation target {target}. The supported \
         cross targets are wasm32 and the other macOS architecture."
    );
}

/// True for a macOS triple. iOS and the other Apple platforms are not
/// macOS: each needs a toolchain of its own, which is what puts them
/// outside what the host compiler alone can serve.
fn is_macos(triple: &str) -> bool {
    triple.ends_with("-apple-darwin")
}

#[cfg(test)]
mod tests {
    use super::require_supported_target;

    const MACOS_ARM: &str = "aarch64-apple-darwin";
    const MACOS_INTEL: &str = "x86_64-apple-darwin";
    const LINUX: &str = "x86_64-unknown-linux-gnu";

    #[test]
    fn a_build_for_the_host_itself_proceeds() {
        require_supported_target(LINUX, LINUX, false);
    }

    #[test]
    fn a_wasm32_build_proceeds() {
        require_supported_target("wasm32-wasip1", LINUX, true);
    }

    #[test]
    fn the_other_macos_architecture_proceeds_in_both_directions() {
        require_supported_target(MACOS_INTEL, MACOS_ARM, false);
        require_supported_target(MACOS_ARM, MACOS_INTEL, false);
    }

    #[test]
    #[should_panic(expected = "unsupported cross-compilation target aarch64-apple-ios")]
    fn another_apple_platform_is_not_the_other_macos_architecture() {
        require_supported_target("aarch64-apple-ios", MACOS_ARM, false);
    }

    #[test]
    #[should_panic(expected = "unsupported cross-compilation target x86_64-apple-darwin")]
    fn macos_is_not_reachable_from_a_host_that_is_not_macos() {
        require_supported_target(MACOS_INTEL, LINUX, false);
    }
}
