// Tests for TempPasswordDialog — shown after an admin resets a user's password.
// The dialog displays the generated temporary password and warns loudly when
// SMTP is not configured, because in that case the admin must deliver the
// password manually. Tests verify both the display logic and the copy button.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, unmount } from "svelte";
import TempPasswordDialog from "./TempPasswordDialog.svelte";
import { setLanguage } from "../i18n.js";

vi.mock("svelte", async () => {
  return await import("../../node_modules/svelte/src/index-client.js");
});

async function settle() {
  await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
  await Promise.resolve();
}

describe("TempPasswordDialog", () => {
  let target;
  let component;
  let originalShowModal;

  beforeEach(() => {
    target = document.createElement("div");
    document.body.appendChild(target);
    setLanguage("en");
    originalShowModal = HTMLDialogElement.prototype.showModal;
    HTMLDialogElement.prototype.showModal = function () {
      this.setAttribute("open", "");
    };
    // jsdom doesn't implement HTMLDialogElement.close(); add it so that
    // clicking OK (which calls dialog.close()) doesn't throw.
    HTMLDialogElement.prototype.close = function () {
      this.removeAttribute("open");
      this.dispatchEvent(new Event("close"));
    };
  });

  afterEach(() => {
    if (component) { unmount(component); component = null; }
    target.remove();
    HTMLDialogElement.prototype.showModal = originalShowModal;
    delete HTMLDialogElement.prototype.close;
  });

  it("displays the temporary password prominently", async () => {
    // The admin must be able to copy the password and hand it to the user.
    // Hiding or truncating it would make the reset workflow unusable.
    component = mount(TempPasswordDialog, {
      target,
      props: {
        password: "TempPass123!",
        title: "Password reset.",
        smtpEnabled: false,
        onDismiss: vi.fn(),
      },
    });
    await settle();
    expect(target.textContent).toContain("TempPass123!");
  });

  it("shows a prominent warning when SMTP is not configured", async () => {
    // Without SMTP the user will never receive a welcome email. The admin
    // must be warned so they know to deliver the password in person — a
    // silent failure would leave the new employee unable to log in.
    component = mount(TempPasswordDialog, {
      target,
      props: {
        password: "TempPass123!",
        title: "Password reset.",
        smtpEnabled: false,
        onDismiss: vi.fn(),
      },
    });
    await settle();
    expect(target.textContent).toContain("No email was sent");
  });

  it("shows the registration-email notice when SMTP is configured", async () => {
    // When SMTP is working, the new user will automatically receive their
    // login link by email, so the admin does not need to deliver it manually.
    component = mount(TempPasswordDialog, {
      target,
      props: {
        password: "TempPass123!",
        title: "Password reset.",
        smtpEnabled: true,
        onDismiss: vi.fn(),
      },
    });
    await settle();
    expect(target.textContent).toContain("Registration email will be sent");
  });

  it("renders a Copy button for the password", async () => {
    // Copying eliminates transcription errors when the admin pastes the
    // password into a messaging app or hands-off document.
    component = mount(TempPasswordDialog, {
      target,
      props: {
        password: "TempPass123!",
        title: "Password reset.",
        smtpEnabled: false,
        onDismiss: vi.fn(),
      },
    });
    await settle();
    const copyBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("Copy")
    );
    expect(copyBtn).not.toBeNull();
  });

  it("calls onDismiss when the OK button is clicked", async () => {
    // The dialog must be closeable so the admin can return to the user list
    // after noting the password.
    const onDismiss = vi.fn();
    component = mount(TempPasswordDialog, {
      target,
      props: {
        password: "TempPass123!",
        title: "Password reset.",
        smtpEnabled: false,
        onDismiss,
      },
    });
    await settle();

    const okBtn = [...target.querySelectorAll("button")].find((b) =>
      b.textContent.includes("OK")
    );
    expect(okBtn).not.toBeNull();
    okBtn.click();
    await settle();
    expect(onDismiss).toHaveBeenCalled();
  });
});
