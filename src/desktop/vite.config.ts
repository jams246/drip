import { fileURLToPath } from 'node:url'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'

export default defineConfig({
  plugins: [
    tailwindcss(),
    {
      name: 'desktop-client-modules',
      enforce: 'pre',
      transform(code, id) {
        if (!id.includes('/node_modules/')) return null
        // WebView2 runs the entire bundle on the client; dependency RSC boundaries do not apply.
        const clientDirective = /^(\s*(?:\/\*[\s\S]*?\*\/\s*)*(?:(?:"use strict"|'use strict');?\s*)?)(['"])use client\2;?\s*/
        if (clientDirective.test(code)) return { code: code.replace(clientDirective, '$1'), map: null }
        return null
      }
    }
  ],
  resolve: { alias: { '@': fileURLToPath(new URL('./ui/', import.meta.url)) } }
})
