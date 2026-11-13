# Toon 2 firmware 6.4.5 parity matrix

Reference hardware is ARMv7 Toon 2. The 29 names below are the stock registration directories under `/qmf/qml/apps`. Some directories only provide translations. A source implementation does not close a hardware or tenant gate.

To reproduce the metadata inventory from a trusted SSH client, run `ssh toon 'sh -s' < scripts/firmware-inventory.sh` in a POSIX shell. Save the output locally and inspect it for identifying paths before sharing.

| Stock app | Firmware or prerequisite | ToonShift status and remaining verification |
| --- | --- | --- |
| airQuality | Thermostat humidity and `hdrv_sensory` | Compensated humidity read verified. CO₂, VOC, thresholds, and sensor fault behavior remain. |
| benchmark | Local display and touch | Compare stock and ToonShift touch latency, cold start, and idle CPU. |
| boilerMonitor | Thermostat, BoilerInfo, boiler adapter | Advanced setup exposes OpenTherm and on/off relay with confirmation and read-back. Reference unit still reports an offline link. Live boiler operation remains unverified. |
| clock | RTC and firmware time service | Home clock and idle clock implemented. Timezone, time sync, and manual clock settings remain. |
| controlPanel | Tenant registration | Map visible stock controls and their firmware contracts. |
| customerService | Tenant and legacy services | Map visible support actions. Add local support and redacted diagnostics export. |
| domesticHotWater | Thermostat DHW BoxTalk actions | Enabled flag, target, and range read verified. Controls need a DHW-capable appliance. |
| eMetersSettings | P1, Z-Wave, power usage services | Counter archives and meter inventory verified. Advanced shows each Z-Wave adapter's electricity, gas, and water endpoint status. A local source override selects electricity and gas independently for live tiles. Physical sensor commissioning, firmware archive routing, and wired P1 setup still use the stock installer. |
| fiona | Tenant-specific registration | Identify visible role before selecting a local equivalent. |
| graph | `hcb_rrd` | Electricity and gas daily charts returned readings on the reference unit. Validate live units and timestamps with an active meter. |
| heatRecovery | `happ_hvac`, configured VHR | Reference unit reported no VHR. Verify on a configured unit. |
| heatingOverview | Thermostat and history | Boiler details and history implemented. Verify real demand, fault recovery, and adapter data. |
| homescreen | Local GUI | Persistent tiles, pages, and reorder implemented. Cross-page drag, tile sizing, and names remain. |
| imageViewer | Local image storage and decode | Map stock formats and browsing flow. Add a bounded local viewer if active. |
| inbox | `happ_usermsg` | Map notification dataset and implement list, read state, and dismissal. |
| installWizard | Stock wizard and config objects | Map steps and dependencies before implementing first-run and commissioning. |
| internetSettings | `hcb_netcon`, wpa_supplicant, DHCP | Scan, credential form, and DHCP rollback implemented. Verify with a test SSID. Static IPv4, forget, and Ethernet remain. |
| settings | Firmware and ToonShift preferences | English/Dutch selection persisted on device. Themes, brightness, and idle settings need full device coverage. |
| smokeDetector | Configured sensor | Verify status, test events, alerts, and faults on real hardware. |
| solar | Configured logger or external collector | Chart and widget implemented. No solar logger on reference unit. Verify with a configured installation. |
| statusUsage | `happ_pwrusage`, `hcb_rrd` | Totals and explicit data gaps implemented. Live readings, tariffs, and meter reset verification remain. |
| strvSettings | Smart radiator valves and HVAC | Identify commissioning flow, then verify real valve operation. |
| systemSettings | Firmware config, watchdog, boot | Stock fallback exercised. Update state, export, and separate preference/factory reset flows remain. |
| thermostat | `happ_thermstat` | Setpoint changes previously verified. HTTP and BoxTalk temperatures matched. Timed hold, faults, and reboot evidence remain. |
| thermostatSettings | Program, states, calibration | Revision-checked schedule write and restore verified. Copy-day has host coverage. Preset writes, vacation, and calibration remain. |
| upsell | Tenant/legacy service | Determine active role and document any local equivalent. |
| weather | Versioned Weather plugin | Device returned live conditions and five daily forecasts. Location search, offline behavior, and radar remain to verify. |
| weatherInt | Tenant weather registration | Verify regional behavior and use the versioned provider where appropriate. |
| winwit | Tenant-specific registration | Identify visible role and firmware dependency. |

## Firmware audit findings

- Port 8000 belongs to OpenVPN. Its configuration binds the management interface to `127.0.0.1:8000`.
- The BoxTalk thermostat action namespace is `urn:hcb-hae-com:serviceId:Thermostat`. `GetTempOffset` returns `offset` in °C and `measuredTemp` in hundredths of °C. `GetBoilerType` returns `ot`. `GetDhwSettings` returns the enabled flag, target, and limits. `GetHolidays` returned an empty list. The `SetHoliday` handler takes `entryId`, `startTimeT`, `endTimeT`, and `setpoint`. Vacation writes need verification before exposing them.
- `hcb_config` exposes the saved wireless interface under `iface-wlan0`. Its payload can contain credentials. Keep the payload out of logs and public fixtures.
- A configured boiler type does not prove that an appliance is connected. The reference unit reported its boiler offline.
- In the last live heating trace, the firmware received a 24.5°C setpoint against a 22.6°C room reading. It requested a 90°C internal boiler target and 100% modulation while reporting `connection=0` and `otCommError=1`. The thermostat daemon owned the native UART. The physical link and both protocol modes still need an on-device test. FreeToon's optional serial proxy forwards the wired adapter traffic and adds telemetry. Its active wireless route requires a separately configured OTGW.
- Advanced setup detected one registered Z-Wave meter and one configured meter source. Touchscreen inclusion started and the controller returned cancellation and stop acknowledgements. No physical device was added or removed during that test.

## Candidate checks on the reference Toon

- Stock Qt started during the reversible UI switch. ToonShift then started with the existing thermostat process still running.
- HTTP and BoxTalk both reported 22.12°C in the same probe, and BoxTalk reported 45% room humidity. Physical accuracy still needs a separate thermometer comparison.
- Bearer authentication rejected an unauthenticated request. A stale schedule revision returned 409. A real schedule edit returned 200, and the original revision matched after restoration. The program was already disabled during this test.
- The energy API returned daily electricity and gas buckets. Export, solar, and water were empty, and missing readings remained explicit.
- The Weather probe returned a current condition and five forecast days. The Store refreshed its cached official catalog from GitHub.
- The touchscreen saved Dutch and was returned to the original English preference. The Comfort preset was changed from 18.5°C to 19°C and restored.
- After a full reboot, ToonShift and the thermostat service restarted. All 19 weekly periods and all five presets matched the pre-upgrade backup by field value. The firmware Wi-Fi scan returned results. A test SSID join, live appliance test, stock performance comparison, and 24-hour soak are still open.
