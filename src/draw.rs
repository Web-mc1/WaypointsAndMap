//! The shapes the mod hands the game: markers over the minimap, and the world
//! map. Both are plain lists of `render` shapes (at most 1024 a mod).
use crate::geom::{self, Anchor, Panel, WorldView};
use horizon_mod_sdk::render::{FillShape, Point, QuadShape, Rect, Shape, TextShape};

const MAX_SHAPES: usize = 1024;
const OUTLINE: u32 = 0xC0000000;
const WHITE: u32 = 0xFFFFFFFF;
/// Waypoint names drawn on the world map; the rest show only their marker.
const MAX_LABELS: usize = 80;

fn argb(rgb: u32, alpha: u32) -> u32 {
    (alpha << 24) | (rgb & 0x00FF_FFFF)
}

fn fill(x: f32, y: f32, w: f32, h: f32, color: u32) -> Shape {
    Shape::Fill(FillShape {
        rect: Rect { x, y, w, h },
        color,
    })
}

fn quad(a: (f32, f32), b: (f32, f32), c: (f32, f32), d: (f32, f32), color: u32) -> Shape {
    let p = |t: (f32, f32)| Point { x: t.0, y: t.1 };
    Shape::Quad(QuadShape {
        corners: (p(a), p(b), p(c), p(d)),
        color,
    })
}

fn diamond(cx: f32, cy: f32, r: f32, color: u32) -> Shape {
    quad((cx, cy - r), (cx + r, cy), (cx, cy + r), (cx - r, cy), color)
}

fn text(s: &str, x: f32, y: f32, rgb: u32, centered: bool) -> Shape {
    Shape::Text(TextShape {
        text: s.chars().take(200).collect(),
        at: Point { x, y },
        color: rgb & 0x00FF_FFFF,
        centered,
        shadow: true,
    })
}

/// A waypoint on the minimap, as an offset from the player in blocks.
pub struct Mark {
    pub dx: f64,
    pub dz: f64,
    pub rgb: u32,
    pub selected: bool,
    /// From the other of Overworld and Nether, scaled: drawn fainter.
    pub other_dim: bool,
}

pub struct Mini<'a> {
    pub gui: (f32, f32),
    pub anchor: Anchor,
    pub size: f32,
    pub blocks_per_pixel: f32,
    pub rotate: bool,
    pub round: bool,
    pub yaw: f32,
    pub marks: &'a [Mark],
    /// The selected waypoint's line and its width in GUI pixels.
    pub caption: Option<(String, f32)>,
}

/// Markers over the game's own minimap, which sits in its corner where
/// `geom::map_box` says: the game draws it first, then a mod's shapes.
pub fn minimap(m: &Mini) -> Vec<Shape> {
    let b = geom::map_box(m.anchor, m.size, m.gui.0, m.gui.1);
    let ax = geom::axes(m.rotate, m.yaw);
    let mut out = Vec::new();
    for mark in m.marks {
        let p = geom::minimap_point(&b, m.round, m.blocks_per_pixel, &ax, mark.dx, mark.dz);
        let alpha = if mark.other_dim { 0xA0 } else { 0xFF };
        let r = if p.clamped { 2.5 } else { 3.5 };
        if mark.selected {
            out.push(diamond(p.x, p.y, r + 2.5, WHITE));
        }
        out.push(diamond(p.x, p.y, r + 1.0, OUTLINE));
        out.push(diamond(p.x, p.y, r, argb(mark.rgb, alpha)));
    }
    if let Some((line, width)) = &m.caption {
        let x = if m.anchor.left() {
            b.x
        } else {
            b.x + b.size - width
        };
        let y = if m.anchor.top() {
            b.y + b.size + 2.0
        } else {
            b.y - 11.0
        };
        out.push(fill(x - 2.0, y - 1.0, width + 4.0, 10.0, 0x78000000));
        out.push(text(line, x, y, 0xFFFFFF, false));
    }
    out.truncate(MAX_SHAPES);
    out
}

