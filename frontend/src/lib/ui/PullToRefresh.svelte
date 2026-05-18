<script>
  import Icon from "../../Icons.svelte";

  let pullStartY = 0;
  let pulling = false;
  let pullDistance = 0;
  let refreshing = false;
  const PULL_THRESHOLD = 80;
  let pullEl;

  function getPullScrollContainer(target) {
    if (target instanceof Element) {
      const container = target.closest(".content-area");
      if (container) return container;
    }
    return document.querySelector(".main-content .content-area");
  }

  function onTouchStart(e) {
    if (
      e.touches.length === 1 &&
      e.target instanceof Element &&
      !e.target.closest(".tp-drum") &&
      !e.target.closest("dialog")
    ) {
      const scrollContainer = getPullScrollContainer(e.target);
      if (
        scrollContainer ? scrollContainer.scrollTop > 0 : window.scrollY > 0
      ) {
        pulling = false;
        pullDistance = 0;
        return;
      }
      pullStartY = e.touches[0].clientY;
      pulling = true;
    }
  }

  function onTouchMove(e) {
    if (!pulling) return;
    const dragDistanceY = e.touches[0].clientY - pullStartY;
    if (dragDistanceY > 0) {
      const wasHidden = pullDistance === 0;
      pullDistance = Math.min(dragDistanceY * 0.5, 120);
      if (wasHidden) {
        try {
          pullEl?.showPopover?.();
        } catch {}
      }
    } else {
      pulling = false;
      pullDistance = 0;
      try {
        pullEl?.hidePopover?.();
      } catch {}
    }
  }

  async function onTouchEnd() {
    if (!pulling) return;
    if (pullDistance >= PULL_THRESHOLD && !refreshing) {
      refreshing = true;
      pullDistance = PULL_THRESHOLD;
      await new Promise((resolveDelay) => setTimeout(resolveDelay, 300));
      location.reload();
      return;
    }
    pulling = false;
    pullDistance = 0;
    try {
      pullEl?.hidePopover?.();
    } catch {}
  }
</script>

<svelte:window
  on:touchstart={onTouchStart}
  on:touchmove={onTouchMove}
  on:touchend={onTouchEnd}
/>

<div
  class="pull-to-refresh"
  class:ptr-open={pullDistance > 0}
  style="height:{pullDistance}px"
  popover="manual"
  bind:this={pullEl}
>
  {#if pullDistance > 0}
    <div class="pull-spinner" class:active={pullDistance >= PULL_THRESHOLD}>
      {#if refreshing}
        <Icon name="Clock" size={20} />
      {:else}
        <span
          style="transform:rotate({pullDistance * 3}deg);display:inline-block"
        >
          &#8595;
        </span>
      {/if}
    </div>
  {/if}
</div>
