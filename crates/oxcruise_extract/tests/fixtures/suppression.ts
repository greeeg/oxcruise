// oxcruise-ignore-next-line no-circular, no-orphans -- legacy
import { a } from "./one";
import { b } from "./two";
import { c } from "./three"; // oxcruise-ignore-line
/* oxcruise-ignore-next-line */
import {
  d,
} from "./four";
// oxcruise-ignore-lines is not a directive
import { e } from "./five";
import { a2 } from "./one";
