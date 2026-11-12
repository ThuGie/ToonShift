mod advanced;
mod boxtalk;
mod download;
mod energy;
mod feed;
mod language;
mod network;
mod plugins;
mod presets;
mod schedule;
mod telemetry;
mod thermostat;
mod ui;

#[cfg(target_os = "linux")]
mod api;
#[cfg(target_os = "linux")]
mod device;

fn main() -> std::process::ExitCode {
    let mut args = std::env::args().skip(1);
    let result = if args.next().as_deref() == Some("--preview") {
        preview(args).map_err(Into::into)
    } else {
        run()
    };
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ToonShift: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn preview(mut args: impl Iterator<Item = String>) -> std::io::Result<()> {
    use std::io::{BufWriter, Write};
    use std::path::Path;

    let path = args
        .next()
        .ok_or_else(|| std::io::Error::other("Expected a .ppm output path"))?;
    let page = args.next();
    let night = args.next().as_deref() == Some("night");
    let settings = plugins::Settings {
        appearance: if night {
            plugins::Appearance::Night
        } else {
            plugins::Appearance::Day
        },
        ..Default::default()
    };
    let root = Path::new(".");
    let plugins = plugins::catalog(root);
    let status = thermostat::Status {
        current_temp: "2140".into(),
        current_setpoint: "2050".into(),
        program_state: "1".into(),
        burner_info: "1".into(),
        connection: "1".into(),
        next_setpoint: "1900".into(),
        next_time: "1".into(),
        current_internal_boiler_setpoint: "43".into(),
        current_modulation_level: "28".into(),
        ot_comm_error: "0".into(),
    };
    let view = ui::View {
        hardware: None,
        settings: &settings,
        plugins: &plugins,
        status: Some(&status),
        pending: false,
        error: None,
        time: if night { "22:18" } else { "12:18" },
        hour: 12,
        root,
        schedule: None,
        presets: None,
        networks: None,
        network_message: None,
        catalog_message: None,
        readings: &Default::default(),
        telemetry: &Default::default(),
        energy: None,
        history: None,
        ambient: false,
        weather_results: None,
    };
    let mut ui = ui::Ui::new();
    ui.draw(&view);
    match page.as_deref() {
        Some("store") => {
            ui.hit(600, 40);
        }
        Some("settings") => {
            ui.hit(750, 40);
        }
        _ => {}
    }
    ui.draw(&view);
    if page.as_deref() == Some("logo") {
        ui.pixels.fill(0xeaf1f8);
        ui.logo(352, 140, 320);
    }
    let mut file = BufWriter::new(std::fs::File::create(path)?);
    write!(file, "P6\n{} {}\n255\n", ui::WIDTH, ui::HEIGHT)?;
    for rgb in ui.pixels {
        file.write_all(&rgb.to_be_bytes()[1..])?;
    }
    file.flush()
}

#[cfg(not(target_os = "linux"))]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    Err("The touchscreen application runs on a Toon 2".into())
}

