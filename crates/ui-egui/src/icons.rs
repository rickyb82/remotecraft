//! The icon set, drawn in code (strokes on a 24-unit grid): no bitmap or vendor artwork.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Logo,
    Customers,
    Customer,
    Company,
    Device,
    Interventions,
    Timer,
    Play,
    Stop,
    Privacy,
    ShieldCheck,
    ShieldOff,
    Reports,
    Settings,
    Search,
    Plus,
    Close,
    Chevron,
    ChevronRight,
    Back,
    Check,
    Warning,
    Info,
    Connect,
    Export,
    Trash,
    Archive,
    Edit,
    Moon,
    Sun,
    Globe,
    Account,
    More,
    Keyboard,
    Phone,
    Mail,
    Onsite,
    Lab,
    Remote,
    Notice,
    Signature,
    History,
    Tag,
    Pin,
    Calendar,
    Sync,
    Help,
    Audit,
    Server,
    Copy,
    Erase,
    Lock,
    Euro,
}

/// Points on a circular arc, angles in degrees (0 = right, clockwise on screen).
fn arc(c: Pos2, r: f32, from: f32, to: f32, n: usize) -> Vec<Pos2> {
    (0..=n)
        .map(|i| {
            let a = (from + (to - from) * i as f32 / n as f32).to_radians();
            c + vec2(a.cos(), a.sin()) * r
        })
        .collect()
}