/// A waypoint on the world map, in the player's dimension's coordinates.
pub struct WMark {
    pub x: f64,
    pub z: f64,
    pub rgb: u32,
    pub label: String,
    pub selected: bool,
    pub other_dim: bool,
}

pub struct World<'a> {
    pub panel: Panel,
    pub view: WorldView,
    /// The player: x, z and yaw.
    pub me: Option<(f64, f64, f32)>,
    pub title: String,
    /// The selected waypoint's line, or empty.
    pub footer: String,
    pub hint: &'a str,
    pub marks: &'a [WMark],
    /// Explored squares, already cut to the panel.
    pub trail: &'a [Panel],
}

/// The world map: a dark panel with a grid, the explored squares, the
/// waypoints and the player.
pub fn world_map(w: &World) -> Vec<Shape> {
    let p = &w.panel;
    let v = &w.view;
    let mut out = Vec::new();

    out.push(fill(p.x, p.y, p.w, p.h, 0xF0101820));
    for r in w.trail {
        out.push(fill(r.x, r.y, r.w, r.h, 0x903A7A4A));
    }

    // Grid lines at a round number of blocks, at least 64 pixels apart.
    let half_w = f64::from(p.w) / 2.0 / v.ppb;
    let half_h = f64::from(p.h) / 2.0 / v.ppb;
    let step = geom::nice_step(64.0 / v.ppb);
    let mut gx = ((v.cx - half_w) / step).ceil() * step;
    let mut n = 0;
    while gx <= v.cx + half_w && n < 64 {
        let (sx, _) = v.to_screen(p, gx, v.cz);
        let axis = gx.abs() < 0.5;
        out.push(fill(sx, p.y, 1.0, p.h, if axis { 0x70FFFFFF } else { 0x26FFFFFF }));
        out.push(text(&format!("{}", gx.round() as i64), sx + 2.0, p.y + 16.0, 0x8899AA, false));
        gx += step;
        n += 1;
    }
    let mut gz = ((v.cz - half_h) / step).ceil() * step;
    n = 0;
    while gz <= v.cz + half_h && n < 64 {
        let (_, sy) = v.to_screen(p, v.cx, gz);
        let axis = gz.abs() < 0.5;
        out.push(fill(p.x, sy, p.w, 1.0, if axis { 0x70FFFFFF } else { 0x26FFFFFF }));
        out.push(text(&format!("{}", gz.round() as i64), p.x + 3.0, sy + 2.0, 0x8899AA, false));
        gz += step;
        n += 1;
    }

    // The selected waypoint last, so nothing covers it.
    let mut labels = 0;
    let order = w
        .marks
        .iter()
        .filter(|m| !m.selected)
        .chain(w.marks.iter().filter(|m| m.selected));
    for m in order {
        let (sx, sy) = v.to_screen(p, m.x, m.z);
        if !p.contains(sx, sy, 6.0) {
            continue;
        }
        let alpha = if m.other_dim { 0xB0 } else { 0xFF };
        if m.selected {
            out.push(diamond(sx, sy, 8.5, WHITE));
        }
        out.push(diamond(sx, sy, 6.0, OUTLINE));
        out.push(diamond(sx, sy, 5.0, argb(m.rgb, alpha)));
        if labels < MAX_LABELS || m.selected {
            labels += 1;
            let color = if m.selected { 0xFFFFFF } else { m.rgb };
            out.push(text(&m.label, sx + 9.0, sy - 4.0, color, false));
        }
    }

    if let Some((x, z, yaw)) = w.me {
        let (sx, sy) = v.to_screen(p, x, z);
        if p.contains(sx, sy, 10.0) {
            // Yaw 0 faces south (+z, down the screen), 90 west (-x).
            let (s, c) = f64::from(yaw).to_radians().sin_cos();
            let (dx, dy) = ((-s) as f32, c as f32);
            let (nx, ny) = (-dy, dx);
            let tip = (sx + dx * 9.0, sy + dy * 9.0);
            let back = (sx - dx * 6.0, sy - dy * 6.0);
            let notch = (sx - dx * 3.0, sy - dy * 3.0);
            let left = (back.0 + nx * 5.0, back.1 + ny * 5.0);
            let right = (back.0 - nx * 5.0, back.1 - ny * 5.0);
            out.push(diamond(sx, sy, 11.0, 0x60000000));
            out.push(quad(tip, right, notch, left, WHITE));
        }
    }

    // Frame, compass, texts, scale.
    let (x1, y1) = (p.x + p.w, p.y + p.h);
    for (x, y, ww, hh) in [
        (p.x, p.y, p.w, 1.0),
        (p.x, y1 - 1.0, p.w, 1.0),
        (p.x, p.y, 1.0, p.h),
        (x1 - 1.0, p.y, 1.0, p.h),
    ] {
        out.push(fill(x, y, ww, hh, 0xFF5A6A7A));
    }
    out.push(text("N", x1 - 14.0, p.y + 5.0, 0xFFFFFF, true));
    out.push(text(&w.title, p.x + 6.0, p.y + 5.0, 0xFFFFFF, false));
    if !w.footer.is_empty() {
        out.push(text(&w.footer, p.x + 6.0, y1 - 24.0, 0xFFFFFF, false));
    }
    out.push(text(w.hint, p.x + 6.0, y1 - 12.0, 0x8899AA, false));
    let bar_blocks = geom::nice_step(80.0 / v.ppb);
    let bar = (bar_blocks * v.ppb) as f32;
    out.push(fill(x1 - 8.0 - bar, y1 - 22.0, bar, 2.0, WHITE));
    out.push(text(
        &geom::fmt_dist(bar_blocks),
        x1 - 8.0 - bar / 2.0,
        y1 - 16.0,
        0xFFFFFF,
        true,
    ));

    out.truncate(MAX_SHAPES);
    out
}

