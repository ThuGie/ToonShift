use crate::plugins::{Appearance, Plugin, Settings, Source, Theme};
use crate::thermostat::{Command, Status};
use fontdue::{Font, FontSettings, Metrics};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;

pub const WIDTH: usize = 1024;
pub const HEIGHT: usize = 600;

#[derive(Clone, Copy)]
struct Rect(usize, usize, usize, usize);

impl Rect {
    fn contains(self, x: usize, y: usize) -> bool {
        x >= self.0 && x < self.0 + self.2 && y >= self.1 && y < self.1 + self.3
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Home,
    Schedule,
    Store,
    Settings,
    Network,
}

#[derive(Clone, Copy)]
pub enum Action {
    Home,
    Schedule,
    ScheduleDay(usize),
    SchedulePage(usize),
    Store,
    Settings,
    Network,
    NetworkPage(usize),
    DashboardPage(usize),
    EditTiles,
    Tile(usize),
    MoveTile(usize, usize),
    Previous,
    Next,
    Toggle(usize),
    Theme(Theme),
    Appearance(Appearance),
    Heat(Command),
    Stock,
}

struct Palette {
    background: u32,
    card: u32,
    raised: u32,
    text: u32,
    muted: u32,
    accent: u32,
    on_accent: u32,
}

fn palette(settings: &Settings, hour: u8) -> Palette {
    let light = match settings.appearance {
        Appearance::Auto => (7..20).contains(&hour),
        Appearance::Day => true,
        Appearance::Night => false,
    };
    let (background, card, raised, accent) = match (light, settings.theme) {
        (true, Theme::Midnight) => (0xeaf1f8, 0xffffff, 0xdce9f5, 0x095abb),
        (true, Theme::Forest) => (0xe7f1eb, 0xffffff, 0xd7ebdf, 0x167050),
        (true, Theme::Plum) => (0xf4edf5, 0xffffff, 0xecdef1, 0x804c9a),
        (false, Theme::Midnight) => (0x101924, 0x192738, 0x263a51, 0xa4d2ff),
        (false, Theme::Forest) => (0x10221b, 0x1a3027, 0x28473a, 0x9fe5be),
        (false, Theme::Plum) => (0x231827, 0x35263b, 0x4c3555, 0xe5b7f6),
    };
    Palette {
        background,
        card,
        raised,
        accent,
        text: if light { 0x152336 } else { 0xf4f7fa },
        muted: if light { 0x4b5e71 } else { 0xb2c1d0 },
        on_accent: if light { 0xffffff } else { background },
    }
}

pub struct Ui {
    pub pixels: Vec<u32>,
    font: Font,
    glyphs: HashMap<(char, u16), (Metrics, Vec<u8>)>,
    buttons: Vec<(Rect, Action)>,
    page: Page,
    offset: usize,
    day: usize,
    schedule_page: usize,
    network_page: usize,
    dashboard_page: usize,
    editing: bool,
    selected_tile: Option<usize>,
    stock_confirm: bool,
}

impl Ui {
    pub fn new() -> Self {
        Self {
            pixels: vec![0; WIDTH * HEIGHT],
            font: Font::from_bytes(
                include_bytes!("../assets/Inter.ttf") as &[u8],
                FontSettings::default(),
            )
            .expect("bundled font"),
            glyphs: HashMap::new(),
            buttons: Vec::new(),
            page: Page::Home,
            offset: 0,
            day: 0,
            schedule_page: 0,
            network_page: 0,
            dashboard_page: 0,
            editing: false,
            selected_tile: None,
            stock_confirm: false,
        }
    }

    fn panel(&mut self, Rect(x, y, w, h): Rect, color: u32, radius: usize) {
        for row in 0..h {
            let edge = (row + 1).min(h - row);
            let inset = if edge < radius {
                let dy = (radius - edge) as f32;
                radius - ((radius * radius) as f32 - dy * dy).sqrt() as usize
            } else {
                0
            };
            let start = (y + row) * WIDTH + x;
            self.pixels[start + inset..start + w - inset].fill(color);
        }
    }

