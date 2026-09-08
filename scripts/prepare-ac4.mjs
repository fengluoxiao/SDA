import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve, dirname } from 'node:path';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const source = resolve(root, 'tmp/MacinDecode-AC4-Core');
const revision = 'a23965312a53d3cfb1a63cbae692a788efe46123';
const python = process.env.PYTHON ?? 'python';
const env = { ...process.env, PYTHONIOENCODING: 'utf-8', PYTHONPATH: resolve(root, 'tmp/ac4-python') };
const run = (command, args, cwd = root) => execFileSync(command, args, { cwd, env, stdio: 'inherit', windowsHide: true });
if (!existsSync(source)) {
  run('git', ['clone', 'https://github.com/SakuzyPeng/MacinDecode-AC4-Core.git', source]);
  run('git', ['switch', '--detach', revision], source);
}
const actual = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: source, encoding: 'utf8', windowsHide: true }).trim();
if (actual !== revision) throw new Error(`AC-4 source must be at ${revision}; found ${actual} in ${source}`);
run(python, ['-m', 'pip', 'install', '--target', env.PYTHONPATH, '-r', resolve(source, 'scripts/requirements-spec.txt')]);
run(python, [resolve(source, 'scripts/fetch_specs.py')]);
run(python, [resolve(source, 'scripts/generate_spec_tables.py')]);
console.log('AC-4 build inputs ready. Run node scripts/build-core.mjs.');
