use super::{Action, Palette, Rect, Ui, View, WifiForm};

impl Ui {
    pub(super) fn network(&mut self, view: &View<'_>, p: &Palette) {
        self.panel(Rect(32, 100, 960, 462), p.card, 26);
        if let Some(stage) = view.network_stage {
            use crate::network::WifiStage;
            let headline = match stage {
                WifiStage::Saving => "Saving Wi-Fi settings…",
                WifiStage::Joining => "Joining the network…",
                WifiStage::GettingAddress => "Connected to Wi-Fi. Getting an IP address…",
                WifiStage::Reconnecting => "Reconnecting to saved Wi-Fi…",
                WifiStage::Restoring => "Restoring the previous Wi-Fi…",
            };
            self.text(56, 151, "Wi-Fi setup", 30, p.text, 960);
            if let Some(name) = self.wifi.as_ref().map(|form| form.join.name.clone()) {
                self.raw_text(56, 197, &name, 25, p.muted, 960);
            }
            self.panel(Rect(56, 230, 912, 122), p.raised, 16);
            self.panel(Rect(76, 258, 10, 10), p.accent, 5);
            self.text(103, 273, headline, 28, p.text, 945);
            self.text(
                103,
                319,
                "Checking the network. Please wait.",
                21,
                p.muted,
                945,
            );
            let step = match stage {
                WifiStage::Saving => Some(0),
                WifiStage::Joining => Some(1),
                WifiStage::GettingAddress => Some(2),
                WifiStage::Reconnecting | WifiStage::Restoring => None,
            };
            if let Some(step) = step {
                for (index, label) in ["Save settings", "Join Wi-Fi", "Get IP address"]
                    .into_iter()
                    .enumerate()
                {
                    let x = 56 + index * 308;
                    self.panel(
                        Rect(x, 388, 296, 5),
                        if index <= step { p.accent } else { p.raised },
                        2,
                    );
                    self.text(
                        x,
                        425,
                        label,
                        19,
                        if index == step { p.text } else { p.muted },
                        x + 296,
                    );
                }
            }
            return;
        }
        if let Some(form) = self.wifi.take() {
            self.wifi_keyboard(&form, p);
            self.wifi = Some(form);
            return;
        }

        self.text(56, 149, "Wi-Fi", 32, p.text, 510);
        if view.networks.is_some() {
            self.button(
                Rect(558, 117, 223, 48),
                "Hidden network",
                p,
                false,
                Action::HiddenNetwork,
            );
            self.button(
                Rect(791, 117, 177, 48),
                "Scan again",
                p,
                false,
                Action::Network,
            );
        }

        self.panel(Rect(56, 176, 912, 78), p.raised, 15);
        match view.wifi_connection {
            Some(Ok(Some(connection))) => {
                let state = if connection.address.is_some() {
                    "Connected"
                } else {
                    "Waiting for IP address"
                };
                self.text(72, 202, state, 17, p.accent, 610);
                self.raw_text(72, 236, &connection.ssid, 24, p.text, 642);
                if let Some(address) = connection.address {
                    let signal = connection
                        .rssi
                        .map_or_else(String::new, |rssi| format!("  ·  {rssi} dBm"));
                    self.raw_text(657, 235, &format!("{address}{signal}"), 19, p.muted, 946);
                }
            }
            Some(Ok(None)) => self.text(72, 224, "Wi-Fi disconnected", 22, p.text, 946),
            Some(Err(_)) => self.text(
                72,
                224,
                "Wi-Fi connection status unavailable",
                22,
                p.text,
                946,
            ),
            None => self.text(72, 224, "Checking current connection…", 22, p.muted, 946),
        }

        let networks: &[_] = match view.networks {
            Some(Ok(networks)) if !networks.is_empty() => networks,
            result => {
                let message = match result {
                    Some(Ok(_)) => "No nearby networks found",
                    Some(Err(_)) => "Wi-Fi scan failed. Tap Scan again to retry.",
                    None => "Scanning for networks…",
                };
                self.text(56, 299, message, 22, p.muted, 968);
                &[]
            }
        };
        let connected = view
            .wifi_connection
            .and_then(|result| result.as_ref().ok())
            .and_then(Option::as_ref);
        let page = self.network_page.min(networks.len().saturating_sub(1) / 4);
        for (row, network) in networks.iter().skip(page * 4).take(4).enumerate() {
            let y = 265 + row * 48;
            let rect = Rect(56, y, 912, 43);
            self.panel(rect, p.raised, 11);
            self.raw_text(72, y + 28, &network.name, 22, p.text, 570);
            let security = if matches!(network.auth.to_ascii_uppercase().as_str(), "NONE" | "OPEN")
            {
                self.language.translate("Open network")
            } else {
                network.auth.as_str()
            };
            self.raw_text(592, y + 28, security, 17, p.muted, 733);
            self.raw_text(741, y + 28, &network.signal, 17, p.muted, 817);
            let joined = connected.is_some_and(|connection| {
                connection.ssid == network.name && connection.address.is_some()
            });
            self.text(
                825,
                y + 28,
                if joined { "Connected" } else { "Select  ›" },
                17,
                p.accent,
                953,
            );
            self.buttons
                .push((rect, Action::ChooseNetwork(page * 4 + row)));
        }
        if page > 0 {
            self.button(
                Rect(56, 463, 144, 44),
                "Previous",
                p,
                false,
                Action::NetworkPage(page - 1),
            );
        }
        if (page + 1) * 4 < networks.len() {
            self.button(
                Rect(824, 463, 144, 44),
                "Next",
                p,
                false,
                Action::NetworkPage(page + 1),
            );
        }
        if view.networks.is_some() {
            self.button(
                Rect(334, 463, 356, 44),
                "Reconnect saved network",
                p,
                false,
                Action::ReconnectWifi,
            );
        }
        if let Some(wired) = view.ethernet {
            let state = match wired.carrier {
                Some(true) => "Ethernet connected",
                Some(false) => "Ethernet disconnected",
                None => "Ethernet link unknown",
            };
            let detail = wired
                .address
                .map_or_else(String::new, |ip| format!(" · {ip}"));
            self.raw_text(
                56,
                539,
                &format!("{}{detail}", self.language.translate(state)),
                18,
                p.muted,
                945,
            );
        }
        if let Some(result) = view.network_result {
            if result.is_ok() {
                self.button(
                    Rect(744, 514, 224, 41),
                    "Remote access",
                    p,
                    false,
                    Action::RemoteAccess,
                );
            }
            let message = match result {
                Ok(()) => "Wi-Fi setup complete.",
                Err(error) => error,
            };
            self.text(56, 589, message, 19, p.accent, 968);
        }
    }