    pub fn logo(&mut self, x: usize, y: usize, size: usize) {
        self.panel(Rect(x, y, size, size), 0x112941, size / 4);
        let monogram = [
            (10.0, 15.0),
            (27.0, 15.0),
            (33.0, 9.0),
            (40.0, 9.0),
            (30.0, 21.0),
            (25.0, 21.0),
            (25.0, 34.0),
            (18.0, 40.0),
            (18.0, 21.0),
            (10.0, 21.0),
        ];
        let accent = [(31.0, 27.0), (39.0, 18.0), (39.0, 26.0), (31.0, 35.0)];
        for (shape, color) in [
            (monogram.as_slice(), 0xf2faffu32),
            (accent.as_slice(), 0x70ddccu32),
        ] {
            for row in 0..size {
                for col in 0..size {
                    let mut coverage = 0;
                    for dy in [0.25, 0.75] {
                        for dx in [0.25, 0.75] {
                            let px = (col as f32 + dx) * 48.0 / size as f32;
                            let py = (row as f32 + dy) * 48.0 / size as f32;
                            let mut inside = false;
                            for edge in 0..shape.len() {
                                let (ax, ay) = shape[edge];
                                let (bx, by) = shape[(edge + 1) % shape.len()];
                                if (ay > py) != (by > py)
                                    && px < (bx - ax) * (py - ay) / (by - ay) + ax
                                {
                                    inside = !inside;
                                }
                            }
                            coverage += u32::from(inside);
                        }
                    }
                    if coverage == 0 {
                        continue;
                    }
                    let pixel = &mut self.pixels[(y + row) * WIDTH + x + col];
                    let blend = |shift: u32| {
                        ((((color >> shift) & 255) * coverage
                            + ((*pixel >> shift) & 255) * (4 - coverage))
                            / 4)
                            << shift
                    };
                    *pixel = blend(16) | blend(8) | blend(0);
                }
            }
        }
    }

    fn text(
        &mut self,
        mut x: usize,
        baseline: usize,
        value: &str,
        size: u16,
        color: u32,
        right: usize,
    ) {
        if self.glyphs.len() > 512 {
            self.glyphs.clear();
        }
        for character in value.chars().filter(|c| !c.is_control()) {
            let (metrics, bitmap) = self
                .glyphs
                .entry((character, size))
                .or_insert_with(|| self.font.rasterize(character, f32::from(size)));
            let left = x as isize + metrics.xmin as isize;
            if left + metrics.width as isize > right as isize {
                break;
            }
            let top = baseline as isize - metrics.height as isize - metrics.ymin as isize;
            for row in 0..metrics.height {
                for col in 0..metrics.width {
                    let px = left + col as isize;
                    let py = top + row as isize;
                    let alpha = u32::from(bitmap[row * metrics.width + col]);
                    if alpha == 0 || px < 0 || !(0..HEIGHT as isize).contains(&py) {
                        continue;
                    }
                    let pixel = &mut self.pixels[py as usize * WIDTH + px as usize];
                    let blend = |shift: u32| {
                        ((((color >> shift) & 255) * alpha
                            + ((*pixel >> shift) & 255) * (255 - alpha))
                            / 255)
                            << shift
                    };
                    *pixel = blend(16) | blend(8) | blend(0);
                }
            }
            x += metrics.advance_width.round() as usize;
        }
    }

    fn button(
        &mut self,
        rect: Rect,
        label: &str,
        palette: &Palette,
        selected: bool,
        action: Action,
    ) {
        let (background, ink) = if selected {
            (palette.accent, palette.on_accent)
        } else {
            (palette.raised, palette.text)
        };
        self.panel(rect, background, 14);
        let size = 22;
        let width: f32 = label
            .chars()
            .map(|c| self.font.metrics(c, size as f32).advance_width)
            .sum();
        let inset = rect.2.saturating_sub(width.ceil() as usize) / 2;
        self.text(
            rect.0 + inset,
            rect.1 + rect.3 / 2 + 8,
            label,
            size,
            ink,
            rect.0 + rect.2,
        );
        self.buttons.push((rect, action));
    }

