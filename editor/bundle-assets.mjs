import { getAssetUrls } from '@tldraw/assets/selfHosted.js'
import { readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
const types = { svg: 'image/svg+xml', woff2: 'font/woff2', json: 'application/json', png: 'image/png' }
const urls = getAssetUrls((path) => {
  const bytes = readFileSync(resolve('node_modules/@tldraw/assets', path))
  const ext = path.split('.').pop()
  return `data:${types[ext] ?? 'application/octet-stream'};base64,${bytes.toString('base64')}`
})
writeFileSync('bundled-assets.json', JSON.stringify(urls))

