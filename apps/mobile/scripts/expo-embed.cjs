const path = require("node:path");

const mobileRoot = path.resolve(__dirname, "..");
process.chdir(mobileRoot);

const args = process.argv.slice(2);
if (args[0] === "--") args.shift();
const embedCommandIndex = args.indexOf("export:embed");
if (embedCommandIndex < 0) args.unshift("export:embed");
else if (embedCommandIndex > 0) args.splice(embedCommandIndex, 1);
const entryIndex = args.indexOf("--entry-file");
if (entryIndex >= 0 && args[entryIndex + 1]) {
  args[entryIndex + 1] = path.resolve(mobileRoot, args[entryIndex + 1]);
}

const expoCli = require.resolve("@expo/cli", { paths: [mobileRoot] });
process.argv = [process.execPath, expoCli, ...args];
require(expoCli);
