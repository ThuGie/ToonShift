# ToonShift and firmware APIs

## ToonShift control API v1

ToonShift binds HTTP to `127.0.0.1:10082` on the device. It requires a random 256-bit bearer token kept in the root-only `/mnt/data/toon-rust/api-token` file. The connection from a client to the device can run inside SSH with the device host key pinned. The API accepts JSON request bodies up to 256 bytes. Setpoint and schedule writes share the same worker as touchscreen commands.

```sh
ssh -N -L 127.0.0.1:18082:127.0.0.1:10082 toon
```

From another terminal, load the token into an environment variable without printing it. An Android client should keep its SSH and API credentials in platform-backed secure storage. Access to the root token file is part of device provisioning.

```sh
TOONSHIFT_TOKEN=$(ssh toon 'cat /mnt/data/toon-rust/api-token')
curl -H "Authorization: Bearer $TOONSHIFT_TOKEN" http://127.0.0.1:18082/v1/status
```

| Method and path | JSON body | Result |
| --- | --- | --- |
| `GET /v1/status` | — | `currentTemp`, `currentSetpoint`, `programState`, `burnerInfo`, `connection`, `nextSetpoint`, `nextTime` as strings |
| `GET /v1/events` | — | Authenticated Server-Sent Events with `status` or `unavailable` events |
| `PUT /v1/setpoint` | `{"centi_celsius":2050}` | Sets the target to 20.50°C. Range is 600–3000 in hundredths of a degree Celsius |
| `PUT /v1/schedule` | `{"enabled":true}` | Enables or disables the stored program |
| `GET /v1/program` | — | Weekly periods with `start` and `end` as minute offsets from Sunday midnight and numeric `preset` |
| `GET /v1/program?revision=1` | — | Returns the periods and their revision token |
| `PUT /v1/program` | `{"revision":"0123456789abcdef","edit":{"operation":"copy_day","source":0,"days":62}}` | Applies a revision-checked edit. Weekday bits run from Sunday (1) to Saturday (64). A stale revision returns 409 |
| `GET /v1/presets` | — | Preset ID, temperature in hundredths of °C, and hot-water setting |
| `PUT /v1/presets` | `{"id":1,"temperature":1950,"hot_water":true}` | Saves a preset through the touchscreen thermostat worker |
| `GET /v1/capabilities` | — | Boiler protocol and link state, hot-water capabilities when present, and configured meter adapters and channels. Unavailable subsystems are null |
| `GET /v1/diagnostics` | — | Firmware version and BoxTalk temperature offset, measured temperature, and boiler/DHW configuration |
| `GET /v1/version` | — | ToonShift version and installed firmware version, with firmware reported as null when unavailable |
| `PUT /v1/preset` | `{"id":1}` | Selects Comfort (0), Home (1), Sleep (2), Away (3), or Vacation (4) |
| `PUT /v1/hold` | `{"preset":1,"mode":"until_next_period"}` | Applies a preset from 0 to 3 until the next scheduled period. Use `"indefinite"` to hold until manually changed. Resume through `PUT /v1/schedule` |
| `GET /v1/energy?meter=electricity&period=day` | — | Time bounds, chart buckets, unit, recorded total, and a flag for incomplete readings |
| `GET /v1/energy.csv?meter=electricity&period=day` | — | Downloadable CSV of the same chart buckets. Times are Unix seconds with an exclusive lower bound and inclusive upper bound. Missing readings have an empty value and `window_partial` applies to the whole chart |
| `GET /v1/vacations` | — | Firmware vacation entries with ID and Unix start and end seconds |
| `POST /v1/vacations` | `{"start":1800000000,"end":1800086400}` | Adds a future, non-overlapping period using the stored Vacation preset |
| `PUT /v1/vacations` | `{"id":"…","start":1800000000,"end":1800086400}` | Changes the dates of an existing future vacation |
| `DELETE /v1/vacations` | `{"id":"…"}` | Removes the identified vacation entry and reads the list back |

Success returns HTTP 200. Writes return `{"result":"ok"}` when the firmware acknowledges the command. Read status to confirm the effective state. Missing or invalid tokens get 401, unknown routes get 404, invalid input gets 422, firmware write failures get 502, service failures get 503, and a 504 means the write outcome is unknown.

