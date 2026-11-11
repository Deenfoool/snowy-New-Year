use resvg::{tiny_skia as skia, usvg};
use std::{env, fs, path::Path};

const ICONS: &str = "assets/lucide";
const BADGE_SIZE: u32 = 32;

fn main() {
    println!("cargo:rerun-if-changed={ICONS}");
    let output = env::var_os("OUT_DIR").expect("Cargo output directory");
    let output = Path::new(&output);
    for file in fs::read_dir(ICONS).expect("Lucide icon directory") {
        let path = file.expect("Lucide asset").path();
        if path.extension().is_some_and(|ext| ext == "svg") {
            let mut pixmap = skia::Pixmap::new(48, 48).unwrap();
            render(
                &path,
                "white",
                skia::Transform::from_scale(2.0, 2.0),
                &mut pixmap,
            );
            let mask: Vec<_> = pixmap
                .data()
                .chunks_exact(4)
                .map(|pixel| pixel[3])
                .collect();
            assert!(
                mask.iter().any(|&alpha| alpha != 0),
                "Empty icon {}",
                path.display()
            );
            let name = path.file_stem().unwrap().to_string_lossy();
            fs::write(output.join(format!("{name}.alpha")), mask).unwrap();
        }
    }
    for (name, icon, color) in [
        ("active", "snowflake", "#a9e6ff"),
        ("paused", "pause", "#c1cde0"),
        ("offline", "cloud-off", "#e3b777"),
    ] {
        let mut pixmap = skia::Pixmap::new(BADGE_SIZE, BADGE_SIZE).unwrap();
        let circle = skia::PathBuilder::from_circle(16.0, 16.0, 15.5).unwrap();
        let mut paint = skia::Paint::default();
        paint.set_color_rgba8(18, 40, 61, 255);
        paint.anti_alias = true;
        pixmap.fill_path(
            &circle,
            &paint,
            skia::FillRule::Winding,
            skia::Transform::identity(),
            None,
        );
        let svg = Path::new(ICONS).join(format!("{icon}.svg"));
        render(
            &svg,
            color,
            skia::Transform::from_translate(4.0, 4.0),
            &mut pixmap,
        );
        fs::write(output.join(format!("{name}.rgba")), pixmap.data()).unwrap();
        if name == "active" {
            let png = pixmap.encode_png().expect("PNG-encoded application icon");
            // ICO container with a single 32-bit PNG image.
            let mut ico = vec![0, 0, 1, 0, 1, 0, 32, 32, 0, 0, 1, 0, 32, 0];
            ico.extend((png.len() as u32).to_le_bytes());
            ico.extend(22u32.to_le_bytes());
            ico.extend(png);
            fs::write(output.join("snowy-new-year.ico"), ico).unwrap();
        }
    }
}

fn render(path: &Path, color: &str, transform: skia::Transform, pixmap: &mut skia::Pixmap) {
    let svg =
        fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let svg = svg.replace("currentColor", color);
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default())
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    resvg::render(&tree, transform, &mut pixmap.as_mut());
}