#[cfg(target_os = "linux")]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use std::path::Path;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;
    use ui::Action;

    let root = Path::new("/mnt/data/toon-rust");
    let display = device::Display::open()?;
    let touch = device::touchscreen()?;
    if std::env::args().nth(1).as_deref() == Some("--probe") {
        println!("{:?}", thermostat::status()?);
        println!("Framebuffer and touchscreen opened");
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--probe-hardware") {
        let boiler = advanced::boiler()?;
        let devices = advanced::devices()?;
        println!(
            "Boiler mode: {}; link: {}; hot-water configuration: {}",
            if boiler.open_therm {
                "OpenTherm"
            } else {
                "on/off relay"
            },
            if boiler.connected {
                "connected"
            } else {
                "offline"
            },
            boiler.hot_water.is_some()
        );
        println!(
            "Registered Z-Wave nodes: {}; meter nodes: {}; configured meter sources: {}",
            devices.nodes.len(),
            devices.nodes.iter().filter(|node| node.meter).count(),
            devices.p1_sources
        );
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--probe-telemetry") {
        let events = telemetry::start();
        let until = std::time::Instant::now() + Duration::from_secs(15);
        let (mut humidity, mut room) = (false, false);
        while let Ok((metric, value)) =
            events.recv_timeout(until.saturating_duration_since(std::time::Instant::now()))
        {
            println!("{metric:?}: {value:?}");
            if metric == plugins::Metric::RoomHumidity {
                humidity |= value.is_some();
            }
            if metric == plugins::Metric::RoomTemperature && value.is_some() {
                println!("HTTP currentTemp: {}", thermostat::status()?.current_temp);
                room = true;
            }
            if humidity && room {
                return Ok(());
            }
        }
        return Err("Incomplete room telemetry from BoxTalk".into());
    }
    if std::env::args().nth(1).as_deref() == Some("--probe-feed") {
        let id = std::env::args().nth(2).ok_or("Expected a plugin ID")?;
        let catalog = plugins::catalog(root);
        let plugin = catalog
            .iter()
            .find(|plugin| plugin.id == id)
            .ok_or("Unknown plugin")?;
        let settings = plugins::Settings::load(root);
        let config = plugin.feed.as_ref().ok_or("Plugin has no weather feed")?;
        let location = feed::place(plugin, &settings).ok_or("Plugin has no location")?;
        let reading = feed::fetch(&download::trust(root)?, config, location)?;
        println!("{}: {} {}", location.name, reading.value, reading.detail);
        for day in reading.forecast {
            println!("{}  {}  {}", day.date, day.temperatures, day.condition);
        }
        return Ok(());
    }
    let (commands, updates) = thermostat::worker();
    if let Err(error) = api::start(root, commands.clone()) {
        eprintln!("ToonShift API unavailable: {error}");
    }
    let mut settings = plugins::Settings::load(root);
    let mut backlight = device::Backlight::open().ok();
    let mut last_touch = std::time::Instant::now();
    let mut ambient = false;
    let mut catalog = plugins::catalog(root);
    let mut feeds = feed::Feeds::new(root)
        .map_err(|error| {
            eprintln!("Live plugin feeds unavailable: {error}");
            error
        })
        .ok();
    let mut readings = std::collections::HashMap::new();
    let events = telemetry::start();
    let mut telemetry = telemetry::Readings::new();
    let mut status = None;
    let mut schedule = None;
    let mut presets = None;
    let (network_sender, network_receiver) = std::sync::mpsc::channel();
    let (join_sender, join_receiver) = std::sync::mpsc::channel();
    let mut joining = false;
    let mut network_message = None;
    let (catalog_sender, catalog_receiver) = std::sync::mpsc::channel();
    let mut catalog_busy = false;
    let mut catalog_message = None;
    let (weather_sender, weather_receiver) = std::sync::mpsc::channel();
    let mut weather_results = None;
    let mut weather_busy = false;
    let (energy_requests, energy_jobs) =
        std::sync::mpsc::channel::<(energy::Meter, energy::Period)>();
    let (energy_replies, energy_results) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(first) = energy_jobs.recv() {
            let (meter, period) = energy_jobs.try_iter().last().unwrap_or(first);
            if energy_replies
                .send(((meter, period), energy::load(meter, period)))
                .is_err()
            {
                break;
            }
        }
    });
    let mut selected_energy = None;
    let mut energy_chart = None;
    let (history_sender, history_receiver) = std::sync::mpsc::channel();
    let mut heating_history = None;
    let mut loading_history = false;
    let mut networks = None;
    let mut scanning = false;
    let mut pending = false;
    let mut error = None;
    let mut dirty = true;
    let mut ui = ui::Ui::new();
    let mut clock = device::time();
    let mut feed_poll = std::time::Instant::now();
    let mut hardware = advanced::State::new();
    loop {
        dirty |= hardware.update();
        if let Ok(result) = weather_receiver.try_recv() {
            weather_busy = false;
            weather_results = Some(result);
            dirty = true;
        }
        let idle = settings.idle_minutes > 0
            && last_touch.elapsed() >= Duration::from_secs(u64::from(settings.idle_minutes) * 60);
        if ambient != idle {
            ambient = idle;
            dirty = true;
        }
        if let Ok(result) = history_receiver.try_recv() {
            heating_history = Some(result);
            loading_history = false;
            dirty = true;
        }
        for (selection, result) in energy_results.try_iter() {
            if selected_energy == Some(selection) {
                energy_chart = Some(result);
                dirty = true;
            }
        }
        if let Ok(result) = catalog_receiver.try_recv() {
            catalog_busy = false;
            catalog_message = Some(match result {
                Ok(count) => {
                    catalog = plugins::catalog(root);
                    ui.reset_store_page();
                    format!("Catalog updated. {count} widgets available from this source.")
                }
                Err(error) => error,
            });
            dirty = true;
        }
        for (metric, value) in events.try_iter() {
            if let Some(value) = value {
                telemetry.insert(metric, (value, std::time::Instant::now()));
                dirty = true;
            } else if telemetry.remove(&metric).is_some() {
                dirty = true;
            }
        }
        if feed_poll.elapsed() >= Duration::from_millis(250) {
            feed_poll = std::time::Instant::now();
            if let Some(feeds) = &mut feeds {
                dirty |= feeds.tick(&catalog, &settings, &mut readings);
            }
        }
        if let Ok(result) = network_receiver.try_recv() {
            networks = Some(result);
            scanning = false;
            dirty = true;
        }
        if let Ok(result) = join_receiver.try_recv() {
            joining = false;
            network_message = Some(match result {
                Ok(()) => "Connected to the selected network.".to_owned(),
                Err(error) => error,
            });
            dirty = true;
        }
        for update in updates.try_iter() {
            let new_clock = device::time();
            dirty |= status != update.status
                || error != update.error
                || clock != new_clock
                || update.completed
                || update.schedule.is_some()
                || update.presets.is_some()
                || catalog.iter().any(|plugin| {
                    matches!(plugin.source, plugins::Source::File)
                        && settings.installed.contains(&plugin.id)
                });
            clock = new_clock;
            status = update.status;
            error = update.error;
            if let Some(result) = update.schedule {
                schedule = Some(result);
            }
            if let Some(result) = update.presets {
                presets = Some(result);
            }
            if update.completed {
                pending = false;
            }
        }
        if dirty {
            let (hour, time) = &clock;
            let hour = *hour;
            let daylight = settings.daylight(hour);
            let brightness = if ambient {
                settings.night_brightness.min(15)
            } else if daylight {
                settings.day_brightness
            } else {
                settings.night_brightness
            };
            if let Some(backlight) = &mut backlight {
                if let Err(error) = backlight.set(brightness) {
                    eprintln!("Backlight adjustment failed: {error}");
                }
            }
            ui.draw(&ui::View {
                hardware: Some(&hardware),
                settings: &settings,
                plugins: &catalog,
                status: status.as_ref(),
                pending,
                error: error.as_deref(),
                time,
                hour,
                root,
                schedule: schedule.as_ref(),
                presets: presets.as_ref(),
                networks: networks.as_ref(),
                network_message: network_message.as_deref(),
                catalog_message: catalog_message.as_deref(),
                readings: &readings,
                telemetry: &telemetry,
                energy: energy_chart.as_ref(),
                history: heating_history.as_ref(),
                ambient,
                weather_results: weather_results.as_ref(),
            });
            display.present(&ui.pixels)?;
            dirty = false;
        }
        let gesture = match touch.recv_timeout(Duration::from_millis(35)) {
            Ok(event) => event?,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                return Err("Touchscreen reader disconnected".into())
            }
        };
        last_touch = std::time::Instant::now();
        if ambient {
            ambient = false;
            dirty = true;
            continue;
        }
        let mut next = settings.clone();
        let mut persist = true;
        match ui.gesture(gesture.from, gesture.to, settings.installed.len()) {
            Some(Action::Hardware(action)) => {
                ui.hardware_action(&mut hardware, action);
                persist = false;
            }
            Some(Action::Heat(command)) => {
                commands
                    .send(thermostat::Work::from_ui(command))
                    .map_err(|_| "Thermostat worker stopped")?;
                pending = true;
                error = None;
                persist = false;
            }
            Some(Action::Theme(theme)) => next.theme = theme,
            Some(Action::Appearance(appearance)) => next.appearance = appearance,
            Some(Action::Language(language)) => next.language = language,
            Some(Action::Brightness(night, offset)) => {
                let brightness = if night {
                    &mut next.night_brightness
                } else {
                    &mut next.day_brightness
                };
                *brightness = (i16::from(*brightness) + i16::from(offset)).clamp(5, 100) as u8;
            }
            Some(Action::AppearanceHour(night, offset)) => {
                let (hour, other) = if night {
                    (&mut next.night_start, next.day_start)
                } else {
                    (&mut next.day_start, next.night_start)
                };
                let adjusted = (i16::from(*hour) + i16::from(offset)).rem_euclid(24) as u8;
                *hour = if adjusted == other {
                    (i16::from(adjusted) + i16::from(offset)).rem_euclid(24) as u8
                } else {
                    adjusted
                };
            }
            Some(Action::IdleMinutes(minutes)) => next.idle_minutes = minutes,
            Some(Action::PluginPlace(plugin, place)) => {
                if let Some(location) = catalog
                    .get(plugin)
                    .and_then(|plugin| plugin.feed.as_ref())
                    .and_then(|feed| feed.places.get(place))
                {
                    if feed::location(&catalog[plugin], &settings) != place
                        || next.custom_locations.remove(&catalog[plugin].id).is_some()
                    {
                        next.locations
                            .insert(catalog[plugin].id.clone(), location.name.clone());
                        next.custom_locations.remove(&catalog[plugin].id);
                    } else {
                        persist = false;
                    }
                }
            }
            Some(Action::WeatherSearch) => {
                if !weather_busy {
                    weather_results = None;
                }
                persist = false;
            }
            Some(Action::RunWeatherSearch) => {
                if !weather_busy {
                    if let Some(query) = ui.take_repository() {
                        let sender = weather_sender.clone();
                        let language = settings.language;
                        weather_busy = true;
                        weather_results = None;
                        std::thread::spawn(move || {
                            let result =
                                feed::search(&root.join("https-roots.pem"), &query, language);
                            let _ = sender.send(result);
                        });
                    }
                }
                persist = false;
            }
            Some(Action::UseWeatherPlace(plugin, index)) => {
                let place = weather_results
                    .as_ref()
                    .and_then(|result| result.as_ref().ok())
                    .and_then(|rows| rows.get(index));
                if let (Some(plugin), Some(place)) = (catalog.get(plugin), place) {
                    next.custom_locations
                        .insert(plugin.id.clone(), place.clone());
                } else {
                    persist = false;
                }
            }
            Some(Action::Toggle(index)) => {
                let id = &catalog[index].id;
                if next.installed.contains(id) {
                    next.installed.retain(|installed| installed != id);
                } else if next.installed.len() < 32 {
                    next.installed.push(id.clone());
                } else {
                    error = Some("Remove a widget to free a dashboard slot.".into());
                    persist = false;
                }
            }
            Some(Action::MoveTile(source, destination)) => {
                let tile = next.installed.remove(source);
                next.installed.insert(destination, tile);
            }
            Some(Action::Stock) => return Ok(()),
            Some(Action::Energy(meter, period)) => {
                selected_energy = Some((meter, period));
                energy_chart = None;
                energy_requests
                    .send((meter, period))
                    .map_err(|_| "Energy worker stopped")?;
                persist = false;
            }
            Some(Action::HeatingDetails) => {
                if !loading_history {
                    loading_history = true;
                    heating_history = None;
                    let sender = history_sender.clone();
                    std::thread::spawn(move || {
                        let _ = sender.send(thermostat::history());
                    });
                }
                persist = false;
            }
            Some(Action::EditSchedule(edit)) => {
                commands
                    .send(thermostat::Work::from_ui(
                        thermostat::Command::EditSchedule(edit),
                    ))
                    .map_err(|_| "Thermostat worker stopped")?;
                schedule = None;
                persist = false;
            }
            Some(Action::Presets) => {
                commands
                    .send(thermostat::Work::from_ui(thermostat::Command::Presets))
                    .map_err(|_| "Thermostat worker stopped")?;
                persist = false;
            }
            Some(Action::SavePreset(preset)) => {
                commands
                    .send(thermostat::Work::from_ui(thermostat::Command::SavePreset(
                        preset,
                    )))
                    .map_err(|_| "Thermostat worker stopped")?;
                presets = None;
                persist = false;
            }
            Some(Action::Schedule) => {
                commands
                    .send(thermostat::Work::from_ui(thermostat::Command::Schedule))
                    .map_err(|_| "Thermostat worker stopped")?;
                persist = false;
            }
            Some(Action::Network) => {
                if !scanning && !joining {
                    scanning = true;
                    networks = None;
                    let sender = network_sender.clone();
                    std::thread::spawn(move || {
                        let result =
                            thermostat::firmware_get("hcb_netcon", "action=getWirelessNetworks")
                                .and_then(|body| {
                                    serde_json::from_str::<Vec<ui::NetworkRow>>(&body)
                                        .map_err(|err| err.to_string())
                                })
                                .map(|mut rows| {
                                    rows.retain(|row| !row.name.trim().is_empty());
                                    let mut seen = std::collections::HashSet::new();
                                    rows.retain(|row| {
                                        seen.insert((row.name.clone(), row.auth.clone()))
                                    });
                                    rows
                                });
                        let _ = sender.send(result);
                    });
                }
                persist = false;
            }
            Some(Action::ChooseNetwork(index)) => {
                if !joining {
                    if let Some(row) = networks
                        .as_ref()
                        .and_then(|rows| rows.as_ref().ok())
                        .and_then(|rows| rows.get(index))
                    {
                        ui.wifi_form(Some(row));
                    }
                }
                persist = false;
            }
            Some(Action::HiddenNetwork) => {
                if !joining {
                    ui.wifi_form(None);
                }
                persist = false;
            }
            Some(Action::JoinNetwork) => {
                if let Some(form) = ui.take_wifi().filter(|_| !joining) {
                    joining = true;
                    network_message = Some("Applying Wi-Fi settings…".into());
                    let sender = join_sender.clone();
                    std::thread::spawn(move || {
                        let _ = sender.send(network::join(form));
                    });
                }
                persist = false;
            }
            Some(Action::Store) => {
                persist = false;
                catalog = plugins::catalog(root);
                ui.reset_store_page();
            }
            Some(action @ (Action::RefreshOfficial | Action::SyncRepository)) => {
                let source = if matches!(action, Action::RefreshOfficial) {
                    Some(plugins::OFFICIAL.to_owned())
                } else {
                    ui.take_repository()
                };
                if let Some(source) = source.filter(|_| !catalog_busy) {
                    catalog_busy = true;
                    catalog_message = Some("Downloading catalog…".into());
                    let sender = catalog_sender.clone();
                    std::thread::spawn(move || {
                        let _ = sender.send(plugins::sync(root, &source));
                    });
                }
                persist = false;
            }
            _ => persist = false,
        }
        if persist {
            match next.save(root) {
                Ok(()) => {
                    settings = next;
                    error = None;
                }
                Err(err) => error = Some(format!("Could not save settings: {err}")),
            }
        }
        dirty = true;
    }
}