The event stream sends the latest full status upon connection and again when status changes. `unavailable` carries an empty JSON object. The stream sends keepalive comments every ten seconds of inactivity, closes after about five minutes, and supports four simultaneous subscriptions. Reconnect using the bearer token to get the current snapshot. The stream does not retain historical events.

Rotate the token by moving the token file out of the device's application directory, restarting ToonShift, and provisioning the new token to clients. The existing token remains active until the process restarts. Keep the SSH tunnel local to the client and use SSH for transport encryption. This API does not configure the firmware's separate web listeners.

## Firmware inventory

The following inventory was gathered over SSH on a Toon 2 running firmware 6.4.5. The local HTTP convention is `http://127.0.0.1:10080/<service>?action=<action>`. The firmware listens on ports 80 and 10080. Behavior, permissions, and route availability vary by image and rooting setup. Some actions use legacy JavaScript-like text instead of strict JSON. Probe with read operations over SSH.

### Thermostat, schedules, and history

| Action on `happ_thermstat` | Evidence and behavior |
| --- | --- |
| `getThermostatInfo` | Verified JSON. Temperatures `currentTemp`, `currentSetpoint`, and `nextSetpoint` are strings in hundredths of °C. `nextTime` is Unix seconds or zero. Includes `currentInternalBoilerSetpoint`, `activeState`, `programState`, `burnerInfo`, `connection`, `errorFound`, `otCommError`, and `currentModulationLevel` |
| `setSetpoint&Setpoint=650` | Verified by moving from 6.0°C to 6.5°C and restoring 6.0°C. Capitalization of `Setpoint` matters |
| `changeSchemeState&state=0` or `state=1` | Manual/program action used by the Rust UI and documented by the rooted integration. Live program switching was left untested during deployment |
| `changeSchemeState&state=2&temperatureState=N` | Temporary preset override described by the rooted integration. Preset values are 0 Comfort, 1 Home, 2 Sleep, 3 Away |
| `getSchemeState` | Verified legacy response shaped like `{schemeState:'0'}` |
| `getWeeklyList` | Verified legacy schedule list with `targetState`, `weekDay`, `startTimeT`, `endTimeT`. Times are Unix seconds. Weekday 0 corresponds to Sunday on the reference device |
| `getActiveNext` | Verified legacy response with active and next program, setpoint, preset, and time bounds |
| `getHeatingHistory` | Verified legacy arrays `setpoints` and `roomTemps`. Timestamp and sampling semantics need verification |
| `getVHRInfo` | Handler responded that no ventilation device was found |