    pub fn hit(&mut self, x: usize, y: usize) -> Option<Action> {
        let action = self
            .buttons
            .iter()
            .rev()
            .find(|(rect, _)| rect.contains(x, y))?
            .1;
        if matches!(action, Action::Schedule | Action::Store | Action::Settings) {
            self.editing = false;
            self.selected_tile = None;
        }
        match action {
            Action::Home => self.page = Page::Home,
            Action::Schedule => {
                self.page = Page::Schedule;
                self.stock_confirm = false;
                return Some(action);
            }
            Action::ScheduleDay(day) => {
                self.day = day;
                self.schedule_page = 0;
            }
            Action::SchedulePage(page) => self.schedule_page = page,
            Action::Store => {
                self.page = Page::Store;
                self.stock_confirm = false;
                return Some(action);
            }
            Action::Settings => self.page = Page::Settings,
            Action::Network => {
                self.page = Page::Network;
                self.network_page = 0;
                self.stock_confirm = false;
                return Some(action);
            }
            Action::NetworkPage(page) => self.network_page = page,
            Action::DashboardPage(page) => self.dashboard_page = page,
            Action::EditTiles => {
                self.editing = !self.editing;
                self.selected_tile = None;
            }
            Action::Tile(index) => match self.selected_tile.take() {
                Some(source) if source != index => return Some(Action::MoveTile(source, index)),
                Some(_) => {}
                None => self.selected_tile = Some(index),
            },
            Action::Previous => self.offset = self.offset.saturating_sub(4),
            Action::Next => self.offset += 4,
            Action::Stock if !self.stock_confirm => {
                self.stock_confirm = true;
                return None;
            }
            _ => return Some(action),
        }
        self.stock_confirm = false;
        None
    }

    pub fn reset_store_page(&mut self) {
        self.offset = 0;
    }

    pub fn draw(&mut self, view: &View<'_>) {
        let p = palette(view.settings, view.hour);
        self.pixels.fill(p.background);
        self.buttons.clear();
        self.logo(32, 22, 48);
        self.text(92, 56, "ToonShift", 31, p.text, 270);
        for (x, title, page, action) in [
            (282, "Schedule", Page::Schedule, Action::Schedule),
            (424, "Home", Page::Home, Action::Home),
            (566, "Store", Page::Store, Action::Store),
            (708, "Settings", Page::Settings, Action::Settings),
        ] {
            let selected =
                self.page == page || (page == Page::Settings && self.page == Page::Network);
            self.button(Rect(x, 18, 130, 54), title, &p, selected, action);
        }
        self.text(900, 52, view.time, 24, p.muted, 992);
        match self.page {
            Page::Home => self.home(view, &p),
            Page::Schedule => self.schedule(view, &p),
            Page::Store => self.store(view, &p),
            Page::Settings => self.settings(view, &p),
            Page::Network => self.network(view, &p),
        }
        if let Some(error) = view.error {
            self.text(32, 581, error, 19, p.text, 992);
        }
    }

