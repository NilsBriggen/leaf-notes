// No console window behind the app on Windows (release builds).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    let shot = args.iter().position(|a| a == "--screenshot").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);
    if let Some(path) = &shot {
        // Safety net for the picture run: never hang if the system stops giving the window frames.
        let path = path.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(60));
            eprintln!("screenshot: window got no frames in 60 s, giving up ({})", path.display());
            std::process::exit(if path.exists() { 0 } else { 1 });
        });
    }
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1360.0, 820.0]).with_min_inner_size([640.0, 420.0]).with_title("Leaf Notes")
            .with_active(true),
        ..Default::default()
    };
    eframe::run_native("Leaf Notes", options, Box::new(move |cc| Ok(Box::new(leafnotes::NotesApp::new(cc, shot)))))
}
