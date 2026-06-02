/// Plugin host skeleton. WASI support is optional via the `wasi` feature.

#[cfg(feature = "wasi")]
pub fn init_plugin_host() {
    println!("plugin host: WASI support enabled (wasmtime feature)");
}

#[cfg(not(feature = "wasi"))]
pub fn init_plugin_host() {
    println!("plugin host: WASI support not enabled (build with --features wasi to enable)");
}
