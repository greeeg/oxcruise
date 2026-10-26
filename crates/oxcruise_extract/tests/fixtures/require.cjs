const a = require("./one");
const b = require("pkg");
const c = require(dynamicName);
const d = require("a", "b");
function f() {
  return require("./nested");
}
