// No console window behind the app on Windows (release builds).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    let shot = args.iter().position(|a| a == "--screenshot").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);
    if let Some(path) = &shot {
        // The window normally draws and saves the picture within a few seconds. A locked or hidden
        // screen gives it no frames at all: then draw the same app without a window instead.
        let path = path.clone();
        std::thread::spawn(move || {
            let start = std::time::Instant::now();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(250));
                let (t, drawn) = (start.elapsed().as_secs_f32(), leafnotes::SHOT_FRAMES.load(std::sync::atomic::Ordering::Relaxed));
                if (t > 6.0 && drawn == 0) || t > 45.0 { break; }
            }
            eprintln!("screenshot: the window got no frames (locked or hidden screen?), drawing it without a window");
            // whatever happens, this thread ends the run: a crash here must not leave it hanging
            let r = std::panic::catch_unwind(|| leafnotes::headless::screenshot(&path, 120, eframe::egui::vec2(1280.0, 800.0), 1.5))
                .unwrap_or_else(|_| Err("drawing without a window crashed".into()));
            if let Err(e) = &r { eprintln!("screenshot failed: {e}"); }
            std::process::exit(if r.is_ok() { 0 } else { 1 });
        });
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1360.0, 820.0]).with_min_inner_size([640.0, 420.0]).with_title("Leaf Notes")
            .with_active(true),
        ..Default::default()
    };
    eframe::run_native("Leaf Notes", options, Box::new(move |cc| Ok(Box::new(leafnotes::NotesApp::new(cc, shot)))))
}
