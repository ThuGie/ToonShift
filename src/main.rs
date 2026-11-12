mod plugins;
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
    };
    let view = ui::View {
        settings: &settings,
        plugins: &plugins,
        status: Some(&status),
        pending: false,
        error: None,
        time: if night { "22:18" } else { "12:18" },
        hour: 12,
        root,
        schedule: None,
        networks: None,
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
    let (commands, updates) = thermostat::worker();
    if let Err(error) = api::start(root, commands.clone()) {
        eprintln!("ToonShift API unavailable: {error}");
    }
    let mut settings = plugins::Settings::load(root);
    let mut catalog = plugins::catalog(root);
    let mut status = None;
    let mut schedule = None;
    let (network_sender, network_receiver) = std::sync::mpsc::channel();
    let mut networks = None;
    let mut scanning = false;
    let mut pending = false;
    let mut error = None;
    let mut dirty = true;
    let mut ui = ui::Ui::new();
    loop {
        if let Ok(result) = network_receiver.try_recv() {
            networks = Some(result);
            scanning = false;
            dirty = true;
        }
        for update in updates.try_iter() {
            status = update.status;
            error = update.error;
            if let Some(result) = update.schedule {
                schedule = Some(result.map(|slots| {
                    slots
                        .into_iter()
                        .map(|slot| ui::ScheduleRow {
                            day: slot.day,
                            start: device::format_time(slot.start).1,
                            end: device::format_time(slot.end).1,
                            preset: slot.preset,
                        })
                        .collect()
                }));
            }
            if update.completed {
                pending = false;
            }
            dirty = true;
        }
        if dirty {
            let (hour, time) = device::time();
            ui.draw(&ui::View {
                settings: &settings,
                plugins: &catalog,
                status: status.as_ref(),
                pending,
                error: error.as_deref(),
                time: &time,
                hour,
                root,
                schedule: schedule.as_ref(),
                networks: networks.as_ref(),
            });
            display.present(&ui.pixels)?;
            dirty = false;
        }
        let (x, y) = match touch.recv_timeout(Duration::from_millis(35)) {
            Ok(event) => event?,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                return Err("Touchscreen reader disconnected".into())
            }
        };
        let mut next = settings.clone();
        let mut persist = true;
        match ui.hit(x, y) {
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
            Some(Action::Schedule) => {
                commands
                    .send(thermostat::Work::from_ui(thermostat::Command::Schedule))
                    .map_err(|_| "Thermostat worker stopped")?;
                persist = false;
            }
            Some(Action::Network) => {
                if !scanning {
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
                                    rows
                                });
                        let _ = sender.send(result);
                    });
                }
                persist = false;
            }
            Some(Action::Store) => {
                persist = false;
                catalog = plugins::catalog(root);
                ui.reset_store_page();
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
