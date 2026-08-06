# Kategoriebezogene Tageskonten

## Ziel

Jede Abwesenheitskategorie kann beim Anlegen optional ein eigenes, unabhängiges
Tageskonto erhalten. Ein Tageskonto umfasst:

- einen Standardanspruch pro Jahr,
- einen individuellen Basisanspruch pro Benutzer,
- individuelle Overrides für einzelne Jahre,
- ein eigenes Verfallsdatum für den Übertrag,
- ein internes Startjahr, ab dem Anspruch und Übertrag gelten.

Die vorhandene Kategorie `Vacation` bleibt das Tageskonto für
Erholungsurlaub. Weitere Arten wie Bildungsurlaub können später als eigene
Tageskonto-Kategorien angelegt werden.

Hinweis zum realen Live-Bestand (read-only geprüft): In der Produktivinstanz
existieren zwei vom Betreiber manuell angelegte Kategorien, die **nicht** von
der offiziellen Migration `039` fachlich korrekt behandelt werden und deshalb
über ein eigenes, installationsspezifisches Nachbehandlungsskript umgezogen
werden (siehe Abschnitt "Installationsspezifisches Nachbehandlungsskript"):

- `regenerationstage` (id 9) steht heute versehentlich auf
  `cost_type = 'vacation'` und belastet dadurch fälschlich das
  Erholungsurlaubskonto. Fachlich waren Regenerationstage immer ein
  **separates** Konto, das bisher nur informell außerhalb von Zerf geführt
  wurde. Die generische Migration `039` würde die eine bereits gebuchte
  Regenerationstage-Abwesenheit dem kanonischen Vacation-Konto zuordnen und
  ihre Tage weiterhin vom Erholungsurlaub abziehen. Genau das ist unerwünscht.
- `bildungsurlaub` (id 8) steht auf `cost_type = 'none'` und kostet heute gar
  nichts; die vorhandenen Bildungsurlaubs-Abwesenheiten sollen künftig ein
  eigenes Tageskonto belasten.

Die generische Migration `039` bleibt bewusst rein `cost_type`-gesteuert und
kennt diese beiden slug-spezifischen Sonderfälle nicht. Das Nachbehandlungs-
skript läuft **nach** `039` und korrigiert beide Kategorien gezielt.

## Terminologie

- Englisch: `leave account`
- Deutsch: `Tageskonto`
- Deutsch für die Benutzerrolle: `Aushilfe` beziehungsweise `Aushilfen`
- Der interne Rollenwert `assistant` bleibt unverändert.
- Der persistierte `cost_type = 'vacation'` bleibt das alleinige technische
  Signal dafür, dass eine Abwesenheitskategorie ein Tageskonto besitzt. Es
  wird kein zusätzliches boolesches Feld eingeführt.
- Neue Service-, Repository-, API- und UI-Begriffe verwenden `leave account`
  statt des bisherigen pauschalen Begriffs `vacation balance`. Der bestehende
  Datenbankwert `vacation` bleibt aus Migrationsgründen erhalten.

## Bestätigtes Produktverhalten

### Kategorien

- Ein Tageskonto kann in der Anwendung nur beim Anlegen einer neuen
  Abwesenheitskategorie aktiviert werden.
- Sobald eine Kategorie ein Tageskonto besitzt, kann es nie wieder
  deaktiviert werden. Die Kategorie kann weiterhin umbenannt, umgefärbt,
  sortiert, deaktiviert und später wieder aktiviert werden.
- Eine bestehende Kategorie ohne Tageskonto kann in der Anwendung nicht
  nachträglich zu einer Tageskonto-Kategorie gemacht werden. Dafür wird ein
  separates SQL-Runbook erstellt.
- Beim Anlegen sind Standardtage im Bereich 0 bis 366 und ein gültiges
  Verfallsdatum im Format `MM-DD` Pflicht. Ein fehlendes Verfallsdatum mit der
  Bedeutung "verfällt nie" wird nicht eingeführt.
- Das interne `leave_account_start_year` ist für Benutzer vollständig
  unsichtbar und in der Anwendung nicht editierbar. Es ist weder Bestandteil
  eines API-Request-DTOs noch einer API-Response. Beim Anlegen einer neuen
  Tageskonto-Kategorie setzt der Service es automatisch auf das aktuelle Jahr
  gemäß konfigurierter App-Zeitzone.
- Der Kategorienstandard wird beim Anlegen einmal auf alle bestehenden
  Benutzer übertragen. Aushilfen erhalten dabei wie bisher 0 Tage.
- Eine spätere Änderung des Kategorienstandards gilt nur für künftig
  angelegte Benutzer. Bereits gespeicherte individuelle Basiswerte werden
  nicht überschrieben. Die Oberfläche erklärt diese Wirkung ausdrücklich.
- Eine Änderung des Verfallsdatums wirkt sofort auf alle neu berechneten
  Salden, also gegebenenfalls auch rückwirkend auf historische Jahre. Die
  Oberfläche weist darauf hin.
- `auto_approve_past = true` bleibt für Tageskonto-Kategorien verboten.
- `unpaid = true` bleibt ausschließlich für `cost_type = 'none'` erlaubt.

### Benutzer

- Für jede Tageskonto-Kategorie kann beim Anlegen und Bearbeiten eines
  Benutzers Folgendes festgelegt werden:
  - individueller Basisanspruch,
  - Override für das aktuelle Jahr,
  - Override für das nächste Jahr.
- Benutzeranlage und Tageskonto-Werte werden atomar in derselben Transaktion
  gespeichert. Es gibt keinen nachgelagerten UI-Aufruf, der einen teilweise
  konfigurierten Benutzer hinterlassen könnte.
- Aushilfen werden in allen Tageskonten mit 0 vorbelegt. Admins und
  Teamleitungen können die individuellen Jahreswerte weiterhin bewusst
  ändern.
- Archivierte Benutzer behalten ihre Konten und historischen Salden.
- Wird der Zugriff auf eine Abwesenheitskategorie entzogen oder die Kategorie
  deaktiviert, verhindert das nur neue Anträge. Konten, vorhandene
  Abwesenheiten und historische Salden verschwinden nicht.

### Übertrag und Startjahr

- Anspruch und Übertrag eines Tageskontos beginnen für einen Benutzer im
  späteren der beiden Jahre:

  ```text
  max(user.start_date.year, category.leave_account_start_year)
  ```

- Vor diesem Jahr existiert für diese Benutzer-Kategorie-Kombination kein
  Anspruch und kein Übertrag. Dadurch erzeugt ein neu angelegtes Tageskonto
  keine rückwirkenden Phantomansprüche für frühere Jahre.
- Die bestehende Logik für `hire_date` bleibt unverändert: Innerhalb eines
  gültigen Anspruchsjahres bestimmt `hire_date`, ersatzweise `start_date`, die
  zeitanteilige Kürzung. Das Startjahr des Tageskontos ist keine zusätzliche
  unterjährige Kürzung.
