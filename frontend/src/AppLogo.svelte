<script>
  export let size = null;

  // Unique gradient ID so multiple instances don't collide in the DOM
  const uid = "logo-" + Math.random().toString(36).slice(2, 8);
</script>

<!-- Matches favicon.svg — keep both in sync -->
<svg
  width={size}
  height={size}
  viewBox="0 0 32 32"
  xmlns="http://www.w3.org/2000/svg"
  class="app-logo"
>
  <defs>
    <linearGradient id={uid} x1="0" y1="0" x2="1" y2="1">
      <stop offset="0%" stop-color="#0D9488"/>
      <stop offset="100%" stop-color="#0F766E"/>
    </linearGradient>
  </defs>
  <!-- Rounded square background (self-contained, works on any page bg) -->
  <rect width="32" height="32" rx="7.5" fill="url(#{uid})"/>
  <!-- Clock face: pure white, fully opaque for clean contrast in dark & light -->
  <circle cx="16" cy="16" r="10.5" fill="#ffffff"/>
  <circle cx="16" cy="16" r="9.5" fill="none" stroke="#0F766E" stroke-width="0.3" opacity="0.15"/>
  <!-- Hour markers (12, 3, 6, 9) -->
  <line x1="16" y1="6.8" x2="16" y2="8.2" stroke="#0F766E" stroke-width="1.2" stroke-linecap="round"/>
  <line x1="22.8" y1="16" x2="24.2" y2="16" stroke="#0F766E" stroke-width="1.2" stroke-linecap="round"/>
  <line x1="16" y1="23.8" x2="16" y2="25.2" stroke="#0F766E" stroke-width="1.2" stroke-linecap="round"/>
  <line x1="7.8" y1="16" x2="9.2" y2="16" stroke="#0F766E" stroke-width="1.2" stroke-linecap="round"/>
  <!-- Small markers (hours 1,2,4,5,7,8,10,11) at r=9 from center -->
  <circle cx="20.5" cy="8.2" r="0.5" fill="#0F766E" opacity="0.3"/>
  <circle cx="23.8" cy="11.5" r="0.5" fill="#0F766E" opacity="0.3"/>
  <circle cx="23.8" cy="20.5" r="0.5" fill="#0F766E" opacity="0.3"/>
  <circle cx="20.5" cy="23.8" r="0.5" fill="#0F766E" opacity="0.3"/>
  <circle cx="11.5" cy="23.8" r="0.5" fill="#0F766E" opacity="0.3"/>
  <circle cx="8.2" cy="20.5" r="0.5" fill="#0F766E" opacity="0.3"/>
  <circle cx="8.2" cy="11.5" r="0.5" fill="#0F766E" opacity="0.3"/>
  <circle cx="11.5" cy="8.2" r="0.5" fill="#0F766E" opacity="0.3"/>
  <!-- Hour hand -->
  <line x1="16" y1="16" x2="11.2" y2="10.8" stroke="#0F766E" stroke-width="2" stroke-linecap="round"/>
  <!-- Minute hand -->
  <line x1="16" y1="16" x2="21.5" y2="10.2" stroke="#0F766E" stroke-width="1.4" stroke-linecap="round"/>
  <!-- Second hand accent -->
  <line x1="16" y1="16" x2="16" y2="22.5" stroke="#14B8A6" stroke-width="0.7" stroke-linecap="round" opacity="0.7"/>
  <!-- Center cap -->
  <circle cx="16" cy="16" r="1.8" fill="#0F766E"/>
  <circle cx="16" cy="16" r="0.9" fill="#ffffff"/>
</svg>
