/** The payload that `oxcruise --visualize` writes into `data.js`. Columns, not objects. */
export interface VizData {
  version: number;
  root: string;
  generatedAt: number;
  summary: {
    error: number;
    warn: number;
    suppressed: number;
    totalModules: number;
    totalDependencies: number;
  };
  symbols: boolean;
  dirs: { parent: number[]; name: string[] };
  modules: {
    dir: number[];
    name: string[];
    /** 0 local, 1 package, 2 builtin */
    kind: number[];
    size: number[];
    /** 1 not followed, 2 orphan, 4 parse error */
    flags: number[];
    package: number[];
  };
  packages: string[];
  rules: string[];
  edges: {
    from: number[];
    to: number[];
    /** 1 type-only, 2 dynamic, 4 require, 8 export, 16 import */
    kinds: number[];
    /** 1 circular, 2 type-only, 4 error, 8 warn */
    flags: number[];
  };
  violations: VizViolation[];
  exports?: [string, boolean, boolean][][];
}

export interface VizViolation {
  rule: number;
  severity: "error" | "warn";
  kind: "dependency" | "module" | "cycle" | "reachability" | "export" | "file";
  from?: number;
  file?: string;
  to?: number;
  line?: number;
  message: string;
  cycle?: number[];
}

export const MODULE_KIND_NAMES = ["local", "package", "builtin"] as const;
export const EDGE_CIRCULAR = 1;
export const EDGE_TYPE_ONLY = 2;
export const EDGE_ERROR = 4;
export const EDGE_WARN = 8;
