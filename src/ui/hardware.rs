use super::{Action, Palette, Rect, TextEntry, Ui, View};
use crate::advanced::{BoilerChange, HotWater, Job, PairMode, PairRequest, PairStep, State, Tab};

#[derive(Clone, Copy)]
pub enum HardwareAction {
    Open,
    Tab(Tab),
    Refresh,
    Protocol(bool),
    Confirm,
    Cancel,
    WaterDelta(i32),
    WaterToggle,
    WaterSave,
    Pair(PairMode),
    StartPair,
    StopPair,
    Key(usize),
    Csa,
    Digit(char),
    Erase,
    ContinuePair,
    Page(usize),
    Device(usize),
    Rename,
    SaveName,
    Switch(bool),
}

pub struct Controls {
    tab: Tab,
    page: usize,
    node: Option<usize>,
    protocol: Option<bool>,
    water: Option<HotWater>,
    pair_confirm: Option<PairMode>,
    keys: u8,
    csa: bool,
    pin: String,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            tab: Tab::Boiler,
            page: 0,
            node: None,
            protocol: None,
            water: None,
            pair_confirm: None,
            keys: 0,
            csa: false,
            pin: String::new(),
        }
    }
}

impl Ui {
    pub fn hardware_action(&mut self, state: &mut State, action: HardwareAction) {
        use HardwareAction::*;
        match action {
            Open => {
                self.page = super::Page::Advanced;
                self.repository = None;
                self.hardware = Controls::default();
                state.submit(Job::Refresh);
            }
            Tab(tab) => {
                self.hardware.tab = tab;
                self.hardware.page = 0;
                self.hardware.node = None;
                self.repository = None;
            }
            Refresh => {
                self.hardware.water = None;
                self.hardware.node = None;
                state.submit(Job::Refresh);
            }
            Protocol(mode) => self.hardware.protocol = Some(mode),
            Confirm => {
                if let Some(mode) = self.hardware.protocol.take() {
                    state.submit(Job::Boiler(BoilerChange::Protocol(mode)));
                }
            }
            Cancel => {
                self.hardware.protocol = None;
                self.hardware.pair_confirm = None;
                self.hardware.water = None;
                self.hardware.node = None;
                self.repository = None;
            }
            WaterDelta(delta) => {
                let Some(water) = self.water(state) else {
                    return;
                };
                self.hardware.water = Some(HotWater {
                    target: (water.target + delta).clamp(water.min, water.max),
                    ..water
                });
            }
            WaterToggle => {
                let Some(water) = self.water(state) else {
                    return;
                };
                self.hardware.water = Some(HotWater {
                    enabled: !water.enabled,
                    ..water
                });
            }
            WaterSave => {
                if let Some(water) = self.hardware.water.take() {
                    state.submit(Job::Boiler(BoilerChange::HotWater(water)));
                }
            }
            Pair(mode) => self.hardware.pair_confirm = Some(mode),
            StartPair => {
                if let Some(mode) = self.hardware.pair_confirm.take() {
                    self.hardware.keys = 0;
                    self.hardware.csa = false;
                    self.hardware.pin.clear();
                    state.submit(Job::Pair(mode, PairRequest::Start));
                }
            }
            StopPair => {
                if let Some(mode) = state.pairing {
                    state.submit(Job::Pair(mode, PairRequest::Stop));
                }
            }
            Key(index) => self.hardware.keys ^= 1 << index,
            Csa => self.hardware.csa = !self.hardware.csa,
            Digit(digit) => {
                if matches!(state.step, Some(PairStep::Pin { digits, .. }) if self.hardware.pin.len() < digits)
                {
                    self.hardware.pin.push(digit);
                }
            }
            Erase => {
                self.hardware.pin.pop();
            }
            ContinuePair => {
                let Some(mode) = state.pairing else {
                    return;
                };
                let request = match &state.step {
                    Some(PairStep::Keys(keys, csa)) if self.hardware.keys != 0 => {
                        PairRequest::Grant {
                            keys: keys
                                .iter()
                                .enumerate()
                                .filter(|(i, _)| self.hardware.keys & (1 << i) != 0)
                                .map(|(_, key)| key.clone())
                                .collect(),
                            csa: *csa && self.hardware.csa,
                        }
                    }
                    Some(PairStep::Pin { digits, .. }) if self.hardware.pin.len() == *digits => {
                        PairRequest::Pin(std::mem::take(&mut self.hardware.pin))
                    }
                    Some(PairStep::ControllerPin(_)) => PairRequest::ConfirmController,
                    _ => return,
                };
                self.hardware.keys = 0;
                state.submit(Job::Pair(mode, request));
            }
            Page(page) => self.hardware.page = page,
            Device(index) => self.hardware.node = Some(index),
            Rename => self.repository = Some(TextEntry::default()),
            SaveName => {
                if let Some(name) = self.take_repository() {
                    if let Some(node) = self
                        .hardware
                        .node
                        .and_then(|index| state.devices.as_ref()?.as_ref().ok()?.nodes.get(index))
                    {
                        state.submit(Job::Rename(node.clone(), name));
                    }
                }
            }
            Switch(on) => {
                if let Some(node) = self
                    .hardware
                    .node
                    .and_then(|index| state.devices.as_ref()?.as_ref().ok()?.nodes.get(index))
                {
                    state.submit(Job::Switch(node.clone(), on));
                }
            }
        }
    }

