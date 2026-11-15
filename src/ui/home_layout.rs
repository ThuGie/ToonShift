use super::*;
use crate::energy::{Chart, Meter, Period};
use crate::plugins::{HomeLayout, Metric};

impl Ui {
    pub fn showing_home(&self) -> bool {
        self.page == Page::Home
    }

    pub(super) fn home_layout_settings(&mut self, view: &View<'_>, p: &Palette) {
        self.panel(Rect(32, 100, 960, 448), p.card, 26);
        self.text(56, 152, "Homepage layout", 30, p.text, 737);
        self.button(
            Rect(761, 115, 205, 49),
            "Display",
            p,
            false,
            Action::Display,
        );
        for (i, layout) in HomeLayout::ALL.into_iter().enumerate() {
            self.button(
                Rect(56 + i % 2 * 459, 188 + i / 2 * 67, 446, 55),
                layout.title(),
                p,
                view.settings.home_layout == layout,
                Action::HomeLayout(layout),
            );
        }
        self.button(Rect(761, 474, 205, 50), "Home", p, false, Action::Home);
    }

    pub(super) fn alternate_home(&mut self, view: &View<'_>, p: &Palette) {
        let layout = view.settings.home_layout;
        let current = temperature(view.status.map(|s| s.current_temp.as_str()));
        let target = temperature(view.status.map(|s| s.current_setpoint.as_str()));
        let adjusted = view.status.is_some() && !view.pending;
        let state = if view.pending {
            "Updating…"
        } else if view.status.is_none() {
            "Thermostat unavailable"
        } else if view.status.is_some_and(Status::heat_requested) {
            "Heat requested"
        } else {
            "Heating idle"
        };
        let target = format!("{} {target}", self.language.translate("Target temperature"));
        if matches!(layout, HomeLayout::Minimal | HomeLayout::WideControls) {
            self.panel(Rect(32, 100, 960, 448), p.card, 26);
            self.centered(
                512,
                147,
                self.language.translate("Room temperature"),
                21,
                p.muted,
            );
            self.centered(512, 247, &current, 93, p.text);
            self.centered(512, 300, &target, 27, p.text);
            self.centered(512, 335, self.language.translate(state), 20, p.accent);
            let width = if layout == HomeLayout::WideControls {
                280
            } else {
                102
            };
            let labels = if width > 102 {
                ["− Cooler", "+ Warmer"]
            } else {
                ["−", "+"]
            };
            self.home_adjust(
                Rect(493 - width, 372, width, 78),
                labels[0],
                -50,
                adjusted,
                p,
            );
            self.home_adjust(Rect(531, 372, width, 78), labels[1], 50, adjusted, p);
            return;
        }

        if layout == HomeLayout::SidePanel {
            self.panel(Rect(32, 100, 414, 448), p.card, 26);
            self.text(58, 151, "Room temperature", 21, p.muted, 424);
            self.raw_text(53, 248, &current, 79, p.text, 424);
            self.raw_text(58, 312, &target, 24, p.text, 424);
            self.text(58, 354, state, 20, p.accent, 424);
            self.home_adjust(Rect(58, 402, 156, 74), "−", -50, adjusted, p);
            self.home_adjust(Rect(238, 402, 156, 74), "+", 50, adjusted, p);
            for (index, (metric, label)) in [
                (Metric::Electricity, "Electricity"),
                (Metric::Gas, "Gas"),
                (Metric::RoomHumidity, "Humidity"),
            ]
            .into_iter()
            .enumerate()
            {
                self.home_reading(
                    Rect(463, 100 + index * 152, 529, 143),
                    label,
                    &crate::telemetry::display(metric, view.telemetry),
                    p,
                );
            }
            return;
        }

        self.panel(Rect(32, 100, 960, 194), p.card, 26);
        self.text(57, 143, "Room temperature", 21, p.muted, 375);
        self.raw_text(53, 239, &current, 76, p.text, 400);
        self.raw_text(415, 182, &target, 23, p.text, 728);
        self.text(415, 227, state, 20, p.accent, 727);
        self.home_adjust(Rect(746, 145, 99, 83), "−", -50, adjusted, p);
        self.home_adjust(Rect(867, 145, 99, 83), "+", 50, adjusted, p);

        match layout {
            HomeLayout::LiveMeters => {
                for (index, (metric, label)) in [
                    (Metric::Electricity, "Electricity"),
                    (Metric::Gas, "Gas flow"),
                    (Metric::RoomHumidity, "Humidity"),
                ]
                .into_iter()
                .enumerate()
                {
                    self.home_reading(
                        Rect(32 + index * 323, 313, 314, 235),
                        label,
                        &crate::telemetry::display(metric, view.telemetry),
                        p,
                    );
                }
            }
            HomeLayout::Usage => {
                for (index, meter) in [Meter::Electricity, Meter::Gas].into_iter().enumerate() {
                    let rect = Rect(32 + index * 486, 313, 474, 235);
                    self.home_chart(rect, meter, view.home_usage.map(|rows| &rows[index]), p);
                    self.buttons
                        .push((rect, Action::Energy(meter, Period::Day)));
                }
            }
            HomeLayout::Schedule => self.home_schedule(view, p),
            HomeLayout::Weather => self.home_weather(view, p),
            _ => {}
        }
    }