- Requested und cancellation-pending Abwesenheiten reservieren weiterhin
  Budget und reduzieren den möglichen Übertrag des Quelljahres.
- Jahresübergreifende Abwesenheiten werden für jedes betroffene Jahr und das
  konkrete Tageskonto getrennt validiert.

### Anzeige und Berichte

- Die bisher fünf beziehungsweise sechs Urlaubskacheln auf der
  Abwesenheitsseite werden zu einer kompakten Kachel pro Tageskonto
  zusammengefasst.
- Der Mitarbeiterbericht verwendet dieselbe Kachelkomponente und zeigt
  ebenfalls eine Kachel pro Tageskonto.
- Jede Kachel enthält Kategorie, Anspruch, Übertrag, genommen, genehmigt
  geplant, beantragt und verfügbar. Übertragsverfall wird angezeigt, wenn er
  für das ausgewählte Jahr relevant ist.
- Der Teambericht erhält genau eine Spalte pro Tageskonto-Kategorie. Eine
  Tabellenzelle zeigt kompakt die Werte "genommen" und "geplant". Es werden
  nicht zwei Spalten pro Kategorie erzeugt.
- Der Teambericht besitzt keinen eigenen CSV-Export. Der persönliche
  Monats-CSV enthält keine Urlaubsspalten und bleibt unverändert.

## Zentrale Dateninvarianten

### Kategorie und belastetes Konto sind für historische Daten getrennt

Vor dieser Änderung gibt es genau einen gemeinsamen Urlaubstag-Topf pro
Benutzer. Falls mehrere Abwesenheitskategorien `cost_type = 'vacation'`
besitzen, belasten sie alle denselben Topf. Bei der Migration wird dieses alte
gemeinsame Konto vollständig der kanonischen Kategorie mit dem stabilen Slug
`vacation` zugeordnet.

Damit bereits vorhandene Abwesenheiten aus anderen bisherigen
`cost_type = 'vacation'`-Kategorien weiterhin das tatsächlich damals
verwendete gemeinsame Konto belasten, speichert jede Abwesenheit künftig das
konkret belastete Tageskonto:

```text
absences.leave_account_category_id
```

Das Feld ist ein nullable Foreign Key auf `absence_categories(id)`:

- Bestehende Abwesenheiten aus allen bisherigen
  `cost_type = 'vacation'`-Kategorien werden bei der Migration auf die
  kanonische `Vacation`-Kategorie gesetzt.
- Neue Abwesenheiten einer Tageskonto-Kategorie speichern deren eigene
  `category_id` auch als `leave_account_category_id`.
- Abwesenheiten ohne Tageskonto speichern `NULL`.
- Wird bei einer späteren Bearbeitung nur Datum oder Kommentar geändert,
  bleibt die gespeicherte Kontozuordnung erhalten.
- Wird die Abwesenheitskategorie tatsächlich geändert, wird auch die
  Kontozuordnung auf das Konto der neuen Kategorie gesetzt oder bei einer
  Kategorie ohne Tageskonto geleert.
- Saldo, Budgetvalidierung, Übertrag und Tageskonto-Spalten in Berichten
  gruppieren nach `leave_account_category_id`, nicht nach dem angezeigten
  `absences.category_id` und nicht mehr pauschal nach `cost_type`.
- Die Abwesenheitshistorie zeigt weiterhin die tatsächliche fachliche
  Abwesenheitskategorie. Die gespeicherte Kontozuordnung ist interne
  Buchungsinformation, wird in öffentlichen Abwesenheits-DTOs nicht
  ausgeliefert und kann vom Client nicht gesetzt werden.

Diese Trennung erhält die historische Belastung des alten gemeinsamen Kontos,
ohne vorhandene Abwesenheiten umzubenennen. Gleichzeitig belasten neue
Abwesenheiten nach der Migration das jeweils eigene Konto ihrer Kategorie.

### Identität

- Konten, Salden, Overrides und Berichtsspalten werden ausschließlich über
  `category_id` identifiziert.
- Name, Slug und Farbe sind Anzeige- beziehungsweise Metadaten und dürfen
  niemals als fachlicher Schlüssel verwendet werden.
- Frontend-Listen und Svelte-Each-Blöcke verwenden `category_id` als Key.

### Übergangsmatrix für `cost_type`

- Neue Kategorie: `vacation` ist erlaubt, sofern Kontofelder gültig sind.
- Bestehende Nicht-Tageskonto-Kategorie nach `vacation`: immer ablehnen.
- Bestehende Tageskonto-Kategorie nach `none` oder `flextime`: immer ablehnen.
- Bestehende Tageskonto-Kategorie mit unverändertem `vacation`: erlauben.
- `none` nach `flextime` und umgekehrt: wie bisher nur erlauben, wenn die
  Kategorie noch keine Abwesenheiten besitzt.

Die Prüfung erfolgt anhand des tatsächlichen Übergangs von aktuellem zu
resultierendem Wert. Ein Update, das den unveränderten Wert `vacation`
mitsendet, darf nicht abgelehnt werden.

### Transaktionen und Parallelität

- Services besitzen die Transaktionsgrenzen.
- Benutzeranlage, Kategorieanlage und Initial-Admin-Anlage verwenden für das
  Benutzer-Tageskonto-Geflecht denselben Advisory Transaction Lock.
- Dadurch können eine parallel angelegte Kategorie und ein parallel
  angelegter Benutzer nicht gegenseitig ihre Zuordnung übersehen.
- Repository-Methoden führen SQL aus, öffnen für diese Abläufe aber keine
  eigenen versteckten Transaktionen.
- Audit und Benachrichtigungen werden erst nach erfolgreichem Commit
  ausgelöst.

## Datenbankmigration `039_leave_accounts.sql`

Die Migration ist vollständig idempotent. `ADD COLUMN`, Tabellen und Indizes
werden mit `IF NOT EXISTS` angelegt; benannte Constraints werden über
`pg_constraint` geschützt. Legacy-Backfills laufen nur, solange die jeweilige
Altspalte beziehungsweise Alttabelle noch existiert, und überschreiben keine
bereits migrierten Zielwerte. Dadurch bleibt auch eine Wiederholung nach dem
Entfernen der Altstruktur sicher. Bereits angewendete Migrationsdateien werden
nicht verändert.

### 1. Migrationswerte validieren

- Die kanonische Kategorie `absence_categories.slug = 'vacation'` muss genau
  einmal existieren. Fehlt sie, bricht die Migration mit einer verständlichen
  Meldung ab. Der Anzeigename ist irrelevant und darf bereits geändert worden
  sein.
- Es ist ausdrücklich zulässig, dass weitere Kategorien
  `cost_type = 'vacation'` besitzen. Vor der Migration teilen diese Kategorien
  den einen vorhandenen Urlaubstag-Topf.
- Der bisherige Organisationsstandard wird aus dem `app_settings`-Eintrag mit
  `key = 'default_annual_leave_days'` geladen, bei fehlendem oder leerem Wert
  mit Fallback 30.
