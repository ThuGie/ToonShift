use crate::plugins::{Appearance, Plugin, Settings, Source, Theme};
use crate::thermostat::{Command, Status};
use fontdue::{Font, FontSettings, Metrics};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;
mod hardware;
pub use hardware::HardwareAction;

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
    Plugin,
    Presets,
    Vacations,
    Energy,
    EnergyRates,
    HeatingDetails,
    Display,
    Advanced,
}

#[derive(Clone, Copy)]
pub enum Action {
    MeterSource(crate::plugins::Metric, usize),
    AutomaticMeter(crate::plugins::Metric),
    Hardware(HardwareAction),
    Home,
    Schedule,
    ScheduleDay(usize),
    SchedulePage(usize),
    OpenSlot(crate::schedule::Slot),
    NewSlot(crate::schedule::Slot),
    ShiftSlot(bool, i16),
    OpenCopyDay,
    CopyDestinations(u8),
    Preset(u8),
    CloseSlot,
    EditSchedule(crate::schedule::Edit),
    Store,
    Settings,
    Network,
    NetworkPage(usize),
    ChooseNetwork(usize),
    HiddenNetwork,
    CloseNetworkForm,
    NetworkField(bool),
    StaticWifi(bool),
    StaticWifiField(usize),
    Key(char),
    Backspace,
    KeyboardMode,
    KeyboardShift,
    JoinNetwork,
    ReconnectWifi,
    RepositoryEntry,
    CloseRepository,
    SyncRepository,
    RefreshOfficial,
    CatalogSources,
    CatalogSourcePage(usize),
    ConfirmRemoveSource(usize),
    RemoveSource(usize),
    Presets,
    Vacations,
    VacationPage(usize),
    EditVacation(Option<usize>),
    VacationField(bool),
    SaveVacation,
    ConfirmRemoveVacation(usize),
    RemoveVacation(usize),
    EditPreset(crate::presets::Preset),
    AdjustPreset(i32),
    ToggleHotWater,
    ClosePreset,
    SavePreset(crate::presets::Preset),
    Language(crate::language::Language),
    Energy(crate::energy::Meter, crate::energy::Period),
    EnergyRates,
    EditEnergyRate(crate::energy::Rate, Option<u32>),
    SaveEnergyRate(crate::energy::Rate),
    ExportDiagnostics,
    ConfirmResetPreferences,
    CancelResetPreferences,
    ResetPreferences,
    HeatingDetails,
    Display,
    Brightness(bool, i8),
    AppearanceHour(bool, i8),
    IdleMinutes(u8),
    IdleDashboard(Option<usize>),
    WideTiles(bool),
    OpenPlugin(usize),
    PluginSettings,
    PluginPlace(usize, usize),
    LocationSearch,
    RunLocationSearch(usize),
    CloseLocationSearch,
    UseLocationResult(usize, usize),
    UpdatePlugin(usize),
    RollbackPlugin(usize),
    DashboardPage(usize),
    EditTiles,
    Tile(usize),
    MoveTile(usize, usize),
    NameDashboard(usize),
    SaveDashboardName(usize),
    DuplicateTile(usize),
    ConfirmResetDashboard,
    ResetDashboard,
    Previous,
    Next,
    Toggle(usize),
    Theme(Theme),
    Appearance(Appearance),
    Heat(Command),
    Stock,
    StockMeterSession,
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
    let light = settings.daylight(hour);
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
    hardware: hardware::Controls,
    pub pixels: Vec<u32>,
    font: Font,
    glyphs: HashMap<(char, u16), (Metrics, Vec<u8>)>,
    buttons: Vec<(Rect, Action)>,
    page: Page,
    offset: usize,
    day: usize,
    schedule_page: usize,
    editing_slot: Option<(Option<crate::schedule::Slot>, crate::schedule::Slot)>,
    copy_days: Option<u8>,
    network_page: usize,
    open_plugin: usize,
    plugin_settings: bool,
    wifi: Option<WifiForm>,
    repository: Option<TextEntry>,
    source_list: bool,
    source_offset: usize,
    remove_source: Option<usize>,
    location_search: bool,
    preset_editor: Option<crate::presets::Preset>,
    vacation_editor: Option<VacationEditor>,
    vacation_offset: usize,
    vacation_remove: Option<usize>,
    energy: (crate::energy::Meter, crate::energy::Period),
    energy_rate: Option<crate::energy::Rate>,
    energy_rate_error: Option<&'static str>,
    language: crate::language::Language,
    mark: Vec<u32>,
    dashboard_page: usize,
    naming_dashboard: bool,
    resetting_dashboard: bool,
    editing: bool,
    selected_tile: Option<usize>,
    stock_confirm: bool,
    reset_preferences: bool,
}

