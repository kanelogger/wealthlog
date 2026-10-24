import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

const LEGACY_IME_KEY_CODE = 229;

/** Detect composition key events, including WebKit versions with incorrect event ordering. */
export function isKeyboardEventComposing(
  event: Pick<KeyboardEvent, "isComposing" | "keyCode">,
): boolean {
  return event.isComposing || event.keyCode === LEGACY_IME_KEY_CODE;
}