- Das bisherige Verfallsdatum wird aus dem `app_settings`-Eintrag mit
  `key = 'carryover_expiry_date'` geladen, bei fehlendem oder leerem Wert mit
  Fallback `03-31`.
- Nicht leere, aber ungültige Altwerte führen zu einer klaren
  Migrationsfehlermeldung statt zu einem stillen Fallback.
- Das aktuelle Jahr wird anhand des `app_settings`-Eintrags mit
  `key = 'timezone'` bestimmt, mit dem bestehenden Zeitzonen-Fallback der
  Anwendung.
- Das Startjahr des kanonischen Vacation-Kontos wird auf das früheste Jahr
  aller `users.start_date` gesetzt. Existiert noch kein Benutzer, wird das
  aktuelle Jahr verwendet.

### 2. Kategorien um Kontoinformationen erweitern

`absence_categories` erhält:

```text
leave_account_default_days BIGINT NULL
leave_account_carryover_expiry TEXT NULL
leave_account_start_year INTEGER NULL
```

Benannte DB-Constraints erzwingen:

- `leave_account_default_days` liegt zwischen 0 und 366.
- Das Verfallsdatum ist ein reales `MM-DD`; `02-29` ist zulässig und wird in
  Nicht-Schaltjahren wie bisher auf den letzten Februartag abgebildet.
- Bei `cost_type = 'vacation'` sind alle drei Kontofelder `NOT NULL`.
- Bei `cost_type != 'vacation'` sind alle drei Kontofelder `NULL`.

Backfill:

- Die kanonische `Vacation`-Kategorie erhält den bisherigen globalen Standard,
  das bisherige globale Verfallsdatum und das früheste Startjahr aller
  Mitarbeiter.
- Jede weitere bereits vorhandene Kategorie mit
  `cost_type = 'vacation'` wird zu einem getrennten Tageskonto mit
  `leave_account_default_days = 0`.
- Diese weiteren Konten erhalten das bisherige Verfallsdatum und als
  `leave_account_start_year` das aktuelle Jahr der Migration. Sie beginnen
  damit als neue, separate 0-Tage-Konten und erzeugen keine rückwirkenden
  Ansprüche.

### 3. Belastetes Tageskonto an Abwesenheiten speichern

`absences` erhält:

```text
leave_account_category_id BIGINT NULL
  REFERENCES absence_categories(id)
```

Backfill:

- Alle bereits vorhandenen Abwesenheiten, deren Kategorie unmittelbar vor
  dem Konten-Backfill `cost_type = 'vacation'` besitzt, erhalten die ID der
  kanonischen `Vacation`-Kategorie.
- Alle übrigen Abwesenheiten behalten `NULL`.
- Ein Index auf `(user_id, leave_account_category_id, start_date, end_date)`
  unterstützt Saldo- und Berichtsabfragen.
- Der Backfill aktualisiert nur `NULL`-Werte, damit eine sichere Wiederholung
  der Migration keine späteren Kontozuordnungen überschreibt.

### 4. Kontingenttabellen anlegen

Neue Basistabelle:

```text
user_leave_accounts
- user_id BIGINT REFERENCES users(id) ON DELETE CASCADE
- category_id BIGINT REFERENCES absence_categories(id) ON DELETE CASCADE
- base_days BIGINT CHECK (base_days BETWEEN 0 AND 366)
- PRIMARY KEY (user_id, category_id)
```

Neue Override-Tabelle:

```text
user_leave_account_year_overrides
- user_id BIGINT REFERENCES users(id) ON DELETE CASCADE
- category_id BIGINT REFERENCES absence_categories(id) ON DELETE CASCADE
- year INTEGER CHECK (year BETWEEN 2000 AND 2100)
- days BIGINT CHECK (days BETWEEN 0 AND 366)
- PRIMARY KEY (user_id, category_id, year)
```

Zusätzliche Indizes beginnen mit `category_id`, damit kategorieweite Seeds und
Prüfungen nicht die benutzerorientierten Primärschlüssel umgehen müssen.

Backfill:

- Für jeden vorhandenen Benutzer wird beim kanonischen Vacation-Konto
  `users.annual_leave_days` als `base_days` übernommen.
- Für jede weitere bereits vorhandene `cost_type = 'vacation'`-Kategorie wird
  pro Benutzer ein Basiswert 0 angelegt.
- Sämtliche Zeilen aus `user_annual_leave` werden ausschließlich der
  kanonischen Vacation-Kategorie als Jahres-Overrides zugeordnet.
- Für die weiteren Konten werden keine Jahres-Overrides erzeugt; ihr
  effektiver Anspruch ist daher 0.
- Archivierte und Tracking-deaktivierte Benutzer werden ebenfalls migriert.

### 5. Vollständigkeit prüfen und Altstruktur entfernen

Vor dem Entfernen alter Daten prüft die Migration mindestens:

- Für jeden Benutzer und jede Tageskonto-Kategorie existiert genau eine
  Basiszeile.
- Die Anzahl migrierter Vacation-Jahres-Overrides entspricht der Anzahl der
  alten `user_annual_leave`-Zeilen.
- Jede vorhandene Abwesenheit aus einer damaligen
  `cost_type = 'vacation'`-Kategorie besitzt die kanonische
  `leave_account_category_id`.
- Jede Tageskonto-Kategorie besitzt gültige Kontofelder.

Erst danach:

- `users.annual_leave_days` entfernen,
- die alte Tabelle `user_annual_leave` entfernen,
- `default_annual_leave_days` und `carryover_expiry_date` aus `app_settings`
  löschen.

Das Löschen der obsoleten Settings ist beabsichtigt. Nach erfolgreichem
Backfill gibt es keine globale Quelle mehr, die mit den Kategorienwerten
auseinanderlaufen könnte.

## Backend

### Repository-Modelle

- `AbsenceCategory` erhält die drei optionalen Kontofelder.
- Der fachliche Helper heißt `has_leave_account()`; intern vergleicht er
  weiterhin mit dem gespeicherten Wert `cost_type = 'vacation'`.
- `Absence` erhält `leave_account_category_id: Option<i64>`.
- Der bisherige `annual_leave_days`-Wert verschwindet aus
  `repository::User`, `middleware::auth::User`, Auth-Responses,
  Report-Projektionen und allen Testkonstruktoren.
- Sämtliche SQL-Projektionen und `ABS_SELECT`/`USER_SELECT`-Konstanten werden
  entsprechend angepasst.
- Doppelte Entitlement-Abfragen in `repository/users.rs` und
  `repository/absences.rs` werden konsolidiert. Kontingent-SQL liegt bei
  `UserDb`; Abwesenheits-SQL bleibt bei `AbsenceDb`.

### Kategorien-Service

`create` validiert und speichert innerhalb einer Service-Transaktion:

