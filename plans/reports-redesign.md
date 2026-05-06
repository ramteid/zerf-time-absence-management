# Plan: Dashboard und Reports - rollenbezogene Aufteilung

Leitlinie: **Dashboard = "Was ist jetzt los?"** (Aktuelles, To-dos, Status). **Reports = "Was war?"** (Auswertungen über Zeiträume, Detailtabellen, Export).

## Angestellte (employee)

### Dashboard (im Wesentlichen wie heute)
- Aktuelle Woche / heutiger Tag: gebuchte Zeit, Status
- Eigener Saldo + kleines Flextime-Chart der letzten ~30 Tage
- To-dos: nicht eingereichte Wochen, offene Change-/Reopen-Requests
- Resturlaub kompakt
- Nächste eigene Abwesenheiten

### Reports
- **Monatsbericht**: Soll/Ist/Diff + Detailtabelle Einträge + Abwesenheiten des Monats. *Kein* User-Dropdown (Employee kann sich ohnehin nur selbst auswählen) - nur Monatsauswahl.
- **Jahresübersicht Saldo**: monatsweise Soll/Ist/Diff/kumuliert (bleibt).
- **Kategorien-Auswertung** über Zeitraum (bleibt, ohne User-Dropdown).
- **Abwesenheits-Auswertung** über Zeitraum: Liste + Summen je Art (Urlaub/Krank/...). Neu.
- **Export**: CSV (bleibt) + PDF-Stundennachweis. **Wichtig: PDF wird im Backend generiert**, alle Texte/Übersetzungen liegen im Backend (`backend/src/i18n.rs`), damit Frontend keine Stringtabellen für PDF braucht.

## Teamleiter (lead) - und analog Admin

### Dashboard
- Alles vom Employee-Dashboard (eigener Saldo, eigene To-dos)
- **Genehmigungs-Kacheln bleiben getrennt**:
  - Kachel 1: Stundenzettel-Genehmigungen (Einträge + Wochen + Reopen-/Change-Requests)
  - Kachel 2: Abwesenheitsanträge
- **Neue Kachel "Wer ist abwesend"** mit Wochen-Slider:
  - Standard: aktuelle Woche
  - Pfeil links / rechts (oder Wisch-Geste mobil): wechselt Woche, mit smooth slide transition (CSS `transform: translateX` + `transition`)
  - Single tap/click auf den Kachel-Inhalt (oder ein "Heute"-Button am Rand): zurück zur aktuellen Woche
  - Inhalt: Liste der im gewählten Zeitraum abwesenden Direct Reports mit Art und Zeitraum

### Reports
- Alles vom Employee-Report (für sich selbst, ohne User-Dropdown da diese Karten den eigenen Kontext nutzen).
- **Teambericht Monat**: Tabelle aller Direct Reports mit Soll/Ist/Diff (bleibt). Kein Drilldown-Popup von hier aus - der Drilldown ist ein eigener Berichtseintrag (siehe unten).
- **Teambericht Kategorien (neu / erweitert)**:
  - Standardmäßig alle Arbeitskategorien je Mitarbeiter im gewählten Zeitraum
  - Filter-Button öffnet Mehrfachauswahl der Kategorien -> Tabelle zeigt für alle Mitarbeiter nur die ausgewählten Kategorien (z. B. nur "Fortbildung" oder "Kernzeit")
  - Spalten: Mitarbeiter, je gewählte Kategorie eine Spalte mit Minuten/Stunden, Summe
- **Mitarbeiter-Detailbericht (neuer Eintrag, ersetzt das frühere Popup-Konzept)**:
  - Eigener Berichts-Eintrag in Reports mit User-Dropdown (alle Direct Reports + man selbst) und Monatsauswahl
  - Öffnet ein Popup/Modal mit Vollansicht für die gewählte Person:
    - Aktueller **Gleitzeitstand** (kumulierter Saldo, prominent)
    - Aktueller **Urlaubsstand** (Anspruch, genommen, geplant, Rest)
    - Monatsbericht-Inhalt (Soll/Ist/Diff, Einträge, Abwesenheiten) wie bisher
    - Kleines Flextime-Chart (nutzt vorhandene `FlextimeChart`-Komponente)
    - Kategorienverteilung des Monats als simples Balken-/Donut-Diagramm
  - Reine Zusammenstellung vorhandener Endpunkte; kein neues Backend-Aggregat zwingend nötig
- **Team-Abwesenheiten** über Zeitraum: Liste je Mitarbeiter mit Art/Zeitraum/Tagen + Summen je Art.
- **Bulk-Export** Team-CSV für gewählten Zeitraum + PDF-Stundennachweis je Mitarbeiter.

## Trennregel

| Frage | Ort |
|---|---|
| "Muss ich gerade etwas tun?" | Dashboard |
| "Wie war mein/das Team-Monat/Quartal/Jahr?" | Reports |
| "Wer ist diese/nächste Woche abwesend?" | Dashboard (Slider-Kachel) |
| "Wieviel Urlaub/Krank im Zeitraum X?" | Reports |
| "Detail-Status eines bestimmten Mitarbeiters" | Reports (Mitarbeiter-Detailbericht) |
| "Export Lohnbuchhaltung / Stundennachweis" | Reports |

## Umsetzungsschritte (knapp)

1. **Reports.svelte aufräumen**: User-Dropdown nur für Lead/Admin anzeigen; Employee-Karten ohne Dropdown.
2. **Karte "Abwesenheiten im Zeitraum"** hinzufügen (Employee + Lead).
3. **Dashboard-Kachel "Wer ist abwesend"** mit Wochen-Slider (Pfeile, Tap-zurück-zu-heute, slide-Transition).
4. **Teambericht Kategorien** erweitern um Kategorien-Mehrfachfilter.
5. **Mitarbeiter-Detailbericht** als neuer Reports-Eintrag mit Modal: Gleitzeitstand, Urlaubsstand, Monatsdetails, Mini-Chart.
6. **PDF-Stundennachweis** im Backend (Übersetzungen in `backend/src/i18n.rs`), Endpunkt z. B. `/reports/range.pdf`.
7. **Bulk-Export** für Lead über `/reports/range` in Schleife (CSV-Bundle).

Keine neuen Backend-Aggregate zwingend nötig - alle Punkte gehen mit vorhandenen Endpunkten in `backend/src/reports.rs` und `backend/src/absences.rs`. Einzige neue Backend-Arbeit: PDF-Generator + dortige Übersetzungen.
