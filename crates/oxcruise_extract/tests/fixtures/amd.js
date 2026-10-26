define(["./dep-a", "require", "exports", "pkg"], function (a, require, exports, pkg) {});
define("named", ["./dep-b"], function (b) {});
require(["./dep-c"], function (c) {});