    fn centered(&mut self, x: usize, y: usize, value: &str, size: u16, color: u32) {
        let width = value
            .chars()
            .map(|c| self.font.metrics(c, size.into()).advance_width)
            .sum::<f32>();
        self.raw_text(
            x.saturating_sub(width as usize / 2),
            y,
            value,
            size,
            color,
            979,
        );
    }

    fn home_adjust(&mut self, rect: Rect, label: &str, delta: i32, enabled: bool, p: &Palette) {
        if enabled {
            self.button(rect, label, p, true, Action::Heat(Command::Adjust(delta)));
        } else {
            self.panel(rect, p.raised, 14);
            self.centered(
                rect.0 + rect.2 / 2,
                rect.1 + rect.3 / 2 + 8,
                self.language.translate(label),
                24,
                p.muted,
            );
        }
    }

    fn home_reading(&mut self, rect: Rect, label: &str, value: &str, p: &Palette) {
        self.panel(rect, p.card, 23);
        self.text(
            rect.0 + 22,
            rect.1 + 44,
            label,
            21,
            p.muted,
            rect.0 + rect.2 - 18,
        );
        self.text(
            rect.0 + 22,
            rect.1 + 100,
            value,
            34,
            p.text,
            rect.0 + rect.2 - 18,
        );
    }

    fn home_chart(
        &mut self,
        rect: Rect,
        meter: Meter,
        result: Option<&Result<Chart, String>>,
        p: &Palette,
    ) {
        let Rect(x, y, w, _) = rect;
        self.panel(rect, p.card, 23);
        self.text(x + 20, y + 33, meter.title(), 22, p.text, x + w - 20);
        self.text(x + 269, y + 33, "Last 24 hours", 17, p.muted, x + w - 17);
        let Some(Ok(chart)) = result else {
            self.text(
                x + 20,
                y + 101,
                if result.is_some() {
                    "Meter history unavailable"
                } else {
                    "Loading meter history…"
                },
                20,
                p.muted,
                x + w - 18,
            );
            return;
        };
        let Some(total) = chart.total else {
            self.text(
                x + 20,
                y + 101,
                "No meter readings in this period.",
                19,
                p.muted,
                x + w - 18,
            );
            return;
        };
        self.raw_text(
            x + 20,
            y + 83,
            &format!("{total:.3} {}", chart.unit),
            32,
            p.text,
            x + w - 18,
        );
        let top = y + 106;
        let bottom = y + 198;
        let left = x + 21;
        let width = w - 42;
        let max = chart
            .bars
            .iter()
            .copied()
            .flatten()
            .fold(0.0_f64, f64::max)
            .max(0.001);
        for grid in [top, (top + bottom) / 2, bottom] {
            self.panel(Rect(left, grid, width, 1), p.raised, 0);
        }
        for (index, value) in chart.bars.iter().enumerate() {
            if let Some(value) = value {
                let bar_x = left + index * width / chart.bars.len();
                let h = (value.max(0.0) / max * (bottom - top) as f64)
                    .round()
                    .max(2.0) as usize;
                self.panel(
                    Rect(
                        bar_x,
                        bottom - h,
                        (width / chart.bars.len()).saturating_sub(3).max(1),
                        h,
                    ),
                    if chart.partial_bars[index] {
                        p.muted
                    } else {
                        p.accent
                    },
                    0,
                );
            }
        }
        self.text(
            x + 21,
            y + 222,
            if chart.partial {
                "Partial history"
            } else {
                "Open Energy"
            },
            15,
            p.muted,
            x + w - 18,
        );
    }

