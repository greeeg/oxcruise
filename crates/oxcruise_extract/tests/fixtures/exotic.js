const a = need("./exotic");
const b = window.require("./member-exotic");
jest.mock("./mocked");
const r = require.resolve("./resolved");
other("./not-a-dep");
