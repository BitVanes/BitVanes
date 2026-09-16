import { defineConfig } from 'vitepress';

export default defineConfig({
  title: 'BitVanes',
  description: 'Interactive AST & data-flow walkthroughs for diffs and code paths — AI-guided, in-editor.',
  head: [['link', { rel: 'icon', type: 'image/svg+xml', href: '/logo.svg' }]],
  themeConfig: {
    nav: [
      { text: 'Guide', link: '/guide/getting-started' },
      { text: 'Demo', link: '/demo' },
      { text: 'Reference', link: '/reference/commands' },
    ],
    sidebar: [
      {
        text: 'Guide',
        items: [
          { text: 'Getting Started', link: '/guide/getting-started' },
          { text: 'Walkthroughs', link: '/guide/walkthroughs' },
        ],
      },
      {
        text: 'Reference',
        items: [
          { text: 'Commands', link: '/reference/commands' },
          { text: 'Settings', link: '/reference/settings' },
        ],
      },
    ],
    socialLinks: [{ icon: 'github', link: 'https://github.com/BitVanes/BitVanes' }],
    footer: {
      message: 'Released under a proprietary license.',
      copyright: 'Copyright © 2026 BitVanes',
    },
    outline: { level: [2, 3] },
  },
});
