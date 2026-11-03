// Copies README.md and LICENSE from the repository root into npm/oxcruise/, so that they are in the
// published package and npmjs.com shows them.
//
//   node tasks/npm/sync-readme.mjs
//
// The pre-commit hook (lefthook.yml) runs this and stages the copies, and CI fails when they are
// out of date. The copies are committed because `npm publish` ships what is in the directory.
//
// npmjs.com cannot resolve the repository-relative links of the README (`website/…`, `docs/…`): it
// resolves them against `repository.directory`, which is `npm/oxcruise`. The copy therefore gets
// absolute links to the repository on GitHub. The part of the README between
// `<!-- repo-only:start -->` and `<!-- repo-only:end -->` (contributor notes) is left out.

import { copyFileSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, posix } from "node:path";
import { fileURLToPath } from "node:url";

const repo = join(dirname(fileURLToPath(import.meta.url)), "../..");
const pkgDir = join(repo, "npm/oxcruise");
const { repository } = JSON.parse(readFileSync(join(pkgDir, "package.json"), "utf8"));
const github = repository.url.replace(/^git\+/, "").replace(/\.git$/, "");
const branch = "master";

function isDirectory(path) {
  try {
    return statSync(join(repo, path)).isDirectory();
  } catch {
    console.warn(`sync-readme: README links to ${path}, which does not exist`);
    return false;
  }
}

function absolute(target) {
  // leave anchors, absolute URLs (any scheme, `//host`) and mail links alone
  if (/^(#|[a-z][a-z0-9+.-]*:|\/\/)/i.test(target)) return target;
  const [path, hash = ""] = target.split(/(?=#)/);
  const clean = posix.normalize(path).replace(/\/$/, "");
  return `${github}/${isDirectory(clean) ? "tree" : "blob"}/${branch}/${clean}${hash}`;
}

function toNpmReadme(source) {
  const text = source.replace(/<!-- repo-only:start -->[\s\S]*?<!-- repo-only:end -->\n*/g, "");
  // rewrite links outside fenced code blocks only
  return text
    .split(/(^```[\s\S]*?^```$)/m)
    .map((part, i) =>
      i % 2
        ? part
        : part
            // [text](target) and ![alt](target); the target ends at the first space or `)`
            .replace(/(\]\()([^)\s]+)/g, (_, open, target) => open + absolute(target))
            // reference definitions: [label]: target
            .replace(/^(\[[^\]]+\]:\s*)(\S+)/gm, (_, label, target) => label + absolute(target)),
    )
    .join("");
}

const readme = toNpmReadme(readFileSync(join(repo, "README.md"), "utf8"));
writeFileSync(join(pkgDir, "README.md"), readme);
copyFileSync(join(repo, "LICENSE"), join(pkgDir, "LICENSE"));
console.log("sync-readme: wrote npm/oxcruise/README.md and LICENSE");
