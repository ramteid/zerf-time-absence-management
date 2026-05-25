import js from "@eslint/js";
import globals from "globals";
import svelte from "eslint-plugin-svelte";

const commonRules = {
  "no-unused-vars": [
    "warn",
    { argsIgnorePattern: "^_", caughtErrorsIgnorePattern: "^_" },
  ],
  "no-empty": ["error", { allowEmptyCatch: true }],
};

export default [
  js.configs.recommended,
  ...svelte.configs.recommended,
  {
    files: ["**/*.js"],
    languageOptions: {
      ecmaVersion: "latest",
      sourceType: "module",
      globals: { ...globals.browser, ...globals.node },
    },
    rules: commonRules,
  },
  {
    files: ["**/*.svelte"],
    languageOptions: {
      globals: { ...globals.browser },
    },
    rules: {
      ...commonRules,
      // Disabled: Svelte 4 rule, crashes on ESLint 10 and irrelevant for Svelte 5
      "svelte/no-reactive-functions": "off",
      // Disabled: requires SvelteMap/SvelteSet/SvelteDate refactor; code is correct as-is
      "svelte/prefer-svelte-reactivity": "off",
    },
  },
  { ignores: ["dist/", "node_modules/"] },
];