    fn wifi_keyboard(&mut self, form: &WifiForm, p: &Palette) {
        if form.static_ipv4 {
            self.wifi_static_address(form, p);
            return;
        }
        self.text(
            56,
            149,
            if form.hidden {
                "Join a hidden network"
            } else {
                "Join Wi-Fi"
            },
            30,
            p.text,
            706,
        );
        let address_label = if form.use_static {
            "IPv4 · Manual"
        } else {
            "IPv4 · Automatic"
        };
        self.button(
            Rect(720, 117, 248, 44),
            address_label,
            p,
            false,
            Action::StaticWifi(true),
        );
        if form.hidden {
            self.wifi_field(
                Rect(56, 166, 350, 52),
                "Network name",
                &form.join.name,
                !form.password_field,
                Action::NetworkField(false),
                p,
            );
            self.button(
                Rect(414, 170, 136, 44),
                if form.is_open() {
                    "Open network"
                } else {
                    "WPA2"
                },
                p,
                false,
                Action::HiddenWifiSecurity,
            );
        } else {
            self.text(56, 182, "Network", 15, p.muted, 525);
            self.raw_text(56, 211, &form.join.name, 24, p.text, 543);
        }
        if !form.is_open() {
            let password = &form.join.password;
            let display = "•".repeat(password.len().min(14));
            self.wifi_field(
                Rect(559, 166, 294, 52),
                "Password",
                &display,
                form.password_field,
                Action::NetworkField(true),
                p,
            );
            self.button(
                Rect(862, 170, 106, 44),
                if form.show_password { "Hide" } else { "Show" },
                p,
                false,
                Action::ShowWifiPassword,
            );
        }
        if !form.is_open() || form.hidden {
            self.keyboard(form.symbols, form.shift, p);
        } else {
            self.text(
                56,
                270,
                "This network does not require a password.",
                22,
                p.muted,
                968,
            );
        }
        self.button(
            Rect(56, 461, 172, 48),
            "Back",
            p,
            false,
            Action::CloseNetworkForm,
        );
        if !form.is_open() && !form.join.password.is_empty() {
            let count = form.join.password.len();
            self.raw_text(
                251,
                492,
                &format!("{count} {}", self.language.translate("characters entered")),
                18,
                p.muted,
                726,
            );
        }
        self.button(
            Rect(752, 461, 216, 48),
            "Connect",
            p,
            true,
            Action::JoinNetwork,
        );
        if form.show_password && !form.is_open() {
            let password: Vec<_> = form.join.password.chars().collect();
            for (index, group) in password.chunks(32).enumerate() {
                let line: String = group.iter().collect();
                self.raw_text(56, 534 + index * 20, &line, 19, p.text, 968);
            }
        }
        if let Some(message) = form.error.as_deref() {
            self.text(
                56,
                if form.show_password { 587 } else { 541 },
                message,
                18,
                p.accent,
                968,
            );
        }
    }

