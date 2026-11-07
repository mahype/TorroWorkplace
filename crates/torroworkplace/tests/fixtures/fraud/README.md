# Betrug und Bekannte (Issue #506)

Erfundene Mails für die Warnzeichen (`crates/torroworkplace/src/mail/warnings.rs`) und das Abfangen im
Feed (`mail/inbox_feed.rs`). Alle Adressen und Domains sind erfunden (`.example`, Reserve nach RFC 2606),
keine echten Personen.

- `fraud-*.eml`: typische Betrugsmuster. Ihre Warnzeichen reichen bis zum Verdacht; der Test erwartet,
  dass sie abgefangen werden, wenn das Modell „Betrug“ sagt.
- `known-*.eml`: Mail von Bekannten mit Merkmalen, die verdächtig wirken (Privatadresse mit „Antworten
  an“, Weiterleitung ohne SPF, Newsletter-Dienst im Namen eines Kollegen, fehlendes SPF). Sie kommen immer
  an: Bekannte werden nie abgefangen, und ihre Zeichen allein reichen nicht.
