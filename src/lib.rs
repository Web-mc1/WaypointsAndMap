//! Waypoints, a minimap and a world map in one Horizon mod.
//!
//! * The minimap is the game's own (`render.set-minimap`, terrain in map
//!   colours); this mod lays waypoint markers over it, at the spot the game
//!   draws it (see `geom::map_box`).
//! * The world map (M) has two views (V switches). Terrain: the game's own map
//!   drawn big (256 px) around you, with your waypoints named on it. Chart: a
//!   full-screen grid with the squares you have explored, which pans and zooms
//!   (mods cannot read blocks, so the game's map is the only terrain there is,
//!   and it is always centred on you).
//! * Waypoints also show in the 3D view: a marker on the spot, projected from
//!   your position, view and field of view (`geom::project_view`).
//! * Waypoints live in the mod's storage, one set per "world" (`/wp world`).
mod draw;
mod geom;
mod model;

use std::collections::{HashMap, HashSet};

use draw::{Mark, Mini, TMark, Terrain, VMark, WMark, World};
use geom::{Anchor, Panel, WorldView};
use horizon_mod_sdk::{
    Mod, Registration, chat,
    game::{self},
    info,
    input::{self, Event},
    render::{self, Corner, Minimap as MapView, Shape},
    storage,
    ui::{self, ButtonWidget, ListWidget, Screen, TextWidget, Widget},
    warn,
};
use model::{MAX_WAYPOINTS, PALETTE, Waypoint};

/// (anchor, stored value, label). The default is the bottom right: the top
/// corners shift under status effects (top right) or give way to F3 (top left),
/// which the mod cannot see, so markers there can be off by a few pixels.
const CORNERS: [(Anchor, &str, &str); 4] = [
    (Anchor::TopLeft, "top_left", "Top left"),
    (Anchor::TopRight, "top_right", "Top right"),
    (Anchor::BottomLeft, "bottom_left", "Bottom left"),
    (Anchor::BottomRight, "bottom_right", "Bottom right"),
];
/// (side in GUI pixels, label).
const SIZES: [(u8, &str); 4] = [(64, "Small"), (96, "Medium"), (128, "Large"), (192, "Huge")];
/// Blocks per map pixel.
const ZOOMS: [f32; 4] = [0.5, 1.0, 2.0, 4.0];

/// (name, label, default key): the keys the player can rebind in Options > Mods.
const KEYS: [(&str, &str, &str); 12] = [
    ("wp_add", "Add waypoint here", "key.keyboard.b"),
    ("wp_map", "World map", "key.keyboard.m"),
    ("wp_menu", "Waypoint manager", "key.keyboard.n"),
    // Not the arrow keys: the game turns the camera with those while you play.
    ("wp_view", "World map: terrain / chart", "key.keyboard.v"),
    ("wp_up", "World map: pan up", "key.keyboard.keypad.8"),
    ("wp_down", "World map: pan down", "key.keyboard.keypad.2"),
    ("wp_left", "World map: pan left", "key.keyboard.keypad.4"),
    ("wp_right", "World map: pan right", "key.keyboard.keypad.6"),
    ("wp_center", "World map: centre on me", "key.keyboard.home"),
    ("wp_fit", "World map: fit all waypoints", "key.keyboard.end"),
    ("wp_next", "Select next waypoint", "key.keyboard.right.bracket"),
    ("wp_prev", "Select previous waypoint", "key.keyboard.left.bracket"),
];

const HINT_CHART: &str = "Wheel: zoom   Numpad 8 4 2 6: pan   Home: centre on me   End: fit all   V: terrain   [ ]: select   M: close";
/// Help beside the terrain map.
const TERRAIN_HINT: [&str; 5] = [
    "World map: terrain around you",
    "Wheel: zoom",
    "V or numpad 8 4 2 6: chart (pans)",
    "[ ]: select waypoint   B: add here",
    "M: close",
];
/// The game's map is at most 256 GUI pixels a side.
const TERRAIN_SIZE: f32 = 256.0;
/// Your eyes above your feet, in blocks.
const EYE_HEIGHT: f64 = 1.62;
/// Most waypoints marked in the 3D view.
const MAX_VIEW: usize = 16;
/// The field of view options offered, in degrees (vertical, as the game's).
const FOVS: [u8; 6] = [60, 70, 80, 90, 100, 110];
const HELP: &str = "/wp add <name> [x y z] | del <name> | list | go <name> | show|hide <name> | color <name> <color> | rename <old> = <new> | map | view terrain|chart | pan up|down|left|right [blocks] | menu | minimap on|off | world <name> | trail clear";

/// Pan speed of the arrow keys, in GUI pixels a tick.
const PAN_PX: f64 = 10.0;
/// Most explored squares kept per dimension.
const TRAIL_CAP: usize = 12_000;
/// The player's surroundings counted as explored, in 32-block cells.
const TRAIL_RADIUS: i32 = 3;
/// Most squares of the trail drawn on the world map.
const TRAIL_DRAW: usize = 400;
/// Most markers on the minimap.
const MAX_MARKS: usize = 120;
const MAX_WORLDS: usize = 12;

// The manager screen's widgets.
const LIST_ID: u32 = 10;
const B_ADD: u32 = 1;
const B_DELETE: u32 = 2;
const B_VISIBLE: u32 = 3;
const B_COLOR: u32 = 4;
const B_MAP: u32 = 5;
// The settings page's buttons.
const S_MINIMAP: u32 = 201;
const S_MARKERS: u32 = 202;
const S_CORNER: u32 = 203;
const S_SIZE: u32 = 204;
const S_ZOOM: u32 = 205;
const S_ROTATE: u32 = 206;
const S_SHAPE: u32 = 207;
const S_SCALE: u32 = 208;
const S_INGAME: u32 = 209;
const S_FOV: u32 = 210;

/// What the game's map was last set to: corner, size, blocks per pixel x 100,
/// rotate, round.
type MapSig = (usize, u32, u32, bool, bool);