    fn water(&self, state: &State) -> Option<HotWater> {
        self.hardware
            .water
            .or_else(|| state.boiler.as_ref()?.as_ref().ok()?.hot_water)
    }

    fn hardware_button(
        &mut self,
        rect: Rect,
        label: &str,
        p: &Palette,
        active: bool,
        action: HardwareAction,
    ) {
        self.button(rect, label, p, active, Action::Hardware(action));
    }

    pub(super) fn hardware_screen(&mut self, view: &View<'_>, p: &Palette) {
        use HardwareAction as A;
        let Some(state) = view.hardware else {
            return;
        };
        self.panel(Rect(32, 100, 960, 448), p.card, 26);
        if state.pairing.is_some() {
            self.pairing_screen(state, p);
            return;
        }
        if let Some(mode) = self.hardware.protocol {
            self.text(56, 151, "Change boiler protocol", 29, p.text, 960);
            self.text(
                56,
                220,
                if mode {
                    "Use OpenTherm for this boiler?"
                } else {
                    "Use the on/off relay for this boiler?"
                },
                24,
                p.text,
                960,
            );
            self.text(
                56,
                264,
                "Choose the protocol specified for your boiler and adapter.",
                20,
                p.muted,
                960,
            );
            self.hardware_button(Rect(56, 464, 200, 55), "Cancel", p, false, A::Cancel);
            self.hardware_button(
                Rect(760, 464, 200, 55),
                "Apply protocol",
                p,
                true,
                A::Confirm,
            );
            return;
        }
        if let Some(mode) = self.hardware.pair_confirm {
            self.text(
                56,
                150,
                if mode == PairMode::Include {
                    "Add Z-Wave device"
                } else {
                    "Remove Z-Wave device"
                },
                29,
                p.text,
                960,
            );
            self.text(
                56,
                221,
                "Start, then press the setup button on the device.",
                23,
                p.muted,
                960,
            );
            self.hardware_button(Rect(56, 464, 200, 55), "Cancel", p, false, A::Cancel);
            self.hardware_button(
                Rect(760, 464, 200, 55),
                "Start setup",
                p,
                true,
                A::StartPair,
            );
            return;
        }
        for (i, (tab, title)) in [
            (Tab::Boiler, "Boiler"),
            (Tab::Meters, "Meters"),
            (Tab::Devices, "Z-Wave"),
        ]
        .into_iter()
        .enumerate()
        {
            self.hardware_button(
                Rect(56 + i * 207, 117, 193, 49),
                title,
                p,
                self.hardware.tab == tab,
                A::Tab(tab),
            );
        }
        if !state.busy {
            self.hardware_button(Rect(780, 117, 187, 49), "Refresh", p, false, A::Refresh);
        }
        match self.hardware.tab {
            Tab::Boiler => self.boiler_setup(state, p),
            Tab::Meters | Tab::Devices => self.device_setup(state, view.settings, p),
        }
        if view.error.is_none() {
            if let Some(message) = state
                .message
                .as_deref()
                .or(state.busy.then_some("Working…"))
            {
                self.text(32, 580, message, 19, p.muted, 992);
            }
        }
    }

