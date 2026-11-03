/**
 * Glob to RegExp with the same meaning as the Rust `pattern` module: `*` stays inside a path
 * segment, `**` crosses segments, `{a,b}` alternates, and a glob also matches everything
 * below a directory it matches (`src/db` matches `src/db/query.ts`). `[abc]`, `[a-z]` and `[!a]`
 * are classes, and a backslash makes the next character literal. `tests/fixtures/glob-cases.json`
 * holds cases that the Rust matchers run as well.
 */

/** The regex body for `glob`, without anchors. */
function body(glob: string): string {
  const g = glob.replace(/^\.\//, "");
  let out = "";
  let endsWithGlobstarDir = false;
  for (let i = 0; i < g.length; i++) {
    const c = g[i]!;
    endsWithGlobstarDir = false;
    if (c === "*" && g[i + 1] === "*") {
      const atSegmentStart = i === 0 || g[i - 1] === "/";
      if (atSegmentStart && g[i + 2] === "/") {
        out += "(?:.*/)?";
        i += 2;
      } else if (i > 0 && g[i - 1] === "/" && i + 2 === g.length) {
        // trailing `/**`: the `/` just emitted becomes optional together with the rest
        out = `${out.slice(0, -1)}(?:/.*)?`;
        endsWithGlobstarDir = true;
        i += 1;
      } else {
        out += ".*";
        i += 1;
      }
    } else if (c === "*") out += "[^/]*";
    else if (c === "?") out += "[^/]";
    else if (c === "[") {
      const close = g.indexOf("]", i);
      if (close < 0) out += "\\[";
      else {
        out += g.slice(i, close + 1).replace("[!", "[^");
        i = close;
      }
    } else if (c === "\\" && i + 1 < g.length) {
      out += g[++i]!.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    } else if (c === "{") {
      const close = g.indexOf("}", i);
      if (close < 0) out += "\\{";
      else {
        out += `(?:${g
          .slice(i + 1, close)
          .split(",")
          .map(body)
          .join("|")})`;
        i = close;
      }
    } else out += c.replace(/[.+^$()|[\]\\]/g, "\\$&");
  }
  return endsWithGlobstarDir ? out : `${out}(?:/.*)?`;
}

export function globToRegExp(glob: string): RegExp {
  return new RegExp(`^${body(glob)}$`);
}

const compiled = new Map<string, RegExp>();
const CACHE_LIMIT = 512;

/** `globToRegExp` for callers that test many paths against few globs. The regexes are not global, so sharing them is safe. */
function cachedRegExp(glob: string): RegExp {
  let re = compiled.get(glob);
  if (!re) {
    if (compiled.size >= CACHE_LIMIT) compiled.clear();
    re = globToRegExp(glob);
    compiled.set(glob, re);
  }
  return re;
}

export function matchesGlob(glob: string, path: string): boolean {
  return cachedRegExp(glob).test(path);
}