    fn wifi_field(
        &mut self,
        rect: Rect,
        label: &str,
        value: &str,
        active: bool,
        action: Action,
        p: &Palette,
    ) {
        if active {
            self.panel(rect, p.accent, 12);
        }
        self.panel(
            Rect(rect.0 + 2, rect.1 + 2, rect.2 - 4, rect.3 - 4),
            p.raised,
            10,
        );
        self.text(
            rect.0 + 13,
            rect.1 + 18,
            label,
            14,
            p.muted,
            rect.0 + rect.2 - 12,
        );
        self.raw_text(
            rect.0 + 13,
            rect.1 + 42,
            value,
            20,
            p.text,
            rect.0 + rect.2 - 12,
        );
        self.buttons.push((rect, action));
    }

    fn wifi_static_address(&mut self, form: &WifiForm, p: &Palette) {
        self.text(56, 149, "Manual IPv4 setup", 30, p.text, 698);
        self.button(
            Rect(710, 117, 258, 48),
            "Use automatic IPv4",
            p,
            false,
            Action::AutomaticWifi,
        );
        for (index, label) in ["IP address", "Subnet mask", "Gateway", "DNS server"]
            .into_iter()
            .enumerate()
        {
            self.wifi_field(
                Rect(56 + 230 * index, 187, 215, 62),
                label,
                &form.address_fields[index],
                index == form.address_field,
                Action::StaticWifiField(index),
                p,
            );
        }
        for (index, key) in "1234567890.".chars().enumerate() {
            self.button(
                Rect(56 + 83 * index, 278, 74, 60),
                &key.to_string(),
                p,
                false,
                Action::Key(key),
            );
        }
        self.button(
            Rect(56, 356, 190, 52),
            "Backspace",
            p,
            false,
            Action::Backspace,
        );
        self.button(
            Rect(56, 461, 265, 48),
            "Back to Wi-Fi credentials",
            p,
            false,
            Action::StaticWifi(false),
        );
        self.button(
            Rect(752, 461, 216, 48),
            "Connect",
            p,
            true,
            Action::JoinNetwork,
        );
        let message = if form.address_error {
            Some("Check the IPv4 address, subnet mask, gateway and DNS server.")
        } else {
            form.error.as_deref()
        };
        if let Some(message) = message {
            self.text(56, 541, message, 18, p.accent, 968);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{palette, Page};
    use super::*;

    #[test]
    fn key_highlights_and_password_survives_a_failed_join() {
        let mut ui = Ui::new();
        ui.page = Page::Network;
        ui.wifi_form(Some(&super::super::NetworkRow {
            name: "Example".into(),
            encryption: "autodetect".into(),
            auth: "WPA2".into(),
            signal: String::new(),
        }));
        let p = palette(&Default::default(), 12);
        let form = ui.wifi.take().unwrap();
        ui.wifi_keyboard(&form, &p);
        ui.wifi = Some(form);
        let key_pixel = super::super::WIDTH * 238 + 90;
        assert_eq!(ui.pixels[key_pixel], p.raised);
        for _ in 0..8 {
            ui.hit(95, 253);
        }
        let form = ui.wifi.take().unwrap();
        ui.wifi_keyboard(&form, &p);
        ui.wifi = Some(form);
        assert_eq!(ui.pixels[key_pixel], p.accent);
        let request = ui.wifi_request().unwrap();
        assert_eq!(request.password, "qqqqqqqq");
        ui.wifi_finished(&Err("Example failure".into()));
        assert_eq!(
            ui.wifi.as_ref().unwrap().error.as_deref(),
            Some("Example failure")
        );
        assert_eq!(ui.wifi_request().unwrap().password, request.password);
        ui.wifi_finished(&Ok(()));
        assert!(ui.wifi.is_none());
    }
}
