//! The only place Hangar runs a browser engine: a process of its own, started
//! by the app for a page that cannot be drawn natively, and spoken to over its
//! standard input and output (see the `hangar_webview` library for the
//! protocol).
//!
//! It takes no arguments. It owns the windows it is asked to open, runs the
//! platform's engine in them (WebKitGTK, WebView2, WKWebView, through wry and
//! tao), and exits when told to or when its input closes.

mod engine;
mod output;

fn main() {
    // On a Wayland session with an NVIDIA GPU, WebKitGTK's DMA-BUF renderer has
    // failed to make a GBM EGL display ("Could not create GBM EGL display"),
    // which leaves the window blank; Hangar's Tauri window needed this setting
    // there. Turning it off costs a little speed everywhere else, and a value
    // already in the environment (a person's own, or `0` to turn the renderer
    // back on) is left alone.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY: nothing else runs yet. Changing the environment is only
        // unsound while another thread might read it, and the first thread is
        // started below.
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }

    engine::run(output::Emitter::start());
}