1. Rollenberechtigung und normale Kategorienfelder,
2. Kontofelder abhängig vom `cost_type`,
3. das unsichtbare aktuelle Startjahr,
4. die Kategorie,
5. die bisherigen Standard-Zugriffe für alle Benutzer,
6. eine `user_leave_accounts`-Zeile für jeden Benutzer,
7. Standardtage für normale Benutzer und 0 Tage für Aushilfen.

Der Kategorien-Request enthält bei einem Tageskonto ausschließlich
Standardtage und Verfallsdatum. Das Startjahr wird nicht deserialisiert,
sondern intern vom Service ergänzt. Auch der Kategorien-Response lässt das
Startjahr aus; nur Saldo- und Übertragsberechnungen lesen es über das
Repository.

`update` verwendet die bestätigte Übergangsmatrix. Bei einem bestehenden
Tageskonto dürfen Standard und Verfallsdatum geändert werden, nicht aber das
Startjahr oder der Tageskonto-Status. Für Nicht-Tageskonto-Kategorien werden
mitgesendete Kontofelder abgelehnt statt ignoriert.

Die bestehenden Retry-Eigenschaften des Kategorien-Dialogs bleiben erhalten:
Ein unverändertes `cost_type = 'vacation'` muss bei einem Retry erfolgreich
sein.

### Benutzer-Service und API

Definitionen für Formulare:

```text
GET /leave-accounts
```

Die Response enthält mindestens `category_id`, Name, Farbe,
Standardbasiswert, Aktivstatus und Verfallsdatum. Admins erhalten alle Konten;
Teamleitungen mit Aushilfenverwaltung erhalten alle für die Anlage relevanten
Konten; reguläre Benutzer erhalten nur ihre eigenen Konten.

Benutzerspezifische Werte:

```text
GET /users/{id}/leave-accounts
```

Die bestehende Zugriffskontrolle für Admin, eigene Daten und direkte
Mitarbeiter wird wiederverwendet. Jeder Eintrag enthält:

```json
{
  "category_id": 1,
  "category_name": "Vacation",
  "color": "#3b82f6",
  "active": true,
  "base_days": 30,
  "current_year": 2026,
  "current_year_days": 30,
  "next_year": 2027,
  "next_year_days": 30
}
```

`POST /users`, `PUT /users/{id}`, `POST /team-users` und
`PUT /team-users/{id}` verwenden ein verschachteltes Array:

```json
{
  "leave_accounts": [
    {
      "category_id": 1,
      "base_days": 30,
      "current_year_days": 30,
      "next_year_days": 30
    }
  ]
}
```

- Bei Benutzeranlage ist das Array optional. Nicht enthaltene Konten werden
  mit ihrem Kategorienstandard angelegt; für Aushilfen mit 0.
- Explizite Einträge überschreiben den Seed innerhalb derselben Transaktion.
- Bei Benutzerupdates bedeutet ein fehlendes Array "Konten unverändert".
- Doppelte IDs, unbekannte IDs, Nicht-Tageskonto-Kategorien und Werte außerhalb
  0 bis 366 werden mit einem übersetzten `BadRequest` abgelehnt.
- Konten, die zwischen Laden und Speichern neu angelegt wurden, werden trotz
  fehlendem Formulareintrag mit ihrem Standard initialisiert.
- Die alten flachen Felder `annual_leave_days`,
  `leave_days_current_year` und `leave_days_next_year` sowie die Route
  `/users/{id}/leave-days` werden vollständig entfernt.
- Die Initial-Admin-Anlage initialisiert alle vorhandenen Tageskonten über
  denselben Service-/Repository-Baustein.
- Benutzer- und Kategorienanlage nehmen denselben Advisory Transaction Lock,
  bevor Zuordnungen erzeugt werden.

### Abwesenheiten und Kontobelastung

Beim Anlegen einer Abwesenheit:

- Tageskonto-Kategorie: `leave_account_category_id = category.id`.
- Andere Kategorie: `leave_account_category_id = NULL`.
- Budgetvalidierung verwendet die gespeicherte Konto-ID und deren Konfiguration.

Beim Bearbeiten:

- Bleibt `category_id` gleich, bleibt auch die bestehende Kontozuordnung
  unverändert. Das ist für migrierte historische Abwesenheiten entscheidend.
- Ändert sich `category_id`, wird die Kontozuordnung aus der neuen Kategorie
  neu bestimmt.
- Die alte und neue Kontobelastung werden korrekt vom `exclude_id`-Pfad der
  Validierung behandelt.

Beim Genehmigen:

- Die gespeicherte Kontozuordnung wird erneut validiert.
- Eine vor der Migration beantragte Abwesenheit einer zusätzlichen damaligen
  Vacation-Kategorie belastet damit weiterhin das kanonische Vacation-Konto.

Stornierung, Widerruf und genehmigte Stornierung geben Budget auf genau dem
gespeicherten Konto frei.

Repository-Abfragen wie die bisherigen
`vacation_workdays_total_filtered`, `vacation_absences_in_year` und
`vacation_ranges_in_year_tx` werden durch konto-spezifische Methoden ersetzt,
die nach `leave_account_category_id` filtern. Es bleiben keine gepoolten
`cost_type = 'vacation'`-Saldohelfer als toter oder versehentlich nutzbarer
Code zurück.

### Saldo und Übertrag

Alle bisher globalen Funktionen erhalten eine Konto-ID und die dazugehörige
Konfiguration:

- effektiver Jahresanspruch,
- Übertrag in ein Jahr,
- Jahreskontext,
- Übertrag aus einem Jahr,
- verbleibender Übertrag,
- Budgetvalidierung,
- jahresübergreifende Validierung.

Die Quelljahresschleife beginnt beim konto-spezifischen effektiven Startjahr.
Das Verfallsdatum kommt ausschließlich aus der belasteten
Tageskonto-Kategorie.

`compute_balance` wird zu `compute_balances` und gibt ein Array zurück. Ein
Saldo enthält mindestens:

```text
category_id
category_name
color
active
annual_entitlement
already_taken
approved_upcoming
requested
available
carryover_days
carryover_remaining
carryover_expiry
carryover_expired
```

Konten werden für ein ausgewähltes Jahr nur berücksichtigt, wenn das Jahr
nicht vor ihrem effektiven Startjahr liegt. Deaktivierte oder für neue Anträge
entzogene Konten bleiben sichtbar, wenn für den Benutzer eine Kontenzeile
existiert.

Route:

```text
GET /leave-balances/{uid}?year=YYYY
```

Die bisherige Singularroute wird entfernt, damit Name und Array-Response
übereinstimmen.

### Teambericht

Die Teambericht-Response wird explizit strukturiert:

```json
{
  "leave_account_categories": [
    { "category_id": 1, "name": "Vacation", "color": "#3b82f6" }
  ],
  "rows": [
    {
      "user_id": 5,
      "leave_account_usage": [
        { "category_id": 1, "taken_days": 4, "planned_days": 2 }
      ]
    }
  ]
}
```