    fn boiler_setup(&mut self, state: &State, p: &Palette) {
        use HardwareAction as A;
        let boiler = match &state.boiler {
            Some(Ok(boiler)) => boiler,
            Some(Err(error)) => {
                self.text(56, 225, error, 23, p.muted, 960);
                return;
            }
            None => {
                self.text(56, 225, "Reading boiler settings…", 23, p.muted, 960);
                return;
            }
        };
        self.text(
            56,
            216,
            if boiler.connected {
                "Boiler connected"
            } else {
                "Boiler link offline"
            },
            24,
            p.text,
            960,
        );
        self.text(56, 258, "Communication protocol", 20, p.muted, 960);
        if state.busy {
            return;
        }
        self.hardware_button(
            Rect(56, 271, 270, 58),
            "OpenTherm",
            p,
            boiler.open_therm,
            A::Protocol(true),
        );
        self.hardware_button(
            Rect(344, 271, 270, 58),
            "On/off relay",
            p,
            !boiler.open_therm,
            A::Protocol(false),
        );
        if let Some(water) = self
            .water(state)
            .filter(|_| boiler.connected && boiler.open_therm)
        {
            self.text(56, 380, "Domestic hot water", 22, p.text, 960);
            self.hardware_button(
                Rect(56, 406, 210, 57),
                if water.enabled {
                    "Hot water on"
                } else {
                    "Hot water off"
                },
                p,
                water.enabled,
                A::WaterToggle,
            );
            self.hardware_button(Rect(300, 406, 69, 57), "−", p, false, A::WaterDelta(-1));
            self.text(398, 443, &format!("{}°C", water.target), 26, p.text, 505);
            self.hardware_button(Rect(520, 406, 69, 57), "+", p, false, A::WaterDelta(1));
            if self.hardware.water.is_some() {
                self.hardware_button(Rect(742, 406, 220, 57), "Save", p, true, A::WaterSave);
            }
        } else {
            self.text(
                56,
                394,
                "Hot-water controls require a connected OpenTherm boiler.",
                21,
                p.muted,
                960,
            );
            if let Some(water) = boiler.hot_water {
                self.text(
                    56,
                    436,
                    &format!(
                        "{} · {}°C",
                        self.language.translate("Saved hot-water target"),
                        water.target
                    ),
                    19,
                    p.muted,
                    960,
                );
            }
        }
    }

