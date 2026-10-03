const { getDefaultConfig } = require('expo/metro-config');
const fs = require('node:fs');
const path = require('node:path');

const projectRoot = __dirname;
const workspaceRoot = path.resolve(projectRoot, '../..');
const rnPackage = require.resolve('react-native/package.json', { paths: [projectRoot] });
const reactPackage = require.resolve('react/package.json', { paths: [path.dirname(rnPackage)] });
const threeEntry = require.resolve('three', { paths: [projectRoot] });
const reactRoot = fs.realpathSync.native(path.dirname(reactPackage));
const config = getDefaultConfig(projectRoot);
config.resolver.disableHierarchicalLookup = true;
// Native compilation creates large trees under the watched workspace. They are
// not JavaScript inputs and can otherwise stall Metro's initial file crawl.
const defaultBlockList = config.resolver.blockList;
config.resolver.blockList = [
  ...(Array.isArray(defaultBlockList) ? defaultBlockList : defaultBlockList ? [defaultBlockList] : []),
  /[\\/](?:target|\.git|\.gradle|\.cxx)[\\/]/,
];

config.watchFolders = [...new Set([...(config.watchFolders ?? []), workspaceRoot])];
config.resolver.nodeModulesPaths = [
  path.join(projectRoot, 'node_modules'),
  path.join(workspaceRoot, 'node_modules'),
];
config.resolver.extraNodeModules = {
  ...(config.resolver.extraNodeModules ?? {}),
  react: reactRoot,
  'react-native': path.dirname(rnPackage),
};

const defaultResolveRequest = config.resolver.resolveRequest;
config.resolver.resolveRequest = (context, moduleName, platform) => {
  // Fiber and shared desktop scene components must see the same Three classes.
  // Cross-version instanceof checks otherwise try to replace readonly vectors.
  if (moduleName === 'three') return { type: 'sourceFile', filePath: threeEntry };
  const match = /^react(?:\/(.*))?$/.exec(moduleName);
  if (match) {
    const request = match[1] ? path.join(reactRoot, match[1]) : path.join(reactRoot, 'index.js');
    const resolvedReactFile = require.resolve(request, { paths: [reactRoot] });
    return { type: 'sourceFile', filePath: resolvedReactFile };
  }
  const resolved = defaultResolveRequest
    ? defaultResolveRequest(context, moduleName, platform)
    : context.resolveRequest(context, moduleName, platform);
  if (resolved.type !== 'sourceFile') return resolved;

  const normalized = resolved.filePath.split(path.sep).join('/');
  const marker = '/node_modules/react/';
  const index = normalized.lastIndexOf(marker);
  if (index < 0) return resolved;
  const suffix = normalized.slice(index + marker.length);
  const canonical = path.join(reactRoot, suffix);
  return fs.existsSync(canonical) ? { type: 'sourceFile', filePath: canonical } : resolved;
};

module.exports = config;