impl Ui {
    pub fn new() -> Self {
        Self {
            hardware: Default::default(),
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
            editing_slot: None,
            copy_days: None,
            network_page: 0,
            open_plugin: 0,
            plugin_settings: false,
            wifi: None,
            repository: None,
            source_list: false,
            source_offset: 0,
            remove_source: None,
            location_search: false,
            preset_editor: None,
            vacation_editor: None,
            vacation_offset: 0,
            vacation_remove: None,
            energy: (
                crate::energy::Meter::Electricity,
                crate::energy::Period::Day,
            ),
            energy_rate: None,
            energy_rate_error: None,
            language: Default::default(),
            mark: Vec::new(),
            dashboard_page: 0,
            naming_dashboard: false,
            resetting_dashboard: false,
            editing: false,
            selected_tile: None,
            stock_confirm: false,
            reset_preferences: false,
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
        x: usize,
        baseline: usize,
        value: &str,
        size: u16,
        color: u32,
        right: usize,
    ) {
        self.raw_text(
            x,
            baseline,
            self.language.translate(value),
            size,
            color,
            right,
        );
    }

    fn raw_text(
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
        let label = if matches!(
            action,
            Action::PluginPlace(_, _)
                | Action::UseLocationResult(_, _)
                | Action::Key(_)
                | Action::NameDashboard(_)
        ) {
            label
        } else {
            self.language.translate(label)
        };
        let nominal: f32 = label
            .chars()
            .map(|c| self.font.metrics(c, 22.0).advance_width)
            .sum();
        let available = rect.2.saturating_sub(16) as f32;
        let size = (22.0 * available / nominal.max(1.0)).clamp(14.0, 22.0) as u16;
        let width = nominal * f32::from(size) / 22.0;
        let inset = rect.2.saturating_sub(width.ceil() as usize) / 2;
        self.raw_text(
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
        if self.reset_preferences
            && !matches!(
                action,
                Action::ResetPreferences | Action::ConfirmResetPreferences
            )
        {
            self.reset_preferences = false;
        }
        if matches!(action, Action::Schedule | Action::Store | Action::Settings) {
            self.editing = false;
            self.selected_tile = None;
        }
        if matches!(
            action,
            Action::Home | Action::Schedule | Action::Store | Action::Settings
        ) {
            self.naming_dashboard = false;
            self.resetting_dashboard = false;
            self.wifi = None;
            self.repository = None;
        }
        match action {
            Action::ConfirmResetPreferences => self.reset_preferences = true,
            Action::CancelResetPreferences => self.reset_preferences = false,
            Action::Vacations => {
                self.page = Page::Vacations;
                self.vacation_editor = None;
                self.vacation_remove = None;
                self.repository = None;
                return Some(action);
            }
            Action::VacationPage(offset) => self.vacation_offset = offset,
            Action::VacationField(end) => {
                if let Some(editor) = &mut self.vacation_editor {
                    if let Some(entry) = self.repository.as_mut() {
                        editor.dates[usize::from(editor.end)].clone_from(&entry.text);
                        editor.end = end;
                        entry.text.clone_from(&editor.dates[usize::from(end)]);
                    }
                }
            }
            Action::ConfirmRemoveVacation(index) => self.vacation_remove = Some(index),
            Action::CatalogSources => {
                self.source_list = true;
                self.remove_source = None;
                self.source_offset = 0;
                self.repository = None;
            }
            Action::CatalogSourcePage(offset) => self.source_offset = offset,
            Action::ConfirmRemoveSource(index) => self.remove_source = Some(index),
            Action::NameDashboard(_) => return Some(action),
            Action::SaveDashboardName(_) => return Some(action),
            Action::ConfirmResetDashboard => self.resetting_dashboard = true,
            Action::ResetDashboard => return Some(action),
            Action::CloseRepository if self.resetting_dashboard => self.resetting_dashboard = false,
            Action::CloseRepository if self.naming_dashboard => {
                self.naming_dashboard = false;
                self.repository = None;
            }
            Action::EnergyRates => {
                self.page = Page::EnergyRates;
                self.close_energy_rate();
            }
            Action::EditEnergyRate(rate, value) => {
                self.energy_rate = Some(rate);
                self.energy_rate_error = None;
                self.repository = Some(TextEntry {
                    text: value.map_or_else(String::new, |value| {
                        format!("{}.{:03}", value / 1000, value % 1000)
                    }),
                    ..TextEntry::default()
                });
            }
            Action::SaveEnergyRate(_) => return Some(action),
            Action::LocationSearch => {
                self.location_search = true;
                self.repository = Some(TextEntry::default());
                return Some(action);
            }
            Action::CloseLocationSearch => {
                self.location_search = false;
                self.repository = None;
            }
            Action::UseLocationResult(_, _) => {
                self.location_search = false;
                self.repository = None;
                self.plugin_settings = false;
                return Some(action);
            }
            Action::Display => self.page = Page::Display,
            Action::HeatingDetails => {
                self.page = Page::HeatingDetails;
                self.wifi = None;
                self.repository = None;
                self.editing = false;
                self.selected_tile = None;
                return Some(action);
            }
            Action::Energy(meter, period) => {
                self.page = Page::Energy;
                self.energy = (meter, period);
                self.wifi = None;
                self.repository = None;
                self.editing = false;
                self.selected_tile = None;
                return Some(action);
            }
            Action::RepositoryEntry => self.repository = Some(TextEntry::default()),
            Action::CloseRepository => self.repository = None,
            Action::Presets => {
                self.page = Page::Presets;
                self.preset_editor = None;
                return Some(action);
            }
            Action::EditPreset(preset) => self.preset_editor = Some(preset),
            Action::AdjustPreset(delta) => {
                if let Some(preset) = &mut self.preset_editor {
                    preset.temperature = (preset.temperature + delta).clamp(600, 3000);
                }
            }
            Action::ToggleHotWater => {
                if let Some(preset) = &mut self.preset_editor {
                    preset.hot_water = !preset.hot_water;
                }
            }
            Action::ClosePreset => self.preset_editor = None,
            Action::SavePreset(_) => {
                self.preset_editor = None;
                return Some(action);
            }
            Action::CloseNetworkForm => self.wifi = None,
            Action::NetworkField(password) => {
                if let Some(form) = &mut self.wifi {
                    form.password_field = password;
                }
            }
            Action::StaticWifi(enabled) => {
                if let Some(form) = &mut self.wifi {
                    form.static_ipv4 = enabled;
                    form.address_error = false;
                }
            }
            Action::StaticWifiField(index) => {
                if let Some(form) = &mut self.wifi {
                    form.address_field = index;
                    form.address_error = false;
                }
            }
            Action::Key(key) => {
                if self.page == Page::Vacations {
                    if let Some(entry) = &mut self.repository {
                        if entry.text.len() < 12 && key.is_ascii_digit() {
                            entry.text.push(key);
                        }
                    }
                } else if self.naming_dashboard {
                    if let Some(entry) = self.repository.as_mut() {
                        if entry.text.chars().count() < 18 {
                            entry.text.push(key);
                        }
                    }
                } else if let Some(form) = &mut self.wifi {
                    if form.static_ipv4 {
                        let input = &mut form.address_fields[form.address_field];
                        if input.len() < 15 && (key.is_ascii_digit() || key == '.') {
                            input.push(key);
                        }
                        return None;
                    }
                    let target = if form.password_field {
                        &mut form.join.password
                    } else {
                        &mut form.join.name
                    };
                    let limit = if form.password_field { 64 } else { 32 };
                    if target.len() + key.len_utf8() <= limit {
                        target.push(key);
                    }
                } else if let Some(entry) = &mut self.repository {
                    if entry.text.len() < 185 {
                        entry.text.push(key);
                    }
                }
            }
            Action::Backspace => {
                if let Some(form) = &mut self.wifi {
                    if form.static_ipv4 {
                        form.address_fields[form.address_field].pop();
                    } else if form.password_field {
                        form.join.password.pop();
                    } else {
                        form.join.name.pop();
                    }
                } else if let Some(entry) = &mut self.repository {
                    entry.text.pop();
                }
            }
            Action::KeyboardMode => {
                if let Some(form) = &mut self.wifi {
                    form.symbols = !form.symbols;
                } else if let Some(entry) = &mut self.repository {
                    entry.symbols = !entry.symbols;
                }
            }
            Action::KeyboardShift => {
                if let Some(form) = &mut self.wifi {
                    form.shift = !form.shift;
                } else if let Some(entry) = &mut self.repository {
                    entry.shift = !entry.shift;
                }
            }
            Action::OpenPlugin(index) => {
                self.page = Page::Plugin;
                self.open_plugin = index;
                self.plugin_settings = false;
                self.location_search = false;
            }
            Action::PluginSettings => self.plugin_settings = !self.plugin_settings,
            Action::OpenSlot(slot) => self.editing_slot = Some((Some(slot), slot)),
            Action::NewSlot(slot) => self.editing_slot = Some((None, slot)),
            Action::ShiftSlot(end, offset) => {
                if let Some((_, slot)) = &mut self.editing_slot {
                    slot.shift(end, offset);
                }
            }
            Action::Preset(preset) => {
                if let Some((_, slot)) = &mut self.editing_slot {
                    slot.preset = preset;
                }
            }
            Action::OpenCopyDay => self.copy_days = Some(0),
            Action::CopyDestinations(days) => self.copy_days = Some(days),
            Action::CloseSlot => {
                self.editing_slot = None;
                self.copy_days = None;
            }
            Action::EditSchedule(_) => {
                self.editing_slot = None;
                self.copy_days = None;
                return Some(action);
            }
            Action::Home => self.page = Page::Home,
            Action::Schedule => {
                self.page = Page::Schedule;
                self.editing_slot = None;
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
                self.source_list = false;
                self.remove_source = None;
                self.stock_confirm = false;
                return Some(action);
            }
            Action::Settings => self.page = Page::Settings,
            Action::Network => {
                self.page = Page::Network;
                self.network_page = 0;
                self.wifi = None;
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
            Action::Stock | Action::StockMeterSession if !self.stock_confirm => {
                self.stock_confirm = true;
                return None;
            }
            _ => return Some(action),
        }
        self.stock_confirm = false;
        None
    }

    pub fn gesture(
        &mut self,
        from: (usize, usize),
        to: (usize, usize),
        tiles: usize,
        tiles_per_page: usize,
    ) -> Option<Action> {
        let dx = from.0.abs_diff(to.0);
        let dy = from.1.abs_diff(to.1);
        if dx.max(dy) < 28 {
            return self.hit(to.0, to.1);
        }
        if self.page != Page::Home || from.0 < 584 || self.naming_dashboard {
            return None;
        }
        if self.editing {
            let slot = |point: (usize, usize)| {
                self.buttons.iter().find_map(|(rect, action)| {
                    if let Action::Tile(index) = action {
                        if rect.contains(point.0, point.1) {
                            return Some(*index);
                        }
                    }
                    None
                })
            };
            let source = slot(from)?;
            let destination = slot(to).or_else(|| {
                let page = if to.0 >= 995 && to.0 > from.0 {
                    self.dashboard_page + 1
                } else if to.0 <= 580 && to.0 < from.0 {
                    self.dashboard_page.checked_sub(1)?
                } else {
                    return None;
                };
                let destination = page * tiles_per_page + (source % tiles_per_page);
                if page * tiles_per_page >= tiles {
                    return None;
                }
                self.dashboard_page = page;
                Some(destination.min(tiles - 1))
            })?;
            self.selected_tile = None;
            return (source != destination).then_some(Action::MoveTile(source, destination));
        }
        if dx > 100 && dx > dy * 2 {
            self.dashboard_page = if from.0 > to.0 {
                (self.dashboard_page + 1).min(tiles.div_ceil(tiles_per_page).saturating_sub(1))
            } else {
                self.dashboard_page.saturating_sub(1)
            };
        }
        None
    }

    pub fn reset_store_page(&mut self) {
        self.offset = 0;
    }

    pub fn edit_dashboard_name(&mut self, name: &str) {
        self.naming_dashboard = true;
        self.repository = Some(TextEntry {
            text: name.into(),
            ..TextEntry::default()
        });
    }

    pub fn dashboard_name(&self) -> Option<&str> {
        self.repository.as_ref().map(|entry| entry.text.as_str())
    }

    pub fn close_dashboard_name(&mut self) {
        self.naming_dashboard = false;
        self.repository = None;
    }

    pub fn dashboard_changed(&mut self, reset: bool) {
        self.selected_tile = None;
        if reset {
            self.resetting_dashboard = false;
            self.editing = false;
            self.dashboard_page = 0;
        }
    }

    pub fn wifi_form(&mut self, network: Option<&NetworkRow>) {
        let (name, encryption, auth) = network.map_or_else(
            || (String::new(), "autodetect".to_owned(), "WPA2".to_owned()),
            |network| {
                (
                    network.name.clone(),
                    network.encryption.clone(),
                    network.auth.to_ascii_uppercase(),
                )
            },
        );
        let password_field = network.is_some() && !matches!(auth.as_str(), "OPEN" | "NONE");
        self.wifi = Some(WifiForm {
            join: crate::network::Join {
                name,
                encryption,
                auth,
                password: String::new(),
                address: None,
            },
            static_ipv4: false,
            address_fields: Default::default(),
            address_field: 0,
            address_error: false,
            hidden: network.is_none(),
            password_field,
            shift: false,
            symbols: false,
        });
    }

    pub fn take_wifi(&mut self) -> Option<crate::network::Join> {
        let form = self.wifi.as_mut()?;
        if form.static_ipv4 {
            form.join.address = crate::network::StaticIpv4::parse(
                form.address_fields.each_ref().map(String::as_str),
            );
            if form.join.address.is_none() {
                form.address_error = true;
                return None;
            }
        }
        self.wifi.take().map(|form| form.join)
    }

    pub fn take_repository(&mut self) -> Option<String> {
        self.repository.take().map(|entry| entry.text)
    }

    pub fn draw(&mut self, view: &View<'_>) {
        self.language = view.settings.language;
        let p = palette(view.settings, view.hour);
        self.pixels.fill(p.background);
        self.buttons.clear();
        if view.ambient {
            self.text(66, 244, view.time, 126, p.text, 950);
            self.text(
                73,
                337,
                &temperature(view.status.map(|s| s.current_temp.as_str())),
                60,
                p.muted,
                950,
            );
            self.text(76, 432, "Tap to wake", 23, p.muted, 545);
            if let Some(page) = view.settings.idle_dashboard_page {
                let last = view.settings.dashboard_pages().saturating_sub(1);
                self.dashboard_tiles(view, &p, page.min(last), false);
            }
            return;
        }
        if self.mark.is_empty() {
            self.logo(32, 22, 48);
            for row in 22..70 {
                self.mark.extend(
                    self.pixels[row * WIDTH + 32..row * WIDTH + 80]
                        .iter()
                        .map(|color| if *color == p.background { 0 } else { *color }),
                );
            }
        } else {
            for (row, colors) in self.mark.as_chunks::<48>().0.iter().enumerate() {
                for (col, color) in colors.iter().enumerate().filter(|(_, color)| **color != 0) {
                    self.pixels[(row + 22) * WIDTH + col + 32] = *color;
                }
            }
        }
        self.text(92, 56, "ToonShift", 31, p.text, 270);
        for (x, width, title, page, action) in [
            (270, 126, "Schedule", Page::Schedule, Action::Schedule),
            (405, 94, "Home", Page::Home, Action::Home),
            (
                508,
                105,
                "Energy",
                Page::Energy,
                Action::Energy(self.energy.0, self.energy.1),
            ),
            (622, 96, "Store", Page::Store, Action::Store),
            (727, 120, "Settings", Page::Settings, Action::Settings),
        ] {
            let selected = self.page == page
                || (page == Page::Settings && self.page == Page::Network)
                || (page == Page::Settings && self.page == Page::Advanced)
                || (page == Page::Schedule && self.page == Page::Presets);
            self.button(Rect(x, 18, width, 54), title, &p, selected, action);
        }
        self.text(900, 52, view.time, 24, p.muted, 992);
        match self.page {
            Page::Home => self.home(view, &p),
            Page::Schedule => self.schedule(view, &p),
            Page::Store => self.store(view, &p),
            Page::Settings => self.settings(view, &p),
            Page::Network => self.network(view, &p),
            Page::Plugin => self.plugin(view, &p),
            Page::Presets => self.presets(view, &p),
            Page::Vacations => self.vacations(view, &p),
            Page::Energy => self.energy(view, &p),
            Page::EnergyRates => self.energy_rates(view, &p),
            Page::HeatingDetails => self.heating_details(view, &p),
            Page::Display => self.display_settings(view, &p),
            Page::Advanced => self.hardware_screen(view, &p),
        }
        if let Some(error) = view.error {
            self.text(32, 581, error, 19, p.text, 992);
        }
    }

    fn plugin(&mut self, view: &View<'_>, p: &Palette) {
        let Some(plugin) = view.plugins.get(self.open_plugin) else {
            return;
        };
        let Some(feed) = &plugin.feed else {
            return;
        };
        if self.location_search {
            self.location_search_view(view, p);
            return;
        }
        self.panel(Rect(32, 100, 960, 448), p.card, 26);
        self.raw_text(56, 148, plugin.title(self.language), 32, p.text, 600);
        self.button(
            Rect(757, 117, 209, 47),
            if self.plugin_settings {
                "Back to readings"
            } else {
                "Change location"
            },
            p,
            false,
            Action::PluginSettings,
        );
        let active = crate::feed::location(plugin, view.settings);
        let location =
            crate::feed::place(plugin, view.settings).map_or("", |place| place.name.as_str());
        let reading = view.readings.get(&plugin.id);
        if !self.plugin_settings {
            self.raw_text(56, 204, location, 26, p.text, 960);
            if let Some(reading) = reading {
                self.text(
                    56,
                    253,
                    &format!(
                        "{}  {}",
                        self.language.translate(&reading.value),
                        self.language.translate(&reading.detail)
                    ),
                    32,
                    p.text,
                    965,
                );
                for (i, day) in reading.rows.iter().enumerate() {
                    let y = 305 + i * 46;
                    self.text(56, y, &day.date, 21, p.muted, 320);
                    self.text(335, y, &day.range, 21, p.text, 600);
                    self.text(631, y, &day.condition, 21, p.muted, 960);
                }
            } else {
                self.text(56, 253, "Loading…", 23, p.muted, 960);
            }
            return;
        }
        self.text(56, 198, "Location", 23, p.muted, 960);
        if feed.search.is_some() {
            self.button(
                Rect(755, 168, 211, 45),
                "Search location",
                p,
                false,
                Action::LocationSearch,
            );
        }
        for (index, place) in feed.places.iter().enumerate() {
            self.button(
                Rect(56 + index % 4 * 229, 218 + index / 4 * 46, 214, 42),
                &place.name,
                p,
                active == index && !view.settings.custom_locations.contains_key(&plugin.id),
                Action::PluginPlace(self.open_plugin, index),
            );
        }
        self.text(
            56,
            525,
            &format!("Version {}", plugin.release),
            18,
            p.muted,
            960,
        );
    }

    fn location_search_view(&mut self, view: &View<'_>, p: &Palette) {
        self.panel(Rect(32, 100, 960, 448), p.card, 26);
        self.text(56, 148, "Search location", 30, p.text, 960);
        if let Some(entry) = self.repository.take() {
            if entry.text.is_empty() {
                self.text(56, 199, "City or postcode", 23, p.muted, 965);
            } else {
                self.raw_text(56, 199, &entry.text, 23, p.text, 965);
            }
            self.keyboard(entry.symbols, entry.shift, p);
            self.repository = Some(entry);
            self.button(
                Rect(752, 473, 215, 52),
                "Search",
                p,
                true,
                Action::RunLocationSearch(self.open_plugin),
            );
        } else {
            match view.location_results {
                Some(Ok(results)) if !results.is_empty() => {
                    for (index, place) in results.iter().enumerate() {
                        self.button(
                            Rect(56, 198 + index * 49, 906, 43),
                            &place.name,
                            p,
                            false,
                            Action::UseLocationResult(self.open_plugin, index),
                        );
                    }
                }
                Some(Ok(_)) => self.text(56, 264, "No matching locations", 23, p.muted, 960),
                Some(Err(error)) => self.text(56, 264, error, 23, p.muted, 960),
                None => self.text(56, 264, "Searching…", 23, p.muted, 960),
            }
            if view.location_results.is_some() {
                self.button(
                    Rect(752, 473, 215, 52),
                    "Search again",
                    p,
                    false,
                    Action::LocationSearch,
                );
            }
        }
        self.button(
            Rect(56, 473, 212, 52),
            "Cancel",
            p,
            false,
            Action::CloseLocationSearch,
        );
    }

    fn network(&mut self, view: &View<'_>, p: &Palette) {
        self.panel(Rect(32, 100, 960, 448), p.card, 26);
        if let Some(form) = self.wifi.take() {
            self.wifi_keyboard(&form, p);
            self.wifi = Some(form);
            return;
        }
        self.text(56, 148, "Wi-Fi", 28, p.text, 600);
        self.button(
            Rect(565, 117, 220, 48),
            "Hidden network",
            p,
            false,
            Action::HiddenNetwork,
        );
        self.button(
            Rect(807, 117, 160, 48),
            "Scan again",
            p,
            false,
            Action::Network,
        );
        match view.wifi_connection {
            Some(Ok(Some(connection))) => {
                let address = connection.address.map_or_else(
                    || self.language.translate("Waiting for IP address").to_owned(),
                    |address| address.to_string(),
                );
                let signal = connection
                    .rssi
                    .map_or_else(String::new, |rssi| format!(" · {rssi} dBm"));
                self.raw_text(
                    56,
                    195,
                    &format!("{} · {address}{signal}", connection.ssid),
                    19,
                    p.text,
                    966,
                );
            }
            Some(Ok(None)) => self.text(56, 195, "Wi-Fi disconnected", 19, p.muted, 966),
            Some(Err(_)) => self.text(
                56,
                195,
                "Wi-Fi connection status unavailable",
                19,
                p.muted,
                966,
            ),
            None => {}
        }
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
        let page = self.network_page.min(networks.len().saturating_sub(1) / 4);
        for (row, network) in networks.iter().skip(page * 4).take(4).enumerate() {
            let y = 243 + row * 49;
            self.raw_text(56, y, &network.name, 23, p.text, 590);
            self.text(610, y, &network.signal, 20, p.muted, 790);
            self.button(
                Rect(822, y - 29, 145, 39),
                "Join",
                p,
                false,
                Action::ChooseNetwork(page * 4 + row),
            );
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
        if (page + 1) * 4 < networks.len() {
            self.button(
                Rect(823, 430, 144, 44),
                "Next",
                p,
                false,
                Action::NetworkPage(page + 1),
            );
        }
        self.button(
            Rect(342, 430, 340, 44),
            "Reconnect saved network",
            p,
            false,
            Action::ReconnectWifi,
        );
        if let Some(wired) = view.ethernet {
            let state = match wired.carrier {
                Some(true) => "Ethernet connected",
                Some(false) => "Ethernet disconnected",
                None => "Ethernet link unknown",
            };
            let detail = wired.address.map_or_else(
                || {
                    if wired.carrier == Some(true) {
                        format!(" · {}", self.language.translate("Waiting for IP address"))
                    } else {
                        String::new()
                    }
                },
                |ip| format!(" · {ip}"),
            );
            self.raw_text(
                56,
                514,
                &format!("{}{detail}", self.language.translate(state)),
                20,
                p.muted,
                963,
            );
        }
        if let Some(message) = view.network_message.filter(|_| view.error.is_none()) {
            self.text(32, 581, message, 19, p.muted, 992);
        }
    }

    fn wifi_keyboard(&mut self, form: &WifiForm, p: &Palette) {
        if form.static_ipv4 {
            self.wifi_static_address(form, p);
            return;
        }
        let open = matches!(form.join.auth.as_str(), "OPEN" | "NONE");
        self.text(56, 148, "Join Wi-Fi", 28, p.text, 960);
        let name = if form.join.name.is_empty() {
            "Network name"
        } else {
            &form.join.name
        };
        if form.hidden {
            self.button(
                Rect(56, 165, 422, 50),
                name,
                p,
                !form.password_field,
                Action::NetworkField(false),
            );
        } else {
            self.raw_text(56, 199, name, 24, p.text, 478);
        }
        if !open {
            let mask = if form.join.password.is_empty() {
                "Password".to_owned()
            } else {
                let count = form.join.password.len();
                format!("{}  ({count})", "•".repeat(count.min(14)))
            };
            self.button(
                Rect(492, 165, 472, 50),
                &mask,
                p,
                form.password_field,
                Action::NetworkField(true),
            );
        }
        if !open || form.hidden {
            self.keyboard(form.symbols, form.shift, p);
        }
        self.button(
            Rect(56, 473, 212, 52),
            "Cancel",
            p,
            false,
            Action::CloseNetworkForm,
        );
        self.button(
            Rect(285, 473, 235, 52),
            "Static IPv4",
            p,
            false,
            Action::StaticWifi(true),
        );
        self.button(
            Rect(752, 473, 215, 52),
            "Join network",
            p,
            true,
            Action::JoinNetwork,
        );
    }

    fn wifi_static_address(&mut self, form: &WifiForm, p: &Palette) {
        self.text(56, 148, "Static IPv4", 28, p.text, 960);
        for (index, label) in ["IP address", "Subnet mask", "Gateway", "DNS server"]
            .into_iter()
            .enumerate()
        {
            let x = 56 + 228 * index;
            self.text(x, 190, label, 20, p.muted, x + 220);
            let value = &form.address_fields[index];
            self.button(
                Rect(x, 204, 215, 55),
                if value.is_empty() { "–" } else { value },
                p,
                index == form.address_field,
                Action::StaticWifiField(index),
            );
        }
        for (index, key) in "1234567890.".chars().enumerate() {
            self.button(
                Rect(56 + 76 * index, 291, 67, 55),
                &key.to_string(),
                p,
                false,
                Action::Key(key),
            );
        }
        self.button(
            Rect(56, 363, 190, 47),
            "Backspace",
            p,
            false,
            Action::Backspace,
        );
        if form.address_error {
            self.text(
                56,
                448,
                "Check the IPv4 address, subnet mask, gateway and DNS server.",
                19,
                p.muted,
                963,
            );
        }
        self.button(
            Rect(56, 473, 245, 52),
            "Back to Wi-Fi credentials",
            p,
            false,
            Action::StaticWifi(false),
        );
        self.button(
            Rect(752, 473, 215, 52),
            "Join network",
            p,
            true,
            Action::JoinNetwork,
        );
    }

    fn keyboard(&mut self, symbols: bool, shift: bool, p: &Palette) {
        let rows = if symbols {
            if shift {
                ["1234567890", "`~[]{}<>\\|", "'\";:,.?/_-"]
            } else {
                ["1234567890", "!@#$%^&*()", "-_=+[]{}\\|"]
            }
        } else {
            ["qwertyuiop", "asdfghjkl", "zxcvbnm,./"]
        };
        for (row, keys) in rows.iter().enumerate() {
            for (column, key) in keys.chars().enumerate() {
                let key = if shift && !symbols {
                    key.to_ascii_uppercase()
                } else {
                    key
                };
                self.button(
                    Rect(56 + 91 * column, 229 + 53 * row, 81, 47),
                    &key.to_string(),
                    p,
                    false,
                    Action::Key(key),
                );
            }
        }
        self.button(
            Rect(56, 390, 173, 52),
            if symbols { "ABC" } else { "123 / #+=" },
            p,
            false,
            Action::KeyboardMode,
        );
        self.button(
            Rect(238, 390, 173, 52),
            if symbols { "More" } else { "Shift" },
            p,
            shift,
            Action::KeyboardShift,
        );
        self.button(Rect(420, 390, 264, 52), "Space", p, false, Action::Key(' '));
        self.button(
            Rect(693, 390, 274, 52),
            "Backspace",
            p,
            false,
            Action::Backspace,
        );
    }

    fn schedule(&mut self, view: &View<'_>, p: &Palette) {
        self.panel(Rect(32, 100, 960, 448), p.card, 26);
        if let Some(days) = self.copy_days {
            self.text(56, 147, "Copy heating day", 28, p.text, 964);
            self.text(
                56,
                200,
                "Replace the schedule on the selected days.",
                22,
                p.muted,
                960,
            );
            let source = 1u8 << self.day;
            for (day, name) in ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
                .into_iter()
                .enumerate()
            {
                let bit = 1u8 << day;
                if bit != source {
                    self.button(
                        Rect(56 + day * 132, 229, 120, 54),
                        name,
                        p,
                        days & bit != 0,
                        Action::CopyDestinations(days ^ bit),
                    );
                }
            }
            self.button(
                Rect(56, 320, 220, 54),
                "Weekdays",
                p,
                false,
                Action::CopyDestinations(0x3e & !source),
            );
            self.button(
                Rect(295, 320, 220, 54),
                "Weekend",
                p,
                false,
                Action::CopyDestinations(0x41 & !source),
            );
            self.button(
                Rect(56, 461, 160, 58),
                "Cancel",
                p,
                false,
                Action::CloseSlot,
            );
            if days != 0 {
                self.button(
                    Rect(800, 461, 160, 58),
                    "Copy",
                    p,
                    true,
                    Action::EditSchedule(crate::schedule::Edit::CopyDay {
                        source: self.day as u8,
                        days,
                    }),
                );
            }
            return;
        }
        if let Some((original, slot)) = self.editing_slot {
            self.text(
                56,
                147,
                if original.is_some() {
                    "Edit heating period"
                } else {
                    "Add heating period"
                },
                28,
                p.text,
                964,
            );
            for (end, title, x) in [(false, "Starts", 56), (true, "Ends", 520)] {
                self.text(x, 205, title, 23, p.muted, x + 400);
                self.button(
                    Rect(x, 223, 68, 55),
                    "−15",
                    p,
                    false,
                    Action::ShiftSlot(end, -15),
                );
                self.text(x + 86, 258, &slot.label(end), 26, p.text, x + 310);
                self.button(
                    Rect(x + 82, 274, 70, 33),
                    "−1",
                    p,
                    false,
                    Action::ShiftSlot(end, -1),
                );
                self.button(
                    Rect(x + 185, 274, 70, 33),
                    "+1",
                    p,
                    false,
                    Action::ShiftSlot(end, 1),
                );
                self.button(
                    Rect(x + 300, 223, 72, 55),
                    "+15",
                    p,
                    false,
                    Action::ShiftSlot(end, 15),
                );
            }
            for (preset, label) in ["Comfort", "Home", "Sleep", "Away"].into_iter().enumerate() {
                self.button(
                    Rect(56 + preset * 229, 310, 205, 56),
                    label,
                    p,
                    slot.preset == preset as u8,
                    Action::Preset(preset as u8),
                );
            }
            self.text(
                56,
                410,
                "Shared boundaries move with adjacent periods.",
                20,
                p.muted,
                960,
            );
            self.button(
                Rect(56, 461, 160, 58),
                "Cancel",
                p,
                false,
                Action::CloseSlot,
            );
            if let Some(original) = original {
                self.button(
                    Rect(245, 461, 160, 58),
                    "Split",
                    p,
                    false,
                    Action::EditSchedule(crate::schedule::Edit::Split { slot: original }),
                );
                self.button(
                    Rect(434, 461, 160, 58),
                    "Remove",
                    p,
                    false,
                    Action::EditSchedule(crate::schedule::Edit::Remove { slot: original }),
                );
            }
            self.button(
                Rect(800, 461, 160, 58),
                "Save",
                p,
                true,
                Action::EditSchedule(match original {
                    Some(original) => crate::schedule::Edit::Replace {
                        original,
                        new: slot,
                    },
                    None => crate::schedule::Edit::Add { slot },
                }),
            );
            return;
        }
        self.text(56, 147, "Weekly schedule", 28, p.text, 964);
        self.button(
            Rect(560, 112, 180, 49),
            "Presets",
            p,
            false,
            Action::Presets,
        );
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
        self.button(
            Rect(372, 112, 170, 49),
            "Copy day",
            p,
            false,
            Action::OpenCopyDay,
        );
        let rows: Vec<_> = slots
            .iter()
            .copied()
            .filter(|slot| slot.day() == self.day)
            .collect();
        if let Some(slot) = crate::schedule::open_period(self.day, slots) {
            self.button(
                Rect(758, 112, 208, 49),
                "Add period",
                p,
                false,
                Action::NewSlot(slot),
            );
        }
        if rows.is_empty() {
            self.text(56, 270, "No entries for this day", 23, p.muted, 964);
        }
        let page = self.schedule_page.min(rows.len().saturating_sub(1) / 5);
        for (row, slot) in rows.iter().skip(page * 5).take(5).enumerate() {
            let y = 272 + row * 46;
            let times = format!("{} – {}", slot.label(false), slot.label(true));
            self.text(56, y, &times, 23, p.muted, 400);
            self.text(414, y, slot.preset_name(), 23, p.text, 752);
            self.button(
                Rect(824, y - 30, 142, 40),
                "Edit",
                p,
                false,
                Action::OpenSlot(*slot),
            );
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

    fn presets(&mut self, view: &View<'_>, p: &Palette) {
        self.panel(Rect(32, 100, 960, 448), p.card, 26);
        if let Some(preset) = self.preset_editor {
            self.text(
                56,
                152,
                crate::presets::NAMES[usize::from(preset.id)],
                30,
                p.text,
                960,
            );
            self.text(56, 227, "Target temperature", 24, p.muted, 960);
            self.button(
                Rect(56, 248, 82, 58),
                "−",
                p,
                false,
                Action::AdjustPreset(-50),
            );
            self.text(
                166,
                291,
                &format!("{:.1}°C", f64::from(preset.temperature) / 100.0),
                33,
                p.text,
                422,
            );
            self.button(
                Rect(442, 248, 82, 58),
                "+",
                p,
                false,
                Action::AdjustPreset(50),
            );
            self.button(
                Rect(56, 330, 310, 58),
                if preset.hot_water {
                    "Hot water on"
                } else {
                    "Hot water off"
                },
                p,
                preset.hot_water,
                Action::ToggleHotWater,
            );
            self.button(
                Rect(56, 461, 190, 55),
                "Cancel",
                p,
                false,
                Action::ClosePreset,
            );
            self.button(
                Rect(775, 461, 190, 55),
                "Save",
                p,
                true,
                Action::SavePreset(preset),
            );
            return;
        }
        self.text(56, 152, "Heating presets", 30, p.text, 560);
        self.button(
            Rect(728, 115, 238, 48),
            "Weekly schedule",
            p,
            false,
            Action::Schedule,
        );
        self.button(
            Rect(512, 115, 202, 48),
            "Vacations",
            p,
            false,
            Action::Vacations,
        );
        match view.presets {
            Some(Ok(presets)) => {
                for (index, preset) in presets.iter().enumerate() {
                    let y = 179 + index * 67;
                    self.text(
                        56,
                        y + 34,
                        crate::presets::NAMES[usize::from(preset.id)],
                        23,
                        p.text,
                        347,
                    );
                    self.text(
                        358,
                        y + 34,
                        &format!("{:.1}°C", f64::from(preset.temperature) / 100.0),
                        23,
                        p.muted,
                        567,
                    );
                    self.button(
                        Rect(480, y, 122, 51),
                        "Edit",
                        p,
                        false,
                        Action::EditPreset(*preset),
                    );
                    if !view.pending {
                        let until_next = preset.id < 4
                            && view.status.is_some_and(|status| {
                                matches!(status.program_state.as_str(), "1" | "2")
                                    && status.next_time.parse::<u64>().is_ok_and(|time| time > 0)
                            });
                        self.button(
                            Rect(618, y, 165, 51),
                            if until_next {
                                "Until next period"
                            } else {
                                "Use preset"
                            },
                            p,
                            true,
                            Action::Heat(if until_next {
                                Command::HoldPreset(
                                    preset.id,
                                    crate::thermostat::HoldMode::UntilNextPeriod,
                                )
                            } else {
                                Command::ApplyPreset(preset.id)
                            }),
                        );
                        if preset.id < 4 {
                            self.button(
                                Rect(799, y, 167, 51),
                                "Hold indefinitely",
                                p,
                                false,
                                Action::Heat(Command::HoldPreset(
                                    preset.id,
                                    crate::thermostat::HoldMode::Indefinite,
                                )),
                            );
                        }
                    }
                }
            }
            Some(Err(error)) => self.text(56, 223, error, 23, p.muted, 960),
            None => self.text(56, 223, "Loading presets…", 23, p.muted, 960),
        }
    }

    fn home(&mut self, view: &View<'_>, p: &Palette) {
        if self.naming_dashboard {
            self.dashboard_name_editor(p);
            return;
        }
        if self.resetting_dashboard {
            self.panel(Rect(32, 100, 960, 448), p.card, 26);
            self.text(56, 165, "Reset dashboard layout?", 30, p.text, 960);
            self.text(
                56,
                215,
                "Restore the default tiles and clear page names.",
                23,
                p.muted,
                960,
            );
            self.button(
                Rect(56, 462, 215, 51),
                "Cancel",
                p,
                false,
                Action::CloseRepository,
            );
            self.button(
                Rect(754, 462, 215, 51),
                "Reset layout",
                p,
                true,
                Action::ResetDashboard,
            );
            return;
        }
        self.panel(Rect(32, 100, 532, 448), p.card, 26);
        self.text(62, 154, "Room temperature", 25, p.muted, 535);
        if let Some(status) = view.status.filter(|status| status.heat_requested()) {
            let label = if status.connection == "0" {
                "Heat requested · boiler link offline"
            } else if status.burner_info == "1" {
                "Boiler firing for heat"
            } else {
                "Heat requested · burner not confirmed"
            };
            self.text(63, 310, label, 20, p.accent, 535);
        }
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
            let mode = view
                .status
                .map_or("", |status| status.program_state.as_str());
            let (label, enabled) = match mode {
                "1" => ("Schedule on", true),
                "2" => ("Hold until next period · Resume schedule", false),
                "8" => ("Indefinite hold · Resume schedule", false),
                _ => ("Schedule off", false),
            };
            self.button(
                Rect(62, 462, 472, 60),
                label,
                p,
                enabled,
                Action::Heat(Command::Program(!enabled)),
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

        let pages = view.settings.dashboard_pages().max(1);
        self.dashboard_page = self.dashboard_page.min(pages - 1);
        self.dashboard_tiles(view, p, self.dashboard_page, true);
        self.dashboard_controls(view, p, pages);
    }

    pub fn edit_vacation(&mut self, id: Option<crate::vacation::Id>, dates: [String; 2]) {
        self.repository = Some(TextEntry {
            text: dates[0].clone(),
            ..Default::default()
        });
        self.vacation_editor = Some(VacationEditor {
            id,
            dates,
            end: false,
            error: None,
        });
    }

    pub fn vacation_input(&mut self) -> Option<(Option<crate::vacation::Id>, [[i32; 5]; 2])> {
        let editor = self.vacation_editor.as_mut()?;
        editor.dates[usize::from(editor.end)].clone_from(&self.repository.as_ref()?.text);
        let parse = |text: &str| {
            if text.len() != 12 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            Some([
                text[..4].parse().ok()?,
                text[4..6].parse().ok()?,
                text[6..8].parse().ok()?,
                text[8..10].parse().ok()?,
                text[10..].parse().ok()?,
            ])
        };
        Some((
            editor.id,
            [parse(&editor.dates[0])?, parse(&editor.dates[1])?],
        ))
    }

    pub fn vacation_input_error(&mut self) {
        if let Some(editor) = &mut self.vacation_editor {
            editor.error = Some("Enter two valid local dates and times.");
        }
    }

    pub fn close_vacation_editor(&mut self) {
        if self.page == Page::Vacations {
            self.repository = None;
        }
        self.vacation_editor = None;
        self.vacation_remove = None;
    }

    fn vacations(&mut self, view: &View<'_>, p: &Palette) {
        self.panel(Rect(32, 100, 960, 448), p.card, 26);
        self.text(56, 152, "Vacations", 30, p.text, 730);
        self.button(
            Rect(728, 115, 238, 48),
            "Heating presets",
            p,
            false,
            Action::Presets,
        );
        if view.vacation_busy {
            self.text(56, 227, "Checking vacation schedule…", 22, p.muted, 967);
            return;
        }
        if let Some(editor) = &self.vacation_editor {
            let end = editor.end;
            let dates = editor.dates.clone();
            let error = editor.error;
            let current = self
                .repository
                .as_ref()
                .map_or(String::new(), |entry| entry.text.clone());
            for (index, label) in ["Start date", "End date"].into_iter().enumerate() {
                let active = index == usize::from(end);
                let x = 56 + 459 * index;
                self.button(
                    Rect(x, 179, 430, 45),
                    label,
                    p,
                    active,
                    Action::VacationField(index == 1),
                );
                self.raw_text(
                    x,
                    256,
                    if active { &current } else { &dates[index] },
                    26,
                    p.text,
                    x + 430,
                );
            }
            for (index, digit) in "1234567890".chars().enumerate() {
                self.button(
                    Rect(56 + index * 81, 283, 72, 62),
                    &digit.to_string(),
                    p,
                    false,
                    Action::Key(digit),
                );
            }
            self.button(
                Rect(56, 360, 200, 46),
                "Backspace",
                p,
                false,
                Action::Backspace,
            );
            self.text(
                280,
                389,
                error.unwrap_or("Enter year, month, day, hour and minute (YYYYMMDDHHMM)."),
                19,
                p.muted,
                969,
            );
            self.button(
                Rect(56, 467, 210, 52),
                "Cancel",
                p,
                false,
                Action::Vacations,
            );
            self.button(
                Rect(752, 467, 215, 52),
                "Save",
                p,
                true,
                Action::SaveVacation,
            );
            if let Some(Err(error)) = view.vacations {
                self.text(56, 446, error, 18, p.muted, 969);
            }
            return;
        }
        if let Some(index) = self.vacation_remove {
            self.text(56, 227, "Remove this vacation?", 27, p.text, 967);
            if let Some(Err(error)) = view.vacations {
                self.text(56, 286, error, 20, p.muted, 969);
                self.button(
                    Rect(752, 467, 215, 52),
                    "Refresh",
                    p,
                    true,
                    Action::Vacations,
                );
            } else {
                self.button(
                    Rect(56, 467, 210, 52),
                    "Cancel",
                    p,
                    false,
                    Action::Vacations,
                );
                self.button(
                    Rect(752, 467, 215, 52),
                    "Remove",
                    p,
                    true,
                    Action::RemoveVacation(index),
                );
            }
            return;
        }
        let entries = match view.vacations {
            Some(Ok(entries)) => entries,
            Some(Err(error)) => {
                self.text(56, 225, error, 21, p.muted, 966);
                self.button(
                    Rect(752, 467, 215, 52),
                    "Refresh",
                    p,
                    true,
                    Action::Vacations,
                );
                return;
            }
            None => return,
        };
        self.vacation_offset = self
            .vacation_offset
            .min(entries.len().saturating_sub(1) / 4 * 4);
        if entries.is_empty() {
            self.text(56, 225, "No vacations scheduled", 23, p.muted, 966);
        }
        for (index, entry) in entries
            .iter()
            .enumerate()
            .skip(self.vacation_offset)
            .take(4)
        {
            let y = 178 + (index - self.vacation_offset) * 65;
            self.raw_text(
                56,
                y + 33,
                &format!(
                    "{} – {}",
                    vacation_label(entry.start),
                    vacation_label(entry.end)
                ),
                19,
                p.text,
                604,
            );
            self.button(
                Rect(612, y, 160, 49),
                "Edit",
                p,
                false,
                Action::EditVacation(Some(index)),
            );
            self.button(
                Rect(784, y, 182, 49),
                "Remove",
                p,
                false,
                Action::ConfirmRemoveVacation(index),
            );
        }
        if self.vacation_offset > 0 {
            self.button(
                Rect(56, 467, 170, 52),
                "Previous",
                p,
                false,
                Action::VacationPage(self.vacation_offset - 4),
            );
        }
        if self.vacation_offset + 4 < entries.len() {
            self.button(
                Rect(240, 467, 170, 52),
                "Next",
                p,
                false,
                Action::VacationPage(self.vacation_offset + 4),
            );
        }
        self.button(
            Rect(752, 467, 215, 52),
            "Add vacation",
            p,
            true,
            Action::EditVacation(None),
        );
    }

    fn dashboard_tiles(&mut self, view: &View<'_>, p: &Palette, page: usize, interactive: bool) {
        let editing = interactive && self.editing;
        let count = view.settings.tiles_per_page();
        let columns = count / 2;
        let width = (408 - (columns - 1) * 12) / columns;
        for (slot, (index, id)) in view
            .settings
            .installed
            .iter()
            .enumerate()
            .skip(page * count)
            .take(count)
            .enumerate()
        {
            let x = 584 + (slot % columns) * 210;
            let y = 100 + (slot / columns) * 203;
            let rect = Rect(x, y, width, 192);
            let end = x + width - 16;
            let selected = editing && self.selected_tile == Some(index);
            let (fill, ink, muted) = if selected {
                (p.accent, p.on_accent, p.on_accent)
            } else {
                (p.card, p.text, p.muted)
            };
            self.panel(rect, fill, 22);
            if let Some((plugin_index, plugin)) = view
                .plugins
                .iter()
                .enumerate()
                .find(|(_, plugin)| &plugin.id == id)
            {
                self.raw_text(x + 16, y + 47, plugin.title(self.language), 21, muted, end);
                self.text(x + 16, y + 113, &plugin_value(plugin, view), 25, ink, end);
                if !editing && matches!(plugin.source, Source::HttpsJson) {
                    let detail = crate::feed::place(plugin, view.settings)
                        .map_or("", |place| place.name.as_str());
                    self.raw_text(x + 16, y + 155, detail, 17, muted, end);
                    if interactive {
                        self.buttons.push((rect, Action::OpenPlugin(plugin_index)));
                    }
                }
                if interactive && !editing && matches!(plugin.source, Source::Boiler) {
                    self.text(x + 16, y + 155, "Details", 17, muted, end);
                    self.buttons.push((rect, Action::HeatingDetails));
                }
            } else {
                self.text(x + 16, y + 47, id, 20, muted, end);
                self.text(x + 16, y + 113, "Unavailable", 23, ink, end);
            }
            if editing {
                self.buttons.push((rect, Action::Tile(index)));
            }
        }
    }

    fn dashboard_controls(&mut self, view: &View<'_>, p: &Palette, pages: usize) {
        let installed = &view.settings.installed;
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
            let default_name = format!("{} / {}", self.dashboard_page + 1, pages);
            let name = view
                .settings
                .dashboard_names
                .get(self.dashboard_page)
                .filter(|name| !name.is_empty())
                .map_or(default_name.as_str(), String::as_str);
            if self.editing {
                self.button(
                    Rect(764, 505, 164, 43),
                    name,
                    p,
                    false,
                    Action::NameDashboard(self.dashboard_page),
                );
            } else {
                self.raw_text(770, 534, name, 18, p.muted, 925);
            }
            if self.editing {
                if let Some(tile) = self.selected_tile {
                    if view.error.is_none() {
                        self.text(
                            32,
                            581,
                            "Choose a destination. You can change pages.",
                            16,
                            p.muted,
                            565,
                        );
                    }
                    if installed.len() < 32 {
                        self.button(
                            Rect(584, 553, 190, 40),
                            "Duplicate tile",
                            p,
                            false,
                            Action::DuplicateTile(tile),
                        );
                    }
                } else if view.error.is_none() {
                    self.text(
                        32,
                        581,
                        "Drag a tile to an edge or tap to move.",
                        16,
                        p.muted,
                        565,
                    );
                }
                self.button(
                    Rect(802, 553, 190, 40),
                    "Reset layout",
                    p,
                    false,
                    Action::ConfirmResetDashboard,
                );
            }
        }
    }

    fn dashboard_name_editor(&mut self, p: &Palette) {
        let Some(entry) = self.repository.take() else {
            return;
        };
        self.panel(Rect(32, 100, 960, 448), p.card, 26);
        self.text(56, 148, "Dashboard page name", 30, p.text, 960);
        self.text(540, 148, "Up to 18 characters", 18, p.muted, 950);
        self.raw_text(56, 199, &entry.text, 25, p.text, 960);
        self.keyboard(entry.symbols, entry.shift, p);
        self.button(
            Rect(56, 473, 212, 52),
            "Cancel",
            p,
            false,
            Action::CloseRepository,
        );
        self.button(
            Rect(752, 473, 215, 52),
            "Save",
            p,
            true,
            Action::SaveDashboardName(self.dashboard_page),
        );
        self.repository = Some(entry);
    }

    fn energy(&mut self, view: &View<'_>, p: &Palette) {
        use crate::energy::{Meter, Period};
        let (meter, period) = self.energy;
        self.panel(Rect(32, 100, 960, 448), p.card, 26);
        for (index, choice) in Meter::ALL.into_iter().enumerate() {
            self.button(
                Rect(54 + 184 * index, 117, 172, 48),
                choice.title(),
                p,
                choice == meter,
                Action::Energy(choice, period),
            );
        }
        for (index, choice) in Period::ALL.into_iter().enumerate() {
            self.button(
                Rect(54 + 184 * index, 177, 172, 43),
                choice.title(),
                p,
                choice == period,
                Action::Energy(meter, choice),
            );
        }
        self.button(
            Rect(754, 231, 215, 45),
            "Energy rates",
            p,
            false,
            Action::EnergyRates,
        );
        let chart = match view.energy {
            Some(Ok(chart)) => chart,
            Some(Err(error)) => {
                self.text(56, 281, error, 22, p.muted, 963);
                return;
            }
            None => {
                self.text(56, 281, "Loading meter history…", 22, p.muted, 963);
                return;
            }
        };
        let values = || chart.bars.iter().copied().flatten();
        if values().next().is_none() {
            self.text(
                56,
                281,
                "No meter readings in this period.",
                22,
                p.muted,
                963,
            );
            return;
        }
        let min = values().fold(0.0, f64::min);
        let max = values().fold(0.0, f64::max);
        let scale = (max - min).max(1e-6);
        let top = 303;
        let bottom = 490;
        let plot_y =
            |value: f64| top + ((max - value) / scale * (bottom - top) as f64).round() as usize;
        let zero = if min == max { bottom } else { plot_y(0.0) };
        let left = 161;
        let width = 797;
        self.panel(Rect(left, zero, width, 1), p.muted, 0);
        for (index, value) in chart.bars.iter().enumerate() {
            let x = left + index * width / chart.bars.len();
            let next = left + (index + 1) * width / chart.bars.len();
            if let Some(value) = value {
                let y = if *value == 0.0 { zero } else { plot_y(*value) };
                self.panel(
                    Rect(
                        x + 2,
                        y.min(zero),
                        (next - x).saturating_sub(4).max(1),
                        y.abs_diff(zero).max(2),
                    ),
                    p.accent,
                    0,
                );
            }
        }
        self.text(
            56,
            318,
            &format!("{max:.1} {}", chart.unit),
            17,
            p.muted,
            155,
        );
        self.text(
            56,
            494,
            &format!("{min:.1} {}", chart.unit),
            17,
            p.muted,
            155,
        );
        let summary = chart.total.map_or_else(
            || self.language.translate("Average per interval").to_owned(),
            |total| {
                format!(
                    "{} {total:.2} {}",
                    self.language.translate("Recorded total"),
                    chart.unit
                )
            },
        );
        self.text(56, 268, &summary, 22, p.text, 395);
        if let Some(cost) = chart.estimated_cost(meter, view.settings.energy_rates) {
            self.text(
                412,
                268,
                &format!("{} €{cost:.2}", self.language.translate("Estimated cost")),
                21,
                p.text,
                745,
            );
        }
        self.text(
            162,
            521,
            self.language.translate("Earlier"),
            17,
            p.muted,
            480,
        );
        self.text(
            865,
            521,
            self.language.translate("Latest"),
            17,
            p.muted,
            960,
        );
        if chart.partial {
            self.text(492, 521, "Incomplete readings", 17, p.muted, 730);
        }
    }

    fn energy_rates(&mut self, view: &View<'_>, p: &Palette) {
        use crate::energy::Rate;
        self.panel(Rect(32, 100, 960, 448), p.card, 26);
        self.text(56, 148, "Energy rates", 30, p.text, 730);
        self.button(
            Rect(754, 115, 215, 47),
            "Energy",
            p,
            false,
            Action::Energy(self.energy.0, self.energy.1),
        );
        if let Some(rate) = self.energy_rate {
            self.text(56, 200, rate.title(), 24, p.text, 960);
            if let Some(entry) = &self.repository {
                let value = entry.text.clone();
                self.raw_text(56, 240, &value, 25, p.text, 960);
            }
            for (index, key) in "123456789.0".chars().enumerate() {
                self.button(
                    Rect(56 + (index % 6) * 104, 271 + (index / 6) * 67, 94, 58),
                    &key.to_string(),
                    p,
                    false,
                    Action::Key(key),
                );
            }
            self.button(Rect(576, 338, 94, 58), "⌫", p, false, Action::Backspace);
            self.button(
                Rect(56, 462, 215, 51),
                "Cancel",
                p,
                false,
                Action::EnergyRates,
            );
            self.button(
                Rect(754, 462, 215, 51),
                "Save",
                p,
                true,
                Action::SaveEnergyRate(rate),
            );
            self.text(
                56,
                430,
                self.energy_rate_error
                    .unwrap_or("Save an empty field to clear the rate."),
                19,
                p.muted,
                960,
            );
        } else {
            for (index, rate) in Rate::ALL.into_iter().enumerate() {
                let value = rate.value(view.settings.energy_rates);
                let label = value.map_or_else(
                    || self.language.translate("Not set").to_owned(),
                    |value| format!("€{}.{:03}", value / 1000, value % 1000),
                );
                self.text(56, 218 + index * 76, rate.title(), 24, p.text, 720);
                self.button(
                    Rect(754, 183 + index * 76, 215, 55),
                    &label,
                    p,
                    false,
                    Action::EditEnergyRate(rate, value),
                );
            }
        }
    }

    pub fn energy_rate_input(&self) -> Option<&str> {
        self.repository.as_ref().map(|entry| entry.text.as_str())
    }

    pub fn close_energy_rate(&mut self) {
        self.energy_rate = None;
        self.energy_rate_error = None;
        self.repository = None;
    }

    pub fn show_energy_rate_error(&mut self, message: &'static str) {
        self.energy_rate_error = Some(message);
    }

    fn heating_details(&mut self, view: &View<'_>, p: &Palette) {
        self.panel(Rect(32, 100, 960, 448), p.card, 26);
        self.text(56, 150, "Heating and sensors", 30, p.text, 740);
        self.button(
            Rect(775, 115, 190, 47),
            "Refresh",
            p,
            false,
            Action::HeatingDetails,
        );
        if let Some(status) = view.status {
            let boiler = if status.connection == "0" {
                "Boiler offline"
            } else {
                "Boiler connected"
            };
            self.text(56, 197, boiler, 23, p.text, 475);
            if status.connection != "0" {
                let detail = format!(
                    "{}°C · {}%",
                    status.current_internal_boiler_setpoint, status.current_modulation_level
                );
                self.text(487, 197, &detail, 23, p.muted, 965);
                if !matches!(status.ot_comm_error.as_str(), "" | "0") {
                    self.text(740, 197, "OpenTherm error", 21, p.muted, 965);
                }
            }
        }
        self.text(56, 235, "Room sensor", 19, p.muted, 300);
        let room = view.status.map(|status| status.current_temp.as_str());
        self.text(285, 235, &temperature(room), 22, p.text, 473);
        self.text(490, 235, "Humidity", 19, p.muted, 704);
        let humidity =
            crate::telemetry::display(crate::plugins::Metric::RoomHumidity, view.telemetry);
        self.text(705, 235, &humidity, 22, p.text, 965);
        let history = match view.history {
            Some(Ok(history)) => history,
            Some(Err(error)) => {
                self.text(56, 294, error, 22, p.muted, 965);
                return;
            }
            None => {
                self.text(56, 294, "Loading heating history…", 22, p.muted, 965);
                return;
            }
        };
        self.text(56, 283, "Recent temperature history", 21, p.text, 650);
        if let Some(diagnostics) = &history.diagnostics {
            let offset = diagnostics
                .calibration_offset_celsius
                .map(|value| format!("{value:+.1}°C"))
                .unwrap_or_else(|| "—".into());
            let firmware = diagnostics.firmware_version.as_deref().unwrap_or("—");
            let label = self.language.translate("Calibration offset");
            self.text(
                56,
                528,
                &format!("{label} {offset}  ·  Firmware {firmware}"),
                16,
                p.muted,
                965,
            );
        }
        if let Some(raw) = history.raw_room {
            let label = self.language.translate("Last raw sensor");
            self.text(651, 283, &format!("{label} {raw:.1}°C"), 17, p.muted, 960);
        }
        let all = || {
            history
                .room
                .iter()
                .chain(&history.target)
                .copied()
                .flatten()
        };
        let min = all().fold(f64::INFINITY, f64::min);
        let max = all().fold(f64::NEG_INFINITY, f64::max);
        if !min.is_finite() || !max.is_finite() {
            self.text(56, 348, "No heating history", 22, p.muted, 965);
            return;
        }
        let range = (max - min).max(1.0);
        let (left, right, top, bottom) = (137, 963, 311, 489);
        self.panel(Rect(left, top, right - left, 1), p.raised, 0);
        self.panel(Rect(left, bottom, right - left, 1), p.raised, 0);
        self.text(56, 323, &format!("{max:.0}°C"), 16, p.muted, 132);
        self.text(56, 494, &format!("{min:.0}°C"), 16, p.muted, 132);
        for (series, color) in [(&history.target, p.muted), (&history.room, p.accent)] {
            let mut previous = None;
            for (index, reading) in series.iter().enumerate() {
                let point = reading.map(|value| {
                    let x = left + index * (right - left) / (series.len() - 1).max(1);
                    let y = bottom - ((value - min) / range * (bottom - top) as f64) as usize;
                    (x, y)
                });
                if let (Some(from), Some(to)) = (previous, point) {
                    self.segment(from, to, color);
                }
                previous = point;
            }
        }
        self.text(142, 506, "Room temperature", 14, p.accent, 500);
        self.text(574, 506, "Target temperature", 14, p.muted, 960);
    }

    fn segment(&mut self, from: (usize, usize), to: (usize, usize), color: u32) {
        let (x, y) = (from.0 as isize, from.1 as isize);
        let (dx, dy) = (to.0 as isize - x, to.1 as isize - y);
        let steps = dx.abs().max(dy.abs()).max(1);
        for i in 0..=steps {
            let px = (x + dx * i / steps) as usize;
            let py = (y + dy * i / steps) as usize;
            self.pixels[py * WIDTH + px] = color;
            self.pixels[(py + 1) * WIDTH + px] = color;
        }
    }

    fn store(&mut self, view: &View<'_>, p: &Palette) {
        if let Some(entry) = self.repository.take() {
            self.panel(Rect(32, 100, 960, 448), p.card, 26);
            self.text(56, 148, "Add a GitHub catalog", 30, p.text, 960);
            let label = if entry.text.is_empty() {
                "owner/repository or owner/repository@commit"
            } else {
                &entry.text
            };
            self.text(56, 201, label, 24, p.muted, 965);
            self.keyboard(entry.symbols, entry.shift, p);
            self.button(
                Rect(56, 473, 212, 52),
                "Cancel",
                p,
                false,
                Action::CloseRepository,
            );
            self.button(
                Rect(752, 473, 215, 52),
                "Add source",
                p,
                true,
                Action::SyncRepository,
            );
            self.repository = Some(entry);
            return;
        }
        if self.source_list {
            self.store_sources(view, p);
            return;
        }
        self.text(32, 130, "Dashboard widgets", 32, p.text, 992);
        for (row, plugin) in view.plugins.iter().skip(self.offset).take(4).enumerate() {
            let y = 150 + row * 88;
            self.panel(Rect(32, y, 960, 80), p.card, 16);
            self.raw_text(54, y + 33, plugin.title(self.language), 23, p.text, 595);
            if let Some(publisher) = &plugin.publisher {
                let release = plugin
                    .installed_release
                    .as_deref()
                    .unwrap_or(&plugin.release);
                self.raw_text(
                    54,
                    y + 61,
                    &format!("{publisher} · {release}"),
                    17,
                    p.muted,
                    590,
                );
            } else {
                self.text(54, y + 61, &plugin.description, 18, p.muted, 590);
            }
            let index = self.offset + row;
            let installed = view.settings.installed.contains(&plugin.id);
            if installed
                && plugin.artifact.is_some()
                && plugin.installed_release.as_deref() != Some(&plugin.release)
            {
                self.button(
                    Rect(608, y + 11, 150, 58),
                    "Update",
                    p,
                    true,
                    Action::UpdatePlugin(index),
                );
            } else if installed && plugin.previous_release.is_some() {
                self.button(
                    Rect(608, y + 11, 150, 58),
                    "Roll back",
                    p,
                    false,
                    Action::RollbackPlugin(index),
                );
            }
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
        self.button(
            Rect(224, 509, 256, 49),
            "Catalog sources",
            p,
            false,
            Action::CatalogSources,
        );
        self.button(
            Rect(496, 509, 256, 49),
            "Add GitHub source",
            p,
            false,
            Action::RepositoryEntry,
        );
        if let Some(message) = view.catalog_message.filter(|_| view.error.is_none()) {
            self.text(32, 581, message, 19, p.muted, 992);
        }
    }

    fn store_sources(&mut self, view: &View<'_>, p: &Palette) {
        self.text(32, 130, "Catalog sources", 30, p.text, 790);
        self.button(Rect(827, 95, 165, 45), "Store", p, false, Action::Store);
        if let Some(index) = self.remove_source {
            if let Some(source) = view.catalog_sources.get(index) {
                self.panel(Rect(32, 151, 960, 340), p.card, 26);
                self.text(56, 212, "Remove catalog source?", 29, p.text, 967);
                self.raw_text(56, 262, &source.repository, 22, p.muted, 967);
                self.text(
                    56,
                    309,
                    "Its widgets become unavailable until the source is added again.",
                    20,
                    p.muted,
                    967,
                );
                self.button(
                    Rect(56, 409, 190, 55),
                    "Cancel",
                    p,
                    false,
                    Action::CatalogSources,
                );
                self.button(
                    Rect(778, 409, 190, 55),
                    "Remove",
                    p,
                    true,
                    Action::RemoveSource(index),
                );
                return;
            }
        }
        let sources = view.catalog_sources;
        self.source_offset = self
            .source_offset
            .min(sources.len().saturating_sub(1) / 4 * 4);
        if sources.is_empty() {
            self.text(
                54,
                207,
                "No GitHub catalogs have been added.",
                22,
                p.muted,
                960,
            );
        }
        for (row, source) in sources.iter().skip(self.source_offset).take(4).enumerate() {
            let y = 150 + row * 85;
            self.panel(Rect(32, y, 960, 76), p.card, 16);
            self.raw_text(54, y + 31, &source.repository, 23, p.text, 750);
            self.raw_text(54, y + 60, &source.revision, 17, p.muted, 750);
            self.button(
                Rect(770, y + 10, 202, 55),
                "Remove",
                p,
                false,
                Action::ConfirmRemoveSource(self.source_offset + row),
            );
        }
        if self.source_offset > 0 {
            self.button(
                Rect(32, 509, 145, 49),
                "Previous",
                p,
                false,
                Action::CatalogSourcePage(self.source_offset - 4),
            );
        }
        if self.source_offset + 4 < sources.len() {
            self.button(
                Rect(847, 509, 145, 49),
                "Next",
                p,
                false,
                Action::CatalogSourcePage(self.source_offset + 4),
            );
        }
        self.button(
            Rect(224, 509, 256, 49),
            "Refresh official",
            p,
            false,
            Action::RefreshOfficial,
        );
        self.button(
            Rect(496, 509, 256, 49),
            "Add GitHub source",
            p,
            false,
            Action::RepositoryEntry,
        );
        if let Some(message) = view.catalog_message.filter(|_| view.error.is_none()) {
            self.text(32, 581, message, 19, p.muted, 992);
        }
    }

    pub fn source_removed(&mut self) {
        self.remove_source = None;
    }

    fn settings(&mut self, view: &View<'_>, p: &Palette) {
        if self.reset_preferences {
            self.panel(Rect(32, 100, 960, 448), p.card, 26);
            self.text(58, 165, "Reset ToonShift preferences?", 30, p.text, 965);
            self.text(
                58,
                226,
                "Restore default display, dashboard, plugin and energy preferences.",
                22,
                p.muted,
                965,
            );
            self.button(
                Rect(58, 454, 220, 55),
                "Cancel",
                p,
                false,
                Action::CancelResetPreferences,
            );
            self.button(
                Rect(702, 454, 264, 55),
                "Reset preferences",
                p,
                true,
                Action::ResetPreferences,
            );
            return;
        }
        self.panel(Rect(32, 100, 960, 400), p.card, 26);
        self.text(58, 153, "Appearance", 28, p.text, 966);
        self.text(638, 153, "Language", 28, p.text, 966);
        for (index, (label, language)) in [
            ("English", crate::language::Language::English),
            ("Nederlands", crate::language::Language::Dutch),
        ]
        .into_iter()
        .enumerate()
        {
            self.button(
                Rect(638 + index * 164, 173, 156, 60),
                label,
                p,
                view.settings.language == language,
                Action::Language(language),
            );
        }
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
            &format!(
                "{} {:02}:00 – {:02}:00",
                self.language.translate("Automatic day hours"),
                view.settings.day_start,
                view.settings.night_start
            ),
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
        self.button(
            Rect(612, 428, 225, 54),
            "Display",
            p,
            false,
            Action::Display,
        );
        self.button(
            Rect(640, 347, 324, 60),
            "Advanced setup",
            p,
            true,
            Action::Hardware(HardwareAction::Open),
        );
        self.button(
            Rect(32, 511, 325, 48),
            "Export diagnostics",
            p,
            false,
            Action::ExportDiagnostics,
        );
        self.button(
            Rect(669, 511, 323, 48),
            "Reset preferences",
            p,
            false,
            Action::ConfirmResetPreferences,
        );
        if let Some(message) = view.diagnostics_message.filter(|_| view.error.is_none()) {
            self.text(32, 581, message, 18, p.muted, 990);
        }
    }

    fn display_settings(&mut self, view: &View<'_>, p: &Palette) {
        self.panel(Rect(32, 100, 960, 448), p.card, 26);
        self.text(56, 152, "Display", 30, p.text, 960);
        for (night, x, brightness, hour) in [
            (
                false,
                56,
                view.settings.day_brightness,
                view.settings.day_start,
            ),
            (
                true,
                524,
                view.settings.night_brightness,
                view.settings.night_start,
            ),
        ] {
            self.text(
                x,
                203,
                if night {
                    "Night brightness"
                } else {
                    "Day brightness"
                },
                23,
                p.muted,
                x + 420,
            );
            self.button(
                Rect(x, 218, 70, 51),
                "−",
                p,
                false,
                Action::Brightness(night, -5),
            );
            self.text(x + 102, 253, &format!("{brightness}%"), 27, p.text, x + 286);
            self.button(
                Rect(x + 310, 218, 70, 51),
                "+",
                p,
                false,
                Action::Brightness(night, 5),
            );
            self.text(
                x,
                319,
                if night {
                    "Night starts at"
                } else {
                    "Day starts at"
                },
                23,
                p.muted,
                x + 420,
            );
            self.button(
                Rect(x, 336, 70, 51),
                "−",
                p,
                false,
                Action::AppearanceHour(night, -1),
            );
            self.text(x + 102, 371, &format!("{hour:02}:00"), 27, p.text, x + 286);
            self.button(
                Rect(x + 310, 336, 70, 51),
                "+",
                p,
                false,
                Action::AppearanceHour(night, 1),
            );
        }
        self.text(56, 419, "Idle display after", 20, p.muted, 960);
        for (index, (label, minutes)) in [("Never", 0), ("1 min", 1), ("5 min", 5), ("15 min", 15)]
            .into_iter()
            .enumerate()
        {
            self.button(
                Rect(56 + index * 230, 431, 207, 45),
                label,
                p,
                minutes == view.settings.idle_minutes,
                Action::IdleMinutes(minutes),
            );
        }
        let pages = view.settings.dashboard_pages();
        let selected = view
            .settings
            .idle_dashboard_page
            .filter(|page| *page < pages);
        self.text(56, 519, "Idle content", 20, p.muted, 300);
        self.button(
            Rect(300, 487, 175, 45),
            "Clock",
            p,
            selected.is_none(),
            Action::IdleDashboard(None),
        );
        if pages > 0 {
            let next = selected.map_or(0, |page| (page + 1) % pages);
            let label = selected.map_or_else(
                || self.language.translate("Show dashboard").to_owned(),
                |page| {
                    format!(
                        "{} {} / {}",
                        self.language.translate("Dashboard page"),
                        page + 1,
                        pages
                    )
                },
            );
            self.button(
                Rect(488, 487, 270, 45),
                &label,
                p,
                selected.is_some(),
                Action::IdleDashboard(Some(next)),
            );
        }
        self.button(
            Rect(773, 487, 180, 45),
            "Wide tiles",
            p,
            view.settings.wide_tiles,
            Action::WideTiles(!view.settings.wide_tiles),
        );
    }
}

#[cfg(test)]
mod dashboard_tests {
    use super::*;

    #[test]
    fn drag_crosses_into_a_partial_page_and_back() {
        let mut ui = Ui::new();
        ui.editing = true;
        ui.buttons.push((Rect(794, 303, 198, 192), Action::Tile(3)));
        assert!(matches!(
            ui.gesture((870, 390), (1015, 390), 5, 4),
            Some(Action::MoveTile(3, 4))
        ));
        assert_eq!(ui.dashboard_page, 1);
        ui.buttons.clear();
        ui.buttons.push((Rect(584, 100, 198, 192), Action::Tile(4)));
        assert!(ui.gesture((645, 150), (1015, 150), 5, 4).is_none());
        assert!(matches!(
            ui.gesture((645, 150), (570, 150), 5, 4),
            Some(Action::MoveTile(4, 0))
        ));
        assert_eq!(ui.dashboard_page, 0);
    }

    #[test]
    fn wide_tile_pages_accept_old_names_and_use_two_slots() {
        let mut settings: crate::plugins::Settings = serde_json::from_str(
            r#"{"dashboard_names":["Energy","","","","","","",""],"installed":["clock","boiler","power"]}"#,
        ).unwrap();
        assert_eq!(settings.dashboard_pages(), 1);
        assert_eq!(settings.dashboard_names[0], "Energy");
        assert_eq!(settings.idle_dashboard_page, None);
        settings.wide_tiles = true;
        assert_eq!(settings.dashboard_pages(), 2);
        let mut ui = Ui::new();
        ui.editing = true;
        ui.buttons.push((Rect(584, 303, 408, 192), Action::Tile(1)));
        assert!(matches!(
            ui.gesture((880, 380), (1015, 380), 3, settings.tiles_per_page()),
            Some(Action::MoveTile(1, 2))
        ));
    }
}

pub struct View<'a> {
    pub vacations: Option<&'a Result<Vec<crate::vacation::Vacation>, String>>,
    pub vacation_busy: bool,
    pub catalog_sources: &'a [crate::plugins::CatalogSource],
    pub diagnostics_message: Option<&'a str>,
    pub hardware: Option<&'a crate::advanced::State>,
    pub settings: &'a Settings,
    pub plugins: &'a [Plugin],
    pub status: Option<&'a Status>,
    pub pending: bool,
    pub error: Option<&'a str>,
    pub time: &'a str,
    pub hour: u8,
    pub root: &'a Path,
    pub schedule: Option<&'a Result<Vec<crate::schedule::Slot>, String>>,
    pub presets: Option<&'a Result<Vec<crate::presets::Preset>, String>>,
    pub networks: Option<&'a Result<Vec<NetworkRow>, String>>,
    pub wifi_connection: Option<&'a std::io::Result<Option<crate::network::Connection>>>,
    pub ethernet: Option<&'a crate::network::Ethernet>,
    pub network_message: Option<&'a str>,
    pub catalog_message: Option<&'a str>,
    pub readings: &'a HashMap<String, crate::feed::Reading>,
    pub telemetry: &'a crate::telemetry::Readings,
    pub energy: Option<&'a Result<crate::energy::Chart, String>>,
    pub history: Option<&'a Result<crate::thermostat::History, String>>,
    pub ambient: bool,
    pub location_results: Option<&'a Result<Vec<crate::plugins::Place>, String>>,
}

struct VacationEditor {
    id: Option<crate::vacation::Id>,
    dates: [String; 2],
    end: bool,
    error: Option<&'static str>,
}

fn vacation_label(seconds: i32) -> String {
    #[cfg(target_os = "linux")]
    if let Some([year, month, day, hour, minute]) = crate::device::vacation_date(seconds) {
        return format!("{day:02}-{month:02}-{year} {hour:02}:{minute:02}");
    }
    seconds.to_string()
}

#[derive(serde::Deserialize)]
pub struct NetworkRow {
    #[serde(rename = "ESSID")]
    pub name: String,
    #[serde(rename = "Signal", default)]
    pub signal: String,
    #[serde(rename = "Enc", default)]
    pub encryption: String,
    #[serde(rename = "Auth", default)]
    pub auth: String,
}

struct WifiForm {
    static_ipv4: bool,
    address_fields: [String; 4],
    address_field: usize,
    address_error: bool,
    join: crate::network::Join,
    hidden: bool,
    password_field: bool,
    shift: bool,
    symbols: bool,
}

#[derive(Default)]
struct TextEntry {
    text: String,
    symbols: bool,
    shift: bool,
}

fn temperature(value: Option<&str>) -> String {
    value
        .and_then(|v| v.parse::<i32>().ok())
        .map_or_else(|| "—".into(), |v| format!("{:.1}°C", f64::from(v) / 100.0))
}

fn plugin_value(plugin: &Plugin, view: &View<'_>) -> String {
    match plugin.source {
        Source::BoxTalk => plugin.metric.map_or_else(
            || "Unavailable".into(),
            |metric| crate::telemetry::display(metric, view.telemetry),
        ),
        Source::HttpsJson | Source::MeterArchive | Source::Wasm => view
            .readings
            .get(&plugin.id)
            .map_or_else(|| "Loading…".into(), |reading| reading.value.clone()),
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
