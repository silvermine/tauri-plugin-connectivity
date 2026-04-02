import { readFileSync } from 'node:fs';
import { dirname } from 'node:path';
import typescript from '@rollup/plugin-typescript';

const pkg = JSON.parse(readFileSync('./package.json', 'utf8'));

export default {
   input: 'guest-js/index.ts',
   output: [
      {
         file: pkg.exports.import,
         format: 'esm',
      },
      {
         file: pkg.exports.require,
         format: 'cjs',
      },
   ],
   plugins: [
      typescript({
         tsconfig: './guest-js/tsconfig.json',
         declaration: true,
         declarationDir: dirname(pkg.exports.import),
      }),
   ],
   external: [
      /^@tauri-apps\/api/,
      ...Object.keys(pkg.dependencies || {}),
      ...Object.keys(pkg.peerDependencies || {}),
   ],
};
