import { createInstance } from "i18next";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import zh from "@/i18n/locales/zh/common.json";
import { profileErrorMessage, profileAwareErrorMessage } from "./error-messages";

const platform = vi.hoisted(() => ({ isWeb: false }));
vi.mock("@/adapters", () => ({
  get isWeb() {
    return platform.isWeb;
  },
}));

const i18n = createInstance();
beforeAll(async () => {
  await i18n.init({ lng: "zh", fallbackLng: false, resources: { zh: { common: zh } } });
});

it.each([
  [
    "PROFILE_ORIGIN_REJECTED: /private/secret/internal-details",
    zh.profiles.errors.originRejected,
  ],
  ["PROFILE_CONNECTION_FAILED", zh.profiles.errors.connectionFailed],
  ["PROFILE_AUTH_REQUIRED", zh.profiles.errors.authRequired],
  ["PROFILE_LOCKED", zh.profiles.errors.locked],
  ["PROFILE_STALE", zh.profiles.errors.stale],
  [
    "profile_drain_timeout: the profile is locked; background database work is still completing.",
    zh.profiles.errors.drainTimeout,
  ],
  ["PROFILE_NOT_FOUND", zh.profiles.errors.notFound],
  ["PROFILE_PASSWORD_INVALID", zh.profiles.errors.incorrectProof],
  ["PROFILE_COOLDOWN: Try again in 1 seconds.", "尝试次数过多。请在 1 秒后重试。"],
  ['"PROFILE_COOLDOWN: Try again in 30 seconds."', "尝试次数过多。请在 30 秒后重试。"],
  ["PROFILE_COOLDOWN", zh.profiles.errors.cooldownUnknown],
  ["PROFILE_INVALID: Use a profile name of 1–60 characters.", zh.profiles.errors.name],
  [
    "PROFILE_INVALID: Enter the profile name to confirm deletion.",
    zh.profiles.errors.confirmDeletion,
  ],
  ["PROFILE_UNAVAILABLE: /private/secret/database", zh.profiles.errors.unavailable],
  ["PROFILE_STARTUP_FAILED: private database details", zh.profiles.errors.startupFailed],
  ["PROFILE_SESSION_INTERRUPTED", zh.profiles.errors.sessionInterrupted],
  ["database /private/secret/database", zh.profiles.errors.generic],
])("renders safe copy for %s", (diagnostic, message) => {
  expect(profileErrorMessage(new Error(diagnostic), i18n.getFixedT("zh", "common"))).toBe(message);
});

it("maps the lowercase desktop drain timeout through the shared adapter", () => {
  const message =
    "profile_drain_timeout: the profile is locked; background database work is still completing.";
  expect(profileAwareErrorMessage(message, i18n.getFixedT("zh", "common"))).toBe(
    zh.profiles.errors.drainTimeout,
  );
});

describe("credential store failures", () => {
  afterEach(() => {
    platform.isWeb = false;
  });
  it.each([
    [false, zh.profiles.errors.credentials],
    [true, zh.profiles.errors.serverCredentials],
  ])("name the platform's credential store (web: %s)", (web, message) => {
    platform.isWeb = web;
    const error = new Error(
      "PROFILE_UNAVAILABLE: The profile credential store could not be accessed.",
    );
    expect(profileErrorMessage(error, i18n.getFixedT("zh", "common"))).toBe(message);
  });
});

const catalogs = import.meta.glob<{ default: typeof zh }>("../../i18n/locales/*/common.json", {
  eager: true,
});
interface Messages {
  [key: string]: string | Messages;
}
function flatten(value: Messages, prefix = ""): Record<string, string> {
  return Object.fromEntries(
    Object.entries(value).flatMap(([key, text]) =>
      typeof text === "string"
        ? [[`${prefix}${key}`, text]]
        : Object.entries(flatten(text, `${prefix}${key}.`)),
    ),
  );
}
const source = flatten(zh.profiles);
const placeholders = (value: string) =>
  [...value.matchAll(/\{\{(\w+)\}\}/g)].map((match) => match[1]).sort();

describe.each(Object.entries(catalogs))("profile translations: %s", (path, catalog) => {
  it("contains every message and preserves interpolation variables", () => {
    const translated = flatten(catalog.default.profiles);
    for (const [key, copy] of Object.entries(source)) {
      expect(translated[key], key).toBeTruthy();
      expect(placeholders(translated[key]), key).toEqual(placeholders(copy));
    }
  });
  it("renders cooldowns in the selected language without falling back to English", async () => {
    const lang = path.split("/").at(-2)!;
    const instance = createInstance();
    await instance.init({
      lng: lang,
      fallbackLng: false,
      resources: { [lang]: { common: catalog.default } },
    });
    const t = instance.getFixedT(lang, "common");
    expect(profileErrorMessage("PROFILE_COOLDOWN: Try again in 30 seconds.", t)).toBe(
      catalog.default.profiles.errors.cooldown_other.replace("{{count}}", "30"),
    );
    expect(profileErrorMessage("PROFILE_ORIGIN_REJECTED: internal details", t)).toBe(
      catalog.default.profiles.errors.originRejected,
    );
  });
});

it.each([
  ["PROFILE_STALE: internal details", "stale"],
  ["Profile operations are running. Wait for them to finish and try again.", "busy"],
] as const)("maps profile errors at existing UI boundaries: %s", (raw, key) => {
  expect(profileAwareErrorMessage(raw, i18n.getFixedT("zh", "common"))).toBe(
    zh.profiles.errors[key],
  );
});
it("preserves unrelated auth copy", () => {
  expect(
    profileAwareErrorMessage("Invalid login credentials", i18n.getFixedT("zh", "common")),
  ).toBe("Invalid login credentials");
});
