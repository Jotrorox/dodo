import { defineConfig } from 'astro/config';
import { satteri } from '@astrojs/markdown-satteri';
import { markdownLinks } from './src/lib/markdown-links.mjs';

export default defineConfig({
  site: 'https://jotrorox.github.io',
  base: '/dodo-docs',
  trailingSlash: 'always',
  output: 'static',
  markdown: {
    processor: satteri({ mdastPlugins: [markdownLinks({ base: '/dodo-docs' })] }),
    shikiConfig: {
      themes: { light: 'github-light', dark: 'github-dark' },
      defaultColor: false,
      langAlias: { dodo: 'rust', ebnf: 'text' },
    },
  },
});
