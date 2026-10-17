const fs = require('node:fs');
const path = require('node:path');
function verifyMobileJsDependencies() {
  const root = path.resolve(__dirname, '..');
  const declared = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8')).dependencies;
  for (const name of ['three', '@react-three/fiber', 'react', 'react-native']) {
    let directory = path.dirname(require.resolve(name, { paths: [root] }));
    let actual;
    while (directory !== path.dirname(directory)) {
      const manifest = path.join(directory, 'package.json');
      if (fs.existsSync(manifest)) {
        const value = JSON.parse(fs.readFileSync(manifest, 'utf8'));
        if (value.name === name) { actual = value.version; break; }
      }
      directory = path.dirname(directory);
    }
    if (actual !== declared[name]) {
      throw new Error(`Mobile ${name} requires ${declared[name]}, resolved ${actual}. Install the mobile workspace dependencies before bundling.`);
    }
  }
}
module.exports = { verifyMobileJsDependencies };
if (require.main === module) {
  verifyMobileJsDependencies();
  console.log('Mobile React, React Native, Three and Fiber resolve to their pinned versions.');
}