fn read_text(key: &str) -> Option<String> {
    storage::get(key).and_then(|b| String::from_utf8(b).ok())
}

fn write_text(key: &str, text: &str) {
    if let Err(e) = storage::set(key, text.as_bytes()) {
        warn(&format!("could not save {key}: {e}"));
    }
}

fn list_key(profile: &str) -> String {
    format!("w.{profile}")
}

fn trail_key(profile: &str, dim: &str) -> String {
    format!("t.{profile}.{}", model::slug(&dim.replace("minecraft:", ""), 24))
}

fn corner_of(a: Anchor) -> Corner {
    match a {
        Anchor::TopLeft => Corner::TopLeft,
        Anchor::TopRight => Corner::TopRight,
        Anchor::BottomLeft => Corner::BottomLeft,
        Anchor::BottomRight => Corner::BottomRight,
    }
}

fn world_panel(gw: f32, gh: f32) -> Panel {
    Panel {
        x: 8.0,
        y: 8.0,
        w: (gw - 16.0).max(64.0),
        h: (gh - 16.0).max(64.0),
    }
}

/// What the minimap and its markers are set to.
#[derive(Clone)]
struct Settings {
    minimap: bool,
    markers: bool,
    corner: usize,
    size: usize,
    zoom: usize,
    rotate: bool,
    round: bool,
    /// Show Overworld waypoints in the Nether (and back), scaled 8:1.
    scale_dims: bool,
    /// Markers for waypoints in the 3D view.
    ingame: bool,
    /// The player's field of view, an index into `FOVS`.
    fov: usize,
    /// The waypoint set in use (`/wp world`).
    profile: String,
}

impl Settings {
    fn new() -> Self {
        Settings {
            minimap: true,
            markers: true,
            corner: 3,
            size: 1,
            zoom: 1,
            rotate: false,
            round: false,
            scale_dims: true,
            ingame: true,
            fov: 1,
            profile: "default".to_owned(),
        }
    }
    fn load() -> Self {
        let mut s = Self::new();
        let Some(text) = read_text("settings") else {
            return s;
        };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let (k, v) = (k.trim(), v.trim());
            let on = v == "1";
            let index = |max: usize| v.parse::<usize>().ok().filter(|i| *i < max);
            match k {
                "minimap" => s.minimap = on,
                "markers" => s.markers = on,
                "rotate" => s.rotate = on,
                "round" => s.round = on,
                "scale_dims" => s.scale_dims = on,
                "ingame" => s.ingame = on,
                "fov" => s.fov = index(FOVS.len()).unwrap_or(s.fov),
                "corner" => s.corner = index(CORNERS.len()).unwrap_or(s.corner),
                "size" => s.size = index(SIZES.len()).unwrap_or(s.size),
                "zoom" => s.zoom = index(ZOOMS.len()).unwrap_or(s.zoom),
                "profile" => s.profile = model::slug(v, 16),
                _ => {}
            }
        }
        s
    }
    fn save(&self) {
        let b = |v: bool| u8::from(v);
        write_text(
            "settings",
            &format!(
                "minimap={}\nmarkers={}\ncorner={}\nsize={}\nzoom={}\nrotate={}\nround={}\nscale_dims={}\ningame={}\nfov={}\nprofile={}\n",
                b(self.minimap),
                b(self.markers),
                self.corner,
                self.size,
                self.zoom,
                b(self.rotate),
                b(self.round),
                b(self.scale_dims),
                b(self.ingame),
                self.fov,
                self.profile
            ),
        );
    }
}

/// The squares of 32 x 32 blocks the player has been near, in one dimension.
#[derive(Default)]
struct Trail {
    cells: HashSet<(i32, i32)>,
    /// Changes whenever a cell is added or cleared.
    rev: u64,
    saved: u64,
    last: Option<(i32, i32)>,
}

fn load_trail(key: &str) -> Trail {
    let mut t = Trail::default();
    if let Some(bytes) = storage::get(key) {
        for c in bytes.chunks_exact(8) {
            let x = i32::from_le_bytes([c[0], c[1], c[2], c[3]]);
            let z = i32::from_le_bytes([c[4], c[5], c[6], c[7]]);
            if t.cells.len() < TRAIL_CAP {
                t.cells.insert((x, z));
            }
        }
    }
    t.rev = 1;
    t.saved = 1;
    t
}

fn save_trail(key: &str, t: &Trail) {
    let mut bytes = Vec::with_capacity(t.cells.len() * 8);
    for (x, z) in &t.cells {
        bytes.extend_from_slice(&x.to_le_bytes());
        bytes.extend_from_slice(&z.to_le_bytes());
    }
    if let Err(e) = storage::set(key, &bytes) {
        warn(&format!("could not save {key}: {e}"));
    }
}

/// The trail squares merged into bigger ones for the current zoom.
struct BinCache {
    dim: String,
    bin: i32,
    rev: u64,
    set: HashSet<(i32, i32)>,
}

/// The local player, as of the last tick.
#[derive(Clone)]
struct Me {
    x: f64,
    y: f64,
    z: f64,
    yaw: f32,
    pitch: f32,
    dim: String,
}

fn fresh_me() -> Option<Me> {
    game::get_player().map(|p| Me {
        x: p.x,
        y: p.y,
        z: p.z,
        yaw: p.yaw,
        pitch: p.pitch,
        dim: p.dimension,
    })
}

struct WorldState {
    open: bool,
    /// Terrain view (the game's map, around you); otherwise the chart.
    terrain: bool,
    /// The terrain view's zoom, an index into `ZOOMS`.
    tzoom: usize,
    view: WorldView,
    /// Up, down, left, right held.
    held: [bool; 4],
    bins: Option<BinCache>,
}

