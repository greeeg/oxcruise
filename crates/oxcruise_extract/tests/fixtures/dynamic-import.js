const a = await import("./literal");
const b = await import(`./template-no-expr`);
const c = await import(`./template-${x}`);
const d = await import(variable);
const e = () => import("./lazy");
