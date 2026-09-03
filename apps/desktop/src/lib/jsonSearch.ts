// Ctrl+F over the parsed structure (spec section 10), not a raw-text
// rescan: this walks the already-parsed JSON value and matches against
// both keys and values, so results are structural paths a tree component
// can expand to, not string offsets into pretty-printed text.

export type JsonPath = (string | number)[];

export function findJsonMatches(value: unknown, query: string): JsonPath[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  const matches: JsonPath[] = [];

  function walk(v: unknown, path: JsonPath) {
    if (v !== null && typeof v === "object") {
      const entries: [string | number, unknown][] = Array.isArray(v)
        ? v.map((item, i) => [i, item])
        : Object.entries(v as Record<string, unknown>);
      for (const [key, child] of entries) {
        const childPath = [...path, key];
        if (typeof key === "string" && key.toLowerCase().includes(q)) {
          matches.push(childPath);
        }
        walk(child, childPath);
      }
    } else if (String(v).toLowerCase().includes(q)) {
      matches.push(path);
    }
  }

  walk(value, []);
  return matches;
}

export function pathKey(path: JsonPath): string {
  return JSON.stringify(path);
}

/** Every prefix of `path`, including the empty root prefix -- the set of
 * nodes that must be force-expanded for `path` to be visible. */
export function ancestorPathKeys(path: JsonPath): string[] {
  const keys: string[] = [];
  for (let i = 0; i <= path.length; i++) {
    keys.push(JSON.stringify(path.slice(0, i)));
  }
  return keys;
}