/// A waypoint drawn in the 3D view.
pub struct VMark {
    pub x: f32,
    pub y: f32,
    /// On the screen; otherwise on its edge, pointing the way to turn.
    pub on: bool,
    pub rgb: u32,
    pub selected: bool,
    pub label: String,
    pub width: f32,
    pub show_label: bool,
}

/// Waypoint markers over the game view, one diamond each; names for the
/// tracked waypoint and the one near the crosshair.
pub fn view_markers(gui: (f32, f32), marks: &[VMark]) -> Vec<Shape> {
    let mut out = Vec::new();
    let order = marks
        .iter()
        .filter(|m| !m.selected)
        .chain(marks.iter().filter(|m| m.selected));
    for m in order {
        let r = if m.on { 5.0 } else { 4.0 };
        if m.selected {
            out.push(diamond(m.x, m.y, r + 2.5, WHITE));
        }
        out.push(diamond(m.x, m.y, r + 1.0, OUTLINE));
        out.push(diamond(m.x, m.y, r, argb(m.rgb, 0xFF)));
        if m.show_label {
            let lo = 2.0 + m.width / 2.0;
            let hi = (gui.0 - 2.0 - m.width / 2.0).max(lo);
            let x = m.x.clamp(lo, hi);
            let y = if m.on || m.y < gui.1 / 2.0 {
                m.y + r + 3.0
            } else {
                m.y - r - 12.0
            };
            out.push(fill(x - m.width / 2.0 - 2.0, y - 1.0, m.width + 4.0, 10.0, 0x78000000));
            out.push(text(&m.label, x, y, if m.selected { 0xFFFFFF } else { m.rgb }, true));
        }
    }
    out.truncate(MAX_SHAPES);
    out
}

/// A waypoint on the terrain map, as an offset from the player in blocks.
pub struct TMark {
    pub dx: f64,
    pub dz: f64,
    pub rgb: u32,
    pub label: String,
    pub width: f32,
    pub selected: bool,
    pub other_dim: bool,
}

pub struct Terrain<'a> {
    pub gui: (f32, f32),
    pub anchor: Anchor,
    pub size: f32,
    pub blocks_per_pixel: f32,
    pub title: String,
    pub footer: String,
    /// Help lines beside the map.
    pub side: &'a [&'a str],
    pub marks: &'a [TMark],
}

