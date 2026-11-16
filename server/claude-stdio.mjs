#!/usr/bin/env node
import { Client, StreamableHTTPClientTransport } from '@modelcontextprotocol/client';
import { Server } from '@modelcontextprotocol/server';
import { StdioServerTransport } from '@modelcontextprotocol/server/stdio';

const address = process.env.CANVAS_MCP_URL;
if (!address || !/^(https:\/\/|http:\/\/127\.0\.0\.1[:/])/.test(address)) {
  console.error('Set CANVAS_MCP_URL to the HTTPS MCP invitation URL copied from Canvas Studio.');
  process.exit(1);
}
const remote = new Client({ name: 'canvas-studio-desktop-relay', version: '1.0.0' });
try {
  await remote.connect(new StreamableHTTPClientTransport(new URL(address)));
  const local = new Server({ name: 'canvas-studio', version: '1.0.0' }, { capabilities: { tools: {} } });
  local.setRequestHandler('tools/list', () => remote.listTools());
  local.setRequestHandler('tools/call', request => remote.callTool(request.params));
  await local.connect(new StdioServerTransport());
  const close = async () => { await local.close(); await remote.close(); };
  process.on('SIGTERM', close); process.on('SIGINT', close);
} catch {
  console.error('Could not connect to the shared board. Check its invitation URL and server availability.');
  process.exit(1);
}
