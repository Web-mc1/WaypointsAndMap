//! Map geometry: where a world position lands on the minimap and on the world
//! map. Nothing here talks to the game, so it is all unit tested.
use std::collections::HashSet;

pub const OVERWORLD: &str = "minecraft:overworld";
pub const NETHER: &str = "minecraft:the_nether";

/// The game's minimap image is this many pixels square, whatever its size on
/// screen (`horizon-client/src/minimap.rs`: `SIZE`).
pub const IMAGE: f32 = 128.0;
/// The gap the game leaves between an overlay and the screen edge
/// (`extras_hud.rs`: `MARGIN`).
const MARGIN: f32 = 2.0;
/// Markers that would leave the map are held this far inside its rim.
const INSET: f32 = 5.0;

/// The explored-area trail is kept in cells of 32 x 32 blocks.
pub const CELL_SHIFT: i32 = 5;
pub const MIN_PPB: f64 = 0.02;
pub const MAX_PPB: f64 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Anchor {
    pub fn left(self) -> bool {
        matches!(self, Anchor::TopLeft | Anchor::BottomLeft)
    }
    pub fn top(self) -> bool {
        matches!(self, Anchor::TopLeft | Anchor::TopRight)
    }
}

/// Where the game puts the minimap: a square in GUI pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MapBox {
    pub x: f32,
    pub y: f32,
    pub size: f32,
}

impl MapBox {
    pub fn centre(&self) -> (f32, f32) {
        (self.x + self.size / 2.0, self.y + self.size / 2.0)
    }
}

/// The minimap is the first overlay the game places in its corner, so it sits
/// MARGIN from the screen edges (`Corners::place`).
pub fn map_box(anchor: Anchor, size: f32, w: f32, h: f32) -> MapBox {
    MapBox {
        x: if anchor.left() {
            MARGIN
        } else {
            w - MARGIN - size
        },
        y: if anchor.top() {
            MARGIN
        } else {
            h - MARGIN - size
        },
        size,
    }
}

/// The world directions (x, z) the map's right and up point to
/// (`horizon-client/src/minimap.rs`: `View::axes`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axes {
    pub right: (f64, f64),
    pub up: (f64, f64),
}

/// North up, or (when it turns) the player's facing up. Yaw 0 faces south.
pub fn axes(rotate: bool, yaw: f32) -> Axes {
    if !rotate {
        return Axes {
            right: (1.0, 0.0),
            up: (0.0, -1.0),
        };
    }
    let (s, c) = f64::from(yaw).to_radians().sin_cos();
    Axes {
        right: (-c, -s),
        up: (-s, c),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenPoint {
    pub x: f32,
    pub y: f32,
    /// The position was off the map: the point sits on its rim.
    pub clamped: bool,
}

/// Where a block `dx`, `dz` away from the player is on the minimap.
pub fn minimap_point(
    b: &MapBox,
    round: bool,
    blocks_per_pixel: f32,
    ax: &Axes,
    dx: f64,
    dz: f64,
) -> ScreenPoint {
    // GUI pixels per block.
    let k = f64::from(b.size / IMAGE) / f64::from(blocks_per_pixel);
    let u = (dx * ax.right.0 + dz * ax.right.1) * k;
    let t = (dx * ax.up.0 + dz * ax.up.1) * k;
    let (ox, oy) = (u as f32, (-t) as f32);
    let half = b.size / 2.0 - INSET;
    let (cx, cy) = b.centre();
    let reach = if round {
        ox.hypot(oy)
    } else {
        ox.abs().max(oy.abs())
    };
    if reach > half && reach > 0.0 {
        let s = half / reach;
        ScreenPoint {
            x: cx + ox * s,
            y: cy + oy * s,
            clamped: true,
        }
    } else {
        ScreenPoint {
            x: cx + ox,
            y: cy + oy,
            clamped: false,
        }
    }
}

/// A waypoint of `from_dim` as it sits in `to_dim`'s coordinates: the same
/// dimension as it is, Overworld and Nether scaled 8:1 when `scale` is on.
pub fn project(from_dim: &str, to_dim: &str, x: i32, z: i32, scale: bool) -> Option<(f64, f64)> {
    let (bx, bz) = (f64::from(x) + 0.5, f64::from(z) + 0.5);
    if from_dim == to_dim {
        return Some((bx, bz));
    }
    if !scale {
        return None;
    }
    match (from_dim, to_dim) {
        (OVERWORLD, NETHER) => Some((bx / 8.0, bz / 8.0)),
        (NETHER, OVERWORLD) => Some((bx * 8.0, bz * 8.0)),
        _ => None,
    }
}

/// N, NE, E ... from an offset (north is -z).
pub fn compass(dx: f64, dz: f64) -> &'static str {
    if dx.abs() < 0.5 && dz.abs() < 0.5 {
        return "here";
    }
    let bearing = dx.atan2(-dz).to_degrees();
    let i = ((bearing + 22.5) / 45.0).floor() as i64;
    ["N", "NE", "E", "SE", "S", "SW", "W", "NW"][i.rem_euclid(8) as usize]
}

pub fn fmt_dist(d: f64) -> String {
    if d < 1000.0 {
        format!("{} m", d.round() as i64)
    } else {
        format!("{:.1} km", d / 1000.0)
    }
}

/// A rectangle in GUI pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Panel {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Panel {
    pub fn centre(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
    pub fn contains(&self, x: f32, y: f32, pad: f32) -> bool {
        x >= self.x + pad && x <= self.x + self.w - pad && y >= self.y + pad && y <= self.y + self.h - pad
    }
}

/// The world map's camera: the block at the panel's centre and the zoom, in
/// GUI pixels per block. North is up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldView {
    pub cx: f64,
    pub cz: f64,
    pub ppb: f64,
}

