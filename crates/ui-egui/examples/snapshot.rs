//! Renders the UI offscreen to a PNG with a small software rasteriser: no window, no GPU.
//!
//! `cargo run -p remotecraft-ui-egui --example snapshot -- out.png [--dark] [--lang it] [--screen customers|devices|interventions|privacy|reports|settings] [--open N] [--new] [--start] [--search QUERY] [--size WxH]`
//! The demo desk is shown; no file or network is needed. `--open N` selects the N-th row of
//! the screen (0-based), `--new` opens the screen's new-record form, `--start` the
//! start-intervention card.

use std::collections::HashMap;

use egui::epaint::{ImageDelta, Primitive};
use egui::{Color32, Pos2, TextureId};
use remotecraft_ui_egui::{DeskApp, Screen};

struct Texture {
    size: [usize; 2],
    pixels: Vec<Color32>,
}

fn apply(textures: &mut HashMap<TextureId, Texture>, id: TextureId, delta: &ImageDelta) {
    let egui::ImageData::Color(img) = &delta.image;
    match delta.pos {
        None => {
            textures.insert(id, Texture { size: img.size, pixels: img.pixels.clone() });
        }
        Some([x0, y0]) => {
            if let Some(t) = textures.get_mut(&id) {
                for y in 0..img.size[1] {
                    for x in 0..img.size[0] {
                        if let (Some(dst), Some(src)) = (t.pixels.get_mut((y0 + y) * t.size[0] + x0 + x), img.pixels.get(y * img.size[0] + x)) {
                            *dst = *src;
                        }
                    }
                }
            }
        }
    }
}

fn mul(a: Color32, b: Color32) -> [f32; 4] {
    let f = |x: u8, y: u8| x as f32 * y as f32 / 255.0;
    [f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()), f(a.a(), b.a())]
}

