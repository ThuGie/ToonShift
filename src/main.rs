mod advanced;
mod boxtalk;
mod download;
mod energy;
mod feed;
mod heating_behaviour;
mod language;
mod network;
mod plugin_runtime;
mod plugin_screen;
mod plugins;
mod presets;
mod schedule;
mod telemetry;
mod thermostat;
mod ui;
#[cfg(target_os = "linux")]
mod updater;
mod vacation;

#[cfg(any(target_os = "linux", test))]
mod api;
#[cfg(target_os = "linux")]
mod device;
#[cfg(target_os = "linux")]
mod remote;

fn main() -> std::process::ExitCode {
    let mut args = std::env::args().skip(1);
    let result = match args.next().as_deref() {
        Some("--version") => {
            println!(
                "ToonShift {} {}",
                env!("CARGO_PKG_VERSION"),
                env!("TOONSHIFT_COMMIT")
            );
            Ok(())
        }
        #[cfg(target_os = "linux")]
        Some("--supervise") => {
            updater::supervise(std::path::Path::new("/mnt/data/toon-rust")).map_err(Into::into)
        }
        Some("--preview") => preview(args).map_err(Into::into),
        Some("--probe-plugin-code") => (|| {
            let path = args.next().ok_or("Expected a plugin package path")?;
            let input = args.next().unwrap_or_default();
            let package: plugins::Plugin = serde_json::from_slice(&std::fs::read(path)?)?;
            let code = plugin_runtime::decode(
                package
                    .wasm
                    .as_deref()
                    .ok_or("Package has no executable code")?,
            )?;
            println!("{}", plugin_runtime::render(&code, input.as_bytes())?);
            Ok(())
        })(),
        _ => run(),
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
    let telemetry = [
        (
            plugins::Metric::Electricity,
            (318.0, std::time::Instant::now()),
        ),
        (plugins::Metric::Gas, (0.0, std::time::Instant::now())),
    ]
    .into();
    let energy = Ok(energy::Chart {
        bars: vec![
            Some(0.3),
            Some(0.32),
            Some(0.29),
            None,
            Some(0.42),
            Some(0.65),
            Some(1.1),
            Some(0.83),
            Some(0.53),
            Some(0.48),
            Some(0.55),
            Some(0.31),
        ],
        partial_bars: vec![
            false, false, false, true, false, false, true, false, false, false, false, false,
        ],
        unit: "kWh",
        total: Some(5.78),
        partial: true,
        from: 0,
        to: 86400,
        tariff_usage: [None; 2],
    });
    let status = thermostat::Status {
        current_temp: "2140".into(),
        current_setpoint: "2200".into(),
        program_state: "1".into(),
        active_state: "1".into(),
        error_found: "0".into(),
        burner_info: "1".into(),
        connection: "1".into(),
        next_setpoint: "1900".into(),
        next_time: "1".into(),
        current_internal_boiler_setpoint: "43".into(),
        current_modulation_level: "28".into(),
        ot_comm_error: "0".into(),
    };
    let view = ui::View {
        remote: None,
        heating_behaviour: None,
        updates: None,
        home_usage: None,
        weekday: 0,
        hardware: None,
        settings: &settings,
        plugins: &plugins,
        status: Some(&status),
        boiler_protocol: Some(true),
        boiler_module: Some(true),
        pending: false,
        error: None,
        time: if night { "22:18" } else { "12:18" },
        hour: 12,
        root,
        schedule: None,
        presets: None,
        vacations: None,
        vacation_busy: false,
        networks: None,
        wifi_connection: None,
        ethernet: None,
        network_message: None,
        catalog_message: None,
        catalog_sources: &[],
        readings: &Default::default(),
        telemetry: &telemetry,
        energy: Some(&energy),
        diagnostics_message: None,
        firmware_version: None,
        history: None,
        ambient: false,
        location_results: None,
    };
    let mut ui = ui::Ui::new();
    ui.draw(&view);
    match page.as_deref() {
        Some("store") => {
            ui.hit(670, 40);
        }
        Some("settings") => {
            ui.hit(750, 40);
        }
        Some("energy") => {
            ui.hit(560, 40);
            ui.draw(&view);
            ui.hit(590, 403);
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
    if std::env::args().nth(1).as_deref() == Some("--api-client") {
        api::manage_clients(root, std::env::args().skip(2))?;
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--probe-meter-configuration") {
        return advanced::probe_meter_configuration().map_err(Into::into);
    }
    if std::env::args().nth(1).as_deref() == Some("--sync-plugin-repo") {
        let repository = std::env::args()
            .nth(2)
            .ok_or("Expected a GitHub catalog source")?;
        download::trust(root)?;
        let count = plugins::sync(root, &repository)?;
        println!("Synced {count} widgets from {repository}");
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--stock-session-return") {
        let pid: libc::pid_t = std::env::args()
            .nth(2)
            .ok_or("Expected a stock UI process ID")?
            .parse()?;
        if pid < 2 {
            return Err("Invalid stock UI process ID".into());
        }
        return wait_for_stock_return(pid);
    }
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
            thermostat::boiler_link_label(
                Some(boiler.open_therm),
                None,
                thermostat::status().ok().as_ref()
            ),
            boiler.hot_water.is_some()
        );
        println!(
            "Registered Z-Wave nodes: {}; meter nodes: {}; configured meter sources: {}",
            devices.nodes.len(),
            devices.nodes.iter().filter(|node| node.meter).count(),
            devices
                .p1_sources
                .map_or_else(|| "unavailable".into(), |count| count.to_string())
        );
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--probe-telemetry") {
        let (module_sender, _module_receiver) = std::sync::mpsc::channel();
        let events = telemetry::start(module_sender);
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
        let config = plugin.feed.as_ref().ok_or("Plugin has no data feed")?;
        let location = feed::place(plugin, &settings).ok_or("Plugin has no location")?;
        let reading = feed::fetch(
            &download::trust(root)?,
            config,
            location,
            settings.language,
            plugin.wasm.as_deref(),
        )?;
        println!("{}: {} {}", location.name, reading.value, reading.detail);
        for day in reading.rows {
            println!("{}  {}  {}", day.date, day.range, day.condition);
        }
        return Ok(());
    }
    if std::env::args().nth(1).as_deref() == Some("--cache-plugin") {
        let mut ids = std::env::args().skip(2).peekable();
        if ids.peek().is_none() {
            return Err("Expected at least one plugin ID".into());
        }
        download::trust(root)?;
        plugins::sync(root, plugins::OFFICIAL)?;
        let catalog = plugins::catalog(root);
        for id in ids {
            let plugin = catalog
                .iter()
                .find(|plugin| plugin.id == id)
                .ok_or("Unknown plugin")?;
            plugins::install(root, plugin)?;
            println!("Cached {} {}", plugin.id, plugin.release);
        }
        return Ok(());
    }
    let mut settings = plugins::Settings::load(root);
    let heating_inputs = heating_behaviour::Shared::default();
    heating_inputs.lock().unwrap().enabled = settings.heating_advice;
    let (commands, updates) = thermostat::worker(root.to_owned(), heating_inputs.clone());
    let api_events = std::sync::Arc::new(api::Events::default());
    let remote = remote::Remote::start(
        root,
        commands.clone(),
        api_events.clone(),
        settings.remote_access,
    );
    let mut remote_view = ui::RemoteView::default();
    let mut remote_refresh = std::time::Instant::now();
    if let Err(error) = api::start(root, commands.clone(), api_events.clone()) {
        eprintln!("ToonShift API unavailable: {error}");
    }
    let mut heating_behaviour = heating_behaviour::Summary::default();
    let mut backlight = device::Backlight::open().ok();
    let mut last_touch = std::time::Instant::now();
    let mut update_check_at = last_touch + Duration::from_secs(120);
    let mut update_backoff = 300_u64;
    let mut update_view = ui::SoftwareUpdates {
        available: updater::staged(root).map(|stage| stage.version),
        previous: root.join("update-previous").is_file(),
        reason: updater::rollback_reason(root),
        ..Default::default()
    };
    let mut update_apply = None;
    let (update_sender, update_receiver) = std::sync::mpsc::channel::<Result<String, String>>();
    let mut update_manual_check = false;
    let mut update_ready = false;
    let mut ambient = false;
    let mut catalog = plugins::catalog(root);
    if settings.migrate_plugin_ids(&catalog) {
        settings.save(root)?;
    }
    let mut feeds = feed::Feeds::new(root)
        .map_err(|error| {
            eprintln!("Live plugin feeds unavailable: {error}");
            error
        })
        .ok();
    let mut readings = std::collections::HashMap::new();
    let (module_sender, module_receiver) = std::sync::mpsc::channel();
    let events = telemetry::start(module_sender);
    let mut boiler_module = None;
    let (protocol_sender, protocol_receiver) = std::sync::mpsc::channel();
    let mut boiler_protocol = None;
    let mut protocol_due = std::time::Instant::now();
    let mut calibration_due = protocol_due;
    let mut telemetry = telemetry::Readings::new();
    let (meter_sender, meter_receiver) =
        std::sync::mpsc::channel::<(advanced::MeterSources, Result<advanced::MeterFlows, String>)>(
        );
    let mut meter_busy = false;
    let mut next_meter_poll = std::time::Instant::now();
    let mut status = None;
    let mut schedule = None;
    let mut presets = None;
    let mut vacations = None;
    let mut vacation_busy = false;
    let (network_sender, network_receiver) = std::sync::mpsc::channel();
    let mut wifi_connection = None;
    let mut ethernet = None;
    let (join_sender, join_receiver) = std::sync::mpsc::channel();
    let mut joining = false;
    let mut network_message = None;
    let (diagnostics_sender, diagnostics_receiver) = std::sync::mpsc::channel();
    let mut exporting_diagnostics = false;
    let mut diagnostics_message = None;
    let (version_sender, version_receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = version_sender.send(thermostat::firmware_version());
    });
    let mut firmware_version = None;
    let (catalog_sender, catalog_receiver) = std::sync::mpsc::channel();
    let mut catalog_busy = false;
    let mut catalog_sources = plugins::sources(root);
    let mut catalog_message = None;
    let (install_sender, install_receiver) = std::sync::mpsc::channel();
    let mut installing = false;
    let (location_sender, location_receiver) = std::sync::mpsc::channel();
    let mut location_results = None;
    let mut location_busy = false;
    let (energy_requests, energy_jobs) =
        std::sync::mpsc::channel::<(energy::Meter, energy::Period, Option<String>)>();
    let (energy_replies, energy_results) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        while let Ok(first) = energy_jobs.recv() {
            let (meter, period, source) = energy_jobs.try_iter().last().unwrap_or(first);
            let chart = energy::load_selected(root, meter, period, source.as_deref());
            if energy_replies
                .send(((meter, period, source), chart))
                .is_err()
            {
                break;
            }
        }
    });
    let mut selected_energy = None;
    let mut next_energy_refresh = std::time::Instant::now();
    let mut energy_chart = None;
    let (home_usage_sender, home_usage_results) = std::sync::mpsc::channel();
    let mut home_usage = None;
    let mut home_usage_busy = false;
    let mut home_usage_due = std::time::Instant::now();
    let mut home_schedule_due = std::time::Instant::now();
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
    let (extension_sender, extension_receiver) = std::sync::mpsc::channel();
    let mut extension_generation = 0_u64;
    let mut extension_busy = false;
    let mut extension_poll = std::time::Instant::now();
    let submit_extension = |generation,
                            readings: &telemetry::Readings,
                            (plugin, allowed, origin, mut event): (
        plugins::Plugin,
        bool,
        Option<String>,
        plugin_screen::Event,
    )| {
        event.telemetry = telemetry::plugin_snapshot(&plugin.telemetry, readings);
        let sender = extension_sender.clone();
        std::thread::spawn(move || {
            let result = plugin_screen::run_installed(root, &plugin, allowed, origin, event);
            let _ = sender.send((generation, result));
        });
    };
    let mut feed_poll = std::time::Instant::now();
    let mut hardware = advanced::State::new();
    loop {
        if ui.showing_remote() && std::time::Instant::now() >= remote_refresh {
            remote_refresh = std::time::Instant::now() + Duration::from_secs(1);
            let next = remote.view();
            dirty |= next != remote_view;
            remote_view = next;
        }
        if let Ok(result) = update_receiver.try_recv() {
            update_view.checking = false;
            update_view.last_check = Some(device::time().1);
            update_view.available = updater::staged(root).map(|stage| stage.version);
            let delay = if result.is_ok() {
                update_backoff = 300;
                21_600
            } else {
                let delay = update_backoff;
                update_backoff = (update_backoff * 2).min(21_600);
                delay
            };
            update_view.message = result.unwrap_or_else(|error| error);
            update_check_at = std::time::Instant::now() + Duration::from_secs(delay);
            dirty = true;
        }
        if !update_view.checking
            && (update_manual_check
                || (settings.automatic_updates && std::time::Instant::now() >= update_check_at))
        {
            update_manual_check = false;
            update_view.checking = true;
            let sender = update_sender.clone();
            std::thread::spawn(move || {
                let _ = sender.send(updater::check(root));
            });
            dirty = true;
        }
        let automatic_apply = settings.automatic_updates
            && update_view.available.is_some()
            && last_touch.elapsed() >= Duration::from_secs(300);
        if (update_apply.is_some() || automatic_apply)
            && !update_view.checking
            && !pending
            && !joining
            && !hardware.busy
            && !extension_busy
            && !installing
            && !catalog_busy
            && !vacation_busy
            && !ui.has_unsaved_changes()
            && !root.join("update-pending.json").exists()
            && thermostat::freeze_work()
        {
            let request = update_apply.take().unwrap_or(updater::Request::Install);
            match updater::request_restart(root, request) {
                Ok(()) => return Ok(()),
                Err(error) => {
                    thermostat::resume_work();
                    update_view.message = error.to_string();
                    update_view.available = None;
                    dirty = true;
                }
            }
        }
        if let Ok((generation, result)) = extension_receiver.try_recv() {
            extension_busy = false;
            if generation == extension_generation {
                ui.extension_result(result);
                dirty = true;
            }
        }
        if !extension_busy && !ambient && extension_poll.elapsed() >= Duration::from_secs(5) {
            extension_poll = std::time::Instant::now();
            if let Some(job) = ui.extension_event(None, true) {
                extension_busy = true;
                submit_extension(extension_generation, &telemetry, job);
            }
        }
        if let Ok((sources, result)) = meter_receiver.try_recv() {
            meter_busy = false;
            if sources == settings.meter_sources {
                match result {
                    Ok(values) => {
                        for (metric, value, total) in values {
                            if metric == plugins::Metric::Gas {
                                heating_inputs.lock().unwrap().gas =
                                    total.map(|value| (value, std::time::Instant::now()));
                            }
                            if let Some(value) = value {
                                telemetry.insert(metric, (value, std::time::Instant::now()));
                            } else {
                                telemetry.remove(&metric);
                            }
                        }
                    }
                    Err(_) => {
                        heating_inputs.lock().unwrap().gas = None;
                        for metric in sources.keys() {
                            telemetry.remove(metric);
                        }
                    }
                }
                dirty = true;
            }
        }
        if !settings.meter_sources.is_empty()
            && !meter_busy
            && std::time::Instant::now() >= next_meter_poll
        {
            next_meter_poll = std::time::Instant::now() + Duration::from_secs(10);
            meter_busy = true;
            let sources = settings.meter_sources.clone();
            let sender = meter_sender.clone();
            std::thread::spawn(move || {
                let result = advanced::meter_flows(&sources);
                if let Ok(readings) = &result {
                    for &(metric, flow, total) in readings {
                        if let Some(source) = sources.get(&metric) {
                            if let Err(error) =
                                energy::record_selected(root, metric, source, flow, total)
                            {
                                eprintln!("Could not record meter history: {error}");
                            }
                        }
                    }
                }
                let _ = sender.send((sources, result));
            });
        }
        if let Ok((id, result)) = install_receiver.try_recv() {
            installing = false;
            catalog_message = Some(match result {
                Ok(()) => {
                    catalog = plugins::catalog(root);
                    let mut next = settings.clone();
                    if !next.installed.contains(&id) && next.installed.len() < 32 {
                        next.installed.push(id);
                    }
                    match next.save(root) {
                        Ok(()) => {
                            settings = next;
                            "Plugin ready".into()
                        }
                        Err(err) => format!("Could not save settings: {err}"),
                    }
                }
                Err(error) => error,
            });
            dirty = true;
        }
        dirty |= hardware.update();
        if let Ok(result) = location_receiver.try_recv() {
            location_busy = false;
            location_results = Some(result);
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
            if selected_energy.as_ref() == Some(&selection) {
                energy_chart = Some(result);
                dirty = true;
            }
        }
        if ui.showing_energy() && std::time::Instant::now() >= next_energy_refresh {
            next_energy_refresh = std::time::Instant::now() + Duration::from_secs(30);
            if let Some((meter, period, _)) = selected_energy.as_ref() {
                let source = meter
                    .selected_metric()
                    .and_then(|metric| settings.meter_sources.get(&metric))
                    .cloned();
                let selection = (*meter, *period, source);
                if selected_energy.as_ref() != Some(&selection) {
                    energy_chart = None;
                }
                selected_energy = Some(selection.clone());
                let _ = energy_requests.send(selection);
            }
        }
        if let Ok((sources, result)) = home_usage_results.try_recv() {
            home_usage_busy = false;
            if sources == settings.meter_sources {
                home_usage = Some(result);
                dirty = true;
            } else {
                home_usage_due = std::time::Instant::now();
            }
        }
        if !ambient && ui.showing_home() {
            let now = std::time::Instant::now();
            match settings.home_layout {
                layout if layout.shows_usage() && !home_usage_busy && now >= home_usage_due => {
                    home_usage_busy = true;
                    home_usage_due = now + Duration::from_secs(45);
                    let sources = settings.meter_sources.clone();
                    let sender = home_usage_sender.clone();
                    std::thread::spawn(move || {
                        let charts =
                            [energy::Meter::Electricity, energy::Meter::Gas].map(|meter| {
                                let source = meter
                                    .selected_metric()
                                    .and_then(|metric| sources.get(&metric));
                                energy::load_selected(
                                    root,
                                    meter,
                                    energy::Period::Day,
                                    source.map(String::as_str),
                                )
                            });
                        let _ = sender.send((sources, charts));
                    });
                }
                plugins::HomeLayout::Schedule if now >= home_schedule_due => {
                    home_schedule_due = now + Duration::from_secs(60);
                    let _ = commands.send(thermostat::Work::from_ui(thermostat::Command::Schedule));
                }
                _ => {}
            }
        }
        if let Ok(result) = catalog_receiver.try_recv() {
            catalog_busy = false;
            catalog_message = Some(match result {
                Ok(count) => {
                    catalog = plugins::catalog(root);
                    catalog_sources = plugins::sources(root);
                    ui.source_removed();
                    ui.reset_store_page();
                    let mut migrated = settings.clone();
                    if migrated.migrate_plugin_ids(&catalog) {
                        match migrated.save(root) {
                            Ok(()) => settings = migrated,
                            Err(error) => {
                                eprintln!("Could not persist migrated plugin IDs: {error}");
                                settings = migrated;
                            }
                        }
                    }
                    format!("Catalog updated. {count} widgets available from this source.")
                }
                Err(error) => error,
            });
            dirty = true;
        }
        for (metric, value) in events.try_iter() {
            if settings.meter_sources.contains_key(&metric) {
                continue;
            }
            if let Some(value) = value {
                telemetry.insert(metric, (value, std::time::Instant::now()));
                dirty = true;
            } else if telemetry.remove(&metric).is_some() {
                dirty = true;
            }
        }
        for module in module_receiver.try_iter() {
            dirty |= boiler_module != module;
            boiler_module = module;
            heating_inputs.lock().unwrap().module = module;
        }
        for mode in protocol_receiver.try_iter() {
            dirty |= boiler_protocol != mode;
            boiler_protocol = mode;
            heating_inputs.lock().unwrap().open_therm = mode;
        }
        if std::time::Instant::now() >= protocol_due {
            let now = std::time::Instant::now();
            protocol_due = now + Duration::from_secs(15);
            let read_calibration = now >= calibration_due;
            if read_calibration {
                calibration_due = now + Duration::from_secs(60);
            }
            let sender = protocol_sender.clone();
            let heating_inputs = heating_inputs.clone();
            std::thread::spawn(move || {
                let _ = sender.send(thermostat::boiler_protocol().ok());
                if read_calibration {
                    if let Some(calibration) = thermostat::calibration_offset() {
                        heating_inputs.lock().unwrap().calibration = Some(calibration);
                    }
                }
            });
        }
        if feed_poll.elapsed() >= Duration::from_millis(250) {
            feed_poll = std::time::Instant::now();
            if let Some(feeds) = &mut feeds {
                dirty |= feeds.tick(&catalog, &settings, &telemetry, &mut readings);
            }
        }
        if let Ok((result, connection, wired)) = network_receiver.try_recv() {
            networks = Some(result);
            wifi_connection = Some(connection);
            ethernet = wired;
            scanning = false;
            dirty = true;
        }
        if let Ok((result, connection, wired)) = join_receiver.try_recv() {
            wifi_connection = Some(connection);
            ethernet = wired;
            joining = false;
            network_message = Some(match result {
                Ok(()) => "Connected to the selected network.".to_owned(),
                Err(error) => error,
            });
            dirty = true;
        }
        if let Ok(result) = diagnostics_receiver.try_recv() {
            exporting_diagnostics = false;
            diagnostics_message = Some(match result {
                Ok(()) => "Saved diagnostics.json in the ToonShift data directory.",
                Err(_) => "Could not save diagnostics.json.",
            });
            dirty = true;
        }
        if let Ok(result) = version_receiver.try_recv() {
            firmware_version = Some(result);
            dirty = true;
        }
        for update in updates.try_iter() {
            dirty |= heating_behaviour != update.heating_behaviour;
            heating_behaviour = update.heating_behaviour;
            let new_clock = device::time();
            dirty |= status != update.status
                || error != update.error
                || clock != new_clock
                || update.completed
                || update.schedule.is_some()
                || update.presets.is_some()
                || update.vacations.is_some()
                || catalog.iter().any(|plugin| {
                    matches!(plugin.source, plugins::Source::File)
                        && settings.installed.contains(&plugin.id)
                });
            clock = new_clock;
            if status != update.status {
                api_events.publish(update.status.as_ref());
            }
            status = update.status;
            error = update.error;
            if let Some(result) = update.schedule {
                schedule = Some(result);
            }
            if let Some(result) = update.presets {
                presets = Some(result);
            }
            if let Some(result) = update.vacations {
                if result.is_ok() && vacation_busy {
                    ui.close_vacation_editor();
                }
                vacations = Some(result);
                vacation_busy = false;
            }
            if update.completed {
                pending = false;
            }
        }
        if dirty {
            api_events.publish_readings(&telemetry);
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
                remote: Some(&remote_view),
                heating_behaviour: Some(&heating_behaviour),
                updates: Some(&update_view),
                home_usage: home_usage.as_ref(),
                weekday: device::weekday(),
                hardware: Some(&hardware),
                settings: &settings,
                plugins: &catalog,
                status: status.as_ref(),
                boiler_protocol,
                boiler_module,
                pending,
                error: error.as_deref(),
                time,
                hour,
                root,
                schedule: schedule.as_ref(),
                presets: presets.as_ref(),
                vacations: vacations.as_ref(),
                vacation_busy,
                networks: networks.as_ref(),
                wifi_connection: wifi_connection.as_ref(),
                ethernet: ethernet.as_ref(),
                network_message: network_message.as_deref(),
                catalog_message: catalog_message.as_deref(),
                catalog_sources: &catalog_sources,
                readings: &readings,
                telemetry: &telemetry,
                energy: energy_chart.as_ref(),
                diagnostics_message,
                firmware_version: firmware_version.as_ref(),
                history: heating_history.as_ref(),
                ambient,
                location_results: location_results.as_ref(),
            });
            display.present(&ui.pixels)?;
            if !update_ready && status.is_some() {
                updater::ready(root)?;
                update_ready = true;
            }
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
        let action = ui.gesture(
            gesture.from,
            gesture.to,
            settings.installed.len(),
            settings.tiles_per_page(),
        );
        match action {
            Some(Action::RemoteAccess) => {
                remote_refresh = std::time::Instant::now();
                persist = false;
            }
            Some(Action::RemoteEnable(enabled)) => next.remote_access = enabled,
            Some(
                action @ (Action::RemoteAddress(_)
                | Action::RemotePair
                | Action::RemoteCancel
                | Action::RemoteApprove
                | Action::RemoteRevoke(_)),
            ) => {
                let result = match action {
                    Action::RemoteAddress(address) => remote.configure(address),
                    Action::RemotePair => remote.pair(),
                    Action::RemoteCancel => {
                        remote.cancel();
                        Ok(())
                    }
                    Action::RemoteApprove => remote.approve(),
                    Action::RemoteRevoke(index) => remote_view.phones.get(index).map_or_else(
                        || Err("Phone no longer present".into()),
                        |(id, _)| remote.revoke(id),
                    ),
                    _ => unreachable!(),
                };
                error = result.err();
                remote_refresh = std::time::Instant::now();
                persist = false;
            }
            Some(Action::CheckUpdates) => {
                update_manual_check = true;
                persist = false;
            }
            Some(Action::AutomaticUpdates(enabled)) => next.automatic_updates = enabled,
            Some(Action::HeatingAdvice(enabled)) => next.heating_advice = enabled,
            Some(Action::ResetHeatingLearning) => {
                let mut inputs = heating_inputs.lock().unwrap();
                inputs.reset = inputs.reset.wrapping_add(1);
                persist = false;
            }
            Some(Action::InstallUpdate) => {
                if update_view.available.is_some() {
                    update_apply = Some(updater::Request::Install);
                    update_view.message = "Waiting to restart safely".into();
                }
                persist = false;
            }
            Some(Action::RestoreUpdate) => {
                if update_view.previous {
                    update_apply = Some(updater::Request::Restore);
                    update_view.message = "Waiting to restart safely".into();
                }
                persist = false;
            }
            Some(action @ (Action::OpenExtension(_) | Action::ExtensionButton(_))) => {
                let button = match action {
                    Action::OpenExtension(index) => {
                        extension_generation = extension_generation.wrapping_add(1);
                        if let Some(plugin) = catalog.get(index).filter(|plugin| {
                            settings.installed.contains(&plugin.id)
                                && matches!(plugin.source, plugins::Source::Interactive)
                        }) {
                            ui.open_extension(plugin.clone());
                        }
                        None
                    }
                    Action::ExtensionButton(index) => Some(index),
                    _ => unreachable!(),
                };
                if extension_busy {
                    ui.extension_result(Err(
                        "Another plugin is still working. Reopen this tile to retry.".into(),
                    ));
                } else if let Some(job) = ui.extension_event(button, false) {
                    extension_busy = true;
                    extension_poll = std::time::Instant::now();
                    submit_extension(extension_generation, &telemetry, job);
                }
                persist = false;
            }
            Some(Action::MeterSource(metric, index)) => {
                if let Some(channel) = hardware
                    .devices
                    .as_ref()
                    .and_then(|result| result.as_ref().ok())
                    .and_then(|devices| {
                        devices
                            .nodes
                            .iter()
                            .flat_map(|node| &node.channels)
                            .nth(index)
                    })
                {
                    next.meter_sources.insert(metric, channel.id.clone());
                    telemetry.remove(&metric);
                    next_meter_poll = std::time::Instant::now();
                } else {
                    persist = false;
                }
            }
            Some(Action::AutomaticMeter(metric)) => {
                next.meter_sources.remove(&metric);
                telemetry.remove(&metric);
            }
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
            Some(Action::HomeLayout(layout)) => {
                next.home_layout = layout;
                home_usage_due = std::time::Instant::now();
                home_schedule_due = std::time::Instant::now();
            }
            Some(Action::Language(language)) => next.language = language,
            Some(Action::ResetPreferences) => {
                if installing {
                    error = Some(
                        settings
                            .language
                            .translate("Wait for the plugin operation to finish.")
                            .into(),
                    );
                    persist = false;
                } else {
                    next = plugins::Settings::default();
                }
            }
            Some(Action::ExportDiagnostics) => {
                if !exporting_diagnostics {
                    exporting_diagnostics = true;
                    diagnostics_message = Some("Exporting diagnostics…");
                    let sender = diagnostics_sender.clone();
                    std::thread::spawn(move || {
                        let _ = sender.send(thermostat::export_diagnostics(root));
                    });
                }
                persist = false;
            }
            Some(Action::SaveEnergyRate(rate)) => match ui.energy_rate_input() {
                Some(input) => {
                    if let Err(message) = rate.save(&mut next.energy_rates, input) {
                        ui.show_energy_rate_error(message);
                        persist = false;
                    }
                }
                None => persist = false,
            },
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
            Some(Action::IdleDashboard(page)) => next.idle_dashboard_page = page,
            Some(Action::WideTiles(wide)) => {
                next.wide_tiles = wide;
                next.idle_dashboard_page = next
                    .idle_dashboard_page
                    .map(|page| page.min(next.dashboard_pages().saturating_sub(1)));
            }
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
            Some(Action::LocationSearch) => {
                if !location_busy {
                    location_results = None;
                }
                persist = false;
            }
            Some(Action::RunLocationSearch(index)) => {
                if !location_busy {
                    let search = catalog.get(index).and_then(|plugin| {
                        Some((plugin.feed.as_ref()?.search.clone()?, plugin.wasm.clone()))
                    });
                    if let (Some(query), Some((search, code))) = (ui.take_repository(), search) {
                        let sender = location_sender.clone();
                        let language = settings.language;
                        location_busy = true;
                        location_results = None;
                        std::thread::spawn(move || {
                            let result = feed::search(
                                &root.join("https-roots.pem"),
                                &search,
                                &query,
                                language,
                                code.as_deref(),
                            );
                            let _ = sender.send(result);
                        });
                    }
                }
                persist = false;
            }
            Some(Action::UseLocationResult(plugin, index)) => {
                let place = location_results
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
            Some(Action::RollbackPlugin(index)) => {
                if let Some(plugin) = catalog.get(index).filter(|_| !installing).cloned() {
                    let sender = install_sender.clone();
                    installing = true;
                    catalog_message = Some("Restoring previous version…".into());
                    std::thread::spawn(move || {
                        let result = plugins::rollback(root, &plugin);
                        let _ = sender.send((plugin.id, result));
                    });
                }
                persist = false;
            }
            Some(action @ (Action::Toggle(index) | Action::UpdatePlugin(index))) => {
                let plugin = &catalog[index];
                let id = &plugin.id;
                if installing {
                    persist = false;
                } else if matches!(action, Action::Toggle(_)) && next.installed.contains(id) {
                    next.installed.retain(|installed| installed != id);
                } else if plugin.artifact.is_some()
                    && (next.installed.contains(id) || next.installed.len() < 32)
                {
                    let plugin = plugin.clone();
                    let sender = install_sender.clone();
                    installing = true;
                    persist = false;
                    catalog_message = Some("Downloading plugin…".into());
                    std::thread::spawn(move || {
                        let result = plugins::install(root, &plugin);
                        let _ = sender.send((plugin.id, result));
                    });
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
            Some(Action::DuplicateTile(source)) => {
                if let Some(tile) = next
                    .installed
                    .get(source)
                    .filter(|_| next.installed.len() < 32)
                    .cloned()
                {
                    next.installed.insert(source + 1, tile);
                } else {
                    persist = false;
                }
            }
            Some(Action::ResetDashboard) => {
                next.installed = plugins::Settings::default().installed;
                next.dashboard_names = plugins::Settings::default().dashboard_names;
                next.wide_tiles = false;
                next.idle_dashboard_page = None;
            }
            Some(Action::NameDashboard(page)) => {
                ui.edit_dashboard_name(
                    settings
                        .dashboard_names
                        .get(page)
                        .map_or("", String::as_str),
                );
                persist = false;
            }
            Some(Action::SaveDashboardName(page)) => {
                next.dashboard_names.resize(16, String::new());
                if let Some((slot, name)) =
                    next.dashboard_names.get_mut(page).zip(ui.dashboard_name())
                {
                    *slot = name.trim().chars().take(18).collect();
                } else {
                    persist = false;
                }
            }
            Some(Action::Stock) => return Ok(()),
            Some(Action::StockMeterSession) => {
                std::fs::write("/run/toonshift-temporary-stock", b"meter")?;
                return Ok(());
            }
            Some(Action::Energy(meter, period)) => {
                let source = meter
                    .selected_metric()
                    .and_then(|metric| settings.meter_sources.get(&metric))
                    .cloned();
                let selection = (meter, period, source);
                selected_energy = Some(selection.clone());
                next_energy_refresh = std::time::Instant::now() + Duration::from_secs(30);
                energy_chart = None;
                energy_requests
                    .send(selection)
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
            Some(Action::Vacations) => {
                if !vacation_busy {
                    commands.send(thermostat::Work::from_ui(thermostat::Command::Vacations))?;
                    vacation_busy = true;
                }
                persist = false;
            }
            Some(Action::EditVacation(index)) => {
                let format = |seconds| {
                    device::vacation_date(seconds).map_or_else(String::new, |[y, m, d, h, min]| {
                        format!("{y:04}{m:02}{d:02}{h:02}{min:02}")
                    })
                };
                if let Some(index) = index {
                    if let Some(entry) = vacations
                        .as_ref()
                        .and_then(|result| result.as_ref().ok())
                        .and_then(|entries| entries.get(index))
                    {
                        if let Some(id) = vacation::Id::parse(&entry.id) {
                            ui.edit_vacation(Some(id), [format(entry.start), format(entry.end)]);
                        }
                    }
                } else {
                    ui.edit_vacation(None, Default::default());
                }
                persist = false;
            }
            Some(Action::SaveVacation) => {
                let input = ui.vacation_input().and_then(|(id, [start, end])| {
                    Some((
                        id,
                        device::vacation_timestamp(start)?,
                        device::vacation_timestamp(end)?,
                    ))
                });
                if let Some((id, start, end)) = input {
                    commands.send(thermostat::Work::from_ui(
                        thermostat::Command::SaveVacation(id, start, end),
                    ))?;
                    vacation_busy = true;
                } else {
                    ui.vacation_input_error();
                }
                persist = false;
            }
            Some(Action::RemoveVacation(index)) => {
                if let Some(id) = vacations
                    .as_ref()
                    .and_then(|result| result.as_ref().ok())
                    .and_then(|entries| entries.get(index))
                    .and_then(|entry| vacation::Id::parse(&entry.id))
                {
                    commands.send(thermostat::Work::from_ui(
                        thermostat::Command::RemoveVacation(id),
                    ))?;
                    vacation_busy = true;
                }
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
                        let _ = sender.send((result, network::connection(), network::ethernet()));
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
                if let Some(form) = (!joining && !scanning).then(|| ui.take_wifi()).flatten() {
                    joining = true;
                    network_message = Some("Applying Wi-Fi settings…".into());
                    let sender = join_sender.clone();
                    std::thread::spawn(move || {
                        let _ = sender.send((
                            network::join(form),
                            network::connection(),
                            network::ethernet(),
                        ));
                    });
                }
                persist = false;
            }
            Some(Action::ReconnectWifi) => {
                if !joining && !scanning {
                    joining = true;
                    network_message = Some("Reconnecting to saved Wi-Fi…".into());
                    let sender = join_sender.clone();
                    std::thread::spawn(move || {
                        let _ = sender.send((
                            network::reconnect(),
                            network::connection(),
                            network::ethernet(),
                        ));
                    });
                }
                persist = false;
            }
            Some(Action::Store) => {
                persist = false;
                catalog = plugins::catalog(root);
                catalog_sources = plugins::sources(root);
                ui.reset_store_page();
            }
            Some(Action::RemoveSource(index)) => {
                if let Some(source) = catalog_sources
                    .get(index)
                    .filter(|_| !catalog_busy && !installing)
                {
                    catalog_message = Some(match plugins::remove_source(source) {
                        Ok(()) => {
                            catalog = plugins::catalog(root);
                            catalog_sources = plugins::sources(root);
                            ui.source_removed();
                            "Catalog source removed".into()
                        }
                        Err(error) => format!("Could not remove catalog source: {error}"),
                    });
                }
                persist = false;
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
                    remote.enable(next.remote_access);
                    if next.remote_access != settings.remote_access {
                        remote_refresh = std::time::Instant::now();
                    }
                    {
                        let mut inputs = heating_inputs.lock().unwrap();
                        inputs.enabled = next.heating_advice;
                        if settings.meter_sources != next.meter_sources {
                            inputs.gas = None;
                        }
                    }
                    if settings.meter_sources != next.meter_sources {
                        home_usage = None;
                        home_usage_due = std::time::Instant::now();
                    }
                    if matches!(action, Some(Action::ResetPreferences)) {
                        for metric in settings.meter_sources.keys() {
                            telemetry.remove(metric);
                        }
                        ui = ui::Ui::new();
                    }
                    settings = next;
                    error = None;
                    if matches!(action, Some(Action::SaveEnergyRate(_))) {
                        ui.close_energy_rate();
                    }
                    if matches!(action, Some(Action::SaveDashboardName(_))) {
                        ui.close_dashboard_name();
                    }
                    if matches!(
                        action,
                        Some(Action::DuplicateTile(_) | Action::ResetDashboard)
                    ) {
                        ui.dashboard_changed(matches!(action, Some(Action::ResetDashboard)));
                    }
                }
                Err(err) => error = Some(format!("Could not save settings: {err}")),
            }
        }
        dirty = true;
    }
}

#[cfg(target_os = "linux")]
fn wait_for_stock_return(pid: libc::pid_t) -> Result<(), Box<dyn std::error::Error>> {
    use std::time::{Duration, Instant};
    let input = device::touchscreen().ok();
    let end = Instant::now() + Duration::from_secs(600);
    let mut first = Instant::now();
    let mut taps = 0;
    while Instant::now() < end && unsafe { libc::kill(pid, 0) } == 0 {
        let interval = Duration::from_millis(500);
        let Some(input) = &input else {
            std::thread::sleep(interval);
            continue;
        };
        if let Ok(Ok(touch)) = input.recv_timeout(interval) {
            let in_corner = |point: (usize, usize)| point.0 < 100 && point.1 < 100;
            if first.elapsed() >= Duration::from_secs(4)
                || !in_corner(touch.from)
                || !in_corner(touch.to)
            {
                taps = 0;
                first = Instant::now();
            }
            if in_corner(touch.from) && in_corner(touch.to) {
                if taps == 0 {
                    first = Instant::now();
                }
                taps += 1;
                if taps >= 5 {
                    break;
                }
            }
        }
    }
    Ok(())
}