struct Waypoints {
    settings: Settings,
    list: Vec<Waypoint>,
    /// The waypoint tracked on the maps, an index into `list`.
    selected: Option<usize>,
    me: Option<Me>,
    world: WorldState,
    trails: HashMap<String, Trail>,
    manager: bool,
    /// The overlay must be built again.
    dirty: bool,
    last_key: Option<(i64, i64, i64, i64, String)>,
    /// The map last sent to the game (None: hidden).
    applied_map: Option<Option<MapSig>>,
}

impl Waypoints {
    fn now(&self) -> Option<Me> {
        fresh_me().or_else(|| self.me.clone())
    }

    fn project(&self, w: &Waypoint, me: &Me) -> Option<(f64, f64)> {
        geom::project(&w.dim, &me.dim, w.x, w.z, self.settings.scale_dims)
    }

    fn save_list(&self) {
        write_text(&list_key(&self.settings.profile), &model::serialize(&self.list));
    }

    fn map_view(&self) -> MapView {
        let s = &self.settings;
        MapView {
            corner: corner_of(CORNERS[s.corner].0),
            size: f32::from(SIZES[s.size].0),
            blocks_per_pixel: ZOOMS[s.zoom],
            rotate: s.rotate,
            round: s.round,
        }
    }

    // ---- ticking -------------------------------------------------------

    fn read_input(&mut self) {
        for event in input::poll() {
            match event {
                Event::KeyDown(k) => self.key_down(&k),
                Event::KeyUp(k) => self.key_up(&k),
                Event::Wheel(steps) => {
                    if self.world.open {
                        if self.world.terrain {
                            // Wheel away from you zooms in: fewer blocks a pixel.
                            let z = self.world.tzoom;
                            self.world.tzoom = if steps > 0.0 {
                                z.saturating_sub(1)
                            } else if steps < 0.0 {
                                (z + 1).min(ZOOMS.len() - 1)
                            } else {
                                z
                            };
                        } else {
                            self.world.view.zoom(steps);
                        }
                        self.dirty = true;
                    }
                }
                _ => {}
            }
        }
    }

    fn key_down(&mut self, key: &str) {
        match key {
            "wp_add" => match self.add(None, None) {
                Ok(m) | Err(m) => chat::show(&m),
            },
            "wp_map" => self.toggle_map(),
            "wp_menu" => self.open_manager(),
            "wp_view" => self.toggle_view(),
            "wp_up" => self.world.held[0] = true,
            "wp_down" => self.world.held[1] = true,
            "wp_left" => self.world.held[2] = true,
            "wp_right" => self.world.held[3] = true,
            "wp_center" => {
                self.center_on_me();
                self.dirty = true;
            }
            "wp_fit" => self.fit(),
            "wp_next" => self.cycle(1),
            "wp_prev" => self.cycle(-1),
            _ => {}
        }
    }

    fn key_up(&mut self, key: &str) {
        match key {
            "wp_up" => self.world.held[0] = false,
            "wp_down" => self.world.held[1] = false,
            "wp_left" => self.world.held[2] = false,
            "wp_right" => self.world.held[3] = false,
            _ => {}
        }
    }

    fn pan(&mut self) {
        if !self.world.held.iter().any(|k| *k) {
            return;
        }
        // The game's terrain map is always centred on you, so panning shows
        // the chart, which can move.
        if self.world.terrain {
            self.world.terrain = false;
            self.center_on_me();
        }
        let h = &self.world.held;
        let dx = f64::from(u8::from(h[3])) - f64::from(u8::from(h[2]));
        let dz = f64::from(u8::from(h[1])) - f64::from(u8::from(h[0]));
        if dx != 0.0 || dz != 0.0 {
            self.world.view.pan_px(dx * PAN_PX, dz * PAN_PX);
            self.dirty = true;
        }
    }

    fn record_trail(&mut self, me: &Me) {
        if !self.trails.contains_key(&me.dim) {
            let t = load_trail(&trail_key(&self.settings.profile, &me.dim));
            self.trails.insert(me.dim.clone(), t);
        }
        let Some(t) = self.trails.get_mut(&me.dim) else {
            return;
        };
        let cell = (geom::cell_of(me.x), geom::cell_of(me.z));
        if t.last == Some(cell) {
            return;
        }
        t.last = Some(cell);
        let before = t.cells.len();
        for dx in -TRAIL_RADIUS..=TRAIL_RADIUS {
            for dz in -TRAIL_RADIUS..=TRAIL_RADIUS {
                if t.cells.len() >= TRAIL_CAP {
                    break;
                }
                t.cells.insert((cell.0 + dx, cell.1 + dz));
            }
        }
        if t.cells.len() != before {
            t.rev += 1;
        }
    }

    /// Writes the trails that changed.
    fn flush(&mut self) {
        let profile = self.settings.profile.clone();
        for (dim, t) in self.trails.iter_mut() {
            if t.rev != t.saved {
                save_trail(&trail_key(&profile, dim), t);
                t.saved = t.rev;
            }
        }
    }

    /// Builds the overlay again and tells the game what changed.
    fn rebuild(&mut self) {
        let Some(me) = self.me.clone() else {
            return;
        };
        let (gw, gh) = render::gui_size();
        let s = self.settings.clone();
        let want = self.current_map(gw, gh);
        let sig = want.as_ref().map(|(_, k)| *k);
        if self.applied_map != Some(sig) {
            render::set_minimap(want.map(|(m, _)| m));
            self.applied_map = Some(sig);
        }
        let mut shapes: Vec<Shape> = if self.world.open {
            if self.world.terrain {
                self.terrain_shapes(&me, gw, gh)
            } else {
                self.world_shapes(&me, gw, gh)
            }
        } else if s.minimap && s.markers {
            self.mini_shapes(&me, gw, gh)
        } else {
            Vec::new()
        };
        if s.ingame && !self.world.open {
            shapes.extend(self.view_shapes(&me, gw, gh));
        }
        shapes.truncate(1024);
        render::set_hud(&shapes);
        self.dirty = false;
    }

