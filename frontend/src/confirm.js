import Confirm from "./Confirm.svelte";

export function confirmDialog(title, text, opts = {}) {
  return new Promise((resolve) => {
    const target = document.createElement("div");
    document.body.appendChild(target);
    const onResolve = (v) => {
      resolve(v);
      cmp.$destroy();
      target.remove();
    };
    const cmp = new Confirm({
      target,
      props: {
        title,
        text,
        confirmLabel: opts.confirm || "OK",
        danger: !!opts.danger,
        needReason: !!opts.reason,
        onResolve,
      },
    });
  });
}