    fn network(&mut self, view: &View<'_>, p: &Palette) {
        self.panel(Rect(32, 100, 960, 448), p.card, 26);
        self.text(56, 148, "Wi-Fi", 28, p.text, 600);
        self.button(
            Rect(807, 117, 160, 48),
            "Scan again",
            p,
            false,
            Action::Network,
        );
        let networks: &[_] = match view.networks {
            Some(Ok(networks)) => networks,
            Some(Err(_)) => {
                self.text(56, 233, "Wi-Fi scan unavailable", 23, p.muted, 970);
                &[]
            }
            None => {
                self.text(56, 233, "Scanning for networks…", 23, p.muted, 970);
                &[]
            }
        };
        if matches!(view.networks, Some(Ok(networks)) if networks.is_empty()) {
            self.text(56, 233, "No nearby networks found", 23, p.muted, 970);
        }
        let page = self.network_page.min(networks.len().saturating_sub(1) / 5);
        for (row, network) in networks.iter().skip(page * 5).take(5).enumerate() {
            let y = 204 + row * 49;
            self.text(56, y, &network.name, 23, p.text, 590);
            self.text(650, y, &network.signal, 20, p.muted, 960);
        }
        if page > 0 {
            self.button(
                Rect(56, 430, 144, 44),
                "Previous",
                p,
                false,
                Action::NetworkPage(page - 1),
            );
        }
        if (page + 1) * 5 < networks.len() {
            self.button(
                Rect(823, 430, 144, 44),
                "Next",
                p,
                false,
                Action::NetworkPage(page + 1),
            );
        }
        let label = if self.stock_confirm {
            "Confirm switch to stock"
        } else {
            "Open stock Wi-Fi setup"
        };
        self.button(Rect(56, 484, 324, 47), label, p, false, Action::Stock);
        self.text(
            403,
            515,
            "In stock settings, open Internet.",
            20,
            p.muted,
            963,
        );
    }

    fn schedule(&mut self, view: &View<'_>, p: &Palette) {
        self.panel(Rect(32, 100, 960, 448), p.card, 26);
        self.text(56, 147, "Weekly schedule", 28, p.text, 964);
        for (day, label) in ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
            .into_iter()
            .enumerate()
        {
            self.button(
                Rect(56 + day * 132, 170, 120, 54),
                label,
                p,
                self.day == day,
                Action::ScheduleDay(day),
            );
        }
        let slots = match view.schedule {
            Some(Ok(slots)) => slots,
            Some(Err(error)) => {
                self.text(56, 270, error, 23, p.muted, 964);
                return;
            }
            None => {
                self.text(56, 270, "Loading schedule…", 23, p.muted, 964);
                return;
            }
        };
        let rows: Vec<_> = slots.iter().filter(|slot| slot.day == self.day).collect();
        if rows.is_empty() {
            self.text(56, 270, "No entries for this day", 23, p.muted, 964);
        }
        let page = self.schedule_page.min(rows.len().saturating_sub(1) / 5);
        for (row, slot) in rows.iter().skip(page * 5).take(5).enumerate() {
            let y = 272 + row * 46;
            let times = format!("{} – {}", slot.start, slot.end);
            self.text(56, y, &times, 23, p.muted, 400);
            self.text(414, y, &slot.preset, 23, p.text, 966);
        }
        if page > 0 {
            self.button(
                Rect(56, 493, 145, 44),
                "Previous",
                p,
                false,
                Action::SchedulePage(page - 1),
            );
        }
        if (page + 1) * 5 < rows.len() {
            self.button(
                Rect(823, 493, 145, 44),
                "Next",
                p,
                false,
                Action::SchedulePage(page + 1),
            );
        }
    }

