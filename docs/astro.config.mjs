import { defineConfig } from 'astro/config';
import { satteri } from '@astrojs/markdown-satteri';
import { markdownLinks } from './src/lib/markdown-links.mjs';

export default defineConfig({
  site: 'https://jotrorox.github.io',
  base: '/dodo',
  trailingSlash: 'always',
  output: 'static',
  markdown: {
    processor: satteri({ mdastPlugins: [markdownLinks({ base: '/dodo' })] }),
    shikiConfig: {
      themes: { light: 'min-light', dark: 'min-dark' },
      defaultColor: false,
      langAlias: { dodo: 'rust', ebnf: 'text' },
    },
  },
});
