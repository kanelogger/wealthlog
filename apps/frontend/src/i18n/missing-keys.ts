const missingKeys = new Set<string>();

export function recordMissingKey(namespace: string, key: string) {
  missingKeys.add(`${namespace}:${key}`);
}

export function consumeMissingKeys(): string[] {
  const keys = [...missingKeys].sort();
  missingKeys.clear();
  return keys;
}
