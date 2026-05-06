export function storePasswordCredential(form) {
  if (typeof window === "undefined" || typeof navigator === "undefined") {
    return Promise.resolve(false);
  }

  const credentialStore = navigator.credentials;
  const PasswordCredential = window.PasswordCredential;

  if (!form || !credentialStore?.store || !PasswordCredential) {
    return Promise.resolve(false);
  }

  let credential;
  try {
    credential = new PasswordCredential(form);
  } catch {
    return Promise.resolve(false);
  }

  return credentialStore
    .store(credential)
    .then(() => true)
    .catch(() => false);
}
