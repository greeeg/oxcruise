/** @import { Foo } from "./foo" */
/**
 * @param {import("./bar").Bar} x
 * @type {import('baz').Baz}
 */
function f(x) {}
// not jsdoc: import("./ignored")
