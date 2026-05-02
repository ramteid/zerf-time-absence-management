<script>
  import { path } from "../stores.js";
  import { t } from "../i18n.js";

  $: pathname = (() => {
    const i = $path.indexOf("?");
    return i >= 0 ? $path.slice(0, i) : $path;
  })();

  const tabs = [
    { href: "/admin/users", key: "Team" },
    { href: "/admin/categories", key: "Categories" },
    { href: "/admin/holidays", key: "Holidays" },
    { href: "/admin/settings", key: "Settings" },
    { href: "/admin/audit-log", key: "Audit Log" },
  ];
</script>

<div
  style="display:flex;gap:2px;padding:0 28px;border-bottom:1px solid var(--border);background:var(--bg-surface)"
>
  {#each tabs as tab}
    <a
      href={tab.href}
      data-link="1"
      style="padding:10px 14px;font-size:13px;font-weight:500;color:{pathname ===
      tab.href
        ? 'var(--accent)'
        : 'var(--text-secondary)'};border-bottom:2px solid {pathname ===
      tab.href
        ? 'var(--accent)'
        : 'transparent'};text-decoration:none;transition:color .12s"
    >
      {$t(tab.key)}
    </a>
  {/each}
</div>
