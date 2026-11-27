//! Manual check for the window helpers: `wallprobe <pid> [hide|show|capture <out.png>]`.
//! Prints the windows of `pid` and whether each is hidden.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let pid: u32 = args.first().and_then(|a| a.parse().ok()).expect("usage: wallprobe <pid> [hide|show|capture <out.png>]");
    let wins = mub_core::window::find_windows(pid);
    for h in &wins {
        println!("hwnd {h:#x} hidden={}", mub_core::window::is_hidden(*h));
    }
    match args.get(1).map(String::as_str) {
        Some("hide") => println!("hidden {}", mub_core::window::hide_pid(pid)),
        Some("show") => println!("shown {}", mub_core::window::show_pid(pid, false)),
        Some("capture") => {
            let out = args.get(2).expect("output path");
            let h = *wins.first().expect("no window");
            let t = std::time::Instant::now();
            let w: u32 = args.get(3).and_then(|a| a.parse().ok()).unwrap_or(320);
            let f = mub_core::window::capture(h, w).expect("capture failed");
            let ms = t.elapsed().as_millis();
            let nonblack = f.rgba.as_chunks::<4>().0.iter().filter(|p| p[0] > 8 || p[1] > 8 || p[2] > 8).count();
            image::RgbaImage::from_raw(f.width, f.height, f.rgba)
                .unwrap()
                .save(out)
                .unwrap();
            println!(
                "captured {}x{} in {ms} ms, {:.0}% non-black -> {out}",
                f.width,
                f.height,
                100.0 * nonblack as f64 / (f.width * f.height) as f64
            );
        }
        _ => {}
    }
}
