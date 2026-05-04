import { mount, unmount } from "svelte";
import Confirm from "./Confirm.svelte";

export function confirmDialog(title, text, opts = {}) {
  return new Promise((resolve) => {
    const target = document.createElement("div");
    document.body.appendChild(target);
    let cmp;
    const onResolve = (v) => {
      resolve(v);
      unmount(cmp);
      target.remove();
    };
    cmp = mount(Confirm, {
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
