import type { Implementation } from '@modelcontextprotocol/sdk/types.js';
import { McpServerComposer } from './server-composer';
import { BunSSEServerTransport } from './bun-sse-transport';
import type { McpServerType } from './utils/schemas';

export class McGravityServer {
  private readonly serverComposer: McpServerComposer;
  private readonly transports: Record<string, BunSSEServerTransport> = {};

  constructor(
    serverInfo: Implementation,
    private readonly serverOptions: {
      port?: number;
      host?: string;
    }
  ) {
    this.serverComposer = new McpServerComposer(serverInfo);
  }

  async loadTargets(targetServers: McpServerType[]) {
    for (const targetServer of targetServers) {
      await this.serverComposer.addTargetServer(new URL(targetServer.url), {
        name: targetServer.name ?? new URL(targetServer.url).hostname,
        version: targetServer.version ?? '1.0.0',
        description: targetServer.description ?? '',
      });
    }
  }

  start() {
    Bun.serve({
      port: this.serverOptions.port ?? 3001,
      hostname: this.serverOptions.host ?? '0.0.0.0',
      idleTimeout: -1,
      routes: {
        '/': () => {
          const transport = new BunSSEServerTransport('/sessions');
          this.serverComposer.server.connect(transport);
          transport.onclose = () => {
            console.log(`Session ${transport.sessionId} closed`);
            delete this.transports[transport.sessionId];
          };
          this.transports[transport.sessionId] = transport;
          console.log(`Session ${transport.sessionId} opened`);
          return transport.createResponse();
        },
        '/sessions': (req) => {
          const url = new URL(req.url);
          const sessionId = url.searchParams.get('sessionId');
          if (!sessionId || !this.transports[sessionId]) {
            return new Response('Invalid session ID', { status: 400 });
          }
          return this.transports[sessionId].handlePostMessage(req);
        },

        // Health check endpoint
        '/health': () => {
          return new Response('OK', { status: 200 });
        },

        // API endpoints
        '/api/list-targets': () => {
          return new Response(JSON.stringify(this.serverComposer.listTargetClients()), {
            headers: {
              'Content-Type': 'application/json',
            },
          });
        },
      },
      fetch() {
        return new Response('Not found', { status: 404 });
      },
      error(error) {
        console.error(error);
        return new Response('Internal server error', { status: 500 });
      },
    });
  }
}