impl WorldView {
    pub fn to_screen(&self, p: &Panel, x: f64, z: f64) -> (f32, f32) {
        let (mx, my) = p.centre();
        (
            mx + ((x - self.cx) * self.ppb) as f32,
            my + ((z - self.cz) * self.ppb) as f32,
        )
    }
    /// One mouse-wheel step is a quarter more or less.
    pub fn zoom(&mut self, steps: f32) {
        self.ppb = (self.ppb * 1.25_f64.powf(f64::from(steps))).clamp(MIN_PPB, MAX_PPB);
    }
    pub fn pan_px(&mut self, dx: f64, dz: f64) {
        self.cx += dx / self.ppb;
        self.cz += dz / self.ppb;
    }
}

/// A view that holds every point with some room round them.
pub fn fit(points: &[(f64, f64)], p: &Panel) -> WorldView {
    let Some(&(fx, fz)) = points.first() else {
        return WorldView {
            cx: 0.0,
            cz: 0.0,
            ppb: 1.0,
        };
    };
    let (mut x0, mut x1, mut z0, mut z1) = (fx, fx, fz, fz);
    for &(x, z) in points {
        x0 = x0.min(x);
        x1 = x1.max(x);
        z0 = z0.min(z);
        z1 = z1.max(z);
    }
    let sx = (x1 - x0).max(32.0);
    let sz = (z1 - z0).max(32.0);
    let ppb = ((f64::from(p.w) * 0.8) / sx)
        .min((f64::from(p.h) * 0.8) / sz)
        .clamp(MIN_PPB, MAX_PPB);
    WorldView {
        cx: (x0 + x1) / 2.0,
        cz: (z0 + z1) / 2.0,
        ppb,
    }
}

/// The smallest 1, 2 or 5 times a power of ten that is at least `min_blocks`.
pub fn nice_step(min_blocks: f64) -> f64 {
    let min = min_blocks.max(1.0);
    let pow = 10f64.powf(min.log10().floor());
    for m in [1.0, 2.0, 5.0, 10.0] {
        if pow * m >= min {
            return pow * m;
        }
    }
    pow * 10.0
}

/// The trail cell a world coordinate is in.
pub fn cell_of(v: f64) -> i32 {
    (v.floor() as i32) >> CELL_SHIFT
}

/// How many cells make one drawn square: enough to be about 3 pixels wide,
/// a power of two so the cache does not churn while zooming.
pub fn bin_for(ppb: f64) -> i32 {
    ((3.0 / (32.0 * ppb)).ceil().max(1.0) as u32).next_power_of_two() as i32
}

pub fn coarsen(cells: &HashSet<(i32, i32)>, bin: i32) -> HashSet<(i32, i32)> {
    cells
        .iter()
        .map(|&(x, z)| (x.div_euclid(bin), z.div_euclid(bin)))
        .collect()
}

fn bin_rect(bx: i64, bz: i64, bs: f64, v: &WorldView, p: &Panel) -> Option<Panel> {
    let (x0, y0) = v.to_screen(p, bx as f64 * bs, bz as f64 * bs);
    let side = (bs * v.ppb) as f32;
    let (cx0, cy0) = (x0.max(p.x), y0.max(p.y));
    let (cx1, cy1) = ((x0 + side).min(p.x + p.w), (y0 + side).min(p.y + p.h));
    (cx1 > cx0 && cy1 > cy0).then_some(Panel {
        x: cx0,
        y: cy0,
        w: cx1 - cx0,
        h: cy1 - cy0,
    })
}

