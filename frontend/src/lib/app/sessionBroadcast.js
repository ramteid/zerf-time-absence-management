let sessionChannel = null;

try {
  if (typeof BroadcastChannel !== "undefined") {
    sessionChannel = new BroadcastChannel("zerf-session");
  }
} catch {}

export function broadcastSession(type) {
  try {
    sessionChannel?.postMessage({ type });
  } catch {}
}

export function onSessionBroadcast(fn) {
  if (!sessionChannel) return () => {};
  const handler = (event) => fn(event.data);
  sessionChannel.addEventListener("message", handler);
  return () => sessionChannel.removeEventListener("message", handler);
}