/// Paints `icon` into `rect` with stroke colour `c`.
pub fn paint(p: &Painter, rect: Rect, icon: Icon, c: Color32) {
    let s = rect.width().min(rect.height()) / 24.0;
    let o = rect.center() - vec2(12.0, 12.0) * s;
    let at = |x: f32, y: f32| -> Pos2 { o + vec2(x, y) * s };
    let w = (1.7 * s).max(1.0);
    let st = Stroke::new(w, c);
    let line = |pts: &[(f32, f32)]| {
        p.add(Shape::line(pts.iter().map(|(x, y)| at(*x, *y)).collect(), st));
    };
    let rrect = |x0: f32, y0: f32, x1: f32, y1: f32, r: f32| {
        p.rect_stroke(Rect::from_min_max(at(x0, y0), at(x1, y1)), r * s, st, egui::StrokeKind::Middle);
    };
    let circle = |x: f32, y: f32, r: f32| {
        p.circle_stroke(at(x, y), r * s, st);
    };
    let dot = |x: f32, y: f32, r: f32| {
        p.circle_filled(at(x, y), r * s, c);
    };
    let person = |x: f32, y: f32, r: f32| {
        circle(x, y, r);
        p.add(Shape::line(arc(at(x, y + r * 3.0), r * 1.8, 200.0, 340.0, 12), st));
    };
    let monitor = || {
        rrect(3.0, 4.5, 21.0, 16.5, 1.5);
        line(&[(12.0, 16.5), (12.0, 20.0)]);
        line(&[(8.0, 20.0), (16.0, 20.0)]);
    };
    let shield = || {
        line(&[(12.0, 3.0), (19.5, 6.0), (19.5, 11.5), (12.0, 21.0), (4.5, 11.5), (4.5, 6.0), (12.0, 3.0)]);
    };
    let document = || {
        line(&[(6.0, 3.5), (14.0, 3.5), (18.5, 8.0), (18.5, 20.5), (6.0, 20.5), (6.0, 3.5)]);
        line(&[(14.0, 3.5), (14.0, 8.0), (18.5, 8.0)]);
    };
    match icon {
        Icon::Logo => {
            // The RemoteCraft mark: a screen with the craft-family spark (brand colours).
            p.rect_filled(Rect::from_min_max(at(2.5, 4.0), at(21.5, 17.5)), 3.0 * s, Color32::from_rgb(0x4a, 0x6c, 0xf7));
            p.add(Shape::line(vec![at(7.0, 11.0), at(10.5, 14.0), at(16.5, 7.5)], Stroke::new(w * 1.1, Color32::WHITE)));
            p.add(Shape::line(vec![at(8.5, 21.0), at(15.5, 21.0)], Stroke::new(w * 1.1, Color32::from_rgb(0x4a, 0x6c, 0xf7))));
            p.circle_filled(at(19.0, 5.0), 3.2 * s, Color32::from_rgb(0xff, 0xb4, 0x3d));
        }
        Icon::Customers => {
            person(9.0, 8.5, 3.2);
            circle(16.5, 8.0, 2.6);
            p.add(Shape::line(arc(at(16.5, 18.5), 5.2 * s, 240.0, 340.0, 10), st));
        }
        Icon::Customer | Icon::Account => person(12.0, 8.5, 4.0),
        Icon::Company => {
            rrect(4.0, 4.0, 15.0, 20.5, 1.0);
            line(&[(15.0, 10.0), (20.5, 10.0), (20.5, 20.5)]);
            line(&[(4.0, 20.5), (20.5, 20.5)]);
            for y in [8.0, 12.0, 16.0] {
                line(&[(7.0, y), (9.0, y)]);
                line(&[(10.5, y), (12.5, y)]);
            }
        }
        Icon::Device => monitor(),
        Icon::Remote => {
            monitor();
            line(&[(8.5, 10.5), (15.5, 10.5)]);
            line(&[(12.5, 7.5), (15.5, 10.5), (12.5, 13.5)]);
        }
        Icon::Interventions | Icon::History => {
            circle(12.0, 12.5, 8.0);
            line(&[(12.0, 7.5), (12.0, 12.5), (15.5, 14.5)]);
            if icon == Icon::History {
                line(&[(2.5, 8.0), (4.0, 12.5), (7.5, 10.0)]);
            }
        }
        Icon::Timer => {
            circle(12.0, 13.5, 7.5);
            line(&[(12.0, 9.5), (12.0, 13.5), (15.0, 15.0)]);
            line(&[(9.5, 3.5), (14.5, 3.5)]);
            line(&[(12.0, 3.5), (12.0, 6.0)]);
        }
        Icon::Play => {
            p.add(Shape::convex_polygon(vec![at(8.0, 5.0), at(19.0, 12.0), at(8.0, 19.0)], c, Stroke::NONE));
        }
        Icon::Stop => {
            p.rect_filled(Rect::from_min_max(at(6.0, 6.0), at(18.0, 18.0)), 2.0 * s, c);
        }
        Icon::Privacy => shield(),
        Icon::ShieldCheck => {
            shield();
            line(&[(8.5, 11.5), (11.0, 14.0), (15.5, 9.0)]);
        }
        Icon::ShieldOff => {
            shield();
            line(&[(9.0, 9.0), (15.0, 15.0)]);
            line(&[(15.0, 9.0), (9.0, 15.0)]);
        }
        Icon::Reports => {
            line(&[(4.0, 4.0), (4.0, 20.0), (20.5, 20.0)]);
            line(&[(8.0, 16.0), (8.0, 11.0)]);
            line(&[(12.5, 16.0), (12.5, 7.0)]);
            line(&[(17.0, 16.0), (17.0, 13.0)]);
        }
        Icon::Settings => {
            circle(12.0, 12.0, 3.2);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                let (cx, cy) = (a.cos(), a.sin());
                line(&[(12.0 + cx * 6.0, 12.0 + cy * 6.0), (12.0 + cx * 9.0, 12.0 + cy * 9.0)]);
            }
            circle(12.0, 12.0, 6.5);
        }
        Icon::Search => {
            circle(10.5, 10.5, 6.0);
            line(&[(15.0, 15.0), (20.5, 20.5)]);
        }
        Icon::Plus => {
            line(&[(12.0, 5.0), (12.0, 19.0)]);
            line(&[(5.0, 12.0), (19.0, 12.0)]);
        }
        Icon::Close => {
            line(&[(6.0, 6.0), (18.0, 18.0)]);
            line(&[(18.0, 6.0), (6.0, 18.0)]);
        }
        Icon::Chevron => line(&[(7.0, 10.0), (12.0, 15.0), (17.0, 10.0)]),
        Icon::ChevronRight => line(&[(10.0, 7.0), (15.0, 12.0), (10.0, 17.0)]),
        Icon::Back => {
            line(&[(10.0, 6.0), (4.0, 12.0), (10.0, 18.0)]);
            line(&[(4.0, 12.0), (20.0, 12.0)]);
        }
        Icon::Check => {
            circle(12.0, 12.0, 8.5);
            line(&[(7.5, 12.5), (10.5, 15.5), (16.5, 9.0)]);
        }
        Icon::Warning => {
            line(&[(12.0, 3.5), (21.0, 20.0), (3.0, 20.0), (12.0, 3.5)]);
            line(&[(12.0, 9.5), (12.0, 14.5)]);
            dot(12.0, 17.2, 1.1);
        }
        Icon::Info => {
            circle(12.0, 12.0, 8.5);
            line(&[(12.0, 11.0), (12.0, 16.5)]);
            dot(12.0, 7.8, 1.1);
        }
        Icon::Connect => {
            line(&[(10.0, 14.0), (14.0, 10.0)]);
            p.add(Shape::line(arc(at(15.5, 8.5), 3.5 * s, 135.0, 405.0, 14), st));
            p.add(Shape::line(arc(at(8.5, 15.5), 3.5 * s, -45.0, 225.0, 14), st));
        }
        Icon::Export => {
            line(&[(12.0, 3.5), (12.0, 15.0)]);
            line(&[(7.5, 10.5), (12.0, 15.0), (16.5, 10.5)]);
            line(&[(4.0, 16.0), (4.0, 20.5), (20.0, 20.5), (20.0, 16.0)]);
        }
        Icon::Trash | Icon::Erase => {
            line(&[(4.0, 6.5), (20.0, 6.5)]);
            line(&[(9.0, 6.5), (9.5, 3.5), (14.5, 3.5), (15.0, 6.5)]);
            line(&[(6.0, 6.5), (7.0, 20.5), (17.0, 20.5), (18.0, 6.5)]);
            line(&[(10.0, 10.0), (10.0, 17.0)]);
            line(&[(14.0, 10.0), (14.0, 17.0)]);
        }
        Icon::Archive => {
            rrect(3.5, 4.0, 20.5, 8.5, 1.0);
            line(&[(5.0, 8.5), (5.0, 20.0), (19.0, 20.0), (19.0, 8.5)]);
            line(&[(10.0, 13.0), (14.0, 13.0)]);
        }
        Icon::Edit => {
            line(&[(4.0, 20.0), (4.5, 16.0), (16.0, 4.5), (19.5, 8.0), (8.0, 19.5), (4.0, 20.0)]);
            line(&[(13.5, 7.0), (17.0, 10.5)]);
        }
        Icon::Moon => {
            p.add(Shape::line(arc(at(12.0, 12.0), 8.0 * s, 40.0, 320.0, 24), st));
            p.add(Shape::line(arc(at(17.0, 9.0), 6.4 * s, 100.0, 232.0, 16), st));
        }
        Icon::Sun => {
            circle(12.0, 12.0, 4.0);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                let (cx, cy) = (a.cos(), a.sin());
                line(&[(12.0 + cx * 7.0, 12.0 + cy * 7.0), (12.0 + cx * 9.5, 12.0 + cy * 9.5)]);
            }
        }
        Icon::Globe => {
            circle(12.0, 12.0, 8.5);
            line(&[(3.5, 12.0), (20.5, 12.0)]);
            for side in [-1.0, 1.0] {
                p.add(Shape::line(
                    arc(at(12.0, 12.0), 8.5 * s, -90.0, 90.0, 16)
                        .into_iter()
                        .map(|q| Pos2::new(at(12.0, 0.0).x + (q.x - at(12.0, 0.0).x) * 0.45 * side, q.y))
                        .collect(),
                    st,
                ));
            }
        }
        Icon::More => {
            dot(6.0, 12.0, 1.7);
            dot(12.0, 12.0, 1.7);
            dot(18.0, 12.0, 1.7);
        }
        Icon::Keyboard => {
            rrect(3.0, 6.5, 21.0, 17.5, 1.5);
            for x in [6.5, 10.0, 13.5, 17.0] {
                dot(x, 10.0, 0.9);
            }
            line(&[(7.5, 14.0), (16.5, 14.0)]);
        }
        Icon::Phone => {
            line(&[(6.0, 4.0), (9.0, 4.0), (10.5, 8.5), (8.5, 10.0), (14.0, 15.5), (15.5, 13.5), (20.0, 15.0), (20.0, 18.0), (18.0, 20.0)]);
            p.add(Shape::line(arc(at(18.0, 8.0), 12.0 * s, 100.0, 170.0, 10), st));
        }
        Icon::Mail => {
            rrect(3.5, 6.0, 20.5, 18.5, 1.5);
            line(&[(3.5, 7.0), (12.0, 13.0), (20.5, 7.0)]);
        }
        Icon::Onsite => {
            line(&[(4.0, 11.5), (12.0, 4.5), (20.0, 11.5)]);
            line(&[(6.0, 10.0), (6.0, 20.0), (18.0, 20.0), (18.0, 10.0)]);
            line(&[(10.0, 20.0), (10.0, 14.5), (14.0, 14.5), (14.0, 20.0)]);
        }
        Icon::Lab => {
            line(&[(4.0, 20.0), (12.0, 12.0)]);
            p.add(Shape::line(arc(at(15.5, 8.5), 5.0 * s, 120.0, 400.0, 16), st));
            line(&[(13.0, 4.5), (17.0, 8.5), (19.5, 6.0)]);
        }
        Icon::Notice => {
            document();
            line(&[(9.0, 12.0), (15.5, 12.0)]);
            line(&[(9.0, 15.5), (15.5, 15.5)]);
        }
        Icon::Signature => {
            document();
            line(&[(8.5, 16.0), (10.0, 13.0), (11.5, 16.5), (13.0, 12.5), (14.0, 16.0), (15.5, 14.5)]);
        }
        Icon::Tag => {
            line(&[(4.0, 4.0), (12.0, 4.0), (20.5, 12.5), (12.5, 20.5), (4.0, 12.0), (4.0, 4.0)]);
            dot(8.5, 8.5, 1.3);
        }
        Icon::Pin => {
            p.add(Shape::line(arc(at(12.0, 10.0), 6.5 * s, 150.0, 390.0, 18), st));
            line(&[(6.4, 13.3), (12.0, 21.0), (17.6, 13.3)]);
            circle(12.0, 10.0, 2.3);
        }
        Icon::Calendar => {
            rrect(3.5, 5.5, 20.5, 20.5, 1.5);
            line(&[(3.5, 10.0), (20.5, 10.0)]);
            line(&[(8.0, 3.5), (8.0, 7.5)]);
            line(&[(16.0, 3.5), (16.0, 7.5)]);
            dot(8.5, 14.5, 1.0);
            dot(12.0, 14.5, 1.0);
            dot(15.5, 14.5, 1.0);
        }
        Icon::Sync => {
            p.add(Shape::line(arc(at(12.0, 12.0), 7.5 * s, 210.0, 330.0, 14), st));
            p.add(Shape::line(arc(at(12.0, 12.0), 7.5 * s, 30.0, 150.0, 14), st));
            line(&[(15.5, 5.0), (18.5, 8.2), (14.8, 9.0)]);
            line(&[(8.5, 19.0), (5.5, 15.8), (9.2, 15.0)]);
        }
        Icon::Help => {
            circle(12.0, 12.0, 8.5);
            p.add(Shape::line(arc(at(12.0, 9.5), 3.0 * s, 180.0, 400.0, 12), st));
            line(&[(12.0, 12.5), (12.0, 14.5)]);
            dot(12.0, 17.3, 1.1);
        }
        Icon::Audit => {
            document();
            line(&[(9.0, 11.0), (10.5, 12.5), (13.5, 9.5)]);
            line(&[(9.0, 16.0), (10.5, 17.5), (13.5, 14.5)]);
        }
        Icon::Server => {
            rrect(3.5, 4.0, 20.5, 10.5, 1.5);
            rrect(3.5, 13.5, 20.5, 20.0, 1.5);
            dot(7.0, 7.2, 1.0);
            dot(7.0, 16.7, 1.0);
        }
        Icon::Copy => {
            rrect(8.0, 8.0, 20.0, 20.5, 1.5);
            line(&[(5.5, 15.5), (4.0, 15.5), (4.0, 3.5), (16.0, 3.5), (16.0, 5.5)]);
        }
        Icon::Lock => {
            rrect(5.0, 10.5, 19.0, 20.5, 1.5);
            p.add(Shape::line(arc(at(12.0, 10.0), 4.5 * s, 180.0, 360.0, 12), st));
            line(&[(7.5, 10.0), (7.5, 10.5)]);
            line(&[(16.5, 10.0), (16.5, 10.5)]);
            dot(12.0, 15.5, 1.3);
        }
        Icon::Euro => {
            p.add(Shape::line(arc(at(13.5, 12.0), 6.5 * s, 50.0, 310.0, 16), st));
            line(&[(4.5, 10.0), (14.0, 10.0)]);
            line(&[(4.5, 14.0), (14.0, 14.0)]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Icon; 53] = [
        Icon::Logo,
        Icon::Customers,
        Icon::Customer,
        Icon::Company,
        Icon::Device,
        Icon::Interventions,
        Icon::Timer,
        Icon::Play,
        Icon::Stop,
        Icon::Privacy,
        Icon::ShieldCheck,
        Icon::ShieldOff,
        Icon::Reports,
        Icon::Settings,
        Icon::Search,
        Icon::Plus,
        Icon::Close,
        Icon::Chevron,
        Icon::ChevronRight,
        Icon::Back,
        Icon::Check,
        Icon::Warning,
        Icon::Info,
        Icon::Connect,
        Icon::Export,
        Icon::Trash,
        Icon::Archive,
        Icon::Edit,
        Icon::Moon,
        Icon::Sun,
        Icon::Globe,
        Icon::Account,
        Icon::More,
        Icon::Keyboard,
        Icon::Phone,
        Icon::Mail,
        Icon::Onsite,
        Icon::Lab,
        Icon::Remote,
        Icon::Notice,
        Icon::Signature,
        Icon::History,
        Icon::Tag,
        Icon::Pin,
        Icon::Calendar,
        Icon::Sync,
        Icon::Help,
        Icon::Audit,
        Icon::Server,
        Icon::Copy,
        Icon::Erase,
        Icon::Lock,
        Icon::Euro,
    ];

    #[test]
    fn every_icon_paints_something_inside_its_rect() {
        let ctx = egui::Context::default();
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0))), ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| {
            for icon in ALL {
                let rect = Rect::from_min_size(Pos2::new(10.0, 10.0), vec2(24.0, 24.0));
                paint(ui.painter(), rect, icon, Color32::BLACK);
            }
            // Zero-sized rects must not panic (a collapsed panel).
            paint(ui.painter(), Rect::NOTHING, Icon::Logo, Color32::BLACK);
            paint(ui.painter(), Rect::from_min_size(Pos2::ZERO, vec2(0.0, 0.0)), Icon::Globe, Color32::BLACK);
        });
        output.textures_delta.clear();
        // Every icon adds shapes: far more than the few the empty frame itself produces.
        assert!(output.shapes.len() > ALL.len() * 2, "{}", output.shapes.len());
    }
}
