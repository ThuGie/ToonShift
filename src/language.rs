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
