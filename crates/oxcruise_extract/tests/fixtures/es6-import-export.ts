import def from "./default";
import * as ns from "./namespace";
import { a, b as c } from "./named";
import "./side-effect";
import dep from "pkg";
export { x } from "./reexport";
export * from "./star";
export * as nsExport from "./star-as";
export const local = 1;
export default function main() {}