fn raster(prims: &[egui::ClippedPrimitive], textures: &HashMap<TextureId, Texture>, w: usize, h: usize, bg: Color32) -> Vec<u8> {
    let mut fb: Vec<[f32; 4]> = vec![[bg.r() as f32, bg.g() as f32, bg.b() as f32, 255.0]; w * h];
    for p in prims {
        let Primitive::Mesh(mesh) = &p.primitive else { continue };
        let tex = textures.get(&mesh.texture_id);
        let clip = p.clip_rect;
        for tri in mesh.indices.as_chunks::<3>().0 {
            let [Some(a), Some(b), Some(c)] = tri.map(|i| mesh.vertices.get(i as usize)) else { continue };
            let area = (b.pos - a.pos).x * (c.pos - a.pos).y - (b.pos - a.pos).y * (c.pos - a.pos).x;
            if area.abs() < 1e-6 {
                continue;
            }
            let minx = a.pos.x.min(b.pos.x).min(c.pos.x).max(clip.min.x).max(0.0).floor() as usize;
            let maxx = a.pos.x.max(b.pos.x).max(c.pos.x).min(clip.max.x).min(w as f32).ceil() as usize;
            let miny = a.pos.y.min(b.pos.y).min(c.pos.y).max(clip.min.y).max(0.0).floor() as usize;
            let maxy = a.pos.y.max(b.pos.y).max(c.pos.y).min(clip.max.y).min(h as f32).ceil() as usize;
            for y in miny..maxy {
                for x in minx..maxx {
                    let pt = Pos2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((b.pos - pt).x * (c.pos - pt).y - (b.pos - pt).y * (c.pos - pt).x) / area;
                    let w1 = ((c.pos - pt).x * (a.pos - pt).y - (c.pos - pt).y * (a.pos - pt).x) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                        continue;
                    }
                    let lerp = |f: fn(Color32) -> u8| {
                        (f(a.color) as f32 * w0 + f(b.color) as f32 * w1 + f(c.color) as f32 * w2).round().clamp(0.0, 255.0) as u8
                    };
                    let col = Color32::from_rgba_premultiplied(lerp(|c| c.r()), lerp(|c| c.g()), lerp(|c| c.b()), lerp(|c| c.a()));
                    let uv = a.uv.to_vec2() * w0 + b.uv.to_vec2() * w1 + c.uv.to_vec2() * w2;
                    let texel = tex
                        .and_then(|t| {
                            let tx = ((uv.x * t.size[0] as f32) as usize).min(t.size[0].saturating_sub(1));
                            let ty = ((uv.y * t.size[1] as f32) as usize).min(t.size[1].saturating_sub(1));
                            t.pixels.get(ty * t.size[0] + tx).copied()
                        })
                        .unwrap_or(Color32::WHITE);
                    let src = mul(col, texel);
                    if let Some(dst) = fb.get_mut(y * w + x) {
                        let k = 1.0 - src[3] / 255.0;
                        for i in 0..3 {
                            dst[i] = src[i] + dst[i] * k;
                        }
                    }
                }
            }
        }
    }
    fb.iter().flat_map(|p| [p[0] as u8, p[1] as u8, p[2] as u8, 255]).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (mut w, mut h) = (1440usize, 900usize);
    let mut out = "snapshot.png".to_string();
    let mut dark = false;
    let mut app = DeskApp::demo();
    let mut language = remotecraft_ui_egui::Language::En;
    let mut open: Option<usize> = None;
    let mut new = false;
    let mut start = false;
    let mut customer: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--lang" => language = it.next().and_then(|c| remotecraft_ui_egui::Language::parse(c)).unwrap_or_default(),
            "--dark" => dark = true,
            "--screen" => app.go(it.next().and_then(|s| Screen::parse(s)).unwrap_or_default()),
            "--open" => open = it.next().and_then(|s| s.parse().ok()).or(Some(0)),
            "--new" => new = true,
            "--start" => start = true,
            "--search" => app.search = it.next().cloned().unwrap_or_default(),
            // The customer typed (not picked) in the new PC form opened by `--new`.
            "--customer" => customer = it.next().cloned(),
            "--size" => {
                if let Some((a, b)) = it.next().and_then(|s| s.split_once('x')) {
                    w = a.parse().unwrap_or(w);
                    h = b.parse().unwrap_or(h);
                }
            }
            p if p.ends_with(".png") => out = p.to_string(),
            _ => {}
        }
    }
    app.language = language;
    app.dark = dark;
    if let Some(n) = open {
        let d = app.session.data();
        match app.screen {
            Screen::Customers => app.selected_customer = d.customers.get(n).map(|c| c.id),
            Screen::Devices => app.selected_device = d.devices.get(n).map(|x| x.id),
            Screen::Interventions => {
                let mut rows: Vec<_> = d.interventions.iter().collect();
                rows.sort_by(|a, b| b.started_at.cmp(&a.started_at));
                app.selected_intervention = rows.get(n).map(|i| i.id);
            }
            Screen::Privacy => app.selected_customer = d.customers.get(n).map(|c| c.id),
            _ => {}
        }
    }
    let ctx = egui::Context::default();
    let mut textures = HashMap::new();
    let mut last = None;
    for i in 0..6 {
        if i == 2 && new {
            app.new_record();
            if let (Some(text), Some(f)) = (&customer, app.devices.form.as_mut()) {
                f.customer = remotecraft_ui_egui::forms::Picker { text: text.clone(), id: None };
            }
        }
        if i == 2 && start {
            app.start_intervention();
        }
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(w as f32, h as f32))), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| app.ui(ui));
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                apply(&mut textures, *id, delta);
            }
        }
        output.textures_delta.clear();
        last = Some(ctx.tessellate(output.shapes, 1.0));
    }
    let bg = if dark { Color32::from_gray(27) } else { Color32::from_gray(248) };
    let pixels = raster(&last.unwrap_or_default(), &textures, w, h, bg);
    let file = std::fs::File::create(&out).expect("create output");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().and_then(|mut wr| wr.write_image_data(&pixels)).expect("write png");
    println!("wrote {out}");
}