/// The world map with real terrain: the game draws its own map (big, north
/// up, around the player) and these shapes go over it: frame, waypoints with
/// names, a scale, and the help beside it.
pub fn terrain_map(t: &Terrain) -> Vec<Shape> {
    let b = geom::map_box(t.anchor, t.size, t.gui.0, t.gui.1);
    let ax = geom::axes(false, 0.0);
    let mut out = Vec::new();

    for (x, y, w, h) in [
        (b.x - 1.0, b.y - 1.0, b.size + 2.0, 1.0),
        (b.x - 1.0, b.y + b.size, b.size + 2.0, 1.0),
        (b.x - 1.0, b.y - 1.0, 1.0, b.size + 2.0),
        (b.x + b.size, b.y - 1.0, 1.0, b.size + 2.0),
    ] {
        out.push(fill(x, y, w, h, 0xFF5A6A7A));
    }
    out.push(fill(b.x, b.y, b.size, 11.0, 0x80000000));
    out.push(text(&t.title, b.x + 3.0, b.y + 2.0, 0xFFFFFF, false));
    out.push(text("N", b.x + b.size - 8.0, b.y + 2.0, 0xFFFFFF, true));

    let order = t
        .marks
        .iter()
        .filter(|m| !m.selected)
        .chain(t.marks.iter().filter(|m| m.selected));
    for m in order {
        let p = geom::minimap_point(&b, false, t.blocks_per_pixel, &ax, m.dx, m.dz);
        let alpha = if m.other_dim { 0xB0 } else { 0xFF };
        let r = if p.clamped { 3.5 } else { 4.5 };
        if m.selected {
            out.push(diamond(p.x, p.y, r + 2.5, WHITE));
        }
        out.push(diamond(p.x, p.y, r + 1.0, OUTLINE));
        out.push(diamond(p.x, p.y, r, argb(m.rgb, alpha)));
        if !p.clamped || m.selected {
            let right = p.x + 8.0 + m.width <= b.x + b.size - 2.0;
            let x = if right { p.x + 8.0 } else { p.x - 8.0 - m.width };
            let y = (p.y - 4.0).clamp(b.y + 12.0, b.y + b.size - 22.0);
            out.push(fill(x - 1.0, y - 1.0, m.width + 2.0, 10.0, 0x78000000));
            out.push(text(&m.label, x, y, if m.selected { 0xFFFFFF } else { m.rgb }, false));
        }
    }

    // Scale bar, above the footer.
    let blocks_per_gui = f64::from(t.blocks_per_pixel) * f64::from(geom::IMAGE / t.size);
    let bar_blocks = geom::nice_step(40.0 * blocks_per_gui);
    let bar = (bar_blocks / blocks_per_gui) as f32;
    let base = b.y + b.size - if t.footer.is_empty() { 13.0 } else { 24.0 };
    out.push(fill(b.x + 4.0, base, bar, 2.0, WHITE));
    out.push(text(&geom::fmt_dist(bar_blocks), b.x + 8.0 + bar, base - 3.0, 0xFFFFFF, false));
    if !t.footer.is_empty() {
        out.push(fill(b.x, b.y + b.size - 11.0, b.size, 11.0, 0x80000000));
        out.push(text(&t.footer, b.x + 3.0, b.y + b.size - 9.0, 0xFFFFFF, false));
    }

    // Help beside the map, on the side with room.
    let widest = t
        .side
        .iter()
        .map(|l| horizon_mod_sdk::render::text_width(l))
        .fold(0.0_f32, f32::max);
    let x = if t.anchor.left() {
        b.x + b.size + 8.0
    } else {
        b.x - 8.0 - widest
    };
    let mut y = if t.anchor.top() {
        b.y
    } else {
        b.y + b.size - 11.0 * t.side.len() as f32
    };
    for line in t.side {
        out.push(fill(x - 2.0, y - 1.0, horizon_mod_sdk::render::text_width(line) + 4.0, 10.0, 0x78000000));
        out.push(text(line, x, y, 0xDDE6EE, false));
        y += 11.0;
    }
    out.truncate(MAX_SHAPES);
    out
}
