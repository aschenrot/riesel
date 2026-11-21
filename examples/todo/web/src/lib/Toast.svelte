<script lang="ts">
  import { onMount } from 'svelte';

  let {
    message,
    onDismiss,
    timeout = 3200,
    actionLabel,
    onAction,
    actionDisabled = false
  }: {
    message: string;
    onDismiss: () => void;
    timeout?: number;
    actionLabel?: string;
    onAction?: () => void;
    actionDisabled?: boolean;
  } = $props();

  let container: HTMLDivElement;
  let timer: ReturnType<typeof setTimeout> | undefined;

  function pause() {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  }

  function schedule() {
    pause();
    timer = setTimeout(onDismiss, timeout);
  }

  function onFocusOut(event: FocusEvent) {
    const next = event.relatedTarget;
    if (!(next instanceof Node) || !container.contains(next)) schedule();
  }

  onMount(() => {
    container.addEventListener('pointerenter', pause);
    container.addEventListener('pointerleave', schedule);
    container.addEventListener('focusin', pause);
    container.addEventListener('focusout', onFocusOut);
    schedule();

    return () => {
      pause();
      container.removeEventListener('pointerenter', pause);
      container.removeEventListener('pointerleave', schedule);
      container.removeEventListener('focusin', pause);
      container.removeEventListener('focusout', onFocusOut);
    };
  });
</script>

<div class="toast" role="status" aria-live="polite" bind:this={container}>
  <span class="toast-check" aria-hidden="true">✓</span>
  <span class="toast-message">{message}</span>
  {#if actionLabel && onAction}
    <button type="button" class="toast-action" disabled={actionDisabled} onclick={onAction}>{actionLabel}</button>
  {/if}
  <button type="button" class="toast-dismiss" aria-label="Dismiss notification" onclick={onDismiss}>
    <span aria-hidden="true">×</span>
  </button>
</div>