    /// The game's map as it should be now: the minimap, or big while the
    /// world map shows terrain; none when neither shows.
    fn current_map(&self, gw: f32, gh: f32) -> Option<(MapView, MapSig)> {
        let s = &self.settings;
        if self.world.open {
            if !self.world.terrain {
                return None;
            }
            let size = TERRAIN_SIZE.min(gw - 8.0).min(gh - 8.0).floor().max(64.0);
            let bpp = ZOOMS[self.world.tzoom];
            let view = MapView {
                corner: corner_of(CORNERS[s.corner].0),
                size,
                blocks_per_pixel: bpp,
                rotate: false,
                round: false,
            };
            return Some((view, (s.corner, size as u32, (bpp * 100.0) as u32, false, false)));
        }
        s.minimap.then(|| {
            (
                self.map_view(),
                (
                    s.corner,
                    u32::from(SIZES[s.size].0),
                    (ZOOMS[s.zoom] * 100.0) as u32,
                    s.rotate,
                    s.round,
                ),
            )
        })
    }

    /// The terrain view's overlay: waypoints named on the game's big map.
    fn terrain_shapes(&self, me: &Me, gw: f32, gh: f32) -> Vec<Shape> {
        let Some((view, _)) = self.current_map(gw, gh) else {
            return Vec::new();
        };
        let mut marks: Vec<TMark> = Vec::new();
        for (i, w) in self.list.iter().enumerate() {
            if !w.visible {
                continue;
            }
            let Some((x, z)) = self.project(w, me) else {
                continue;
            };
            let (dx, dz) = (x - me.x, z - me.z);
            let label = format!("{} {}", w.name, geom::fmt_dist(dx.hypot(dz)));
            let width = render::text_width(&label);
            marks.push(TMark {
                dx,
                dz,
                rgb: w.rgb(),
                label,
                width,
                selected: self.selected == Some(i),
                other_dim: w.dim != me.dim,
            });
        }
        marks.truncate(MAX_MARKS);
        let footer = self
            .selected
            .and_then(|i| self.list.get(i))
            .filter(|w| w.visible)
            .and_then(|w| {
                let (x, z) = self.project(w, me)?;
                let (dx, dz) = (x - me.x, z - me.z);
                Some(format!(
                    "{} {} {}",
                    w.name,
                    geom::fmt_dist(dx.hypot(dz)),
                    geom::compass(dx, dz)
                ))
            })
            .unwrap_or_default();
        let title = format!(
            "{} - {}",
            model::dim_label(&me.dim),
            model::plural(marks.len(), "waypoint")
        );
        draw::terrain_map(&Terrain {
            gui: (gw, gh),
            anchor: CORNERS[self.settings.corner].0,
            size: view.size,
            blocks_per_pixel: view.blocks_per_pixel,
            title,
            footer,
            side: &TERRAIN_HINT,
            marks: &marks,
        })
    }

    /// Waypoint markers in the 3D view: where each one falls on the screen
    /// from the player's eyes (the tracked one pins to the edge when off it).
    fn view_shapes(&self, me: &Me, gw: f32, gh: f32) -> Vec<Shape> {
        let fov = f32::from(FOVS[self.settings.fov]);
        let eye = (me.x, me.y + EYE_HEIGHT, me.z);
        let spot = |w: &Waypoint| {
            (
                f64::from(w.x) + 0.5,
                f64::from(w.y) + 1.0,
                f64::from(w.z) + 0.5,
            )
        };
        let mut near: Vec<(f64, usize)> = Vec::new();
        for (i, w) in self.list.iter().enumerate() {
            // Only this dimension's: the 8:1 ones have no place in the view.
            if !w.visible || w.dim != me.dim {
                continue;
            }
            let t = spot(w);
            let d = ((t.0 - eye.0).powi(2) + (t.1 - eye.1).powi(2) + (t.2 - eye.2).powi(2)).sqrt();
            if d >= 1.5 {
                near.push((d, i));
            }
        }
        near.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut pick: Vec<(f64, usize)> = near.iter().copied().take(MAX_VIEW).collect();
        if let Some(sel) = self.selected
            && !pick.iter().any(|p| p.1 == sel)
            && let Some(&c) = near.iter().find(|p| p.1 == sel)
        {
            pick.push(c);
        }
        let (cx, cy) = (gw / 2.0, gh / 2.0);
        let mut marks: Vec<VMark> = Vec::new();
        for (d, i) in pick {
            let w = &self.list[i];
            let selected = self.selected == Some(i);
            let p = geom::project_view(eye, me.yaw, me.pitch, fov, gw, gh, spot(w));
            if !p.on && !selected {
                continue;
            }
            let centred = (p.x - cx).abs() < gw * 0.15 && (p.y - cy).abs() < gh * 0.15;
            let show_label = selected || (p.on && centred);
            let label = format!("{} {}", w.name, geom::fmt_dist(d));
            let width = if show_label { render::text_width(&label) } else { 0.0 };
            marks.push(VMark {
                x: p.x,
                y: p.y,
                on: p.on,
                rgb: w.rgb(),
                selected,
                label,
                width,
                show_label,
            });
        }
        draw::view_markers((gw, gh), &marks)
    }

    fn mini_shapes(&self, me: &Me, gw: f32, gh: f32) -> Vec<Shape> {
        let s = &self.settings;
        let mut marks: Vec<Mark> = Vec::new();
        for (i, w) in self.list.iter().enumerate() {
            if !w.visible {
                continue;
            }
            let Some((x, z)) = self.project(w, me) else {
                continue;
            };
            marks.push(Mark {
                dx: x - me.x,
                dz: z - me.z,
                rgb: w.rgb(),
                selected: self.selected == Some(i),
                other_dim: w.dim != me.dim,
            });
        }
        // The nearest are kept when there are more than fit.
        marks.sort_by(|a, b| {
            let (da, db) = (a.dx.hypot(a.dz), b.dx.hypot(b.dz));
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        });
        marks.truncate(MAX_MARKS);
        let caption = self
            .selected
            .and_then(|i| self.list.get(i))
            .filter(|w| w.visible)
            .and_then(|w| {
                let (x, z) = self.project(w, me)?;
                let (dx, dz) = (x - me.x, z - me.z);
                let line = format!(
                    "{} {} {}",
                    w.name,
                    geom::fmt_dist(dx.hypot(dz)),
                    geom::compass(dx, dz)
                );
                let width = render::text_width(&line);
                Some((line, width))
            });
        draw::minimap(&Mini {
            gui: (gw, gh),
            anchor: CORNERS[s.corner].0,
            size: f32::from(SIZES[s.size].0),
            blocks_per_pixel: ZOOMS[s.zoom],
            rotate: s.rotate,
            round: s.round,
            yaw: me.yaw,
            marks: &marks,
            caption,
        })
    }

