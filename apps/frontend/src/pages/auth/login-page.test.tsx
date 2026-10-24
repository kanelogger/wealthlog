import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { SSO_REDIRECT_GUARD_STORAGE_KEY } from "@/context/auth-context";
import { LoginPage } from "./login-page";

const useAuthMock = vi.hoisted(() => vi.fn());

vi.mock("@/context/auth-context", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/context/auth-context")>();
  return { ...actual, useAuth: useAuthMock };
});

const SSO_LOGIN_URL = "/api/v1/auth/oidc/login";

function ssoOnlyAuth(overrides: object = {}) {
  return {
    requiresAuth: true,
    requiresPassword: false,
    oidcEnabled: true,
    serverAuthEnabled: true,
    listenScope: "loopback" as const,
    isAuthenticated: false,
    statusLoading: false,
    loginLoading: false,
    loginError: null,
    login: vi.fn(),
    logout: vi.fn(),
    clearError: vi.fn(),
    ...overrides,
  };
}

describe("LoginPage automatic SSO redirect", () => {
  const replaceMock = vi.fn();

  beforeEach(() => {
    window.sessionStorage.clear();
    useAuthMock.mockReturnValue(ssoOnlyAuth());
    vi.stubGlobal("location", { href: "http://localhost/", replace: replaceMock });
  });

  afterEach(() => {
    replaceMock.mockReset();
    vi.unstubAllGlobals();
    window.sessionStorage.clear();
  });

  it("arms the guard and redirects once when SSO is the only method", () => {
    render(<LoginPage />);

    expect(replaceMock).toHaveBeenCalledTimes(1);
    expect(replaceMock).toHaveBeenCalledWith(SSO_LOGIN_URL);
    expect(window.sessionStorage.getItem(SSO_REDIRECT_GUARD_STORAGE_KEY)).toBe("1");
  });

  it("offers the manual button instead of restarting SSO after a callback that failed to establish a session", () => {
    // An armed guard is exactly what a failed round-trip leaves behind: the
    // automatic redirect set it, the callback returned, but `/auth/me` never
    // confirmed a session, so nothing cleared it.
    window.sessionStorage.setItem(SSO_REDIRECT_GUARD_STORAGE_KEY, "1");

    render(<LoginPage />);

    expect(replaceMock).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "使用 SSO 登录" })).toBeInTheDocument();
  });

  it("lets the manual SSO button navigate while the guard is armed", async () => {
    window.sessionStorage.setItem(SSO_REDIRECT_GUARD_STORAGE_KEY, "1");
    const user = userEvent.setup();

    render(<LoginPage />);
    await user.click(screen.getByRole("button", { name: "使用 SSO 登录" }));

    expect(window.location.href).toBe(SSO_LOGIN_URL);
    expect(window.sessionStorage.getItem(SSO_REDIRECT_GUARD_STORAGE_KEY)).toBe("1");
  });

  it("does not redirect while a login error is displayed", () => {
    useAuthMock.mockReturnValue(ssoOnlyAuth({ loginError: "SSO sign-in failed." }));

    render(<LoginPage />);

    expect(replaceMock).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("SSO sign-in failed.");
  });
});
