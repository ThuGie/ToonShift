use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    English,
    Dutch,
}

impl Language {
    pub fn translate(self, message: &str) -> &str {
        if self == Self::English {
            return message;
        }
        match message {
            "Schedule" => "Schema",
            "Home" => "Thuis",
            "Energy" => "Energie",
            "Boiler module offline" => "Ketelmodule offline",
            "Boiler module connected" => "Ketelmodule verbonden",
            "On/off relay · module connected" => "Aan/uit-relais · module verbonden",
            "On/off relay · link unknown" => "Aan/uit-relais · verbinding onbekend",
            "On/off relay · heat requested" => "Aan/uit-relais · warmtevraag",
            "On/off relay · heating idle" => "Aan/uit-relais · geen warmtevraag",
            "OpenTherm communication error" => "OpenTherm-communicatiefout",
            "OpenTherm connected" => "OpenTherm verbonden",
            "OpenTherm · link unknown" => "OpenTherm · verbinding onbekend",
            "Heat requested" => "Warmtevraag",
            "Smart home" => "Slim huis",
            "Switches" => "Schakelaars",
            "Checking Home Assistant…" => "Home Assistant controleren…",
            "No selected switches found" => "Geen geselecteerde schakelaars gevonden",
            "Connect Home Assistant to get started." => "Verbind Home Assistant om te beginnen.",
            "Connection setup" => "Verbinding instellen",
            "Home Assistant HTTPS URL" => "Home Assistant HTTPS-adres",
            "Long-lived access token" => "Langlevend toegangstoken",
            "Find switches" => "Schakelaars zoeken",
            "Choose up to 32 switches" => "Kies maximaal 32 schakelaars",
            "No switches found in Home Assistant" => "Geen schakelaars gevonden in Home Assistant",
            "Save connection" => "Verbinding opslaan",
            "Disconnect" => "Verbinding verwijderen",
            "Remove the Home Assistant connection?" => "De Home Assistant-verbinding verwijderen?",
            "Selected" => "Geselecteerd",
            "Select" => "Selecteren",
            "All widgets" => "Alle widgets",
            "On dashboard" => "Op dashboard",
            "No widgets on the dashboard" => "Geen widgets op het dashboard",
            "Heating hot water" => "Warm water opwarmen",
            "Heating idle" => "Verwarming in rust",
            "+ Add widget" => "+ Widget toevoegen",
            "Live" => "Actueel",
            "Gaps and muted bars indicate missing data" => {
                "Gaten en grijze balken tonen ontbrekende data"
            }
            "Just received" => "Zojuist ontvangen",
            "min ago" => "min geleden",
            "Recorded flow" => "Gemeten aanvoer",
            "Return" => "Retour",
            "Heating fault" => "Verwarmingsstoring",
            "Heating fault · OpenTherm error" => "Verwarmingsstoring · OpenTherm-fout",
            "Boiler link unknown" => "Ketelverbinding onbekend",
            "Ethernet connected" => "Ethernet verbonden",
            "Ethernet disconnected" => "Ethernet niet verbonden",
            "Ethernet link unknown" => "Ethernet-linkstatus onbekend",
            "Reconnect saved network" => "Opnieuw verbinden",
            "Reconnecting to saved Wi-Fi…" => "Opnieuw verbinden met opgeslagen Wi-Fi…",
            "No saved Wi-Fi network." => "Geen opgeslagen Wi-Fi-netwerk.",
            "Could not request a Wi-Fi reconnect." => "Kon niet opnieuw verbinden met Wi-Fi.",
            "Could not reconnect to the saved Wi-Fi network." => {
                "Kon niet verbinden met het opgeslagen Wi-Fi-netwerk."
            }
            "IP address" => "IP-adres",
            "Subnet mask" => "Subnetmasker",
            "Gateway" => "Gateway",
            "DNS server" => "DNS-server",
            "Back to Wi-Fi credentials" => "Terug naar Wi-Fi-inloggegevens",
            "Check the IPv4 address, subnet mask, gateway and DNS server." => {
                "Controleer het IPv4-adres, subnetmasker, gateway en DNS-server."
            }
            "Waiting for IP address" => "Wachten op IP-adres",
            "Wi-Fi disconnected" => "Wi-Fi niet verbonden",
            "Wi-Fi connection status unavailable" => "Wi-Fi-verbindingsstatus niet beschikbaar",
            "Reset preferences" => "Voorkeuren herstellen",
            "Wait for the plugin operation to finish." => {
                "Wacht tot de pluginbewerking is voltooid."
            }
            "Reset ToonShift preferences?" => "ToonShift-voorkeuren herstellen?",
            "Restore default display, dashboard, plugin and energy preferences." => {
                "Herstel de standaardvoorkeuren voor scherm, dashboard, plugins en energie."
            }
            "Vacations" => "Vakanties",
            "Start date" => "Begindatum",
            "End date" => "Einddatum",
            "Add vacation" => "Vakantie toevoegen",
            "Remove this vacation?" => "Deze vakantie verwijderen?",
            "Checking vacation schedule…" => "Vakantieschema controleren…",
            "No vacations scheduled" => "Geen vakanties gepland",
            "Enter two valid local dates and times." => {
                "Voer twee geldige lokale datums en tijden in."
            }
            "Enter year, month, day, hour and minute (YYYYMMDDHHMM)." => {
                "Voer jaar, maand, dag, uur en minuut in (JJJJMMDDUUMM)."
            }
            "Hold indefinitely" => "Vasthouden",
            "Until next period" => "Tot volgende periode",
            "Hold until next period · Resume schedule" => {
                "Tot volgende periode · Programma hervatten"
            }
            "Indefinite hold · Resume schedule" => "Vastgehouden · Programma hervatten",
            "Z-Wave inventory unavailable" => "Z-Wave-apparatenlijst niet beschikbaar",
            "Catalog sources" => "Catalogusbronnen",
            "Remove catalog source?" => "Catalogusbron verwijderen?",
            "Catalog source removed" => "Catalogusbron verwijderd",
            "No GitHub catalogs have been added." => "Er zijn geen GitHub-catalogi toegevoegd.",
            "Its widgets become unavailable until the source is added again." => {
                "De widgets zijn pas weer beschikbaar nadat je de bron opnieuw toevoegt."
            }
            "Idle content" => "Inhoud rustscherm",
            "Show dashboard" => "Dashboard tonen",
            "Dashboard page" => "Dashboardpagina",
            "Wide tiles" => "Brede tegels",
            "Dashboard page name" => "Naam van dashboardpagina",
            "Duplicate tile" => "Tegel dupliceren",
            "Reset layout" => "Indeling herstellen",
            "Reset dashboard layout?" => "Dashboardindeling herstellen?",
            "Restore the default tiles and clear page names." => {
                "Herstel de standaardtegels en wis paginanamen."
            }
            "Drag a tile to an edge or tap to move." => {
                "Sleep een tegel naar de rand of tik om te verplaatsen."
            }
            "Up to 18 characters" => "Maximaal 18 tekens",
            "Roll back" => "Terugzetten",
            "Restoring previous version…" => "Vorige versie terugzetten…",
            "Export diagnostics" => "Diagnostiek exporteren",
            "Exporting diagnostics…" => "Diagnostiek exporteren…",
            "Saved diagnostics.json in the ToonShift data directory." => {
                "diagnostics.json opgeslagen in de ToonShift-map."
            }
            "Could not save diagnostics.json." => "Kon diagnostics.json niet opslaan.",
            "Energy rates" => "Energietarieven",
            "Standard electricity (€/kWh)" => "Normaaltarief stroom (€/kWh)",
            "Off-peak electricity (€/kWh)" => "Daltarief stroom (€/kWh)",
            "Gas rate (€/m³)" => "Gastarief (€/m³)",
            "Estimated cost" => "Geschatte kosten",
            "Not set" => "Niet ingesteld",
            "Enter a rate from 0 to 99.999." => "Voer een tarief in van 0 tot 99,999.",
            "Save an empty field to clear the rate." => {
                "Sla een leeg veld op om het tarief te wissen."
            }
            "Store" => "Winkel",
            "Settings" => "Instellingen",
            "Language" => "Taal",
            "Advanced setup" => "Geavanceerd instellen",
            "Meters" => "Meters",
            "Boiler link offline" => "Ketelverbinding offline",
            "Heat requested · boiler link offline" => "Warmtevraag · ketelverbinding offline",
            "Boiler firing for heat" => "Ketel verwarmt",
            "Heat requested · burner not confirmed" => "Warmtevraag · branderstatus onbevestigd",
            "No channel reported" => "Geen kanaal gemeld",
            "Counter available" => "Meterstand beschikbaar",
            "Channel present · no reading" => "Kanaal aanwezig · geen meting",
            "Use this adapter" => "Gebruik deze adapter",
            "Use firmware default" => "Gebruik firmwarebron",
            "Communication protocol" => "Communicatieprotocol",
            "On/off relay" => "Aan/uit-relais",
            "Change boiler protocol" => "Ketelprotocol wijzigen",
            "Apply protocol" => "Protocol toepassen",
            "Use OpenTherm for this boiler?" => "OpenTherm gebruiken voor deze ketel?",
            "Use the on/off relay for this boiler?" => "Aan/uit-relais gebruiken voor deze ketel?",
            "Choose the protocol specified for your boiler and adapter." => {
                "Kies het protocol van je ketel en adapter."
            }
            "Domestic hot water" => "Warm tapwater",
            "Saved hot-water target" => "Opgeslagen tapwatertemperatuur",
            "Stock meter installer" => "Originele meterinstallatie",
            "Open stock for 10 min" => "Open origineel 10 min",
            "To return sooner, tap the top-left corner five times." => {
                "Tik vijf keer linksboven om eerder terug te keren."
            }
            "Hot-water controls require a connected OpenTherm boiler." => {
                "Tapwaterinstellingen vereisen een verbonden OpenTherm-ketel."
            }
            "Reading boiler settings…" => "Ketelinstellingen laden…",
            "Reading device inventory…" => "Apparaten laden…",
            "configured meter sources" => "ingestelde meterbronnen",
            "Registered" => "Geregistreerd",
            "No matching devices" => "Geen passende apparaten",
            "Add Z-Wave device" => "Z-Wave-apparaat toevoegen",
            "Remove Z-Wave device" => "Z-Wave-apparaat verwijderen",
            "Start, then press the setup button on the device." => {
                "Start en druk op de instelknop van het apparaat."
            }
            "Start setup" => "Instellen starten",
            "Cancel setup" => "Instellen annuleren",
            "Z-Wave setup" => "Z-Wave instellen",
            "Choose which security keys to grant." => {
                "Kies welke beveiligingssleutels je toestaat."
            }
            "Controller PIN mode" => "Controller-pincode",
            "Enter device PIN digits" => "Voer pincodecijfers in",
            "Enter the controller PIN on the Z-Wave device." => {
                "Voer de controller-pincode in op het Z-Wave-apparaat."
            }
            "Waiting for the controller. Press the setup button on the device." => {
                "Wachten op de controller. Druk op de instelknop van het apparaat."
            }
            "Save name" => "Naam opslaan",
            "Rename" => "Naam wijzigen",
            "Switch on" => "Inschakelen",
            "Switch off" => "Uitschakelen",
            "Back" => "Terug",
            "Continue" => "Doorgaan",
            "Working…" => "Bezig…",
            "Display" => "Scherm",
            "Tap to wake" => "Tik om te activeren",
            "Day brightness" => "Helderheid overdag",
            "Night brightness" => "Helderheid 's nachts",
            "Night starts at" => "Nacht begint om",
            "Day starts at" => "Dag begint om",
            "Idle display after" => "Rustscherm na",
            "Never" => "Nooit",
            "Automatic day hours" => "Automatische dagstand",
            "Details" => "Details",
            "Heating and sensors" => "Verwarming en sensoren",
            "Refresh" => "Vernieuwen",
            "Boiler offline" => "Ketel offline",
            "Boiler connected" => "Ketel verbonden",
            "Room sensor" => "Kamersensor",
            "Calibration offset" => "Kalibratiecorrectie",
            "Loading heating history…" => "Verwarmingshistorie laden…",
            "Recent temperature history" => "Recente temperatuurhistorie",
            "Last raw sensor" => "Laatste ruwe meting",
            "No heating history" => "Geen verwarmingshistorie",
            "Back to readings" => "Terug naar metingen",
            "Change location" => "Wijzig locatie",
            "Search location" => "Zoek locatie",
            "City or postcode" => "Plaats of postcode",
            "No matching locations" => "Geen passende locaties",
            "Searching…" => "Zoeken…",
            "Search" => "Zoeken",
            "Search again" => "Opnieuw zoeken",
            "Loading…" => "Laden…",
            "Location" => "Locatie",
            "Hidden network" => "Verborgen netwerk",
            "Scan again" => "Opnieuw zoeken",
            "Wi-Fi scan unavailable" => "Wi-Fi-scan niet beschikbaar",
            "Scanning for networks…" => "Netwerken zoeken…",
            "No nearby networks found" => "Geen netwerken gevonden",
            "Join" => "Verbind",
            "Previous" => "Vorige",
            "Next" => "Volgende",
            "Join Wi-Fi" => "Verbinden met wifi",
            "Network name" => "Netwerknaam",
            "Password" => "Wachtwoord",
            "Cancel" => "Annuleren",
            "Join network" => "Verbinden",
            "More" => "Meer",
            "Shift" => "Hoofdletter",
            "Space" => "Spatie",
            "Backspace" => "Wissen",
            "Edit heating period" => "Verwarmingsperiode wijzigen",
            "Add heating period" => "Verwarmingsperiode toevoegen",
            "Starts" => "Begint",
            "Ends" => "Eindigt",
            "Sleep" => "Slapen",
            "Away" => "Weg",
            "Vacation" => "Vakantie",
            "Unknown" => "Onbekend",
            "Shared boundaries move with adjacent periods." => {
                "Aansluitende periodes verschuiven mee."
            }
            "Split" => "Splitsen",
            "Remove" => "Verwijderen",
            "Save" => "Opslaan",
            "Weekly schedule" => "Weekschema",
            "Copy day" => "Kopieer dag",
            "Copy heating day" => "Verwarmingsdag kopiëren",
            "Replace the schedule on the selected days." => {
                "Vervang het schema op de gekozen dagen."
            }
            "Weekdays" => "Werkdagen",
            "Copy" => "Kopieer",
            "Presets" => "Standen",
            "Sun" => "Zo",
            "Mon" => "Ma",
            "Tue" => "Di",
            "Wed" => "Wo",
            "Thu" => "Do",
            "Fri" => "Vr",
            "Sat" => "Za",
            "Loading schedule…" => "Schema laden…",
            "Add period" => "Periode toevoegen",
            "No entries for this day" => "Geen periodes op deze dag",
            "Edit" => "Wijzig",
            "Target temperature" => "Gewenste temperatuur",
            "Hot water on" => "Warm water aan",
            "Hot water off" => "Warm water uit",
            "Heating presets" => "Verwarmingsstanden",
            "Use preset" => "Gebruik stand",
            "Loading presets…" => "Standen laden…",
            "Room temperature" => "Kamertemperatuur",
            "Schedule on" => "Schema aan",
            "Schedule off" => "Schema uit",
            "Updating thermostat…" => "Thermostaat bijwerken…",
            "Waiting for thermostat…" => "Wachten op thermostaat…",
            "Unavailable" => "Niet beschikbaar",
            "Browse widgets" => "Bekijk widgets",
            "Done" => "Klaar",
            "Edit tiles" => "Wijzig tegels",
            "Choose a destination. You can change pages." => {
                "Kies een plek, ook op een andere pagina."
            }
            "Choose the tile you want to move." => "Kies de tegel die je wilt verplaatsen.",
            "Electricity" => "Stroom",
            "Grid export" => "Teruglevering",
            "Solar" | "Solar output" => "Zonne-energie",
            "Water" => "Water",
            "Hour" => "Uur",
            "Day" => "Dag",
            "Week" => "Week",
            "Month" => "Maand",
            "Year" => "Jaar",
            "Loading meter history…" => "Meterhistorie laden…",
            "No meter readings in this period." => "Geen meterstanden in deze periode.",
            "Average per interval" => "Gemiddelde per interval",
            "Recorded total" => "Gemeten totaal",
            "Earlier" => "Eerder",
            "Chart" => "Grafiek",
            "Homepage layout" => "Startschermindeling",
            "Software updates" => "Software-updates",
            "Remote access" => "Externe toegang",
            "Remote access on" => "Externe toegang aan",
            "Remote access off" => "Externe toegang uit",
            "Compare the code with your phone." => "Vergelijk de code met je telefoon.",
            "3. Compare this code with your phone." => "3. Vergelijk deze code met je telefoon.",
            "2. Scan this code in the Android app" => "2. Scan deze code in de Android-app",
            "Use the same Wi-Fi as this Toon." => "Gebruik hetzelfde wifi-netwerk als deze Toon.",
            "Then compare the code on both screens." => {
                "Vergelijk daarna de code op beide schermen."
            }
            "Starting secure access…" => "Veilige toegang wordt gestart…",
            "1. Enable secure access for your phone" => {
                "1. Schakel veilige toegang voor je telefoon in"
            }
            "Enable and pair phone" => "Inschakelen en telefoon koppelen",
            "Connect the Toon to your home network first." => {
                "Verbind de Toon eerst met je thuisnetwerk."
            }
            "Turn off remote access" => "Externe toegang uitschakelen",
            "Phone approved. Return to the Android app." => {
                "Telefoon goedgekeurd. Ga terug naar de Android-app."
            }
            "Remote access could not start. Check the LAN address." => {
                "Externe toegang kon niet starten. Controleer het LAN-adres."
            }
            "Allow control" => "Bediening toestaan",
            "Scan with ToonShift Android" => "Scan met ToonShift Android",
            "This code expires after two minutes." => "Deze code verloopt na twee minuten.",
            "Pair phone" => "Telefoon koppelen",
            "Choose the Toon's reserved LAN address" => {
                "Kies het gereserveerde LAN-adres van de Toon"
            }
            "Changing address revokes paired phones." => {
                "Een ander adres trekt bestaande koppelingen in."
            }
            "Paired phones" => "Gekoppelde telefoons",
            "Revoke" => "Intrekken",
            "Heating behaviour" => "Verwarmingsgedrag",
            "Firmware control" => "Firmwareregeling",
            "Learn and advise" => "Leren en adviseren",
            "Reset learning" => "Leergegevens wissen",
            "Adaptive control needs verified firmware support." => {
                "Adaptieve regeling vereist geverifieerde firmwareondersteuning."
            }
            "Firmware is controlling heating" => "De firmware regelt de verwarming",
            "Waiting for valid thermostat readings" => "Wachten op geldige thermostaatmetingen",
            "Learning advice requires a Home or Comfort schedule period" => {
                "Advies vereist een geplande periode Thuis of Comfort"
            }
            "Could coast for another minute (advice only)" => {
                "Kan nog een minuut wachten (alleen advies)"
            }
            "Residual heat may reach the target (advice only)" => {
                "Restwarmte kan de doeltemperatuur bereiken (alleen advies)"
            }
            "Collecting qualified heating observations" => {
                "Geschikte verwarmingsmetingen verzamelen"
            }
            "Estimated heat calls" => "Geschatte warmtevragen",
            "Calls under 3 minutes" => "Vragen korter dan 3 minuten",
            "Median call" => "Mediane duur warmtevraag",
            "Calls per heating hour" => "Vragen per verwarmingsuur",
            "Below comfort floor" => "Onder comfortgrens",
            "Burner stops during demand" => "Branderstops tijdens warmtevraag",
            "Cooling" => "Afkoeling",
            "Warming" => "Opwarming",
            "Residual heat" => "Restwarmte",
            "Installed version" => "Geïnstalleerde versie",
            "Available version" => "Beschikbare versie",
            "Last check" => "Laatste controle",
            "Checking for updates…" => "Updates zoeken…",
            "Automatic updates on" => "Automatische updates aan",
            "Automatic updates off" => "Automatische updates uit",
            "Check now" => "Nu controleren",
            "Install now" => "Nu installeren",
            "Restore previous version" => "Vorige versie herstellen",
            "No new stable release" => "Geen nieuwe stabiele versie",
            "Update ready" => "Update gereed",
            "Waiting to restart safely" => "Wachten op een veilige herstart",
            "Interrupted update restored" => "Afgebroken update teruggedraaid",
            "Update failed startup checks" => "Update teruggedraaid na mislukte opstartcontrole",
            "Update probe failed" => "Apparaatcontrole voor update mislukt",
            "Update could not start" => "Update kon niet starten",
            "Not enough free space for the update" => "Onvoldoende vrije ruimte voor de update",
            "Tile dashboard" => "Tegeldashboard",
            "Minimal" => "Minimaal",
            "Live meters" => "Live meterstanden",
            "Electricity and gas charts" => "Stroom- en gasgrafieken",
            "Live flow and charts" => "Live verbruik en grafieken",
            "Wide controls" => "Grote knoppen",
            "Side panel" => "Zijpaneel",
            "− Cooler" => "− Koeler",
            "+ Warmer" => "+ Warmer",
            "Thermostat unavailable" => "Thermostaat niet beschikbaar",
            "Partial history" => "Onvolledige geschiedenis",
            "Open Energy" => "Open Energie",
            "Last 24 hours" => "Afgelopen 24 uur",
            "Meter history unavailable" => "Metergeschiedenis niet beschikbaar",
            "Today’s schedule" => "Programma van vandaag",
            "Schedule unavailable" => "Programma niet beschikbaar",
            "No schedule periods today" => "Geen perioden voor vandaag",
            "Install Weather from the Store" => "Installeer Weer via de Store",
            "Outside now" => "Buiten nu",
            "Gas flow" => "Gasdoorstroming",
            "Forecast" => "Verwachting",
            "Table" => "Tabel",
            "Time range" => "Periode",
            "Reading" => "Meetwaarde",
            "Data quality" => "Gegevenskwaliteit",
            "Partial" => "Onvolledig",
            "Complete" => "Volledig",
            "Newer" => "Nieuwer",
            "Older" => "Ouder",
            "Recording selected meter history. Usage needs two samples." => {
                "Metergeschiedenis wordt opgeslagen. Verbruik vereist twee metingen."
            }
            "Latest" => "Meest recent",
            "Incomplete readings" => "Onvolledige meetgegevens",
            "Add a GitHub catalog" => "GitHub-catalogus toevoegen",
            "Add source" => "Bron toevoegen",
            "Dashboard widgets" => "Dashboardtegels",
            "Install" => "Installeren",
            "Refresh official" => "Catalogus vernieuwen",
            "Add GitHub source" => "GitHub-bron toevoegen",
            "Appearance" => "Weergave",
            "Night" => "Nacht",
            "Auto follows local time from 07:00 to 20:00." => {
                "Automatische dagstand van 07:00 tot 20:00."
            }
            "Color theme" => "Kleurthema",
            "Blue" => "Blauw",
            "Forest" => "Bosgroen",
            "Plum" => "Pruim",
            "Confirm switch to stock" => "Wisselen bevestigen",
            "Switch to stock UI" => "Originele interface openen",
            "Clock" => "Klok",
            "Boiler" => "Ketel",
            "Next target" => "Volgende doel",
            "Quick note" => "Notitie",
            "Downloading plugin…" => "Plugin downloaden…",
            "Plugin ready" => "Plugin gereed",
            "Update" => "Bijwerken",
            "Gas usage" => "Gasverbruik",
            "Humidity" => "Luchtvochtigheid",
            "No reading" => "Geen meting",
            "No meter data" => "Geen meterdata",
            "Offline" => "Offline",
            "Heating" => "Verwarmt",
            "Hot water" => "Warm water",
            "Preheating" => "Voorverwarmen",
            "Idle" => "In rust",
            "Connected to the selected network." => "Verbonden met het gekozen netwerk.",
            "Applying Wi-Fi settings…" => "Wifi-instellingen toepassen…",
            "Downloading catalog…" => "Catalogus downloaden…",
            "Could not refresh data" => "Gegevens konden niet worden vernieuwd",
            _ => message,
        }
    }
}