- Die beiden Felder `vacation_days` und `vacation_planned_days` entfallen.
- Berichtsspalten werden über `category_id`, niemals über Namen verbunden.
- Eine gebündelte Repository-Abfrage lädt die relevanten Abwesenheitsbereiche
  für alle Konten eines Benutzers beziehungsweise des Teams. Eine Schleife mit
  einer DB-Abfrage pro Benutzer und Konto wird vermieden.
- Genommen umfasst wie bisher den heutigen Tag; geplant beginnt morgen.
- cancellation-pending bleibt bis zur Entscheidung reserviert und wird wie
  bisher in den genehmigten Nutzungswerten berücksichtigt.
- Historische migrierte Abwesenheiten werden unter dem tatsächlich belasteten
  kanonischen Vacation-Konto ausgewiesen.
- Kategorien mit Startjahr nach dem Berichtsjahr erzeugen keine Spalte.
- Inaktive Konten bleiben für historische Berichtsjahre verfügbar.
- `auto_approve_past`- beziehungsweise Krankheitsaggregation bleibt
  unverändert und unabhängig von Tageskonten.
- Tageskonto-Kategorien bleiben aus dem automatischen Payroll-Abwesenheitsblock
  ausgeschlossen.

### Settings, Auth und Querschnitt

- `default_annual_leave_days` und `carryover_expiry_date` werden aus
  `UpdateSettings`, `PublicSettingsData`, `AdminSettingsData`,
  Settings-Repository und Settings-Service entfernt.
- Die Ersteinrichtungsprüfung verlangt diese globalen Werte nicht mehr.
- Die Frontend-Settings-Stores und alle Settings-Weiterleitungen werden
  bereinigt.
- Backend-Fehlermeldungen werden zentral in `backend/src/i18n.rs` übersetzt.
- Audit-Einträge für Kategorien- und Benutzeränderungen enthalten
  konto-spezifische Werte mit `category_id`, aber keine redundanten oder
  geheimen Daten.
- Benachrichtigungen verwenden weiterhin den sichtbaren Kategorienamen.
- Handler enthalten kein SQL; Services importieren keine Axum-HTTP-Typen;
  sämtliche neue Datenbankoperationen liegen in Repository-Modulen.

## Frontend

### `AbsenceCategoryDialog.svelte`

- Neue Kategorien zeigen die Auswahl "Uses a leave account" beziehungsweise
  "Verwendet ein Tageskonto".
- Bei Auswahl erscheinen Pflichtfelder für Standardtage und Verfallsdatum.
- Das Startjahr wird weder angezeigt noch übertragen.
- Bei bestehenden Nicht-Tageskonto-Kategorien ist die Tageskonto-Option
  deaktiviert.
- Bei bestehenden Tageskonto-Kategorien ist der Kontotyp gesperrt; der
  unveränderte Wert darf dennoch gespeichert werden.
- Standardtage und Verfallsdatum bleiben editierbar und erhalten Hinweise zu
  ihrer jeweiligen Wirkung.
- Die bisherigen Regeln für `unpaid` und `auto_approve_past` bleiben sichtbar
  und konsistent.

### `UserDialog.svelte`

- Die drei fest verdrahteten Urlaubswerte werden durch ein Array pro
  `category_id` ersetzt.
- Neue Benutzer laden Kontodefinitionen über `GET /leave-accounts`.
- Bestehende Benutzer laden ihre Werte über
  `GET /users/{id}/leave-accounts`.
- Basis, aktuelles Jahr und nächstes Jahr werden pro Konto angezeigt.
- Die Rollenwechsel-Snapshot-Logik arbeitet über alle Konten. Bei Auswahl der
  Rolle Aushilfe werden alle Werte auf 0 gesetzt; beim Zurückwechseln werden
  die vorherigen Werte wiederhergestellt.
- Admin- und Teamleitungsdialog verwenden dieselbe Komponente und senden die
  Werte atomar im jeweiligen Benutzer-Request.

### Gemeinsame Tageskonto-Kachel

Eine neue gemeinsame Komponente, beispielsweise
`LeaveAccountCard.svelte`, wird von `Absences.svelte` und
`PersonReport.svelte` verwendet. Sie zeigt kompakt:

- Kategoriename und Farbe,
- prominenten verfügbaren Wert,
- Jahresanspruch und gegebenenfalls Übertrag,
- genommen, genehmigt geplant und beantragt,
- verbleibenden Übertrag und Verfallsstatus.

Dadurch haben Abwesenheitsseite und Mitarbeiterbericht identische
Terminologie, Berechnung und Darstellung.

### Weitere Frontend-Anpassungen

- `Account.svelte` zeigt aktuelles und nächstes Jahreskontingent für jedes
  eigene Tageskonto.
- `reportsApi.js` verwendet die neue Pluralroute und Array-Response.
- `TeamReport.svelte` erzeugt genau eine Spalte pro Konto und zeigt in jeder
  Zelle genommen/geplant. Die bereits vorhandenen lokalen Änderungen für
  umbrechende Header und Tabellenbreite werden erhalten und auf die
  dynamischen Spalten erweitert.
- Report-Helper verwenden `category_id` als Schlüssel.
- `AdminSettings.svelte` entfernt globalen Urlaubsstandard und globales
  Verfallsdatum.
- `AdminUsers.svelte` entfernt die bisherige Normalisierung des globalen
  Verfallsdatums beim Speichern anderer Settings.
- Alle sichtbaren Texte werden in `frontend/src/i18n.js` auf Englisch und
  Deutsch ergänzt. Im Deutschen wird ausschließlich `Aushilfe` verwendet,
  niemals `Assistent`.
- Styling bleibt in CSS beziehungsweise in scoped Styles; es werden keine
  statischen Inline-`style=`-Attribute eingeführt.

## Manuelles Migrations-Runbook

Neue Datei:

```text
docs/leave-account-migration-runbook.md
```

Sie beschreibt, wie bereits im Live-System vorhandene Kategorien wie
`bildungsurlaub` (`cost_type = 'none'`) später bewusst per SQL in eine
Tageskonto-Kategorie umgewandelt werden. Die Anwendung selbst verweigert
diesen Übergang weiterhin.

Achtung: Für die konkrete Produktivinstanz werden `regenerationstage` und
`bildungsurlaub` nicht über dieses allgemeine Runbook, sondern über das weiter
unten beschriebene, einmalige installationsspezifische Nachbehandlungsskript
umgezogen. Der `flextime`-Fall unten betrifft ausschließlich Kategorien, die
tatsächlich `cost_type = 'flextime'` sind (live: `training`/"Fortbildung (keine
Arbeitszeit)" und `flextime_reduction`).

### Variante A: bestehende Kategorie direkt umstellen

In einer Transaktion:

1. Kategorie eindeutig über ID und Slug prüfen.
2. `cost_type`, Standardtage, Verfallsdatum und das gewünschte interne
   Startjahr gemeinsam setzen, damit die DB-Constraint nie einen ungültigen
   Zwischenzustand sieht.