    fn home(&mut self, view: &View<'_>, p: &Palette) {
        self.panel(Rect(32, 100, 532, 448), p.card, 26);
        self.text(62, 154, "Room temperature", 25, p.muted, 535);
        self.text(
            55,
            270,
            &temperature(view.status.map(|s| s.current_temp.as_str())),
            98,
            p.text,
            535,
        );
        self.text(63, 337, "Target temperature", 23, p.muted, 535);
        self.text(
            145,
            420,
            &temperature(view.status.map(|s| s.current_setpoint.as_str())),
            50,
            p.text,
            428,
        );
        if view.status.is_some() && !view.pending {
            self.button(
                Rect(62, 365, 70, 75),
                "−",
                p,
                false,
                Action::Heat(Command::Adjust(-50)),
            );
            self.button(
                Rect(464, 365, 70, 75),
                "+",
                p,
                false,
                Action::Heat(Command::Adjust(50)),
            );
            let scheduled = view
                .status
                .is_some_and(|s| matches!(s.program_state.as_str(), "1" | "2"));
            self.button(
                Rect(62, 462, 472, 60),
                if scheduled {
                    "Schedule on"
                } else {
                    "Schedule off"
                },
                p,
                scheduled,
                Action::Heat(Command::Program(!scheduled)),
            );
        } else {
            self.text(
                62,
                501,
                if view.pending {
                    "Updating thermostat…"
                } else {
                    "Waiting for thermostat…"
                },
                22,
                p.muted,
                534,
            );
        }

        let installed = &view.settings.installed;
        let pages = installed.len().div_ceil(4).max(1);
        self.dashboard_page = self.dashboard_page.min(pages - 1);
        for (slot, (index, id)) in installed
            .iter()
            .enumerate()
            .skip(self.dashboard_page * 4)
            .take(4)
            .enumerate()
        {
            let x = 584 + (slot % 2) * 210;
            let y = 100 + (slot / 2) * 203;
            let rect = Rect(x, y, 198, 192);
            let selected = self.editing && self.selected_tile == Some(index);
            let (fill, ink, muted) = if selected {
                (p.accent, p.on_accent, p.on_accent)
            } else {
                (p.card, p.text, p.muted)
            };
            self.panel(rect, fill, 22);
            if let Some(plugin) = view.plugins.iter().find(|plugin| &plugin.id == id) {
                self.text(x + 16, y + 47, &plugin.title, 21, muted, x + 182);
                self.text(
                    x + 16,
                    y + 113,
                    &plugin_value(plugin, view),
                    25,
                    ink,
                    x + 182,
                );
            } else {
                self.text(x + 16, y + 47, id, 20, muted, x + 182);
                self.text(x + 16, y + 113, "Unavailable", 23, ink, x + 182);
            }
            if self.editing {
                self.buttons.push((rect, Action::Tile(index)));
            }
        }
        if installed.is_empty() {
            self.button(
                Rect(584, 100, 408, 78),
                "Browse widgets",
                p,
                false,
                Action::Store,
            );
        } else {
            self.button(
                Rect(584, 505, 100, 43),
                if self.editing { "Done" } else { "Edit tiles" },
                p,
                self.editing,
                Action::EditTiles,
            );
            if self.dashboard_page > 0 {
                self.button(
                    Rect(700, 505, 58, 43),
                    "‹",
                    p,
                    false,
                    Action::DashboardPage(self.dashboard_page - 1),
                );
            }
            if self.dashboard_page + 1 < pages {
                self.button(
                    Rect(934, 505, 58, 43),
                    "›",
                    p,
                    false,
                    Action::DashboardPage(self.dashboard_page + 1),
                );
            }
            self.text(
                790,
                534,
                &format!("{} / {}", self.dashboard_page + 1, pages),
                21,
                p.muted,
                925,
            );
            if self.editing && view.error.is_none() {
                let prompt = if self.selected_tile.is_some() {
                    "Choose a destination. You can change pages."
                } else {
                    "Choose the tile you want to move."
                };
                self.text(584, 581, prompt, 16, p.muted, 992);
            }
        }
    }

    fn store(&mut self, view: &View<'_>, p: &Palette) {
        self.text(32, 130, "Dashboard widgets", 32, p.text, 992);
        for (row, plugin) in view.plugins.iter().skip(self.offset).take(4).enumerate() {
            let y = 150 + row * 88;
            self.panel(Rect(32, y, 960, 80), p.card, 16);
            self.text(54, y + 33, &plugin.title, 23, p.text, 738);
            self.text(54, y + 61, &plugin.description, 18, p.muted, 738);
            let index = self.offset + row;
            let installed = view.settings.installed.contains(&plugin.id);
            self.button(
                Rect(770, y + 11, 202, 58),
                if installed { "Remove" } else { "Install" },
                p,
                !installed,
                Action::Toggle(index),
            );
        }
        if self.offset > 0 {
            self.button(
                Rect(32, 509, 145, 49),
                "Previous",
                p,
                false,
                Action::Previous,
            );
        }
        if self.offset + 4 < view.plugins.len() {
            self.button(Rect(847, 509, 145, 49), "Next", p, false, Action::Next);
        }
    }

