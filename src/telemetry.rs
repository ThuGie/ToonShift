use crate::plugins::Metric;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub fn start() -> Receiver<(Metric, Option<f64>)> {
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || loop {
        if connect(&send).is_err() {
            for metric in [
                Metric::RoomHumidity,
                Metric::RoomTemperature,
                Metric::Electricity,
                Metric::Gas,
            ] {
                if send.send((metric, None)).is_err() {
                    return;
                }
            }
        }
        std::thread::sleep(Duration::from_secs(5));
    });
    receive
}

fn connect(send: &Sender<(Metric, Option<f64>)>) -> io::Result<()> {
    let host = std::fs::read_to_string("/proc/sys/kernel/hostname")?;
    let host = host.trim();
    if host.is_empty()
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(io::Error::other("Invalid BoxTalk hostname"));
    }
    let mut connection = TcpStream::connect("127.0.0.1:1337")?;
    connection.set_read_timeout(Some(Duration::from_secs(120)))?;
    connection.set_write_timeout(Some(Duration::from_secs(2)))?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let pid = std::process::id();
    let uuid = format!("{host}:toonshift-{pid}");
    for event in ["connect", "alive"] {
        write!(connection, "<discovery nts=\"ssdp:{event}\" uuid=\"{uuid}\" type=\"urn:schemas-hcb-hae-com:device:toonshift\" version=\"v\" sessionKey=\"{pid}-{now}\"></discovery>\0")?;
    }
    for service in ["ElectricityFlowMeter", "GasFlowMeter"] {
        write!(connection, "<subscribe uuid=\"{uuid}\" destuuid=\"\"><target uuid=\"\" serviceid=\"urn:hcb-hae-com:serviceId:{service}\"></target></subscribe>\0")?;
    }
    write!(connection, "<action class=\"invoke\" uuid=\"{uuid}\" destuuid=\"{host}:happ_thermstat\" serviceid=\"urn:hcb-hae-com:serviceId:specific1\"><u:UpdateDataSetSubscription xmlns:u=\"urn:hcb-hae-com:service:specific1:1\"><updateAction>add</updateAction><dataSet>thermostatInfo</dataSet><autoExpire>0</autoExpire><supportsNaN>1</supportsNaN><supportsPartial>0</supportsPartial><requestFullSet>1</requestFullSet></u:UpdateDataSetSubscription></action>\0")?;
    write!(connection, "<action class=\"invoke\" uuid=\"{uuid}\" destuuid=\"\" serviceid=\"urn:hcb-hae-com:serviceId:specific1\"><u:reSendAllNotifies xmlns:u=\"urn:hcb-hae-com:service:specific1:1\"></u:reSendAllNotifies></action>\0")?;
    let mut reader = BufReader::new(connection);
    let mut frame = Vec::new();
    loop {
        frame.clear();
        let size = reader.by_ref().take(65536).read_until(0, &mut frame)?;
        if size == 0 || frame.last() != Some(&0) {
            return Err(io::Error::other("Incomplete BoxTalk frame"));
        }
        let Ok(message) = std::str::from_utf8(&frame[..size - 1]) else {
            continue;
        };
        for (tag, metric, source) in [
            ("currentHumidity", Metric::RoomHumidity, "happ_thermstat"),
            ("currentTemp", Metric::RoomTemperature, "happ_thermstat"),
            (
                "CurrentElectricityFlow",
                Metric::Electricity,
                "happ_pwrusage",
            ),
            ("CurrentGasFlow", Metric::Gas, "happ_pwrusage"),
        ] {
            if !message.contains(source) {
                continue;
            }
            if let Some(text) = message
                .split_once(&format!("<{tag}>"))
                .and_then(|(_, body)| body.split_once(&format!("</{tag}>")))
                .map(|(value, _)| value.trim())
            {
                let value = text.parse::<f64>().ok().filter(|value| {
                    value.is_finite()
                        && (metric != Metric::RoomHumidity || (0.0..=100.0).contains(value))
                });
                send.send((metric, value)).map_err(io::Error::other)?;
            }
        }
    }
}

pub type Readings = std::collections::HashMap<Metric, (f64, Instant)>;

pub fn age(metric: Metric, values: &Readings) -> Option<Duration> {
    values.get(&metric).map(|(_, received)| received.elapsed())
}

pub fn display(metric: Metric, values: &Readings) -> String {
    match current(metric, values) {
        Some((value, _)) => match metric {
            Metric::RoomHumidity => format!("{value:.0}% RH"),
            Metric::RoomTemperature => format!("{:.1}°C", value / 100.0),
            Metric::Electricity => format!("{value:.0} W"),
            Metric::Gas => format!("{value:.0} L/h"),
        },
        None if matches!(metric, Metric::RoomHumidity | Metric::RoomTemperature) => {
            "No reading".into()
        }
        None => "No meter data".into(),
    }
}

pub fn current(metric: Metric, values: &Readings) -> Option<(f64, Duration)> {
    let lifetime = if metric == Metric::Gas { 7200 } else { 900 };
    let (value, received) = values.get(&metric)?;
    let age = received.elapsed();
    (age < Duration::from_secs(lifetime)).then_some((*value, age))
}
