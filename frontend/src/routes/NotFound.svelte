<script>
  import { t } from "../i18n.js";
</script>

<div class="content-area not-found">
  <div class="not-found-message">
    <div class="not-found-code">404</div>
    <div>{$t("Page not found")}</div>
  </div>
</div>

<style>
  .not-found {
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .not-found-message {
    text-align: center;
    color: var(--text-tertiary);
    font-size: 0.9375rem;
  }

  .not-found-code {
    font-size: 3rem;
    font-weight: 700;
    margin-bottom: 8px;
  }
</style>