    fn device_setup(&mut self, state: &State, settings: &crate::plugins::Settings, p: &Palette) {
        use HardwareAction as A;
        let inventory = match &state.devices {
            Some(Ok(inventory)) => inventory,
            Some(Err(error)) => {
                self.text(56, 225, error, 23, p.muted, 960);
                return;
            }
            None => {
                self.text(56, 225, "Reading device inventory…", 23, p.muted, 960);
                return;
            }
        };
        if let Some(node) = self
            .hardware
            .node
            .and_then(|index| inventory.nodes.get(index))
        {
            if let Some(entry) = self.repository.take() {
                self.raw_text(56, 213, &entry.text, 23, p.text, 960);
                self.keyboard(entry.symbols, entry.shift, p);
                self.repository = Some(entry);
                self.hardware_button(Rect(56, 477, 200, 50), "Cancel", p, false, A::Cancel);
                self.hardware_button(Rect(760, 477, 200, 50), "Save name", p, true, A::SaveName);
                return;
            }
            self.raw_text(56, 222, &node.name, 28, p.text, 960);
            self.raw_text(56, 263, &node.kind, 19, p.muted, 960);
            if node.meter {
                for (index, label) in ["Electricity", "Gas", "Water"].into_iter().enumerate() {
                    let channels: Vec<_> = node
                        .channels
                        .iter()
                        .filter(|channel| channel.name == label)
                        .collect();
                    let unit = match label {
                        "Gas" => "L/h",
                        "Water" => "L/min",
                        _ => "W",
                    };
                    let metric = match label {
                        "Electricity" => Some(crate::plugins::Metric::Electricity),
                        "Gas" => Some(crate::plugins::Metric::Gas),
                        _ => None,
                    };
                    let selected = metric.and_then(|metric| settings.meter_sources.get(&metric));
                    let flow = channels.iter().find_map(|channel| channel.flow);
                    let summary = if channels.is_empty() {
                        "No channel reported".to_owned()
                    } else if let Some(flow) = flow {
                        format!("{flow:.1} {unit}")
                    } else if channels.iter().any(|channel| channel.total.is_some()) {
                        "Counter available".to_owned()
                    } else {
                        "Channel present · no reading".to_owned()
                    };
                    self.text(56, 306 + index * 45, label, 22, p.text, 328);
                    self.text(342, 306 + index * 45, &summary, 21, p.muted, 700);
                    if let Some(metric) = metric {
                        let row = 277 + index * 45;
                        if node
                            .channels
                            .iter()
                            .any(|channel| channel.name == label && selected == Some(&channel.id))
                        {
                            self.button(
                                Rect(708, row, 255, 39),
                                "Use firmware default",
                                p,
                                true,
                                Action::AutomaticMeter(metric),
                            );
                        } else if let Some(channel) = channels
                            .iter()
                            .find(|channel| channel.flow.is_some())
                            .or_else(|| channels.first())
                        {
                            if let Some(global_index) = inventory
                                .nodes
                                .iter()
                                .flat_map(|node| &node.channels)
                                .position(|entry| entry.id == channel.id)
                            {
                                self.button(
                                    Rect(708, row, 255, 39),
                                    "Use this adapter",
                                    p,
                                    false,
                                    Action::MeterSource(metric, global_index),
                                );
                            }
                        }
                    }
                }
                if !state.busy {
                    self.hardware_button(Rect(760, 477, 200, 50), "Rename", p, false, A::Rename);
                }
                self.hardware_button(Rect(56, 477, 200, 50), "Back", p, false, A::Cancel);
                return;
            }
            if !state.busy {
                self.hardware_button(Rect(56, 301, 210, 55), "Rename", p, false, A::Rename);
                if node.switch {
                    self.hardware_button(
                        Rect(292, 301, 170, 55),
                        "Switch on",
                        p,
                        true,
                        A::Switch(true),
                    );
                    self.hardware_button(
                        Rect(480, 301, 170, 55),
                        "Switch off",
                        p,
                        false,
                        A::Switch(false),
                    );
                }
            }
            self.hardware_button(Rect(56, 477, 200, 50), "Back", p, false, A::Cancel);
            return;
        }
        let meters = self.hardware.tab == Tab::Meters;
        if let Some(count) = inventory.p1_sources.filter(|_| meters) {
            self.text(
                56,
                207,
                &format!(
                    "{} {}",
                    count,
                    self.language.translate("configured meter sources")
                ),
                20,
                p.muted,
                960,
            );
        }
        let nodes: Vec<_> = inventory
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| !meters || node.meter)
            .collect();
        let page = self.hardware.page.min(nodes.len().saturating_sub(1) / 4);
        if nodes.is_empty() {
            self.text(
                56,
                260,
                if inventory.zwave_available {
                    "No matching devices"
                } else {
                    "Z-Wave inventory unavailable"
                },
                23,
                p.muted,
                960,
            );
        }
        for (row, &(index, node)) in nodes.iter().skip(page * 4).take(4).enumerate() {
            let y = 223 + 54 * row;
            self.raw_text(56, y + 29, &node.name, 22, p.text, 505);
            self.text(
                510,
                y + 29,
                match node.connected {
                    Some(true) => "Connected",
                    Some(false) => "Offline",
                    None => "Registered",
                },
                19,
                p.muted,
                755,
            );
            self.hardware_button(Rect(784, y, 180, 43), "Details", p, false, A::Device(index));
        }
        if !state.busy {
            if inventory.zwave_available {
                self.hardware_button(
                    Rect(56, 477, 220, 50),
                    "Add Z-Wave device",
                    p,
                    true,
                    A::Pair(PairMode::Include),
                );
            }
            if meters {
                self.button(
                    Rect(294, 477, 310, 50),
                    if self.stock_confirm {
                        "Open stock for 10 min"
                    } else {
                        "Stock meter installer"
                    },
                    p,
                    false,
                    Action::StockMeterSession,
                );
                if self.stock_confirm {
                    self.text(
                        56,
                        453,
                        "To return sooner, tap the top-left corner five times.",
                        18,
                        p.muted,
                        960,
                    );
                }
            } else if inventory.zwave_available {
                self.hardware_button(
                    Rect(294, 477, 238, 50),
                    "Remove Z-Wave device",
                    p,
                    false,
                    A::Pair(PairMode::Exclude),
                );
            }
        }
        if page > 0 {
            self.hardware_button(
                Rect(620, 477, 155, 50),
                "Previous",
                p,
                false,
                A::Page(page - 1),
            );
        }
        if (page + 1) * 4 < nodes.len() {
            self.hardware_button(Rect(792, 477, 172, 50), "Next", p, false, A::Page(page + 1));
        }
    }

    fn pairing_screen(&mut self, state: &State, p: &Palette) {
        use HardwareAction as A;
        self.text(56, 151, "Z-Wave setup", 29, p.text, 960);
        match &state.step {
            Some(PairStep::Keys(keys, csa)) => {
                self.text(
                    56,
                    207,
                    "Choose which security keys to grant.",
                    23,
                    p.text,
                    960,
                );
                for (i, key) in keys.iter().enumerate() {
                    self.hardware_button(
                        Rect(56 + i % 3 * 302, 227 + i / 3 * 59, 285, 50),
                        key,
                        p,
                        self.hardware.keys & (1 << i) != 0,
                        A::Key(i),
                    );
                }
                if *csa {
                    self.hardware_button(
                        Rect(56, 354, 285, 50),
                        "Controller PIN mode",
                        p,
                        self.hardware.csa,
                        A::Csa,
                    );
                }
            }
            Some(PairStep::Pin { digits, dsk }) => {
                self.text(
                    56,
                    205,
                    &format!(
                        "{} {digits} · {dsk}",
                        self.language.translate("Enter device PIN digits")
                    ),
                    21,
                    p.muted,
                    963,
                );
                self.text(
                    56,
                    255,
                    &"•".repeat(self.hardware.pin.len()),
                    30,
                    p.text,
                    960,
                );
                for (i, digit) in "1234567890".chars().enumerate() {
                    self.hardware_button(
                        Rect(56 + i * 90, 280, 76, 61),
                        &digit.to_string(),
                        p,
                        false,
                        A::Digit(digit),
                    );
                }
                self.hardware_button(Rect(56, 360, 210, 50), "Backspace", p, false, A::Erase);
            }
            Some(PairStep::ControllerPin(dsk)) => {
                self.text(
                    56,
                    211,
                    "Enter the controller PIN on the Z-Wave device.",
                    23,
                    p.muted,
                    965,
                );
                self.raw_text(56, 264, dsk, 23, p.text, 966);
            }
            _ => self.text(
                56,
                222,
                "Waiting for the controller. Press the setup button on the device.",
                22,
                p.muted,
                960,
            ),
        }
        self.hardware_button(
            Rect(56, 466, 202, 54),
            "Cancel setup",
            p,
            false,
            A::StopPair,
        );
        if matches!(
            state.step,
            Some(PairStep::Keys(..) | PairStep::Pin { .. } | PairStep::ControllerPin(_))
        ) {
            self.hardware_button(
                Rect(760, 466, 202, 54),
                "Continue",
                p,
                true,
                A::ContinuePair,
            );
        }
    }
}