    fn world_shapes(&mut self, me: &Me, gw: f32, gh: f32) -> Vec<Shape> {
        let panel = world_panel(gw, gh);
        let mut marks: Vec<WMark> = Vec::new();
        for (i, w) in self.list.iter().enumerate() {
            if !w.visible {
                continue;
            }
            let Some((x, z)) = self.project(w, me) else {
                continue;
            };
            let d = (x - me.x).hypot(z - me.z);
            marks.push(WMark {
                x,
                z,
                rgb: w.rgb(),
                label: format!("{} {}", w.name, geom::fmt_dist(d)),
                selected: self.selected == Some(i),
                other_dim: w.dim != me.dim,
            });
        }
        let trail = self.trail_rects(&me.dim, &panel);
        let title = format!(
            "World map  -  {}  -  set '{}'  -  {}",
            model::dim_label(&me.dim),
            self.settings.profile,
            model::plural(marks.len(), "waypoint")
        );
        draw::world_map(&World {
            panel,
            view: self.world.view,
            me: Some((me.x, me.z, me.yaw)),
            title,
            footer: self.footer(me),
            hint: HINT_CHART,
            marks: &marks,
            trail: &trail,
        })
    }

    fn trail_rects(&mut self, dim: &str, panel: &Panel) -> Vec<Panel> {
        let Some(trail) = self.trails.get(dim) else {
            return Vec::new();
        };
        let bin = geom::bin_for(self.world.view.ppb);
        let stale = match &self.world.bins {
            Some(b) => b.dim != dim || b.bin != bin || b.rev != trail.rev,
            None => true,
        };
        if stale {
            self.world.bins = Some(BinCache {
                dim: dim.to_owned(),
                bin,
                rev: trail.rev,
                set: geom::coarsen(&trail.cells, bin),
            });
        }
        match &self.world.bins {
            Some(b) => geom::trail_rects(&b.set, b.bin, &self.world.view, panel, TRAIL_DRAW),
            None => Vec::new(),
        }
    }

    /// The selected waypoint's line under the world map.
    fn footer(&self, me: &Me) -> String {
        let Some(w) = self.selected.and_then(|i| self.list.get(i)) else {
            return String::new();
        };
        let tail = match self.project(w, me) {
            Some((x, z)) => {
                let (dx, dz) = (x - me.x, z - me.z);
                format!("{} {}", geom::fmt_dist(dx.hypot(dz)), geom::compass(dx, dz))
            }
            None => "another dimension".to_owned(),
        };
        format!(
            "{}: {}, {}, {} ({})  {}",
            w.name,
            w.x,
            w.y,
            w.z,
            model::dim_label(&w.dim),
            tail
        )
    }

    // ---- the world map -------------------------------------------------

    fn toggle_map(&mut self) {
        self.world.open = !self.world.open;
        input::claim_wheel(self.world.open);
        self.world.held = [false; 4];
        if self.world.open {
            self.center_on_me();
        } else {
            self.flush();
        }
        self.dirty = true;
    }

    /// Terrain view <-> chart, while the world map is open.
    fn toggle_view(&mut self) {
        if !self.world.open {
            return;
        }
        self.world.terrain = !self.world.terrain;
        if !self.world.terrain {
            self.center_on_me();
        }
        self.dirty = true;
    }

    fn center_on_me(&mut self) {
        if let Some(me) = self.now() {
            self.world.view.cx = me.x;
            self.world.view.cz = me.z;
        }
    }

    /// Zooms and centres the world map on every waypoint here and the player.
    fn fit(&mut self) {
        let Some(me) = self.now() else {
            return;
        };
        let mut points = vec![(me.x, me.z)];
        for w in self.list.iter().filter(|w| w.visible) {
            if let Some(p) = self.project(w, &me) {
                points.push(p);
            }
        }
        let (gw, gh) = render::gui_size();
        self.world.view = geom::fit(&points, &world_panel(gw, gh));
        self.dirty = true;
    }

    fn select(&mut self, i: usize) {
        self.selected = Some(i);
        if self.world.open
            && let (Some(me), Some(w)) = (self.now(), self.list.get(i))
            && let Some((x, z)) = geom::project(&w.dim, &me.dim, w.x, w.z, self.settings.scale_dims)
        {
            self.world.view.cx = x;
            self.world.view.cz = z;
        }
        self.dirty = true;
    }

    /// The next or previous waypoint that shows in this dimension.
    fn cycle(&mut self, dir: i32) {
        let Some(me) = self.now() else {
            return;
        };
        let ids: Vec<usize> = self
            .list
            .iter()
            .enumerate()
            .filter(|(_, w)| w.visible && self.project(w, &me).is_some())
            .map(|(i, _)| i)
            .collect();
        if ids.is_empty() {
            chat::show("No waypoints to select here");
            return;
        }
        let at = self.selected.and_then(|s| ids.iter().position(|&i| i == s));
        let next = match at {
            Some(a) => (a as i32 + dir).rem_euclid(ids.len() as i32) as usize,
            None if dir > 0 => 0,
            None => ids.len() - 1,
        };
        self.select(ids[next]);
    }

    // ---- waypoints -----------------------------------------------------

