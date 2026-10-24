import { readFileSync, readdirSync, statSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const frontendRoot = fileURLToPath(new URL("..", import.meta.url));
const catalogRoot = process.argv[2] ?? path.join(frontendRoot, "src/i18n/locales/zh");
const sourceRoot = process.argv[3] ?? path.join(frontendRoot, "src");

const flatten = (value, prefix = "") => {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return Object.fromEntries(
      Object.entries(value).flatMap(([key, child]) =>
        Object.entries(flatten(child, prefix ? `${prefix}.${key}` : key)),
      ),
    );
  }
  return { [prefix]: value };
};

function walk(dir, files = []) {
  for (const entry of readdirSync(dir)) {
    const full = path.join(dir, entry);
    if (statSync(full).isDirectory()) {
      if (entry === "locales" || entry === "node_modules") continue;
      walk(full, files);
      continue;
    }
    if (/\.(ts|tsx)$/.test(entry) && !/\.test\.(ts|tsx)$/.test(entry) && !full.includes(`${path.sep}__tests__${path.sep}`)) {
      files.push(full);
    }
  }
  return files;
}

const namespaces = Object.fromEntries(
  readdirSync(catalogRoot)
    .filter((file) => file.endsWith(".json"))
    .map((file) => [file.replace(/\.json$/, ""), flatten(JSON.parse(readFileSync(path.join(catalogRoot, file), "utf8")))]),
);

const keyPattern = /\b(?:i18n\.)?t\(\s*["'`]([A-Za-z0-9_-]+):([A-Za-z0-9_.-]+)["'`]/g;
const errors = [];

for (const [namespace, messages] of Object.entries(namespaces)) {
  for (const [key, value] of Object.entries(messages)) {
    if (typeof value === "string" && !value.trim()) errors.push(`${namespace}.json:${key}: empty text`);
    if (typeof value === "string" && (value.match(/{{/g) ?? []).length !== (value.match(/}}/g) ?? []).length) {
      errors.push(`${namespace}.json:${key}: interpolation mismatch`);
    }
  }
}

if (statSync(sourceRoot).isDirectory()) {
  for (const file of walk(sourceRoot)) {
    const text = readFileSync(file, "utf8");
    for (const match of text.matchAll(keyPattern)) {
      const namespace = match[1];
      const key = match[2];
      const messages = namespaces[namespace];
      const where = `${path.relative(sourceRoot, file)}:${namespace}:${key}`;
      if (!messages) {
        errors.push(`${where}: missing namespace`);
        continue;
      }
      if (!(key in messages) && !(`${key}_other` in messages)) {
        errors.push(`${where}: missing`);
      }
    }
  }
}

console.log(
  `${Object.keys(namespaces).length} namespaces checked; ${errors.length} issue(s).`,
);
errors.forEach((error) => console.log(`  ${error}`));
process.exitCode = errors.length ? 1 : 0;
