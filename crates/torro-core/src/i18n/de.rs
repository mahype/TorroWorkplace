//! German translations: `(English, German)`. Keep entries grouped by app and sorted
//! within a group; the test in `mod.rs` checks for duplicates and matching placeholders.

pub(super) static TABLE: &[(&str, &str)] = &[
    // Tray menu (torroworkplace)
    ("Add", "Hinzufügen"),
    ("Busy …", "Läuft gerade …"),
    ("Delete", "Löschen"),
    ("Load here", "Hier laden"),
    ("Load template", "Vorlage laden"),
    ("No templates yet", "Noch keine Vorlagen"),
    ("Quit", "Beenden"),
    ("Replace", "Ersetzen"),
    ("Save workspace", "Workspace speichern"),
    ("Update", "Aktualisieren"),
    ("Workspace {workspace} · {backend}", "Workspace {workspace} · {backend}"),
    ("{name} not supported yet", "{name} fehlt noch"),
    // Notifications (torroworkplace)
    ("Done", "Erledigt"),
    ("Failed", "Fehlgeschlagen"),
    ("Template deleted", "Vorlage gelöscht"),
    ("Template loaded", "Vorlage geladen"),
    ("Template partly loaded", "Vorlage teilweise geladen"),
    ("Template saved", "Vorlage gespeichert"),
    (
        "The workspace is not empty. Choose “Add” or “Replace”.",
        "Der Workspace ist nicht leer. Wähle „Hinzufügen“ oder „Ersetzen“.",
    ),
    ("{name} · {n} windows", "{name} · {n} Fenster"),
    ("{n} windows", "{n} Fenster"),
    ("{restored} windows in place, {failed} failed", "{restored} Fenster da, {failed} fehlgeschlagen"),
    // Shared app texts (torro-gui, torrocalendar, torrotasks)
    ("Delete failed: {error}", "Löschen fehlgeschlagen: {error}"),
    ("Loading …", "Lade …"),
    ("Save failed: {error}", "Speichern fehlgeschlagen: {error}"),
    ("Sign in in your browser … ({url})", "Im Browser anmelden … ({url})"),
    // Calendar
    ("+ Event", "+ Termin"),
    ("Agenda", "Agenda"),
    ("Calendar", "Kalender"),
    ("Calendar · {heading}", "Kalender · {heading}"),
    ("Calendars", "Kalender"),
    ("Day", "Tag"),
    ("From {date}", "Ab {date}"),
    ("Month", "Monat"),
    ("Today", "Heute"),
    ("Week", "Woche"),
    ("Week {week} · {month} {year}", "KW {week} · {month} {year}"),
    // Tasks
    ("All", "Alle"),
    ("Completed", "Erledigte"),
    ("List", "Liste"),
    ("Lists", "Listen"),
    ("New task …", "Neue Aufgabe …"),
    ("No task list yet", "Keine Aufgabenliste vorhanden"),
    ("Planned", "Geplant"),
    ("Tasks", "Aufgaben"),
    ("Tasks · {filter}", "Aufgaben · {filter}"),
];
