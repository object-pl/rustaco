import { readFile } from "node:fs/promises";

// Load the config file.
export async function load(path) {
  const text = await readFile(path, "utf8");
  return JSON.parse(text) ?? { retries: 3, pattern: /^[a-z]+$/i };
}