3. `user_leave_accounts` für alle Benutzer anlegen; Aushilfen standardmäßig
   mit 0.
4. Gewünschte Jahres-Overrides anlegen.
5. Explizit entscheiden, welche bestehenden Abwesenheiten dieses neue Konto
   belasten sollen, und für diese
   `leave_account_category_id = category_id` setzen.
6. Zugriffszuordnungen prüfen.
7. Salden vor Commit über Kontrollabfragen plausibilisieren.

Das Runbook erklärt ausdrücklich:

- Ein Wechsel von `flextime` zu Tageskonto ändert rückwirkend die Sollzeit-
  und Gleitzeitinterpretation aller betroffenen Abwesenheiten.
- Das gewählte Startjahr begrenzt Anspruch und Übertrag. Abwesenheiten vor
  diesem Jahr dürfen nur nach bewusster Entscheidung dem Konto zugeordnet
  werden.
- Bereits requested oder cancellation-pending Abwesenheiten reservieren nach
  der Umstellung das neue Konto.

### Variante B: neue Kategorie anlegen und Einträge selektiv verschieben

1. Neue Tageskonto-Kategorie regulär über die UI anlegen.
2. Nach ID, Zeitraum oder anderen eindeutigen Kriterien festlegen, welche
   Abwesenheiten verschoben werden.
3. Für diese Zeilen sowohl `category_id` als auch
   `leave_account_category_id` auf die neue Kategorie setzen.
4. Nicht verschobene Zeilen behalten ihre alte Kategorie und bisherige
   Kontozuordnung.
5. Alte Kategorie gegebenenfalls deaktivieren.

Beide Varianten enthalten:

- verpflichtendes Backup vor Beginn,
- Verweis auf `scripts/backup.sh` und `scripts/restore.sh`,
- Vorher-/Nachher-Abfragen,
- `BEGIN`, Validierung und bewusstes `COMMIT`,
- Hinweise zu Archivbenutzern, Aushilfen, Zugriffen und Jahres-Overrides,
- Warnung, dass direkte SQL-Änderungen den normalen Anwendungsaudit umgehen.

## Installationsspezifisches Nachbehandlungsskript

Neue Datei:

```text
docs/leave-account-post-migration-this-install.sql
```

Dieses Skript ist **installationsspezifisch** und gilt ausschließlich für die
Produktivinstanz, deren `regenerationstage`- und `bildungsurlaub`-Kategorien
vom Betreiber manuell angelegt wurden. Es ist ausdrücklich **kein** Bestandteil
der offiziellen `sqlx`-Migrationen und läuft **genau einmal, nach** dem
erfolgreichen Deployment mit Migration `039`. Es wird von Hand in einer
Transaktion gegen die Live-Datenbank ausgeführt.

### Read-only geprüfter Ausgangszustand

- `regenerationstage` = id 9, `cost_type = 'vacation'`, aktiv. Genau eine
  Abwesenheit: id 21, user 8, 24.-25.08.2026, `approved` (2 Werktage). Diese
  Tage werden heute fälschlich vom Erholungsurlaub abgezogen. 16
  Zugriffszeilen in `user_absence_category_access`.
- `bildungsurlaub` = id 8, `cost_type = 'none'`, aktiv. Vier Abwesenheiten
  von user 8: id 16 (21.07.2026, approved), id 18 (12.08.2026, cancelled),
  id 19 (12.11.2026, approved), id 20 (10.12.2026, approved). 17
  Zugriffszeilen.
- Kanonisches Vacation-Konto = id 1.

Vom Betreiber bestätigte Zielwerte:

- Regenerationstage: 2 Tage pro Jahr, Verfallsdatum `01-01`.
- Bildungsurlaub: 5 Tage pro Jahr, Verfallsdatum `01-01`.
- `01-01` bedeutet fachlich "kein Übertrag ins Folgejahr": ein Übertrag gilt
  für jede Abwesenheit ab dem 1. Januar als verfallen, sodass diese Konten
  jedes Jahr frisch und ohne Restübertrag starten. `01-01` ist ein gültiges
  `MM-DD` und wird von der bestehenden Übertragslogik korrekt als
  sofort-verfallen interpretiert.

### Was das Skript tut

Reihenfolge ist wichtig, weil Migration `039` `regenerationstage` bereits als
`cost_type = 'vacation'`-Kategorie eingelesen und dabei:

- ein eigenes `user_leave_accounts`-0-Tage-Konto für id 9 angelegt,
- `leave_account_default_days = 0`, ein Verfallsdatum und
  `leave_account_start_year = Migrationsjahr` gesetzt und
- die Abwesenheit id 21 auf `leave_account_category_id = 1` (kanonisches
  Vacation-Konto) gebucht hat.

Das Nachbehandlungsskript korrigiert genau diese von `039` erzeugte, fachlich
falsche Zuordnung:

1. **Regenerationstage vom Erholungsurlaub lösen.** Für Abwesenheit id 21
   `leave_account_category_id` von 1 (Vacation) auf 9 (Regenerationstage)
   umsetzen. Damit belastet der bereits genommene Regenerationstag nicht mehr
   den Erholungsurlaub, sondern das eigene Regenerationstage-Konto. Die
   Absicherung erfolgt slug- und id-geprüft, nicht positionsabhängig.
2. **Regenerationstage-Konto mit Anspruch versehen.** Das von `039` erzeugte
   0-Tage-Konto erhält 2 Tage pro Jahr: `leave_account_default_days = 2`,
   `leave_account_carryover_expiry = '01-01'` und `user_leave_accounts.base_days
   = 2` für alle regulären Benutzer (Aushilfen 0). Damit überschreiten die schon
   genommenen 2 Tage von user 8 in 2026 das Budget nicht.
3. **Startjahr des Regenerationstage-Kontos prüfen.** Migration `039` setzt
   `leave_account_start_year` auf das Migrationsjahr; dieses ist in der
   Produktivinstanz 2026 (App-Zeitzone Europe/Berlin) und stimmt mit dem Jahr
   der bestehenden Buchung (24.-25.08.2026) überein. Es ist daher keine
   Korrektur nötig; das Skript verifiziert nur, dass das Startjahr nicht nach
   2026 liegt, damit die vorhandene Abwesenheit nicht vor dem Kontostart fällt.
4. **Bildungsurlaub zum Tageskonto machen.** `bildungsurlaub` (id 8) atomar
   von `cost_type = 'none'` auf `'vacation'` mit gültigen Kontofeldern
   umstellen: `leave_account_default_days = 5`,
   `leave_account_carryover_expiry = '01-01'` und
   `leave_account_start_year = 2026` (früheste betroffene Buchung ist
   21.07.2026) in einem einzigen `UPDATE`, sodass die DB-Constraints keinen
   ungültigen Zwischenzustand sehen.