    /// Adds a waypoint at `at`, or where the player stands.
    fn add(&mut self, name: Option<&str>, at: Option<(i32, i32, i32)>) -> Result<String, String> {
        let me = self.now().ok_or("Not in a world")?;
        if self.list.len() >= MAX_WAYPOINTS {
            return Err(format!("A set holds at most {MAX_WAYPOINTS} waypoints"));
        }
        let name = match name.map(model::clean_name).filter(|n| !n.is_empty()) {
            Some(n) => n,
            None => model::unique_name(&self.list),
        };
        if model::exists(&self.list, &name) {
            return Err(format!("A waypoint called '{name}' already exists"));
        }
        let (x, y, z) = at.unwrap_or((
            me.x.floor() as i32,
            me.y.floor() as i32,
            me.z.floor() as i32,
        ));
        let color = (self.list.len() % PALETTE.len()) as u8;
        self.list.push(Waypoint {
            name: name.clone(),
            x,
            y,
            z,
            dim: me.dim,
            color,
            visible: true,
        });
        self.selected = Some(self.list.len() - 1);
        self.save_list();
        self.dirty = true;
        Ok(format!("Waypoint '{name}' added at {x}, {y}, {z}"))
    }

    fn remove(&mut self, i: usize) -> Waypoint {
        let w = self.list.remove(i);
        self.selected = match self.selected {
            Some(s) if s == i => None,
            Some(s) if s > i => Some(s - 1),
            other => other,
        };
        self.save_list();
        self.dirty = true;
        w
    }

    fn profiles(&self) -> Vec<String> {
        let mut all: Vec<String> = read_text("worlds")
            .map(|t| t.lines().map(|l| model::slug(l, 16)).collect())
            .unwrap_or_default();
        if !all.contains(&self.settings.profile) {
            all.push(self.settings.profile.clone());
        }
        all
    }

    fn switch_profile(&mut self, name: &str) -> Result<String, String> {
        let slug = model::slug(name, 16);
        if slug == self.settings.profile {
            return Ok(format!("Already using set '{slug}'"));
        }
        let mut all = self.profiles();
        if !all.contains(&slug) {
            if all.len() >= MAX_WORLDS {
                return Err(format!("At most {MAX_WORLDS} waypoint sets"));
            }
            all.push(slug.clone());
        }
        self.flush();
        self.trails.clear();
        self.world.bins = None;
        self.settings.profile = slug.clone();
        self.settings.save();
        write_text("worlds", &all.join("\n"));
        self.list = read_text(&list_key(&slug))
            .map(|t| model::parse(&t))
            .unwrap_or_default();
        self.selected = None;
        self.dirty = true;
        Ok(format!(
            "Using waypoint set '{slug}' ({})",
            model::plural(self.list.len(), "waypoint")
        ))
    }

    // ---- commands ------------------------------------------------------