Program states 0, 1, and 2 represent manual, active schedule, and temporary override. Burner states 0–3 represent idle, room heating, hot water, and preheating. The reference device reported no boiler connection during testing. [Rooted Toon climate integration](https://github.com/cyberjunky/home-assistant-toon_climate)

The editor reads the recurring program through `hcb_config?action=getObjectConfig&package=happ_thermstat&internalAddress=thermostatProgram`. Firmware 6.4.5 writes it through `setObjectConfig` with `packageName=happ_thermstat` and a URL-encoded XML `config` argument. Use a `Config` document containing only the target `device`, its UUID and program metadata, and its `schedule`. JSON writes can return `result: ok` without applying the edit. ToonShift reads the program back after each write.

The stock interface subscribes to the BoxTalk `thermostatInfo` dataset for compensated room readings. ToonShift uses HTTP `currentTemp` for the dashboard and the same BoxTalk dataset for humidity. Its diagnostic `--probe-telemetry` prints both temperature sources. Verifying physical calibration requires a separate room thermometer. [FreeToon BoxTalk implementation](https://github.com/Ierlandfan/freetoon-lvgl/blob/main/lvgl_ui_recovered/src/boxtalk.c)

### Network, configuration, meters, and accessories

| Service and actions | Evidence and behavior |
| --- | --- |
| `hcb_netcon` · `getWirelessNetworks` | Verified scan results with `ESSID`, `Address`, `Signal`, `Quality`, `Channel`, and security fields. Can take several seconds |
| `hcb_netcon` · `getWirelessNetworkInformation` | Verified output with connection details including SSID and IP configuration. Treat this response as private |
| `hcb_netcon` · `SetWirelessNetworkInformation` | BoxTalk `NetworkInformation` action. Firmware parses `iface`, `type`, `essid`, `enc`, `auth`, `key`, and optional `bssid`. Static IPv4 adds `ipaddress`, `netmask`, `default_gateway`, and `nameserver`. Live writes need verification |
| `hcb_netcon` · `ResetWirelessNetwork` | Binary inspection shows a reset of the current default-route interface through `ifi_defgw_reset`. It does not clear saved credentials. ToonShift checks the saved Wi-Fi association after requesting reconnect |
| `hcb_netcon` · `getWirelessChannels` | Identified in the firmware. Timed out during the read probe |
| `hcb_config` · `getLocale`, `getVersionInfo.json` | Verified locale/timezone and firmware/platform metadata |
| `hcb_config` · `getDeviceList.json`, `getLocationList.json`, `getObjectList.json`, `getPackageHandlerList.json` | Verified inventory responses. They may contain private device identifiers |
| `hcb_config` · `getObjectConfig.json`, `getObjectConfigTree.json` | Handler reports required UUID or package/internal-address identifiers |
| `hcb_config` · `getPackageConfig.json`, `getPackageConfigTree.json` | Handler reports required package name |
| `hcb_config` · `getBinaryVersionsList.json` | Found in binary strings. Timed out during the read probe |
| `hcb_rrd` · `getRrdData` | Verified with `loggerName`, `rra`, `from` and `to` Unix seconds, `nullForNaN=1`, and `samples`. Without `readableTime`, timestamp keys are Unix seconds. Flow uses `5min` archives. Verified electricity and gas counters in `5yrhours` and `10yrdays`. The reference Toon had no solar logger |
| `happ_pwrusage` · `GetHistoricTariffs.json` | Handler responded that the historic tariff file could not be opened |
| `hdrv_zwave` · `getDevices.json` | Verified inventory with connection and sensor fields. On the reference device, known electricity and gas devices reported `NaN` values |
| `hdrv_zwave` · `getControllerInfo` | Verified controller metadata. Treat network identifiers as private |

The firmware 6.4.5 `hdrv_p1` service registration places the meter actions in its `default` service. Binary analysis identified `GetMeasureCapabilities` and `GetSensorConfiguration` with a `deviceUuid` argument and `GetMeterConfiguration` with `devUuid`. `GetAvailableSensorsForConfiguration` emits a `devices` list. The write handlers `SetSensorConfiguration` and `SetMeterConfiguration` process nested device payloads and return `success` and `reason`. Run `/mnt/data/toon-rust/toon-rust --probe-meter-configuration` over SSH to check the read contract. The probe reports source numbers and known sensor names without printing device IDs. These BoxTalk calls still need a live firmware check before wiring the write controls.

The Z-Wave browser client under `/qmf/www/hdrv_zwave/js/` references reads such as `GetAssociation`, `GetBattery`, `GetConfiguration`, `GetControlStatus`, `GetWakeUpInterval`, and `MeterGet`. It also references writes such as `IncludeDevice`, `ExcludeDevice`, `SetAssociation`, `SetConfigurationInt`, `replaceFailedNode`, and `revertToFactoryDefaults`. The client is the local source for exact parameters. These actions were identified by inspection.

### Local device interfaces and discovery

- BoxTalk listens on `127.0.0.1:1337`. It uses XML messages with NUL framing for queries, commands, and subscriptions. Device-facing service names can be inspected in `/HCBv2/etc/start.d`. [FreeToon BoxTalk client](https://github.com/Ierlandfan/freetoon-lvgl/blob/main/lvgl_ui_recovered/src/boxtalk.c)
- Display uses `/dev/fb0` with a 1024 × 600 visible area and two virtual pages. The active page offset can be read from `/sys/class/graphics/fb0/pan`. Touch events are on `/dev/input/event1`. Backlight controls are under `/sys/class/backlight/mp3309-bl`.
- Wi-Fi has a firmware-managed WPA supplicant socket at `/run/wpa_supplicant/wlan0`. ToonShift submits DHCP join settings through `SetWirelessNetworkInformation` and checks association and addressing. Existing DHCP settings can be restored if the new connection fails. Validate the join and rollback against a test SSID before deployment on another device.
- OpenVPN owns the loopback management listener on port 8000. See the [parity matrix](parity-6.4.5.md) for verified BoxTalk actions and open gates.
- For additional undocumented actions, inspect the stock web client, handler names in service binaries, and read-only requests. A symbol name alone is insufficient to identify an HTTP action.
