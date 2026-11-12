# ToonShift

<img src="assets/toonshift-mark.png" alt="ToonShift logo" width="112">

ToonShift is a native Rust touchscreen interface for the Eneco Toon 2. It draws directly to the framebuffer, reads the touchscreen through evdev, and uses the firmware's local thermostat service for heat and scheduling.

## On the device

- View the firmware's compensated room temperature, adjust the setpoint, and toggle the existing program. Choose a day in **Schedule** and tap **Edit** to change, split, or remove a heating period. Use **Add period** when the day has an open interval.
- Choose automatic, day, or night appearance, adjust the display brightness and idle screen, and select **English** or **Nederlands**.
- Install widgets from Store. Home has four widget slots per page and supports up to eight pages. Tap **Edit tiles**, choose a tile, and then choose its destination. The page arrows work while editing. Tap the installed Weather tile to open its forecast and location settings.
- Use **Energy** to browse electricity, gas, solar, and water archives across hourly, daily, weekly, monthly, and yearly ranges. Available charts depend on the connected meter and the samples it has saved.
- Use **Settings → Wi-Fi** to scan for and join networks. WPA/WPA2 credentials go directly to the local firmware service.
- Open **Schedule → Presets** to adjust the saved heating modes. Tap the Boiler tile for heating history and sensor information.
- Open **Settings → Advanced setup** to choose OpenTherm or on/off relay, see boiler connection status, configure hot water when an OpenTherm boiler is connected, and manage registered Z-Wave devices. The Meters tab shows configured sources and has a shortcut to the stock meter installer.

The stock meter installer is a temporary session. Tap the top-left corner five times within four seconds to return to ToonShift, or wait ten minutes. Rebooting also returns to ToonShift. The separate **Switch to stock UI** option in Settings remains available for persistent recovery.
- Use the authenticated, loopback-only [ToonShift control API](docs/api.md) for integration over SSH.

## Build and install

The initial target is Toon 2 firmware 6.4.5 on ARMv7 with a 1024 × 600 touchscreen. Verify compatibility before switching another device.

```sh
rustup target add armv7-unknown-linux-musleabihf
cargo build --locked --release --target armv7-unknown-linux-musleabihf
```

Configure an SSH host alias named `toon` with the device's host key pinned. The following expects a rooted Toon with SSH and legacy SCP available.

```sh
ssh toon 'umask 077; mkdir -p /mnt/data/toon-rust/catalog /mnt/data/toon-rust/values'
scp -O target/armv7-unknown-linux-musleabihf/release/toon-rust scripts/launcher.sh scripts/switch-ui.sh toon:/mnt/data/toon-rust/
ssh toon 'cd /mnt/data/toon-rust && chmod 755 toon-rust launcher.sh switch-ui.sh && ./switch-ui.sh rust'
```

The switch script probes the framebuffer, touchscreen, and thermostat first. It backs up `/etc/inittab` and changes the UI entry. To switch back over SSH, run `/mnt/data/toon-rust/switch-ui.sh stock`. The touchscreen's **Switch to stock UI** action also opens the original interface. Run the script with `rust` to return. The launcher falls back to stock if ToonShift exits. Its log is at `/var/volatile/tmp/toon-rust.log`.

When updating an active install, upload the binary under a temporary name and run it with `--probe`. Switch to stock before replacing the active binary, then switch back to Rust.

## Project docs

- [Parity and performance roadmap](docs/roadmap.md)
- [Firmware 6.4.5 parity matrix and device checks](docs/parity-6.4.5.md)
- [Firmware API inventory and ToonShift control API](docs/api.md)
- [Plugin format and additional repositories](docs/plugins.md)
- [Official ToonShift plugin catalog](https://github.com/ThuGie/ToonShift-plugins)