    fn home_schedule(&mut self, view: &View<'_>, p: &Palette) {
        self.panel(Rect(32, 313, 960, 235), p.card, 23);
        self.text(55, 354, "Today’s schedule", 24, p.text, 745);
        self.button(
            Rect(754, 327, 215, 49),
            "Schedule",
            p,
            false,
            Action::Schedule,
        );
        let Some(Ok(slots)) = view.schedule else {
            self.text(
                55,
                442,
                if view.schedule.is_some() {
                    "Schedule unavailable"
                } else {
                    "Loading schedule…"
                },
                22,
                p.muted,
                962,
            );
            return;
        };
        let day = view.weekday as u16 * crate::schedule::DAY;
        let minute = view
            .time
            .split_once(':')
            .and_then(|(hour, min)| Some(hour.parse::<u16>().ok()? * 60 + min.parse::<u16>().ok()?))
            .unwrap_or(0);
        let mut today: Vec<_> = slots
            .iter()
            .copied()
            .filter(|slot| slot.day() == view.weekday)
            .collect();
        today.sort_by_key(|slot| slot.start);
        let index = today
            .partition_point(|slot| slot.start <= day + minute)
            .saturating_sub(1)
            .min(today.len().saturating_sub(3));
        if today.is_empty() {
            self.text(55, 442, "No schedule periods today", 22, p.muted, 962);
        }
        for (column, slot) in today.into_iter().skip(index).take(3).enumerate() {
            let x = 55 + column * 306;
            let active = slot.start <= day + minute && slot.end > day + minute;
            self.panel(
                Rect(x, 392, 288, 131),
                if active { p.raised } else { p.background },
                14,
            );
            self.raw_text(x + 15, 434, &slot.label(false), 21, p.muted, x + 277);
            self.text(x + 15, 481, slot.preset_name(), 27, p.text, x + 277);
        }
    }

    fn home_weather(&mut self, view: &View<'_>, p: &Palette) {
        let plugin = view
            .plugins
            .iter()
            .enumerate()
            .find(|(_, plugin)| plugin.id == "toonshift-weather");
        let Some((index, plugin)) =
            plugin.filter(|(_, plugin)| view.settings.installed.contains(&plugin.id))
        else {
            self.home_reading(
                Rect(32, 313, 960, 235),
                "Weather",
                "Install Weather from the Store",
                p,
            );
            self.button(Rect(762, 458, 206, 55), "Store", p, false, Action::Store);
            return;
        };
        let reading = view.readings.get(&plugin.id);
        for column in 0..3 {
            let x = 32 + column * 323;
            let rect = Rect(x, 313, 314, 235);
            if column == 0 {
                self.home_reading(
                    rect,
                    "Outside now",
                    reading.map_or("Loading…", |row| row.value.as_str()),
                    p,
                );
                if let Some(row) = reading {
                    self.raw_text(x + 22, 471, &row.detail, 18, p.muted, x + 297);
                }
            } else if let Some(day) = reading.and_then(|row| row.rows.get(column - 1)) {
                self.home_reading(rect, &day.date, &day.range, p);
                self.raw_text(x + 22, 471, &day.condition, 18, p.muted, x + 297);
            } else {
                self.home_reading(rect, "Forecast", "No reading", p);
            }
            self.buttons.push((rect, Action::OpenPlugin(index)));
        }
    }
}
