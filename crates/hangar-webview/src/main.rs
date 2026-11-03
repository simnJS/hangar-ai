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
    // WebKitGTK's DMA-BUF renderer cannot make a GBM EGL display on a Wayland
    // session with an NVIDIA GPU ("Could not create GBM EGL display"), and the
    // window stays blank. Turning it off costs a little speed and is what Tauri's
    // window needed there; the person can still override it from the outside.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY: nothing else runs yet. Changing the environment is only
        // unsound while another thread might read it, and the first thread is
        // started below.
        unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") };
    }

    engine::run(output::Emitter::start());
}
