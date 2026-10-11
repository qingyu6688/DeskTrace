import js from '@eslint/js';
import tseslint from 'typescript-eslint';
import vue from 'eslint-plugin-vue';

export default [
  { ignores: ['dist/**', 'node_modules/**'] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  ...vue.configs['flat/recommended'],
  {
    files: ['**/*.vue'],
    languageOptions: { parserOptions: { parser: tseslint.parser } },
    // Vue 模板读取与 TypeScript 的类型名字由 vue-tsc 校验，避免核心规则误报。
    rules: { 'no-useless-assignment': 'off', 'no-undef': 'off' },
  },
  {
    files: ['src/**/*.{ts,vue}', 'tests/**/*.ts', 'vite.config.ts'],
    rules: {
      '@typescript-eslint/explicit-function-return-type': 'error',
      'vue/multi-word-component-names': ['error', { ignores: ['App'] }],
      'vue/html-self-closing': ['error', { html: { void: 'always', normal: 'always', component: 'always' } }],
    },
  },
];
