import React from 'react'
import bundledAssets from './bundled-assets.json'
import { createRoot } from 'react-dom/client'
import { Tldraw, getSnapshot, loadSnapshot } from 'tldraw'
import 'tldraw/tldraw.css'

const send = (data) => window.CanvasBridge?.postMessage(JSON.stringify(data))
let editor
let timer
window.canvasApp = {
  open(snapshot) {
    if (snapshot) loadSnapshot(editor.store, snapshot)
    send({ type: 'opened' })
  },
  save() { send({ type: 'snapshot', data: getSnapshot(editor.store) }) },
  snapshot() { return JSON.stringify(getSnapshot(editor.store)) },
}
createRoot(document.getElementById('root')).render(
  <div style={{ position: 'fixed', inset: 0 }}>
    <Tldraw licenseKey={import.meta.env.VITE_TLDRAW_LICENSE_KEY}
      assetUrls={bundledAssets}
      onMount={(instance) => {
        editor = instance
        editor.store.listen(() => {
          clearTimeout(timer)
          timer = setTimeout(() => window.canvasApp.save(), 300)
        }, { scope: 'document', source: 'user' })
        send({ type: 'ready' })
        return () => clearTimeout(timer)
      }} />
  </div>
)