    fn run(&mut self, args: &str) -> Result<Option<String>, String> {
        let args = args.trim();
        let (sub, rest) = match args.split_once(char::is_whitespace) {
            Some((a, b)) => (a, b.trim()),
            None => (args, ""),
        };
        match sub.to_lowercase().as_str() {
            "" | "help" => Ok(Some(HELP.to_owned())),
            "add" => {
                let words: Vec<&str> = rest.split_whitespace().collect();
                let n = words.len();
                let coords = if n >= 3 {
                    match (
                        words[n - 3].parse::<i32>(),
                        words[n - 2].parse::<i32>(),
                        words[n - 1].parse::<i32>(),
                    ) {
                        (Ok(x), Ok(y), Ok(z)) => Some((x, y, z)),
                        _ => None,
                    }
                } else {
                    None
                };
                let name_words = if coords.is_some() { &words[..n - 3] } else { &words[..] };
                let name = name_words.join(" ");
                self.add(Some(name.as_str()), coords).map(Some)
            }
            "del" | "delete" | "remove" => {
                let i = model::find(&self.list, rest)?;
                let w = self.remove(i);
                Ok(Some(format!("Removed '{}'", w.name)))
            }
            "list" => {
                if self.list.is_empty() {
                    return Ok(Some(format!(
                        "No waypoints in set '{}' yet. /wp add <name>, or press B",
                        self.settings.profile
                    )));
                }
                let items: Vec<String> = self
                    .list
                    .iter()
                    .map(|w| format!("{} ({}, {}, {})", w.name, w.x, w.y, w.z))
                    .collect();
                Ok(Some(format!(
                    "{} in set '{}': {}",
                    model::plural(self.list.len(), "waypoint"),
                    self.settings.profile,
                    items.join("; ")
                )))
            }
            "go" | "track" | "select" => {
                let i = model::find(&self.list, rest)?;
                self.select(i);
                let w = &self.list[i];
                let tail = self
                    .now()
                    .and_then(|me| {
                        let (x, z) = self.project(w, &me)?;
                        let (dx, dz) = (x - me.x, z - me.z);
                        Some(format!(
                            ": {} {}",
                            geom::fmt_dist(dx.hypot(dz)),
                            geom::compass(dx, dz)
                        ))
                    })
                    .unwrap_or_default();
                Ok(Some(format!("Tracking '{}'{tail}", w.name)))
            }
            "show" | "hide" => {
                let i = model::find(&self.list, rest)?;
                self.list[i].visible = sub.eq_ignore_ascii_case("show");
                self.save_list();
                self.dirty = true;
                let state = if self.list[i].visible { "shown" } else { "hidden" };
                Ok(Some(format!("'{}' is {state}", self.list[i].name)))
            }
            "color" | "colour" => {
                let (name, color) = rest
                    .rsplit_once(' ')
                    .ok_or("Usage: /wp color <name> <color>")?;
                let c = model::color_index(color)
                    .ok_or_else(|| format!("Colors: {}", model::color_names()))?;
                let i = model::find(&self.list, name)?;
                self.list[i].color = c;
                self.save_list();
                self.dirty = true;
                Ok(Some(format!("'{}' is now {}", self.list[i].name, self.list[i].color_name())))
            }
            "rename" => {
                let (old, new) = rest
                    .split_once('=')
                    .ok_or("Usage: /wp rename <old> = <new>")?;
                let i = model::find(&self.list, old)?;
                let new = model::clean_name(new);
                if new.is_empty() {
                    return Err("Give the new name after the =".to_owned());
                }
                let taken = self
                    .list
                    .iter()
                    .enumerate()
                    .any(|(j, w)| j != i && w.name.to_lowercase() == new.to_lowercase());
                if taken {
                    return Err(format!("A waypoint called '{new}' already exists"));
                }
                self.list[i].name = new.clone();
                self.save_list();
                self.dirty = true;
                Ok(Some(format!("Renamed to '{new}'")))
            }
            "map" => {
                self.toggle_map();
                Ok(None)
            }
            "view" => {
                match rest.to_lowercase().as_str() {
                    "terrain" => self.world.terrain = true,
                    "chart" => {
                        self.world.terrain = false;
                        self.center_on_me();
                    }
                    _ => return Err("Usage: /wp view terrain|chart".to_owned()),
                }
                self.dirty = true;
                Ok(None)
            }
            "pan" => {
                let mut words = rest.split_whitespace();
                let dir = words.next().unwrap_or("").to_lowercase();
                let n: f64 = words.next().and_then(|v| v.parse().ok()).unwrap_or(64.0);
                let (dx, dz) = match dir.as_str() {
                    "up" | "north" => (0.0, -n),
                    "down" | "south" => (0.0, n),
                    "left" | "west" => (-n, 0.0),
                    "right" | "east" => (n, 0.0),
                    _ => return Err("Usage: /wp pan up|down|left|right [blocks]".to_owned()),
                };
                if !self.world.open {
                    return Err("Open the world map first (M)".to_owned());
                }
                if self.world.terrain {
                    self.world.terrain = false;
                    self.center_on_me();
                }
                self.world.view.cx += dx;
                self.world.view.cz += dz;
                self.dirty = true;
                Ok(None)
            }
            "menu" => {
                self.open_manager();
                Ok(None)
            }
            "minimap" => {
                self.settings.minimap = match rest.to_lowercase().as_str() {
                    "on" => true,
                    "off" => false,
                    "" | "toggle" => !self.settings.minimap,
                    _ => return Err("Usage: /wp minimap on|off".to_owned()),
                };
                self.settings.save();
                self.dirty = true;
                self.page();
                let state = if self.settings.minimap { "on" } else { "off" };
                Ok(Some(format!("Minimap {state}")))
            }
            "world" | "set" => {
                if rest.is_empty() {
                    return Ok(Some(format!(
                        "Using waypoint set '{}'. Sets: {}. /wp world <name> switches (or makes) one",
                        self.settings.profile,
                        self.profiles().join(", ")
                    )));
                }
                self.switch_profile(rest).map(Some)
            }
            "trail" => {
                if !rest.eq_ignore_ascii_case("clear") {
                    return Err("Usage: /wp trail clear".to_owned());
                }
                let Some(me) = self.now() else {
                    return Err("Not in a world".to_owned());
                };
                let key = trail_key(&self.settings.profile, &me.dim);
                storage::delete(&key);
                self.trails.insert(
                    me.dim.clone(),
                    Trail {
                        rev: 1,
                        saved: 1,
                        ..Trail::default()
                    },
                );
                self.world.bins = None;
                self.dirty = true;
                Ok(Some(format!(
                    "Explored area of the {} cleared",
                    model::dim_label(&me.dim)
                )))
            }
            other => Err(format!("Unknown subcommand '{other}'. /wp help lists them")),
        }
    }

    // ---- screens -------------------------------------------------------

    fn manager_screen(&self) -> Screen {
        let me = self.now();
        let mut rows = Vec::new();
        for w in &self.list {
            let away = match me.as_ref().and_then(|m| self.project(w, m).map(|p| (p, m))) {
                Some(((x, z), m)) => geom::fmt_dist((x - m.x).hypot(z - m.z)),
                None => "other dimension".to_owned(),
            };
            rows.push(format!(
                "{} {}   {}, {}, {}   {}   {}",
                if w.visible { "[x]" } else { "[ ]" },
                w.name,
                w.x,
                w.y,
                w.z,
                away,
                model::dim_label(&w.dim)
            ));
        }
        let text = |id: u32, text: String| Widget::Text(TextWidget { id, text, color: 0 });
        let button = |id: u32, label: String| Widget::Button(ButtonWidget { id, label });
        let mut widgets = vec![text(
            1,
            format!(
                "Set '{}': {}",
                self.settings.profile,
                model::plural(self.list.len(), "waypoint")
            ),
        )];
        if rows.is_empty() {
            widgets.push(text(2, "None yet: Add waypoint here saves where you stand.".to_owned()));
        } else {
            widgets.push(Widget::List(ListWidget {
                id: LIST_ID,
                rows,
                selected: self.selected.map(|i| i as u32),
            }));
        }
        let chosen = self.selected.and_then(|i| self.list.get(i));
        widgets.push(button(B_ADD, "Add waypoint here".to_owned()));
        widgets.push(button(B_DELETE, "Delete selected".to_owned()));
        widgets.push(button(B_VISIBLE, "Show / hide selected".to_owned()));
        widgets.push(button(
            B_COLOR,
            format!("Colour of selected: {}", chosen.map_or("-", |w| w.color_name())),
        ));
        widgets.push(button(B_MAP, "Open the world map".to_owned()));
        Screen {
            title: "Waypoints".to_owned(),
            widgets,
        }
    }

    fn open_manager(&mut self) {
        ui::open_screen(&self.manager_screen());
        self.manager = true;
    }

    fn refresh_manager(&self) {
        if self.manager {
            ui::open_screen(&self.manager_screen());
        }
    }

