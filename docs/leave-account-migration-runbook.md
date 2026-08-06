# Tageskonto-Kategorien manuell umziehen

Dieses Runbook gilt fuer eine bestehende Abwesenheitskategorie, die bewusst zu
einer Tageskonto-Kategorie umgestellt werden soll. Die Zerf-Oberflaeche erlaubt
diesen Wechsel absichtlich nicht: Eine direkte Umstellung veraendert die
fachliche Bedeutung vorhandener Abwesenheiten und muss einzeln geprueft werden.

Fertigen Sie vor Beginn ein verschluesseltes Backup an und pruefen Sie dessen
Wiederherstellbarkeit:

```bash
scripts/backup.sh
# Bei Bedarf: scripts/restore.sh
```

Direkte SQL-Aenderungen erscheinen nicht im Zerf-Audit-Log. Dokumentieren Sie
Anlass, ausfuehrende Person, Zeitpunkt und die geprueften Abfragen separat.

## Vorbereitung

1. Waehlen Sie die Kategorie ausschliesslich ueber ihre stabile ID und ihren
   Slug aus. Verwenden Sie nie den Anzeigenamen oder eine Sortierreihenfolge.
2. Legen Sie Standardtage (0 bis 366), ein echtes Verfallsdatum im Format
   `MM-DD` und ein Startjahr fest.
3. Entscheiden Sie fuer jede vorhandene Abwesenheit, ob sie das neue Konto
   belasten soll. Abwesenheiten vor dem Startjahr duerfen nur nach einer
   ausdruecklichen fachlichen Entscheidung zugeordnet werden.
4. Pruefen Sie archivierte Benutzer, Aushilfen, Kategorie-Zugriffe sowie
   vorhandene Jahres-Overrides. Aushilfen erhalten standardmaessig 0 Tage.

Die folgenden Abfragen muessen vor dem `BEGIN` genau eine Zielkategorie und
die erwarteten Abwesenheiten zeigen. Ersetzen Sie die Platzhalter niemals
blind.

```sql
SELECT id, slug, name, cost_type,
       leave_account_default_days, leave_account_carryover_expiry,
       leave_account_start_year
FROM absence_categories
WHERE id = :category_id AND slug = :category_slug;

SELECT id, user_id, category_id, leave_account_category_id,
       start_date, end_date, status
FROM absences
WHERE category_id = :category_id
ORDER BY user_id, start_date, id;

SELECT u.id, u.email, u.role, u.archived_at, a.category_id, a.base_days
FROM users u
LEFT JOIN user_leave_accounts a
  ON a.user_id = u.id AND a.category_id = :category_id
ORDER BY u.id;
```

## Variante A: Bestehende Kategorie umstellen

Fuehren Sie die Aenderung in einer einzigen Transaktion aus. Die Zielwerte
werden gemeinsam gesetzt, damit die Datenbank-Constraint nie einen
Zwischenzustand ohne vollstaendige Konto-Konfiguration sieht.

```sql
BEGIN;

DO $$
DECLARE
    matches integer;
BEGIN
    SELECT count(*) INTO matches
    FROM absence_categories
    WHERE id = :category_id AND slug = :category_slug;
    IF matches <> 1 THEN
        RAISE EXCEPTION 'Expected exactly one target category (% / %)',
            :category_id, :category_slug;
    END IF;
END $$;

UPDATE absence_categories
SET cost_type = 'vacation',
    leave_account_default_days = :default_days,
    leave_account_carryover_expiry = :carryover_expiry,
    leave_account_start_year = :start_year
WHERE id = :category_id AND slug = :category_slug;

INSERT INTO user_leave_accounts (user_id, category_id, base_days)
SELECT u.id,
       :category_id,
       CASE WHEN u.role = 'assistant' THEN 0 ELSE :default_days END
FROM users u
ON CONFLICT (user_id, category_id) DO NOTHING;

INSERT INTO user_leave_account_year_overrides (user_id, category_id, year, days)
VALUES (:user_id, :category_id, :year, :override_days)
ON CONFLICT (user_id, category_id, year) DO UPDATE
SET days = EXCLUDED.days;

UPDATE absences
SET leave_account_category_id = :category_id
WHERE id = ANY(:absence_ids)
  AND category_id = :category_id
  AND status IN ('requested', 'approved', 'cancellation_pending');
```

Eine Umstellung von `flextime` auf ein Tageskonto veraendert rueckwirkend die
Sollzeit- und Gleitzeitinterpretation vorhandener Abwesenheiten. Nehmen Sie
diese Variante nur nach einer ausdruecklichen fachlichen Freigabe. Bereits
beantragte oder stornierungs-pendente Abwesenheiten reservieren nach ihrer
Zuordnung das neue Konto.

## Variante B: Neue Kategorie anlegen und Eintraege verschieben

Legen Sie die neue Tageskonto-Kategorie zuerst regulaer in Zerf an. Die
Anwendung erzeugt Kontenzeilen und Zugriffe fuer alle Benutzer. Waehlen Sie
anschliessend die zu verschiebenden Abwesenheiten anhand stabiler Kriterien
wie IDs oder eines klaren Zeitraums aus.

```sql
BEGIN;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM absence_categories
        WHERE id = :new_category_id
          AND slug = :new_category_slug
          AND cost_type = 'vacation'
    ) THEN
        RAISE EXCEPTION 'The new category is not the expected leave account';
    END IF;
END $$;

UPDATE absences
SET category_id = :new_category_id,
    leave_account_category_id = :new_category_id
WHERE id = ANY(:absence_ids);
```

Nicht ausgewaehlte Zeilen behalten Kategorie und Kontozuordnung. Deaktivieren
Sie die alte Kategorie erst nach der fachlichen Kontrolle.

## Kontrolle vor dem Commit

Fuehren Sie mindestens diese Abfragen aus. Erst wenn sie fachlich plausibel
sind, ersetzen Sie `ROLLBACK` durch `COMMIT`.

```sql
SELECT c.id, c.slug, c.cost_type,
       c.leave_account_default_days,
       c.leave_account_carryover_expiry,
       c.leave_account_start_year,
       count(a.user_id) AS account_rows,
       (SELECT count(*) FROM users) AS users
FROM absence_categories c
LEFT JOIN user_leave_accounts a ON a.category_id = c.id
WHERE c.id = :category_id
GROUP BY c.id, c.slug, c.cost_type, c.leave_account_default_days,
         c.leave_account_carryover_expiry, c.leave_account_start_year;

SELECT a.id, a.user_id, a.category_id, a.leave_account_category_id,
       a.start_date, a.end_date, a.status
FROM absences a
WHERE a.id = ANY(:absence_ids)
ORDER BY a.user_id, a.start_date, a.id;

SELECT user_id, category_id, year, days
FROM user_leave_account_year_overrides
WHERE category_id = :category_id
ORDER BY user_id, year;

ROLLBACK;
```

Nach der Kontrolle kann dieselbe Transaktion noch einmal mit `COMMIT`
ausgefuehrt werden. Oeffnen Sie danach Abwesenheitsseite, Mitarbeiterbericht
und Teambericht fuer betroffene Benutzer und vergleichen Sie Anspruch,
Uebertrag und Nutzung mit den vorher dokumentierten Werten.