/// The squares of `bins` that are on the panel, cut to it, at most `cap`.
pub fn trail_rects(
    bins: &HashSet<(i32, i32)>,
    bin: i32,
    v: &WorldView,
    p: &Panel,
    cap: usize,
) -> Vec<Panel> {
    let bs = f64::from(bin << CELL_SHIFT);
    let half_w = f64::from(p.w) / 2.0 / v.ppb;
    let half_h = f64::from(p.h) / 2.0 / v.ppb;
    let bx0 = ((v.cx - half_w) / bs).floor() as i64;
    let bx1 = ((v.cx + half_w) / bs).floor() as i64;
    let bz0 = ((v.cz - half_h) / bs).floor() as i64;
    let bz1 = ((v.cz + half_h) / bs).floor() as i64;
    let span = (bx1 - bx0 + 1) * (bz1 - bz0 + 1);
    let mut out = Vec::new();
    if span <= bins.len() as i64 {
        'rows: for bz in bz0..=bz1 {
            for bx in bx0..=bx1 {
                if bins.contains(&(bx as i32, bz as i32))
                    && let Some(r) = bin_rect(bx, bz, bs, v, p)
                {
                    out.push(r);
                    if out.len() >= cap {
                        break 'rows;
                    }
                }
            }
        }
    } else {
        for &(bx, bz) in bins {
            let (bx, bz) = (i64::from(bx), i64::from(bz));
            if bx < bx0 || bx > bx1 || bz < bz0 || bz > bz1 {
                continue;
            }
            if let Some(r) = bin_rect(bx, bz, bs, v, p) {
                out.push(r);
                if out.len() >= cap {
                    break;
                }
            }
        }
    }
    out
}

/// Where a block shows on the screen, as seen from the player's eyes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewPoint {
    pub x: f32,
    pub y: f32,
    /// On the screen. Otherwise (x, y) is on the screen's edge, in the
    /// direction the player would have to turn.
    pub on: bool,
}

