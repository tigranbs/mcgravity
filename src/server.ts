import type { Implementation } from "@modelcontextprotocol/sdk/types.js";
import { McpServerComposer } from "./server-composer";
import { BunSSEServerTransport } from "./bun-sse-transport";

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

  async loadTargets(targetServers: string[]) {
    for (const targetServer of targetServers) {
      const targetServerUrl = new URL(targetServer);
      await this.serverComposer.addTargetServer(targetServerUrl, {
        name: targetServerUrl.hostname,
        version: "1.0.0",
      });
    }
  }

  start() {
    Bun.serve({
      port: this.serverOptions.port ?? 3001,
      hostname: this.serverOptions.host ?? "0.0.0.0",
      idleTimeout: -1,
      routes: {
        "/": () => {
          return new Response(
            JSON.stringify(this.serverComposer.listTargetClients()),
            {
              headers: {
                "Content-Type": "application/json",
              },
            }
          );
        },
        "/sse": () => {
          const transport = new BunSSEServerTransport("/messages");
          this.serverComposer.server.connect(transport);
          transport.onclose = () => {
            delete this.transports[transport.sessionId];
          };
          this.transports[transport.sessionId] = transport;
          return transport.createResponse();
        },
        "/messages": (req) => {
          const url = new URL(req.url);
          const sessionId = url.searchParams.get("sessionId");
          if (!sessionId || !this.transports[sessionId]) {
            return new Response("Invalid session ID", { status: 400 });
          }
          return this.transports[sessionId].handlePostMessage(req);
        },
      },
      fetch(req) {
        return new Response("Not found", { status: 404 });
      },
      error(error) {
        console.error(error);
        return new Response("Internal server error", { status: 500 });
      },
    });
  }
}
