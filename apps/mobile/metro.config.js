require("./scripts/verify-js-deps.cjs").verifyMobileJsDependencies();
const { getDefaultConfig } = require('expo/metro-config');
const fs = require('node:fs');
const path = require('node:path');

const projectRoot = __dirname;
const workspaceRoot = path.resolve(projectRoot, '../..');
// A managed/worktree checkout may reuse dependencies through a directory link.
// Metro must watch the actual target as well as the logical node_modules path.
const workspaceModules = path.join(workspaceRoot, 'node_modules');
const physicalModules = fs.existsSync(workspaceModules) ? fs.realpathSync.native(workspaceModules) : workspaceModules;
const mobileModules = path.join(projectRoot, 'node_modules');
const physicalMobileModules = fs.existsSync(mobileModules) ? fs.realpathSync.native(mobileModules) : mobileModules;
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

config.watchFolders = [...new Set([...(config.watchFolders ?? []), workspaceRoot, physicalModules, physicalMobileModules])];
config.resolver.nodeModulesPaths = [
  mobileModules,
  physicalMobileModules,
  workspaceModules,
  physicalModules,
];
config.resolver.extraNodeModules = {
  ...(config.resolver.extraNodeModules ?? {}),
  react: reactRoot,
  'react-native': path.dirname(rnPackage),
};

const defaultResolveRequest = config.resolver.resolveRequest;
config.resolver.resolveRequest = (context, moduleName, platform) => {
  // Shared ESM TypeScript uses emitted .js specifiers. Metro consumes source
  // directly; translate only workspace renderer imports that have a .ts file.
  const rendererRoot = path.join(workspaceRoot, 'packages', 'renderer', 'src') + path.sep;
  if (context.originModulePath.startsWith(rendererRoot) && moduleName.startsWith('.') && moduleName.endsWith('.js')) {
    const source = path.resolve(path.dirname(context.originModulePath), moduleName.slice(0, -3) + '.ts');
    if (source.startsWith(rendererRoot) && fs.existsSync(source)) return { type: 'sourceFile', filePath: source };
  }
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