/// Looks from `eye` along `yaw` / `pitch` (the game's: yaw 0 faces south,
/// pitch 90 looks down) with a vertical field of view of `fov_deg` on a
/// `gw` x `gh` GUI, and says where `target` falls. Same maths as the game's
/// perspective matrix (`camera::perspective`).
pub fn project_view(
    eye: (f64, f64, f64),
    yaw: f32,
    pitch: f32,
    fov_deg: f32,
    gw: f32,
    gh: f32,
    target: (f64, f64, f64),
) -> ViewPoint {
    let (sy, cy) = f64::from(yaw).to_radians().sin_cos();
    let (sp, cp) = f64::from(pitch).to_radians().sin_cos();
    // Forward, right and up, in world axes.
    let f = (-sy * cp, -sp, cy * cp);
    let r = (-cy, 0.0, -sy);
    let u = (-r.2 * f.1, r.2 * f.0 - r.0 * f.2, r.0 * f.1);
    let d = (target.0 - eye.0, target.1 - eye.1, target.2 - eye.2);
    let dot = |a: (f64, f64, f64)| a.0 * d.0 + a.1 * d.1 + a.2 * d.2;
    let (zc, xc, yc) = (dot(f), dot(r), dot(u));

    let (w, h) = (f64::from(gw), f64::from(gh));
    let (mx, my) = (w / 2.0, h / 2.0);
    let t = (f64::from(fov_deg).to_radians() / 2.0).tan();
    let aspect = w / h;
    // Held this far from the screen's edge.
    const EDGE: f64 = 14.0;
    let (dx, dy) = if zc > 0.05 {
        let px = mx + xc / (zc * t * aspect) * mx;
        let py = my - yc / (zc * t) * my;
        if px >= EDGE && px <= w - EDGE && py >= EDGE && py <= h - EDGE {
            return ViewPoint {
                x: px as f32,
                y: py as f32,
                on: true,
            };
        }
        (px - mx, py - my)
    } else {
        // Behind: point to the side it is on.
        (if xc >= 0.0 { 1.0 } else { -1.0 }, 0.0)
    };
    let s = ((mx - EDGE) / dx.abs().max(1e-6)).min((my - EDGE) / dy.abs().max(1e-6));
    ViewPoint {
        x: (mx + dx * s) as f32,
        y: (my + dy * s) as f32,
        on: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_ahead_is_the_screen_centre() {
        // Yaw 0 faces south (+z).
        let p = project_view((0.0, 0.0, 0.0), 0.0, 0.0, 70.0, 400.0, 200.0, (0.0, 0.0, 10.0));
        assert!(p.on);
        assert!((p.x - 200.0).abs() < 0.01 && (p.y - 100.0).abs() < 0.01);
    }

    #[test]
    fn right_is_right_and_up_is_up() {
        // Facing south, the player's right hand points west (-x).
        let r = project_view((0.0, 0.0, 0.0), 0.0, 0.0, 70.0, 400.0, 200.0, (-2.0, 0.0, 10.0));
        assert!(r.on && r.x > 200.0);
        let up = project_view((0.0, 0.0, 0.0), 0.0, 0.0, 70.0, 400.0, 200.0, (0.0, 2.0, 10.0));
        assert!(up.on && up.y < 100.0);
        // Facing west (yaw 90), north (-z) is on the right.
        let n = project_view((0.0, 0.0, 0.0), 90.0, 0.0, 70.0, 400.0, 200.0, (-10.0, 0.0, -2.0));
        assert!(n.on && n.x > 200.0);
    }

    #[test]
    fn behind_and_far_to_the_side_sit_on_the_edge() {
        let b = project_view((0.0, 0.0, 0.0), 0.0, 0.0, 70.0, 400.0, 200.0, (0.0, 0.0, -10.0));
        assert!(!b.on);
        assert!(b.x <= 400.0 && b.x >= 0.0);
        let side = project_view((0.0, 0.0, 0.0), 0.0, 0.0, 70.0, 400.0, 200.0, (-50.0, 0.0, 10.0));
        assert!(!side.on && side.x > 300.0);
    }

    fn square() -> MapBox {
        MapBox {
            x: 0.0,
            y: 0.0,
            size: 128.0,
        }
    }

    #[test]
    fn the_map_sits_in_its_corner() {
        let b = map_box(Anchor::BottomRight, 96.0, 400.0, 300.0);
        assert_eq!((b.x, b.y), (302.0, 202.0));
        let b = map_box(Anchor::TopLeft, 96.0, 400.0, 300.0);
        assert_eq!((b.x, b.y), (2.0, 2.0));
    }

    #[test]
    fn north_up_puts_east_right_and_north_up() {
        let ax = axes(false, 123.0);
        let east = minimap_point(&square(), false, 1.0, &ax, 10.0, 0.0);
        assert_eq!((east.x, east.y), (74.0, 64.0));
        let north = minimap_point(&square(), false, 1.0, &ax, 0.0, -10.0);
        assert_eq!((north.x, north.y), (64.0, 54.0));
    }

    #[test]
    fn a_turning_map_puts_what_is_ahead_up() {
        // Yaw 0 faces south (+z): a block ahead is up, and one to the west
        // (-x, the player's right hand when facing south) is to the right.
        let ax = axes(true, 0.0);
        let ahead = minimap_point(&square(), false, 1.0, &ax, 0.0, 10.0);
        assert!((ahead.x - 64.0).abs() < 1e-4 && (ahead.y - 54.0).abs() < 1e-4);
        let right = minimap_point(&square(), false, 1.0, &ax, -10.0, 0.0);
        assert!((right.x - 74.0).abs() < 1e-4 && (right.y - 64.0).abs() < 1e-4);
        // Facing west (yaw 90): west is ahead.
        let ax = axes(true, 90.0);
        let ahead = minimap_point(&square(), false, 1.0, &ax, -10.0, 0.0);
        assert!((ahead.x - 64.0).abs() < 1e-3 && (ahead.y - 54.0).abs() < 1e-3);
    }

    #[test]
    fn zoom_and_size_scale_the_offset() {
        let ax = axes(false, 0.0);
        // 2 blocks per pixel on a 128 px map: 20 blocks are 10 px.
        let p = minimap_point(&square(), false, 2.0, &ax, 20.0, 0.0);
        assert_eq!(p.x, 74.0);
        // A 64 px map shows the same image at half size.
        let small = MapBox {
            x: 0.0,
            y: 0.0,
            size: 64.0,
        };
        let p = minimap_point(&small, false, 1.0, &ax, 20.0, 0.0);
        assert_eq!(p.x, 42.0);
    }

    #[test]
    fn far_markers_stay_on_the_rim() {
        let ax = axes(false, 0.0);
        let square_rim = minimap_point(&square(), false, 1.0, &ax, 1000.0, 0.0);
        assert!(square_rim.clamped);
        assert!((square_rim.x - (64.0 + 59.0)).abs() < 1e-3);
        let round_rim = minimap_point(&square(), true, 1.0, &ax, 1000.0, 1000.0);
        let (dx, dy) = (round_rim.x - 64.0, round_rim.y - 64.0);
        assert!(round_rim.clamped);
        assert!((dx.hypot(dy) - 59.0).abs() < 1e-3);
        assert!(!minimap_point(&square(), true, 1.0, &ax, 10.0, 10.0).clamped);
    }

    #[test]
    fn nether_coordinates_scale_by_eight() {
        let (x, z) = project(OVERWORLD, NETHER, 800, -80, true).unwrap();
        assert_eq!((x, z), (100.0 + 0.0625, -10.0 + 0.0625));
        assert_eq!(project(NETHER, OVERWORLD, 10, 10, true), Some((84.0, 84.0)));
        assert_eq!(project(OVERWORLD, NETHER, 1, 1, false), None);
        assert_eq!(project(OVERWORLD, "minecraft:the_end", 1, 1, true), None);
        assert_eq!(project(OVERWORLD, OVERWORLD, 1, 1, false), Some((1.5, 1.5)));
    }

    #[test]
    fn the_compass_has_eight_points() {
        assert_eq!(compass(0.0, -10.0), "N");
        assert_eq!(compass(10.0, 0.0), "E");
        assert_eq!(compass(0.0, 10.0), "S");
        assert_eq!(compass(-10.0, 0.0), "W");
        assert_eq!(compass(10.0, -10.0), "NE");
        assert_eq!(compass(-10.0, 10.0), "SW");
        assert_eq!(compass(0.1, 0.1), "here");
    }

    #[test]
    fn steps_are_one_two_or_five() {
        assert_eq!(nice_step(64.0), 100.0);
        assert_eq!(nice_step(100.0), 100.0);
        assert_eq!(nice_step(101.0), 200.0);
        assert_eq!(nice_step(300.0), 500.0);
        assert_eq!(nice_step(0.2), 1.0);
    }

    #[test]
    fn distances_read_in_blocks_then_kilometres() {
        assert_eq!(fmt_dist(12.4), "12 m");
        assert_eq!(fmt_dist(1500.0), "1.5 km");
    }

    #[test]
    fn the_wheel_zooms_within_limits() {
        let mut v = WorldView {
            cx: 0.0,
            cz: 0.0,
            ppb: 1.0,
        };
        v.zoom(1.0);
        assert!((v.ppb - 1.25).abs() < 1e-9);
        v.zoom(-1000.0);
        assert_eq!(v.ppb, MIN_PPB);
        v.zoom(1000.0);
        assert_eq!(v.ppb, MAX_PPB);
    }

    #[test]
    fn panning_moves_by_screen_pixels() {
        let mut v = WorldView {
            cx: 0.0,
            cz: 0.0,
            ppb: 2.0,
        };
        v.pan_px(10.0, -20.0);
        assert_eq!((v.cx, v.cz), (5.0, -10.0));
    }

    #[test]
    fn fit_holds_every_point() {
        let p = Panel {
            x: 0.0,
            y: 0.0,
            w: 400.0,
            h: 200.0,
        };
        let v = fit(&[(0.0, 0.0), (1000.0, 100.0)], &p);
        assert_eq!((v.cx, v.cz), (500.0, 50.0));
        for &(x, z) in &[(0.0, 0.0), (1000.0, 100.0)] {
            let (sx, sy) = v.to_screen(&p, x, z);
            assert!(p.contains(sx, sy, 0.0), "{sx},{sy}");
        }
        assert_eq!(fit(&[], &p).ppb, 1.0);
    }

    #[test]
    fn trail_squares_are_cut_to_the_panel() {
        let p = Panel {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        };
        let v = WorldView {
            cx: 0.0,
            cz: 0.0,
            ppb: 1.0,
        };
        let mut cells = HashSet::new();
        cells.insert((0, 0));
        cells.insert((100, 100));
        let rects = trail_rects(&cells, 1, &v, &p, 100);
        assert_eq!(rects.len(), 1);
        let r = rects[0];
        // Cell (0, 0) covers blocks 0..32, from the centre (50, 50).
        assert_eq!((r.x, r.y, r.w, r.h), (50.0, 50.0, 32.0, 32.0));
        // Zoomed out, cells merge into bigger squares.
        assert_eq!(bin_for(1.0), 1);
        assert_eq!(bin_for(0.02), 8);
        assert!(coarsen(&cells, 8).contains(&(0, 0)));
    }

    #[test]
    fn cells_round_toward_minus_infinity() {
        assert_eq!(cell_of(0.0), 0);
        assert_eq!(cell_of(31.9), 0);
        assert_eq!(cell_of(32.0), 1);
        assert_eq!(cell_of(-0.1), -1);
    }
}
