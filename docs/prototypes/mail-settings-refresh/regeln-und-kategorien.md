# Hauptkategorien und Regeln: Entwurfsstand

## Hierarchie im aktuellen Code

Torro kennt derzeit vier feste `CategoryGroup`-Werte. Die 23 bearbeitbaren `CategoryDef`-Einträge gehören jeweils zu einer dieser Gruppen. Das Icon einer Unterkategorie ist bereits frei aus 25 stabilen `CategoryIcon`-Werten wählbar; seine Farbe stammt aus der Gruppenpalette des aktiven Designs. Gruppennamen, Gruppen-Icons und Farben lassen sich derzeit nicht als eigene Datensätze bearbeiten. Siehe `crates/torro-mail/src/category.rs` und `crates/torro-settings-ui/src/mail/editor.rs`.

Der neue HTML-Entwurf behandelt die bisherigen Gruppen als **Hauptkategorien**: Name, Icon und Farbe sind im Mockup bearbeitbar. Ihre **Unterkategorien** haben einen Namen, ein eigenes Icon und eine Beschreibung; die Iconfarbe wird von der Hauptkategorie übernommen. In beiden Bearbeitungsdialogen sind alle 25 vorhandenen Icons als sichtbare Kacheln auswählbar. Neue Haupt- und Unterkategorien können angelegt werden. Allgemeine Regeln werden im ersten Tab „Automatische Einsortierung“ gepflegt und wählen eine Ziel-Unterkategorie. Für die native Umsetzung braucht es statt des festen Enums stabile Hauptkategorie-IDs, eine bearbeitbare Farbwahl und eine Zuordnung der Unterkategorien über diese ID. Bestehende Kategorien und Regeln müssen dabei ihre stabilen Schlüssel behalten.

## Was andere Mailprogramme bei eingehenden Nachrichten anbieten

| Programm | Relevante Möglichkeiten |
|---|---|
| [Gmail](https://support.google.com/mail/answer/6579?hl=en) | Filter aus Suchkriterien oder einer vorhandenen Mail anlegen; mit Labels versehen, archivieren, löschen, markieren oder weiterleiten. [Suchoperatoren](https://support.google.com/mail/answer/7190?hl=en) decken unter anderem Absender, Empfänger, Betreff, Zeit, Anhang, Dateinamen, Größe, Mailinglisten und Header ab. |
| [Outlook](https://support.microsoft.com/en-gb/outlook/mail/manage-email-messages-by-using-rules-in-outlook) | Bedingungen, Aktionen und Ausnahmen kombinieren; Regeln aktivieren/deaktivieren, ordnen und auf vorhandene Mail anwenden. „Keine weiteren Regeln verarbeiten“ bestimmt, ob nach einem Treffer Schluss ist. |
| [Thunderbird](https://support.mozilla.org/en-US/kb/organize-your-messages-using-filters) | Eigenschaft + Vergleich + Wert je Bedingung; mehrere Bedingungen mit „alle“ oder „eine“ verknüpfen. Filter können beim Empfang oder manuell laufen und stehen in einer Reihenfolge. |
| [Apple Mail](https://support.apple.com/en-ae/guide/mail/cpmlprefrulesadd) | „Eine“ oder „alle“ Bedingungen, mehrere Bedingungen und mehrere Aktionen je Regel. |

## Sinnvolle Felder für Torros Kategorisierungsregeln

| Bereich | Bedingung | Vergleich | Priorität |
|---|---|---|---|
| Adressen | Absenderadresse, Absenderdomain, Antwortadresse, Empfänger in An/Cc, zugestellte Adresse | ist / ist nicht; enthält / enthält nicht; Domain samt Unterdomains | Hoch |
| Betreff und Inhalt | Betreff, Nachrichtentext, exakte Wortfolge | enthält / enthält nicht; beginnt mit / endet mit | Hoch |
| Anhänge | hat Anhang, Dateiname, Erweiterung oder Dateityp | ja/nein; ist/enthält | Hoch |
| Technische Header | `List-ID`, Auto-Submitted, eigener Header | ist / enthält / vorhanden | Mittel |
| Metadaten | Konto, ursprünglicher Ordner, Nachrichtengröße, Eingangsdatum | ist; größer/kleiner; vor/nach | Mittel |

**Bei Inhalt und Anhängen muss der Zeitpunkt klar sein:** Absender, Empfänger, Betreff und die meisten Header sind beim Eingang verfügbar. Nachrichtentext und manche Anhangdetails können erst nach dem Laden der Nachricht zuverlässig geprüft werden. Eine Regel darf nicht stillschweigend so tun, als sei eine fehlende Information eine negative Bedingung.

## Empfohlene Regelbedienung

1. **Einfache Regel zuerst:** Feld, Vergleich, Wert und Ziel-Unterkategorie im Tab „Automatische Einsortierung“.
2. **Mehrere Bedingungen:** „Alle Bedingungen“ oder „Mindestens eine Bedingung“, dazu explizite Ausnahmen. Diese Ergänzungen gehören in einen späteren erweiterten Regel-Editor.
3. **Eindeutige Priorität:** Manuelle Zuordnung bleibt bestehen. Im Entwurf werden Kalendereinladungen, Abwesenheiten und Zustellfehler zuerst erkannt. Danach gelten allgemeine Regeln in sichtbarer Reihenfolge; die erste passende Unterkategorie gewinnt. Erst danach entscheidet das standardmäßig aktivierte Modell anhand der Beschreibung, falls es keinen Regeltreffer gab.
4. **Verständliche Prüfung:** Vor dem Speichern passende Beispielmails anzeigen, Konflikte mit früheren Regeln markieren und bestehende Mails auf Wunsch erneut zuordnen. Regeln lassen sich deaktivieren und später bearbeiten.
5. **Getrennter Funktionsumfang:** Verschieben, Archivieren, Weiterleiten, Löschen und Markieren sind bei anderen Programmen Regelaktionen. Für diese Kategorisierungsseite bleibt das Ziel zunächst **Unterkategorie zuweisen**. Solche Mailaktionen brauchen später einen eigenen Automationsbereich und klare Regeln für Server- und Geräteausführung.

Der aktuelle Torro-Regeltyp unterstützt nur Absenderadresse, Domain, Betreff und Text; mehrere Bedingungen gelten gemeinsam, Ausnahmen sind möglich und die erste passende Regel entscheidet. Siehe `crates/torro-inbox/src/item.rs` (`SortRule`, `SortCondition`, `SortField`). Das HTML-Mockup zeigt bereits eine **Vorschau** mit elf auswählbaren Prüffeldern, passenden Vergleichen, Ziel-Unterkategorie und umsortierbaren Regeln. Eine Regel wird direkt innerhalb ihrer eigenen Box bearbeitet; eine neue Regel beginnt am Listenanfang. Die zusätzlichen Felder und Vergleichsoperatoren benötigen noch Datenfelder und Auswertungslogik im Produkt. „Eine von mehreren“-Verknüpfung und Ausnahmen je Regel sind hier als nächste Ausbaustufe beschrieben; sie werden im Mockup noch nicht als Bedienung vorgetäuscht.