    /// The settings page in Options > Mods.
    fn page(&self) {
        let s = &self.settings;
        let on = |b: bool| if b { "ON" } else { "OFF" };
        let button = |id: u32, label: String| Widget::Button(ButtonWidget { id, label });
        let blocks = ZOOMS[s.zoom];
        ui::set_settings(&[
            button(S_MINIMAP, format!("Minimap: {}", on(s.minimap))),
            button(S_MARKERS, format!("Waypoints on the minimap: {}", on(s.markers))),
            button(S_CORNER, format!("Corner: {}", CORNERS[s.corner].2)),
            button(S_SIZE, format!("Size: {}", SIZES[s.size].1)),
            button(S_ZOOM, format!("Zoom: {blocks} blocks per pixel")),
            button(S_ROTATE, format!("Turn with the player: {}", on(s.rotate))),
            button(
                S_SHAPE,
                format!("Shape: {}", if s.round { "Round" } else { "Square" }),
            ),
            button(
                S_SCALE,
                format!("Show Overworld/Nether waypoints in both (8:1): {}", on(s.scale_dims)),
            ),
            button(S_INGAME, format!("Waypoints in the 3D view: {}", on(s.ingame))),
            button(
                S_FOV,
                format!("My field of view (for the 3D markers): {}", FOVS[s.fov]),
            ),
        ]);
    }
}

impl Mod for Waypoints {
    fn init() -> Result<(Self, Registration), String> {
        for (name, label, key) in KEYS {
            if let Err(e) = input::register_key(name, label, key) {
                warn(&format!("key {name}: {e}"));
            }
        }
        let settings = Settings::load();
        let list = read_text(&list_key(&settings.profile))
            .map(|t| model::parse(&t))
            .unwrap_or_default();
        let state = Waypoints {
            settings,
            list,
            selected: None,
            me: None,
            world: WorldState {
                open: false,
                terrain: true,
                tzoom: 2,
                view: WorldView {
                    cx: 0.0,
                    cz: 0.0,
                    ppb: 1.0,
                },
                held: [false; 4],
                bins: None,
            },
            trails: HashMap::new(),
            manager: false,
            dirty: true,
            last_key: None,
            applied_map: None,
        };
        state.page();
        info("Waypoints ready: B adds a waypoint, M opens the world map, N the manager, /wp help");
        Ok((state, Registration::new().command("wp").on_tick()))
    }

    fn tick(&mut self, tick: u64) {
        self.read_input();
        let Some(me) = fresh_me() else {
            return;
        };
        self.record_trail(&me);
        if self.world.open {
            self.pan();
        }
        // The minimap's markers only move with the player (and, when the map
        // turns, with the view); the world map follows the view too.
        // The 3D view's markers follow where you look, so they need the
        // view and a finer position.
        let looking = self.settings.ingame && !self.world.open && !self.list.is_empty();
        let yaw = if self.settings.rotate || self.world.open || looking {
            (me.yaw * 4.0).round() as i64
        } else {
            0
        };
        let pitch = if looking { (me.pitch * 4.0).round() as i64 } else { 0 };
        let grain = if looking { 4.0 } else { 2.0 };
        let key = (
            (me.x * grain).floor() as i64,
            (me.z * grain).floor() as i64,
            yaw,
            pitch,
            me.dim.clone(),
        );
        let moved = self.last_key.as_ref() != Some(&key);
        self.me = Some(me);
        if moved || self.dirty {
            self.last_key = Some(key);
            self.rebuild();
        }
        if tick % 600 == 0 {
            self.flush();
        }
    }

    fn command(&mut self, _name: &str, args: &str) -> Result<Option<String>, String> {
        self.run(args)
    }

    fn ui_select(&mut self, id: u32, row: u32) {
        match id {
            LIST_ID => {
                let i = row as usize;
                if i < self.list.len() {
                    self.select(i);
                }
                self.refresh_manager();
            }
            B_ADD => {
                match self.add(None, None) {
                    Ok(m) | Err(m) => chat::show(&m),
                }
                self.refresh_manager();
            }
            B_DELETE => {
                if let Some(i) = self.selected.filter(|i| *i < self.list.len()) {
                    self.remove(i);
                }
                self.refresh_manager();
            }
            B_VISIBLE => {
                if let Some(w) = self.selected.and_then(|i| self.list.get_mut(i)) {
                    w.visible = !w.visible;
                    self.save_list();
                    self.dirty = true;
                }
                self.refresh_manager();
            }
            B_COLOR => {
                if let Some(w) = self.selected.and_then(|i| self.list.get_mut(i)) {
                    w.color = (w.color + 1) % PALETTE.len() as u8;
                    self.save_list();
                    self.dirty = true;
                }
                self.refresh_manager();
            }
            B_MAP => {
                ui::close_screen();
                self.manager = false;
                if !self.world.open {
                    self.toggle_map();
                }
            }
            S_MINIMAP | S_MARKERS | S_CORNER | S_SIZE | S_ZOOM | S_ROTATE | S_SHAPE | S_SCALE
            | S_INGAME | S_FOV => {
                let s = &mut self.settings;
                match id {
                    S_MINIMAP => s.minimap = !s.minimap,
                    S_MARKERS => s.markers = !s.markers,
                    S_CORNER => s.corner = (s.corner + 1) % CORNERS.len(),
                    S_SIZE => s.size = (s.size + 1) % SIZES.len(),
                    S_ZOOM => s.zoom = (s.zoom + 1) % ZOOMS.len(),
                    S_ROTATE => s.rotate = !s.rotate,
                    S_SHAPE => s.round = !s.round,
                    S_INGAME => s.ingame = !s.ingame,
                    S_FOV => s.fov = (s.fov + 1) % FOVS.len(),
                    _ => s.scale_dims = !s.scale_dims,
                }
                s.save();
                self.dirty = true;
                self.page();
            }
            _ => {}
        }
    }

    fn ui_closed(&mut self) {
        self.manager = false;
    }

    fn ui_settings_opened(&mut self) {
        self.page();
    }
}

horizon_mod_sdk::export_mod!(Waypoints);