5. **Bildungsurlaub-Konten und -Buchungen anlegen.** Für alle regulären
   Benutzer eine `user_leave_accounts`-Zeile mit `base_days = 5` (Aushilfen 0)
   erzeugen und die drei nicht stornierten Abwesenheiten (id 16, 19, 20) auf
   `leave_account_category_id = 8` buchen. Die stornierte Abwesenheit id 18
   bleibt bewusst unberührt (reserviert kein Budget). Da alle drei Buchungen im
   selben Jahr 2026 liegen und user 8 mit 3 genommenen Tagen unter 5 bleibt,
   ist kein Jahres-Override nötig; das Skript verifiziert die Budgetgrenze
   dennoch.
6. **Verifikation vor Commit.** Kontrollabfragen bestätigen: kein
   Regenerationstag belastet mehr das Vacation-Konto; das Vacation-Konto
   verliert genau die 2 fälschlich abgezogenen Tage; jede nicht stornierte
   Bildungsurlaubs-Abwesenheit trägt `leave_account_category_id = 8`; jede
   Tageskonto-Kategorie besitzt gültige Kontofelder (Regenerationstage 2 Tage,
   Bildungsurlaub 5 Tage, beide Verfall `01-01`); für jeden Benutzer existiert
   je Konto genau eine Basiszeile; user 8 bleibt in 2026 auf beiden neuen
   Konten innerhalb des Budgets (2 von 2 Regenerationstagen, 3 von 5
   Bildungsurlaubstagen).

### Verpflichtende Rahmenbedingungen

- Vorheriges Backup über `scripts/backup.sh`; dokumentierter Rollback über
  `scripts/restore.sh`.
- Selektion ausschließlich über stabile `slug`-Werte und geprüfte IDs, nie
  über Namen oder Sortierreihenfolge.
- Idempotenz: erneutes Ausführen darf bereits korrigierte Zuordnungen nicht
  doppelt verschieben (Guards auf den Zielzustand).
- Ausführung in einer einzigen Transaktion mit bewusstem `COMMIT` erst nach
  bestandener Verifikation.
- Hinweis, dass die direkte SQL-Korrektur den Anwendungsaudit umgeht, sowie
  Vorher-/Nachher-Saldo-Abfragen für user 8 auf Vacation-, Regenerationstage-
  und Bildungsurlaub-Konto.

## Nachtrag (2026-08-06): Zugriff koppelt jetzt an Guthaben und Kachel-Sichtbarkeit

Bei der Ausführung des installationsspezifischen Nachbehandlungsskripts stellte
sich heraus, dass für diese Instanz zwei Benutzer (ein regulärer Mitarbeiter
ohne Zugriff auf `regenerationstage`, sowie mehrere Aushilfen mit Zugriff auf
`regenerationstage`/`bildungsurlaub`, den sie fachlich nicht haben sollten) ein
sichtbares, aber nicht beantragbares Guthaben erhalten hätten. Der Betreiber
hat daraufhin bewusst folgende, **generelle** Verschärfung der oben unter
"Benutzer" beschriebenen Regel "Konten... verschwinden nicht" verlangt:

- **Zugriffsänderung koppelt jetzt an das Guthaben.** Nur über die
  Kategoriendialog-Zugriffsliste (`PUT /absence-categories/{id}/users`) sowie
  bei der Benutzeranlage (`absence_category_ids`) — **nicht** beim Bearbeiten
  eines bestehenden Benutzers, dort bleibt der Zugriff weiterhin ausschließlich
  über den Kategoriendialog änderbar. Wird einem Benutzer der Zugriff auf ein
  Tageskonto entzogen, wird `base_days` auf 0 gesetzt und alle Jahres-Overrides
  für dieses Konto werden gelöscht. Wird der Zugriff (wieder) gewährt, wird
  `base_days` auf den Kategorienstandard gesetzt (Aushilfen weiterhin 0) und
  das aktuelle sowie nächste Jahr werden explizit auf denselben Standard
  gesetzt. Die Kontenzeile selbst wird nie gelöscht.
- **Kachel-Sichtbarkeit folgt dem Zugriff, mit einer Ausnahme.** Eine
  Tageskonto-Kachel ist sichtbar, wenn der Benutzer Zugriff auf die Kategorie
  hat. Fehlt der Zugriff, bleibt die Kachel dennoch sichtbar, solange eine
  nicht stornierte/abgelehnte Abwesenheit existiert, die dieses Konto belastet
  und deren Enddatum nicht in der Vergangenheit liegt (laufend oder zukünftig).
  Erst wenn jede Belastung dieses Kontos vollständig in der Vergangenheit liegt
  (oder es nie eine gab), verschwindet die Kachel. Das ist eine bewusste
  Verschärfung der ursprünglichen Aussage "entzogene Konten bleiben sichtbar"
  für genau diesen Fall — historische Salden verschwinden dadurch nicht, nur
  eine für den Benutzer nicht mehr beantragbare, aber ungenutzte Kachel.
- Betrifft ausschließlich Backend-Logik
  (`services::absence_categories::set_category_users`,
  `repository::UserDb::revoke_leave_account_tx`/`grant_leave_account_tx`,
  `repository::UserDb::create` mit `absence_category_ids`,
  `services::absence_balance::leave_account_tile_is_visible`, aufgerufen aus
  `services::absences::compute_balances`); keine neue Frontend-Oberfläche, da
  Zugriffsliste und Kacheln bereits existierten.

## Dokumentation und Hilfsskripte

- `docs/user-guide.md` beschreibt Tageskonten aus Benutzersicht, einschließlich
  Kategorieanlage, individuellen Werten, Übertrag, Kacheln und Teambericht.
- Veraltete Abschnitte zu einem einzigen globalen Urlaubskonto und globalem
  Verfallsdatum werden ersetzt, nicht parallel stehen gelassen.
- `AGENTS.md` wird bei Schema, Settings und API-Referenz aktualisiert.
- `scripts/seed_test_data.py` legt Basiswerte und Jahres-Overrides pro
  Kategorie an und setzt bei Tageskonto-Abwesenheiten die belastete Konto-ID.
- `e2e/backup-restore-check.sh` ersetzt `user_annual_leave` in der
  Tabellenliste durch beide neuen Kontingenttabellen und prüft deren
  Wiederherstellung.
- Weitere Test-Fixtures und direkte SQL-Helfer werden über eine vollständige
  Repository-Suche nach den alten Tabellen-, Feld-, Routen- und
  Settingsnamen aktualisiert.

## Tests

### Migration und Repository

- Neuinstallation erzeugt genau das Vacation-Tageskonto mit Standard 30 und
  gültigem Verfallsdatum.
- Migration mit ausschließlich Vacation übernimmt Basiswerte, Overrides und
  Verfallsdatum und setzt das neue Startjahr auf das früheste Jahr aller
  Mitarbeiter-`start_date`-Werte.
- Migration mit mehreren bisherigen `cost_type = 'vacation'`-Kategorien:
  - übernimmt alte Basiswerte und Overrides nur für Vacation,
  - erzeugt alle anderen Konten mit Basiswert 0,
  - setzt deren Startjahr auf das Migrationsjahr,
  - ordnet alle bestehenden betroffenen Abwesenheiten weiterhin dem
    kanonischen Vacation-Konto als belastetes Konto zu.
