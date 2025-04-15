import { McpServer, ResourceTemplate } from '@modelcontextprotocol/sdk/server/mcp.js';
import { z } from 'zod';
import { BunSSEServerTransport } from '../src/bun-sse-transport.js';

const server = new McpServer({
  name: 'Echo',
  version: '1.0.0',
});

server.resource(
  'echo',
  new ResourceTemplate('echo://{message}', { list: undefined }),
  async (uri, { message }) => ({
    contents: [
      {
        uri: uri.href,
        text: `Resource echo: ${message}`,
      },
    ],
  })
);

server.tool('echo', { message: z.string() }, async ({ message }) => ({
  content: [{ type: 'text', text: `Tool echo: ${message}` }],
}));

server.prompt('echo', { message: z.string() }, ({ message }) => ({
  messages: [
    {
      role: 'user',
      content: {
        type: 'text',
        text: `Please process this message: ${message}`,
      },
    },
  ],
}));

const transports: Record<string, BunSSEServerTransport> = {};

Bun.serve({
  port: 3000,
  idleTimeout: 255,
  routes: {
    '/sse': () => {
      const transport = new BunSSEServerTransport('/messages');
      server.connect(transport);
      transport.onclose = () => {
        delete transports[transport.sessionId];
      };
      transports[transport.sessionId] = transport;
      return transport.createResponse();
    },
    '/messages': (req) => {
      const url = new URL(req.url);
      const sessionId = url.searchParams.get('sessionId');
      if (!sessionId || !transports[sessionId]) {
        return new Response('Invalid session ID', { status: 400 });
      }

      return transports[sessionId].handlePostMessage(req);
    },
  },
  fetch(req) {
    console.log('fetch', req.url);
    const url = new URL(req.url);

    // Home page
    if (url.pathname === '/') {
      return new Response('Hello World!');
    }

    return new Response('Not Found', { status: 404 });
  },
});
