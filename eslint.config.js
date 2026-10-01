// ESLint 扁平配置（根级，随 `npm run lint` 生效）。
// 只启用「能真报错」的规则（未定义变量、语法、可疑比较等），不引入风格规则：全仓一次性重排会
// 制造巨大噪声 diff，需要时再单独立项。
import js from '@eslint/js';

export default [
  {
    ignores: [
      '**/node_modules/**',
      'dist/**',
      'src-tauri/target/**',
      'src-tauri/gen/**',
    ],
  },
  js.configs.recommended,
  {
    files: ['**/*.{js,mjs}'],
    languageOptions: {
      ecmaVersion: 2023,
      sourceType: 'module',
      globals: {
        // 控制面板运行在 WebView 里；测试与脚本运行在 Node 里，两者共用的全局取并集。
        window: 'readonly',
        document: 'readonly',
        performance: 'readonly',
        console: 'readonly',
        process: 'readonly',
        URL: 'readonly',
        fetch: 'readonly',
        setTimeout: 'readonly',
        clearTimeout: 'readonly',
        setInterval: 'readonly',
        clearInterval: 'readonly',
        AbortController: 'readonly',
        AbortSignal: 'readonly',
        Buffer: 'readonly',
      },
    },
    rules: {
      'no-unused-vars': ['warn', { argsIgnorePattern: '^_', caughtErrors: 'none' }],
      'no-empty': ['error', { allowEmptyCatch: true }],
      eqeqeq: ['warn', 'smart'],
    },
  },
];
