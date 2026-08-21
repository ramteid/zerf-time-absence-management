<script>
  // One person's flextime account: the running balance, every admin booking
  // behind it, and (for admins) the form to add or undo one.
  //
  // This is the only place a flextime balance can be changed by hand. It
  // deliberately lives outside the user profile: a booking is dated, so it
  // moves the balance from that day onwards and leaves earlier history alone.
  import { t, formatHours, fmtDecimal, parseDecimal } from "../i18n.js";
  import { fmtDate, fmtDateTime, minToHM, appTodayIsoDate } from "../format.js";
  import { settings, toast } from "../stores.js";
  import { confirmDialog } from "../confirm.js";
  import Dialog from "../Dialog.svelte";
  import DatePicker from "../DatePicker.svelte";
  import Icon from "../Icons.svelte";
  import {
    getFlextimeAccount,
    createFlextimeAdjustment,
    reverseFlextimeAdjustment,
  } from "../lib/api/usersApi.js";

  export let userId;
  export let onClose;
  // Non-admins get a read-only view: they may understand their balance, but
  // not change it. The backend enforces the same rule.
  export let canEdit = false;

  let dialog;
  let account = null;
  let loading = true;
  let loadError = "";
  let saving = false;
  let error = "";
  // TRUE once anything was booked or removed, so the caller can refresh the
  // views that show a balance.
  let changed = false;

  let effective_date = appTodayIsoDate($settings?.timezone);
  let hours = "";
  let reason = "";

  $: todayIso = appTodayIsoDate($settings?.timezone);
  // Any date from the contract start onwards, the future included: an overtime
  // payout agreed for month end is recorded when it is agreed and takes effect
  // when that day arrives.
  $: canBook = canEdit && !!account?.has_flextime_account;

  async function load() {
    loading = true;
    loadError = "";
    try {
      account = await getFlextimeAccount(userId);
      // Default to today, unless the contract has not started yet — the ledger
      // does not exist before then, so its first day is the earliest option.
      const today = appTodayIsoDate($settings?.timezone);
      effective_date = account.start_date > today ? account.start_date : today;
    } catch (e) {
      account = null;
      loadError = $t(e?.message || "Error");
    } finally {
      loading = false;
    }
  }
  load();

  // Same signed HH:MM presentation the report tiles use: minToHM already
  // carries the minus sign, so only the plus has to be added.
  function signedHours(minutes) {
    return formatHours((minutes >= 0 ? "+" : "") + minToHM(minutes));
  }

  function kindLabel(adjustment) {
    if (adjustment.reverses_id) return $t("Cancellation");
    return adjustment.kind === "opening_balance"
      ? $t("Hours brought along")
      : $t("Correction");
  }

  async function book() {
    error = "";
    const parsed = parseDecimal(hours);
    if (!Number.isFinite(parsed) || parsed === 0) {
      error = $t("Enter the number of hours to add or subtract.");
      return;
    }
    if (!effective_date) {
      error = $t("Invalid date.");
      return;
    }
    // Round to whole minutes the same way the user dialog does, so a value
    // typed as hours never lands on a fractional minute.
    const minutes = Math.round((Math.round(parsed * 100) / 100) * 60);
    saving = true;
    try {
      await createFlextimeAdjustment(userId, {
        effective_date,
        minutes,
        reason: reason.trim() || null,
      });
      hours = "";
      reason = "";
      changed = true;
      await load();
      toast($t("Flextime balance updated."), "ok");
    } catch (e) {
      error = $t(e?.message || "Error");
    } finally {
      saving = false;
    }
  }

  // Entries are never deleted. Cancelling one books its opposite on the same
  // date, so the balance returns to what it was while both rows stay visible —
  // deleting would move every balance since that date with nothing left to
  // explain it, which is the problem this whole account exists to solve.
  async function reverse(adjustment) {
    if (
      !(await confirmDialog(
        $t("Cancel this entry?"),
        $t(
          "The opposite amount is booked on the same date, so the balance returns to what it was. Both entries stay on the record.",
        ),
        { confirm: $t("Cancel entry") },
      ))
    )
      return;
    try {
      await reverseFlextimeAdjustment(adjustment.id);
      changed = true;
      await load();
      toast($t("Flextime balance updated."), "ok");
    } catch (e) {
      toast($t(e?.message || "Error"), "error");
    }
  }
</script>

<Dialog
  bind:this={dialog}
  title={$t("Flextime account")}
  onClose={() => onClose(changed)}
  wide
  let:dlg
