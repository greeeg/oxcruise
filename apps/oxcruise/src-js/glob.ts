/**
 * Glob to RegExp with the same meaning as the Rust `pattern` module: `*` stays inside a path
 * segment, `**` crosses segments, `{a,b}` alternates, and a glob also matches everything
 * below a directory it matches (`src/db` matches `src/db/query.ts`).
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
    else if (c === "{") {
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

export function matchesGlob(glob: string, path: string): boolean {
  return globToRegExp(glob).test(path);
}