    fn settings(&mut self, view: &View<'_>, p: &Palette) {
        self.panel(Rect(32, 100, 960, 400), p.card, 26);
        self.text(58, 153, "Appearance", 28, p.text, 966);
        for (index, (label, mode)) in [
            ("Auto", Appearance::Auto),
            ("Day", Appearance::Day),
            ("Night", Appearance::Night),
        ]
        .into_iter()
        .enumerate()
        {
            self.button(
                Rect(58 + 184 * index, 173, 168, 60),
                label,
                p,
                mode as u8 == view.settings.appearance as u8,
                Action::Appearance(mode),
            );
        }
        self.text(
            58,
            266,
            "Auto follows local time from 07:00 to 20:00.",
            19,
            p.muted,
            966,
        );
        self.text(58, 327, "Color theme", 28, p.text, 966);
        for (index, (label, theme)) in [
            ("Blue", Theme::Midnight),
            ("Forest", Theme::Forest),
            ("Plum", Theme::Plum),
        ]
        .into_iter()
        .enumerate()
        {
            self.button(
                Rect(58 + 184 * index, 347, 168, 60),
                label,
                p,
                theme as u8 == view.settings.theme as u8,
                Action::Theme(theme),
            );
        }
        self.button(
            Rect(58, 428, 306, 54),
            if self.stock_confirm {
                "Confirm switch to stock"
            } else {
                "Switch to stock UI"
            },
            p,
            false,
            Action::Stock,
        );
        self.button(Rect(380, 428, 214, 54), "Wi-Fi", p, false, Action::Network);
    }
}

pub struct View<'a> {
    pub settings: &'a Settings,
    pub plugins: &'a [Plugin],
    pub status: Option<&'a Status>,
    pub pending: bool,
    pub error: Option<&'a str>,
    pub time: &'a str,
    pub hour: u8,
    pub root: &'a Path,
    pub schedule: Option<&'a Result<Vec<ScheduleRow>, String>>,
    pub networks: Option<&'a Result<Vec<NetworkRow>, String>>,
}

#[derive(serde::Deserialize)]
pub struct NetworkRow {
    #[serde(rename = "ESSID")]
    pub name: String,
    #[serde(rename = "Signal", default)]
    pub signal: String,
}

pub struct ScheduleRow {
    pub day: usize,
    pub start: String,
    pub end: String,
    pub preset: String,
}

fn temperature(value: Option<&str>) -> String {
    value
        .and_then(|v| v.parse::<i32>().ok())
        .map_or_else(|| "—".into(), |v| format!("{:.1}°C", f64::from(v) / 100.0))
}

fn plugin_value(plugin: &Plugin, view: &View<'_>) -> String {
    match plugin.source {
        Source::Clock => view.time.into(),
        Source::Boiler => match view.status {
            Some(s) if s.connection == "0" => "Offline",
            Some(s) if s.burner_info == "1" => "Heating",
            Some(s) if s.burner_info == "2" => "Hot water",
            Some(s) if s.burner_info == "3" => "Preheating",
            Some(_) => "Idle",
            None => "Unavailable",
        }
        .into(),
        Source::NextTarget => temperature(
            view.status
                .filter(|s| s.next_time != "0")
                .map(|s| s.next_setpoint.as_str()),
        ),
        Source::Note => plugin.text.clone(),
        Source::File => {
            let path = view.root.join("values").join(format!("{}.txt", plugin.id));
            let mut text = String::new();
            match fs::metadata(&path) {
                Ok(metadata) if metadata.is_file() && metadata.len() <= 1024 => {
                    let result = fs::File::open(path)
                        .and_then(|file| file.take(1024).read_to_string(&mut text));
                    if result.is_ok() {
                        return text.lines().next().unwrap_or_default().into();
                    }
                }
                _ => {}
            }
            "Unavailable".into()
        }
    }
}
