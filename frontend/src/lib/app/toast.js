import { writable } from "svelte/store";

export const toasts = writable([]);
let nextToastId = 0;

export function toast(message, type = "info") {
  const toastId = ++nextToastId;
  toasts.update((arr) => [...arr, { id: toastId, message, type }]);
  setTimeout(
    () => toasts.update((arr) => arr.filter((item) => item.id !== toastId)),
    3500,
  );
}
