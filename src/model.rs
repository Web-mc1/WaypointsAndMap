//! Waypoints: what they are, how they are stored and how a name finds one.

pub const MAX_WAYPOINTS: usize = 256;
pub const MAX_NAME: usize = 32;

/// (name, 0xRRGGBB): the colours a waypoint can have.
pub const PALETTE: [(&str, u32); 10] = [
    ("red", 0xFF5555),
    ("orange", 0xFFAA00),
    ("yellow", 0xFFFF55),
    ("green", 0x55FF55),
    ("aqua", 0x55FFFF),
    ("blue", 0x5577FF),
    ("purple", 0xAA55FF),
    ("pink", 0xFF77DD),
    ("white", 0xFFFFFF),
    ("gray", 0xAAAAAA),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waypoint {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// `minecraft:overworld`, `minecraft:the_nether`, ...
    pub dim: String,
    /// Index into [`PALETTE`].
    pub color: u8,
    pub visible: bool,
}

impl Waypoint {
    pub fn rgb(&self) -> u32 {
        PALETTE[usize::from(self.color) % PALETTE.len()].1
    }
    pub fn color_name(&self) -> &'static str {
        PALETTE[usize::from(self.color) % PALETTE.len()].0
    }
}

/// A name as stored: one line, single spaces, at most [`MAX_NAME`] characters.
pub fn clean_name(raw: &str) -> String {
    let flat: String = raw
    .chars()
    .map(|c| if c.is_control() { ' ' } else { c })
    .collect();
    let joined = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = joined.chars().take(MAX_NAME).collect();
    cut.trim().to_owned()
}

/// A lowercase `[a-z0-9_-]` word for storage keys, never empty.
pub fn slug(raw: &str, max: usize) -> String {
    let s: String = raw
    .trim()
    .to_lowercase()
    .chars()
    .map(|c| {
        if c.is_ascii_alphanumeric() || c == '-' {
            c
        } else {
            '_'
        }
    })
    .take(max)
    .collect();
    let s = s.trim_matches('_').to_owned();
    if s.is_empty() { "default".to_owned() } else { s }
}

/// One waypoint per line: `name TAB x TAB y TAB z TAB dim TAB colour TAB visible`.
pub fn serialize(list: &[Waypoint]) -> String {
    let mut out = String::new();
    for w in list {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            w.name,
            w.x,
            w.y,
            w.z,
            w.dim,
            w.color,
            u8::from(w.visible)
        ));
    }
    out
}

/// The inverse of [`serialize`]; a line that does not parse is skipped.
pub fn parse(text: &str) -> Vec<Waypoint> {
    let mut list = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() != 7 {
            continue;
        }
        let (Ok(x), Ok(y), Ok(z), Ok(color)) = (
            f[1].parse::<i32>(),
                                                f[2].parse::<i32>(),
                                                f[3].parse::<i32>(),
                                                f[5].parse::<u8>(),
        ) else {
            continue;
        };
        let name = clean_name(f[0]);
        if name.is_empty() || exists(&list, &name) {
            continue;
        }
        list.push(Waypoint {
            name,
            x,
            y,
            z,
            dim: f[4].to_owned(),
                  color: color % PALETTE.len() as u8,
                  visible: f[6] != "0",
        });
        if list.len() >= MAX_WAYPOINTS {
            break;
        }
    }
    list
}

/// Whether a waypoint already has this name (case does not matter).
pub fn exists(list: &[Waypoint], name: &str) -> bool {
    let n = name.to_lowercase();
    list.iter().any(|w| w.name.to_lowercase() == n)
}

/// The waypoint a typed name means: an exact name, else the one name it starts.
pub fn find(list: &[Waypoint], query: &str) -> Result<usize, String> {
    let q = clean_name(query).to_lowercase();
    if q.is_empty() {
        return Err("Say which waypoint".to_owned());
    }
    if let Some(i) = list.iter().position(|w| w.name.to_lowercase() == q) {
        return Ok(i);
    }
    let hits: Vec<usize> = list
    .iter()
    .enumerate()
    .filter(|(_, w)| w.name.to_lowercase().starts_with(&q))
    .map(|(i, _)| i)
    .collect();
    match hits.as_slice() {
        [i] => Ok(*i),
        [] => Err(format!("No waypoint called '{}'", clean_name(query))),
        many => Err(format!(
            "'{}' matches {} waypoints; type more of the name",
            clean_name(query),
                            many.len()
        )),
    }
}

/// `Waypoint 3`: the first number not taken.
pub fn unique_name(list: &[Waypoint]) -> String {
    let mut n = list.len() + 1;
    loop {
        let name = format!("Waypoint {n}");
        if !exists(list, &name) {
            return name;
        }
        n += 1;
    }
}

pub fn color_index(word: &str) -> Option<u8> {
    let w = word.trim().to_lowercase();
    PALETTE
    .iter()
    .position(|(name, _)| *name == w)
    .map(|i| i as u8)
}

pub fn color_names() -> String {
    PALETTE
    .iter()
    .map(|(name, _)| *name)
    .collect::<Vec<_>>()
    .join(", ")
}

pub fn dim_label(dim: &str) -> &str {
    match dim {
        "minecraft:overworld" => "Overworld",
        "minecraft:the_nether" => "Nether",
        "minecraft:the_end" => "End",
        other => other,
    }
}

pub fn plural(n: usize, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wp(name: &str) -> Waypoint {
        Waypoint {
            name: name.to_owned(),
            x: 1,
            y: 2,
            z: 3,
            dim: "minecraft:overworld".to_owned(),
            color: 0,
            visible: true,
        }
    }

    #[test]
    fn storage_round_trips() {
        let mut a = wp("Home Base");
        a.visible = false;
        a.color = 4;
        let mut b = wp("Mine");
        b.x = -120;
        b.dim = "minecraft:the_nether".to_owned();
        let list = vec![a, b];
        assert_eq!(parse(&serialize(&list)), list);
    }

    #[test]
    fn bad_lines_are_skipped() {
        let text = "ok\t1\t2\t3\tminecraft:overworld\t0\t1\nnot a waypoint\nbad\tx\t2\t3\td\t0\t1\n";
        let list = parse(text);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "ok");
    }

    #[test]
    fn names_are_one_clean_line() {
        assert_eq!(clean_name("  my \t base\n"), "my base");
        assert_eq!(clean_name(&"x".repeat(100)).chars().count(), MAX_NAME);
    }

    #[test]
    fn a_name_finds_its_waypoint() {
        let list = vec![wp("Home"), wp("Honey farm"), wp("Mine")];
        assert_eq!(find(&list, "home"), Ok(0));
        assert_eq!(find(&list, "mi"), Ok(2));
        assert!(find(&list, "ho").unwrap_err().contains("matches 2"));
        assert!(find(&list, "zzz").unwrap_err().contains("No waypoint"));
    }

    #[test]
    fn default_names_skip_taken_ones() {
        let list = vec![wp("Waypoint 2")];
        assert_eq!(unique_name(&list), "Waypoint 3");
    }

    #[test]
    fn slugs_are_storage_safe() {
        assert_eq!(slug("My World!", 16), "my_world");
        assert_eq!(slug("", 16), "default");
        assert_eq!(slug("a.b/c", 16), "a_b_c");
    }

    #[test]
    fn colours_are_found_by_name() {
        assert_eq!(color_index("Aqua"), Some(4));
        assert_eq!(color_index("teal"), None);
    }
}
