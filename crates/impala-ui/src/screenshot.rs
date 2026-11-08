//! `--screenshot <png>`: render one frame with Slint's software renderer
//! into memory and save it. Needs no display, GPU or windowing system, so
//! it works on the headless Pi and in CI.

use crate::MainWindow;
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{PhysicalSize, PlatformError, Rgb8Pixel};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

struct OffscreenPlatform {
    window: Rc<MinimalSoftwareWindow>,
    start: Instant,
}

impl Platform for OffscreenPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }
    fn duration_since_start(&self) -> Duration {
        self.start.elapsed()
    }
}

/// Render `MainWindow` at `width`x`height` physical pixels after `setup`
/// has filled in its properties, and write a PNG to `out`.
pub fn render(out: &Path, width: u32, height: u32, setup: impl FnOnce(&MainWindow)) -> Result<(), String> {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(OffscreenPlatform { window: window.clone(), start: Instant::now() }))
        .map_err(|e| format!("set_platform: {e}"))?;
    let ui = MainWindow::new().map_err(|e| format!("create UI: {e}"))?;
    window.set_size(PhysicalSize::new(width, height));
    setup(&ui);
    ui.show().map_err(|e| format!("show: {e}"))?;
    slint::platform::update_timers_and_animations();

    let mut pixels = vec![Rgb8Pixel::default(); (width * height) as usize];
    window.request_redraw();
    let drawn = window.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, width as usize);
    });
    if !drawn {
        return Err("renderer produced no frame".into());
    }
    let raw: Vec<u8> = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
    let img = image::RgbImage::from_raw(width, height, raw).ok_or("bad buffer size")?;
    img.save(out).map_err(|e| format!("write {}: {e}", out.display()))?;
    let _ = ui.hide();
    Ok(())
}