>
  {#if loading}
    <div class="field-hint">{$t("Loading...")}</div>
  {:else if loadError}
    <div class="error-text">{loadError}</div>
  {:else if account}
    <div class="account-head">
      <div>
        <div class="zf-item-title">{account.user_name}</div>
        {#if account.has_flextime_account}
          <div class="text-hint">
            {$t("Approved hours counted up to {date}", {
              date: fmtDate(account.balance_as_of),
            })}
          </div>
        {/if}
      </div>
      {#if account.has_flextime_account}
        <div
          class="account-balance tab-num"
          class:is-negative={account.balance_min < 0}
        >
          {signedHours(account.balance_min || 0)}
        </div>
      {/if}
    </div>

    {#if !account.has_flextime_account}
      <div class="field-hint">
        {$t(
          "This person has no flextime account, so there is no balance to correct.",
        )}
      </div>
    {:else}
      {#if canBook}
        <div class="zf-card zf-card-section booking-form">
          <div class="field-section-label">{$t("Add an entry")}</div>
          <div class="field-row">
            <div>
              <label class="zf-label" for="flextime-adjustment-date"
                >{$t("Effective from")}</label
              >
              <DatePicker
                id="flextime-adjustment-date"
                bind:value={effective_date}
                min={account.start_date}
                container={dlg}
              />
            </div>
            <div>
              <label class="zf-label" for="flextime-adjustment-hours"
                >{$t("Hours")}</label
              >
              <input
                id="flextime-adjustment-hours"
                class="zf-input"
                type="text"
                inputmode="decimal"
                placeholder={fmtDecimal(-2.5, 2)}
                bind:value={hours}
              />
            </div>
          </div>
          <div>
            <label class="zf-label" for="flextime-adjustment-reason"
              >{$t("Note")}</label
            >
            <input
              id="flextime-adjustment-reason"
              class="zf-input"
              type="text"
              maxlength="500"
              bind:value={reason}
            />
            <div class="field-hint">
              {$t(
                "Negative subtracts hours, for example when overtime is paid out. Balances before the chosen date stay as they are.",
              )}
            </div>
          </div>
          {#if error}
            <div class="error-text">{error}</div>
          {/if}
          <div class="booking-actions">
            <button
              class="zf-btn zf-btn-primary zf-btn-sm"
              on:click={book}
              disabled={saving}
            >
              {saving ? $t("Saving...") : $t("Add entry")}
            </button>
          </div>
        </div>
      {/if}

      <div class="field-section-label">{$t("Entries")}</div>
      {#if account.adjustments.length === 0}
        <div class="field-hint">
          {$t("No entries yet — the balance comes purely from booked hours.")}
        </div>
      {:else}
        <div class="zf-card zf-table-wrap">
          {#each account.adjustments as adjustment (adjustment.id)}
            <div class="adjustment-row" class:is-reversed={adjustment.reversed}>
              <div class="flex-min0">
                <div class="zf-item-title">
                  {fmtDate(adjustment.effective_date)}
                  <span class="text-hint">· {kindLabel(adjustment)}</span>
                  {#if adjustment.reversed}
                    <span class="zf-chip">{$t("Cancelled")}</span>
                  {/if}
                  {#if adjustment.effective_date > todayIso}
                    <span class="zf-chip">{$t("Takes effect later")}</span>
                  {/if}
                </div>
                <div class="text-hint">
                  {adjustment.reason || $t("No note")}
                  · {$t("Added by {name} on {date}", {
                    name: adjustment.created_by_name || $t("System"),
                    date: fmtDateTime(adjustment.created_at),
                  })}
                </div>
              </div>
              <div
                class="adjustment-amount tab-num"
                class:is-negative={adjustment.minutes < 0}
              >
                {signedHours(adjustment.minutes)}
              </div>
              <!-- Cancelling writes the opposite entry; an entry that is
                   already cancelled, or is itself a cancellation, has nothing
                   left to cancel. -->
              {#if canEdit && !adjustment.reversed && !adjustment.reverses_id}
                <button
                  class="zf-btn zf-btn-ghost zf-btn-sm"
                  title={$t("Cancel entry")}
                  on:click={() => reverse(adjustment)}
                >
                  <Icon name="X" size={13} />
                </button>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  {/if}
  <svelte:fragment slot="footer">
    <span class="flex-1"></span>
    <button class="zf-btn" on:click={() => dialog.close()}>{$t("Close")}</button
    >
  </svelte:fragment>
</Dialog>

<style>
  .account-head {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 16px;
  }

  .account-balance {
    margin-left: auto;
    font-size: 1.5rem;
    font-weight: 600;
    color: var(--success-text);
  }

  .account-balance.is-negative,
  .adjustment-amount.is-negative {
    color: var(--danger-text);
  }

  .booking-form {
    margin-bottom: 16px;
  }

  .booking-actions {
    display: flex;
    justify-content: flex-end;
    padding-top: 12px;
  }

  .adjustment-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 16px;
  }

  .adjustment-row:not(:last-child) {
    border-bottom: 1px solid var(--border);
  }

  .adjustment-amount {
    font-weight: 600;
    color: var(--success-text);
    white-space: nowrap;
  }

  /* A cancelled entry still counts in the ledger — its cancellation is a row
     of its own — but it is no longer the live reason for the balance, so it
     recedes rather than disappearing. */
  .adjustment-row.is-reversed {
    opacity: 0.6;
  }

  .adjustment-row.is-reversed .adjustment-amount {
    text-decoration: line-through;
  }
</style>
