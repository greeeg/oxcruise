const u = new URL("./asset.png", import.meta.url);
const bare = new URL("bare.txt", import.meta.url);
const w = new Worker(new URL("./worker.js", import.meta.url));
const not = new URL("./nope", "http://example.com");
const here = new URL(".", import.meta.url);
const up = new URL("../", import.meta.url);
