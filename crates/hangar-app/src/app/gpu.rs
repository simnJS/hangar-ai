//! Which of wgpu's backends iced is let open, decided before the first window.
//!
//! iced asks wgpu for every backend it was built with (`Backends::all()`), and
//! on Linux that is Vulkan *and* OpenGL: wgpu sets up an EGL display and a GL
//! context next to the Vulkan instance only to compare the adapters, then
//! draws with Vulkan. That GL side stays loaded for the life of the app: the
//! EGL and GL halves of the driver, a context's worth of driver mappings,
//! Mesa's EGL with Gallium — some 17 to 40 MB of the app's memory at rest
//! (PSS, 4 shell panes, an NVIDIA card), and the time to set it all up.
//!
//! iced reads the backends from `WGPU_BACKEND` and nowhere else, so this sets
//! it to Vulkan when a Vulkan adapter that is a real GPU is there. A machine
//! whose only Vulkan device is a software one (llvmpipe) keeps every backend:
//! its GPU may still draw through OpenGL, which beats Vulkan on the CPU. A
//! `WGPU_BACKEND` the person set is never touched, and reaches the panes; the
//! one set here does not (`hangar_core::pty::keep_from_panes`).

use std::sync::OnceLock;

use iced::wgpu;

/// The variable iced reads its backends from (`wgpu::Backends::from_env`).
const VAR: &str = "WGPU_BACKEND";

/// The instance the adapters were looked for with, kept: dropping it would
/// unload the Vulkan drivers, which iced's own instance loads again a moment
/// later (some 30 ms of the start, measured). It holds no device.
static PROBE: OnceLock<wgpu::Instance> = OnceLock::new();

/// Called first thing in `run`, while the process has one thread (the
/// drivers start none of their own while the adapters are listed).
pub fn prefer_vulkan() {
    if !cfg!(target_os = "linux") || std::env::var_os(VAR).is_some() {
        return;
    }
    let instance = PROBE.get_or_init(|| {
        wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..Default::default()
        })
    });
    let kinds: Vec<_> = instance
        .enumerate_adapters(wgpu::Backends::VULKAN)
        .iter()
        .map(|adapter| adapter.get_info().device_type)
        .collect();
    if let Some(value) = backend_for(&kinds) {
        // SAFETY: `run` calls this before iced, tokio or any service starts a
        // thread, so nothing reads the environment while it changes.
        unsafe { std::env::set_var(VAR, value) };
        // Hangar's choice, not the person's: the panes' shells start without it.
        hangar_core::pty::keep_from_panes(VAR);
    }
}

/// The `WGPU_BACKEND` value for the kinds of the Vulkan adapters found, or
/// `None` to leave iced every backend.
fn backend_for(vulkan: &[wgpu::DeviceType]) -> Option<&'static str> {
    vulkan
        .iter()
        .any(|kind| *kind != wgpu::DeviceType::Cpu)
        .then_some("vulkan")
}

#[cfg(test)]
mod tests {
    use super::*;
    use wgpu::DeviceType::{Cpu, DiscreteGpu, IntegratedGpu, Other, VirtualGpu};

    #[test]
    fn a_real_vulkan_gpu_closes_the_gl_backend() {
        assert_eq!(backend_for(&[DiscreteGpu]), Some("vulkan"));
        assert_eq!(
            backend_for(&[DiscreteGpu, IntegratedGpu, Cpu]),
            Some("vulkan")
        );
        assert_eq!(backend_for(&[VirtualGpu]), Some("vulkan"));
        assert_eq!(backend_for(&[Other]), Some("vulkan"));
    }

    #[test]
    fn software_vulkan_or_none_keeps_every_backend() {
        assert_eq!(backend_for(&[]), None);
        assert_eq!(backend_for(&[Cpu]), None);
    }

    #[test]
    fn the_value_set_is_vulkan_alone_for_wgpu() {
        let value = backend_for(&[DiscreteGpu]).unwrap();
        assert_eq!(
            wgpu::Backends::from_comma_list(value),
            wgpu::Backends::VULKAN
        );
    }
}