- Eine fehlende kanonische Vacation-Kategorie bricht verständlich ab.
- Ungültige globale Altwerte brechen verständlich ab.
- Wiederholtes Ausführen der Migrationslogik verändert keine bereits
  migrierten Kontozuordnungen.
- Foreign Keys, Cascades, Checks und Unique Constraints werden direkt geprüft.

### Backend-Unit- und Integrationstests

- Zwei neue Tageskonten werden unabhängig belastet.
- Historisch migrierte Abwesenheiten einer anderen Kategorie belasten weiter
  Vacation; neue Abwesenheiten derselben Kategorie belasten deren eigenes
  0-Tage-Konto.
- Ein reines Datums-/Kommentarupdate verschiebt die Kontobelastung nicht.
- Ein tatsächlicher Kategorienwechsel aktualisiert die Kontobelastung.
- Unterschiedliche Verfallsdaten wirken unabhängig.
- Ein neues Konto erzeugt keinen Übertrag vor seinem Startjahr.
- Vacation behält mit dem frühesten Mitarbeiterstartjahr seine historische
  Berechnung.
- Requested, approved und cancellation-pending werden pro Konto korrekt
  behandelt; rejected und cancelled wirken nicht.
- Jahresübergreifende Anträge validieren beide Jahre desselben Kontos.
- Antrag, Bearbeitung und Genehmigung verwenden die gespeicherte Konto-ID.
- Unverändertes `cost_type = 'vacation'` ist editierbar; beide verbotenen
  Übergangsrichtungen werden unabhängig von Nutzung abgelehnt.
- `none`/`flextime` behält die bisherige Nutzungssperre.
- Kategorieanlage seedet bestehende aktive und archivierte Benutzer;
  Aushilfen erhalten 0.
- Benutzeranlage seedet alle Konten und überschreibt explizite Werte atomar.
- Parallel angelegter Benutzer und parallel angelegtes Konto hinterlassen
  keine fehlende Zuordnung.
- Initial-Admin-Anlage initialisiert Konten korrekt.
- Berechtigungen für Admin, eigene Daten, Teamleitungen und fremde Benutzer
  bleiben korrekt.
- Deaktivierte beziehungsweise zugriffsentzogene Kategorien behalten
  historische Salden.
- Teambericht liefert ID-basierte Kontometadaten und korrekte Werte ohne
  N+1-Abfragemuster.
- Tageskonto-Kategorien werden nicht als Payroll-relevante Abwesenheiten
  aufgenommen.
- Settings-API und Ersteinrichtungsprüfung enthalten keine alten globalen
  Urlaubssettings mehr.

### Frontendtests

- Kategorienformular validiert Standardtage und Pflicht-Verfallsdatum.
- Das unsichtbare Startjahr erscheint weder im DOM noch im Request.
- Bestehende Kategorien können nicht nachträglich Tageskonten werden.
- Bestehende Tageskonten können nicht verlassen, aber mit unverändertem Typ
  gespeichert werden.
- UserDialog rendert beliebig viele Konten per ID und sendet die atomare
  Payload.
- Aushilfen-Nullsetzung und Snapshot-Wiederherstellung funktionieren für alle
  Konten.
- Account-, Abwesenheits- und Mitarbeiterbericht rendern mehrere Konten.
- Die gemeinsame Kachel zeigt Anspruch, Nutzung, Verfügbarkeit und Übertrag
  korrekt.
- Teambericht erzeugt eine Spalte pro Konto und zeigt genommen/geplant in
  einer Zelle.
- Doppelte Kategorienamen verursachen keine falsche Zuordnung.
- AdminSettings und AdminUsers senden keine entfernten globalen Felder.

### Playwright-E2E

Der bestehende realistische Ablauf wird erweitert, ohne Szenariocode zu
duplizieren:

1. Admin legt nach den bereits vorhandenen Mitarbeitern ein zweites
   Tageskonto, beispielsweise `E2E Bildungsurlaub`, mit Standard und eigenem
   Verfallsdatum an.
2. Der Test prüft, dass ein bestehender normaler Mitarbeiter den Standard
   erhalten hat.
3. Der Admin öffnet die Kategorie erneut und prüft, dass der Tageskonto-Typ
   gesperrt ist, Standard und Verfallsdatum aber editierbar bleiben.
4. Eine später angelegte Aushilfe erhält in beiden Konten 0 und kann im
   aktuellen sowie nächsten Jahr individuell bearbeitet werden.
5. Der Mitarbeiter beantragt eine Abwesenheit im zweiten Konto.
6. Die Abwesenheitsseite zeigt beide kompakten Kacheln und beweist, dass nur
   das zweite Konto reduziert wurde.
7. Nach Genehmigung zeigt dieselbe Kachel den Wert als geplant statt
   beantragt.
8. Der Mitarbeiterbericht zeigt dieselben beiden Konten mit denselben Werten.
9. Der Teambericht zeigt je Konto genau eine Spalte und die richtigen Werte
   für genommen/geplant.
10. Der Ablauf prüft zusätzlich, dass die bestehende Vacation-Kachel
    unverändert bleibt.

Wiederverwendbare Helfer liegen in `e2e/tests/helpers.js`; Konstanten in
`e2e/tests/users.js`. Selektoren verwenden stabile IDs, Rollen und
`data-testid`-Attribute statt positionsabhängiger DOM-Annahmen.

## Verifikation

Backend:

```bash
cd backend
cargo fmt --check
cargo build
cargo clippy -- -D warnings
cargo test --lib
TEST_REFERENCE_DATE=2030-01-07 TEST_DATABASE_URL=postgres://... cargo test
```

Architekturgrenzen:

```bash
grep -rn "sqlx::" backend/src/handlers/
grep -rn "axum::extract\|axum::response\|axum::routing\|axum::Json" backend/src/services/
```

Beide Ausgaben müssen leer sein.

Frontend:

```bash
cd frontend
npm run format
npm run lint
npm test -- --run
npm run build
```

End-to-End:

```bash
./e2e/run.sh
```

Abschließend wird repositoryweit geprüft, dass keine produktive Referenz auf
folgende entfernte Schnittstellen oder Felder verbleibt:

```text
users.annual_leave_days
user_annual_leave
default_annual_leave_days
carryover_expiry_date
/users/{id}/leave-days
/leave-balance/{uid}
vacation_days
vacation_planned_days
```

Vorkommen in alten, bereits angewendeten Migrationsdateien bleiben
selbstverständlich unverändert.


Minor tightening points (unchanged, not errors)
GET /leave-accounts regular-user scope (PLAN.md:396) is redundant with /users/{id}/leave-accounts.
Add the renamed internal helpers to the final residual-reference grep (PLAN.md:877) to enforce the no-dead-code promise.
Call out that the first-run gate conjunct in auth.rs:254 must be removed, not just the settings storage.
